//! Runtime do projeto: o que a máquina REAL mostra sobre ele agora.
//!
//! Nada daqui vai para o workspace portátil. A detecção é passiva (arquivos
//! conhecidos na raiz; nada de npm install/cargo build) e os scripts que podem
//! ser executados vêm EXCLUSIVAMENTE do package.json local — nunca do workspace
//! portátil, do Git remoto do LKR LAB nem de `Project.commands`.
use crate::{
    git::{self, GitSummary},
    models::{Location, Project},
    ports, projects,
    supervisor::{RunInfo, RunState},
    system, HubResult,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const MAX_PACKAGE_JSON: u64 = 1_000_000;
const MAX_SCRIPTS: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StackItem {
    pub id: &'static str,
    pub label: &'static str,
    /// Arquivo/dependência que justifica a detecção.
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PackageManager {
    pub name: &'static str,
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptKind {
    /// Fica rodando (dev, start, serve…).
    Service,
    /// Executa e termina (build, test, lint…).
    Task,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Script {
    pub name: String,
    /// Texto do package.json, só para exibição.
    pub command: String,
    pub kind: ScriptKind,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub stack: Vec<StackItem>,
    pub package_manager: Option<PackageManager>,
    /// Por que não há gerenciador (sem lockfile, ambíguo…).
    pub package_manager_note: Option<String>,
    pub scripts: Vec<Script>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeStatus {
    Unbound,
    Missing,
    Ready,
    Running,
    Partial,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProcess {
    pub pid: u32,
    pub name: String,
    pub memory: u64,
    pub start_time: u64,
    /// Iniciado pelo LKR LAB (pode ser parado daqui). Externo: nunca.
    pub managed: bool,
    /// cwd | managed | descendant
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePort {
    pub port: u16,
    pub protocol: String,
    pub pid: Option<u32>,
    pub process: String,
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredPort {
    pub name: String,
    pub port: u16,
    /// listening (com dono verificado) | free | occupied_unverified
    pub state: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeService {
    pub label: String,
    pub port: Option<u16>,
    pub pid: Option<u32>,
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRuntime {
    pub project_id: String,
    pub status: RuntimeStatus,
    pub status_detail: Option<String>,
    #[serde(flatten)]
    pub detection: Detection,
    pub git: Option<GitSummary>,
    pub processes: Vec<RuntimeProcess>,
    pub ports: Vec<RuntimePort>,
    pub declared_ports: Vec<DeclaredPort>,
    pub services: Vec<RuntimeService>,
    pub runs: Vec<RunInfo>,
    /// Há processo externo (não gerenciado) do projeto em execução.
    pub external_running: bool,
    pub can_run: bool,
    pub run_blocked_reason: Option<String>,
}

/// O que o supervisor pode executar, já validado.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub display: String,
    pub kind: ScriptKind,
}

// ------------------------------------------------------------------ detecção

fn valid_script_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '.' | '-'))
}

pub fn classify(name: &str) -> ScriptKind {
    let base = name.split(':').next().unwrap_or(name);
    match base {
        "dev" | "start" | "serve" | "preview" | "watch" => ScriptKind::Service,
        "build" | "test" | "lint" | "format" | "typecheck" | "check" | "fmt" | "clean" => {
            ScriptKind::Task
        }
        _ => ScriptKind::Other,
    }
}

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_PACKAGE_JSON {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// O arquivo precisa estar de fato dentro do projeto (sem escapar por symlink).
fn inside(root: &Path, file: &Path) -> bool {
    match (root.canonicalize(), file.canonicalize()) {
        (Ok(root), Ok(file)) => file.starts_with(root),
        _ => false,
    }
}

fn exists_any(root: &Path, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find(|n| root.join(n).is_file())
        .map(|n| n.to_string())
}

fn has_dep(package: &serde_json::Value, name: &str) -> bool {
    ["dependencies", "devDependencies"]
        .iter()
        .any(|k| package[*k][name].is_string())
}

fn detect_package_manager(
    root: &Path,
    package: &serde_json::Value,
) -> (Option<PackageManager>, Option<String>) {
    let candidates = [
        ("package-lock.json", "npm"),
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
    ];
    let mut found: Vec<(&'static str, &str)> = candidates
        .iter()
        .filter(|(file, _)| root.join(file).is_file())
        .map(|(file, name)| (*name, *file))
        .collect();
    found.dedup_by_key(|(name, _)| *name);
    match found.len() {
        1 => (
            Some(PackageManager {
                name: found[0].0,
                evidence: found[0].1.into(),
            }),
            None,
        ),
        0 => {
            // Sem lockfile: só o campo "packageManager" do próprio package.json serve de evidência.
            let declared = package["packageManager"].as_str().unwrap_or("");
            let name = declared.split('@').next().unwrap_or("");
            match ["npm", "pnpm", "yarn", "bun"].iter().find(|n| **n == name) {
                Some(name) => (
                    Some(PackageManager {
                        name,
                        evidence: "package.json › packageManager".into(),
                    }),
                    None,
                ),
                None => (
                    None,
                    Some(
                        "Nenhum lockfile nem campo packageManager: gerenciador não identificado."
                            .into(),
                    ),
                ),
            }
        }
        _ => (
            None,
            Some(format!(
                "Mais de um lockfile ({}): gerenciador ambíguo.",
                found.iter().map(|(_, f)| *f).collect::<Vec<_>>().join(", ")
            )),
        ),
    }
}

fn parse_scripts(package: &serde_json::Value) -> Vec<Script> {
    let Some(map) = package["scripts"].as_object() else {
        return vec![];
    };
    let mut scripts: Vec<Script> = map
        .iter()
        .filter_map(|(name, value)| {
            let command = value.as_str()?;
            if !valid_script_name(name) {
                return None;
            }
            // pre/post são ganchos automáticos de outro script: não são ações.
            for prefix in ["pre", "post"] {
                if let Some(rest) = name.strip_prefix(prefix) {
                    if map.contains_key(rest) {
                        return None;
                    }
                }
            }
            Some(Script {
                name: name.clone(),
                command: command.chars().take(500).collect(),
                kind: classify(name),
            })
        })
        .collect();
    let rank = |s: &Script| match (s.name.as_str(), s.kind) {
        ("dev", _) => 0,
        ("start", _) => 1,
        (_, ScriptKind::Service) => 2,
        (_, ScriptKind::Other) => 3,
        (_, ScriptKind::Task) => 4,
    };
    scripts.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.name.cmp(&b.name)));
    scripts.truncate(MAX_SCRIPTS);
    scripts
}

type Fingerprint = Vec<Option<(u64, SystemTime)>>;
type DetectionCache = Mutex<HashMap<PathBuf, (Fingerprint, Detection)>>;
const WATCHED: [&str; 17] = [
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "Pipfile",
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
    "tauri.conf.json",
    "vite.config.ts",
];

fn fingerprint(root: &Path) -> Fingerprint {
    WATCHED
        .iter()
        .map(|name| {
            std::fs::metadata(root.join(name))
                .ok()
                .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        })
        .collect()
}

/// Detecção passiva da raiz do projeto. Cache efêmero invalidado pelos arquivos relevantes.
pub fn detect(root: &Path) -> Detection {
    static CACHE: OnceLock<DetectionCache> = OnceLock::new();
    let print = fingerprint(root);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((cached, detection)) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(root) {
        if *cached == print {
            return detection.clone();
        }
    }
    let detection = detect_uncached(root);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(root.to_path_buf(), (print, detection.clone()));
    detection
}

fn detect_uncached(root: &Path) -> Detection {
    let mut detection = Detection::default();
    let mut add = |id: &'static str, label: &'static str, evidence: String| {
        if !detection.stack.iter().any(|s| s.id == id) {
            detection.stack.push(StackItem {
                id,
                label,
                evidence,
            });
        }
    };
    let package_path = root.join("package.json");
    let package = if package_path.is_file() && inside(root, &package_path) {
        read_small(&package_path).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    } else {
        None
    };
    if package.is_some() || package_path.is_file() {
        add("node", "Node.js", "package.json".into());
    }
    if root.join("Cargo.toml").is_file() {
        add("rust", "Rust", "Cargo.toml".into());
    }
    if let Some(file) = exists_any(root, &["pyproject.toml", "requirements.txt", "Pipfile"]) {
        add("python", "Python", file);
    }
    let null = serde_json::Value::Null;
    let pkg = package.as_ref().unwrap_or(&null);
    if root.join("src-tauri").join("tauri.conf.json").is_file()
        || root.join("tauri.conf.json").is_file()
    {
        add("tauri", "Tauri", "tauri.conf.json".into());
    } else if has_dep(pkg, "@tauri-apps/cli") {
        add("tauri", "Tauri", "@tauri-apps/cli".into());
    }
    for (id, label, configs, dep) in [
        (
            "vite",
            "Vite",
            &[
                "vite.config.ts",
                "vite.config.js",
                "vite.config.mjs",
                "vite.config.mts",
            ][..],
            "vite",
        ),
        (
            "next",
            "Next.js",
            &["next.config.js", "next.config.mjs", "next.config.ts"][..],
            "next",
        ),
        (
            "astro",
            "Astro",
            &[
                "astro.config.mjs",
                "astro.config.ts",
                "astro.config.js",
                "astro.config.mts",
            ][..],
            "astro",
        ),
    ] {
        if let Some(file) = exists_any(root, configs) {
            add(id, label, file);
        } else if has_dep(pkg, dep) {
            add(id, label, format!("dependência {dep}"));
        }
    }
    if has_dep(pkg, "react") {
        add("react", "React", "dependência react".into());
    }
    if let Some(file) = exists_any(
        root,
        &[
            "Dockerfile",
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yml",
            "compose.yaml",
        ],
    ) {
        add("docker", "Docker", file);
    }
    if package.is_some() {
        let (pm, note) = detect_package_manager(root, pkg);
        detection.package_manager = pm;
        detection.package_manager_note = note;
        detection.scripts = parse_scripts(pkg);
    }
    detection
}

/// Remove o prefixo verbatim do Windows (`\\?\C:\…`): o cmd.exe, que roda os shims
/// .cmd do npm/pnpm/yarn, não aceita esse formato como diretório de trabalho.
pub fn plain_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// Valida tudo para executar `script` e devolve o comando a rodar.
/// Fronteira de confiança: o nome precisa existir no package.json LOCAL da pasta vinculada;
/// o programa é o gerenciador detectado (caminho absoluto do PATH); os argumentos são fixos.
pub fn launch_spec(project: &Project, script: &str) -> HubResult<LaunchSpec> {
    let cwd = projects::local_dir(project)?; // recusa unbound e missing
    let cwd = plain_path(
        cwd.canonicalize()
            .map_err(|_| "A pasta do projeto não está acessível.".to_string())?,
    );
    if !valid_script_name(script) {
        return Err("Nome de script inválido.".into());
    }
    let detection = detect(&cwd);
    let found = detection
        .scripts
        .iter()
        .find(|s| s.name == script)
        .ok_or_else(|| format!("O script “{script}” não existe no package.json deste projeto."))?;
    let manager = detection.package_manager.ok_or_else(|| {
        detection
            .package_manager_note
            .unwrap_or_else(|| "Gerenciador de pacotes não identificado.".into())
    })?;
    let program = crate::commands::resolve_tool(manager.name).ok_or_else(|| {
        format!(
            "{} não foi encontrado no PATH. Instale-o e reinicie o aplicativo.",
            manager.name
        )
    })?;
    Ok(LaunchSpec {
        program,
        args: vec!["run".into(), script.into()],
        cwd,
        display: format!("{} run {}", manager.name, script),
        kind: found.kind,
    })
}

// ------------------------------------------------------------------ snapshot

/// Nome do serviço numa porta. Só afirma um framework quando há evidência: ele está na stack E
/// o script de serviço do projeto cita a porta (`--port 1420`) ou a porta é a padrão do framework
/// sem porta explícita no script. Caso contrário o nome é genérico (nunca um palpite).
fn service_label(detection: &Detection, port: u16) -> String {
    for (id, label, default_port) in [
        ("vite", "Vite", 5173u16),
        ("next", "Next.js", 3000),
        ("astro", "Astro", 4321),
    ] {
        if !detection.stack.iter().any(|s| s.id == id) {
            continue;
        }
        let mentions = |command: &str, number: u16| {
            command
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|token| token == number.to_string())
        };
        let evidence = detection
            .scripts
            .iter()
            .filter(|s| s.kind == ScriptKind::Service && s.command.contains(id))
            .any(|s| {
                mentions(&s.command, port) || (port == default_port && !s.command.contains("port"))
            });
        if evidence {
            return format!("{label} dev server");
        }
    }
    "Servidor local".into()
}

/// Monta o runtime do projeto. `managed` vem do supervisor (PID → projeto).
pub fn inspect(
    project: &Project,
    all_projects: &[Project],
    runs: Vec<RunInfo>,
    managed: &HashMap<u32, String>,
) -> ProjectRuntime {
    inspect_live(project, all_projects, runs, &|| managed.clone())
}

/// `managed` é consultado depois da leitura de processos (ver `system::processes_managed_with`).
pub fn inspect_live(
    project: &Project,
    all_projects: &[Project],
    runs: Vec<RunInfo>,
    managed: &dyn Fn() -> HashMap<u32, String>,
) -> ProjectRuntime {
    let mut runtime = ProjectRuntime {
        project_id: project.id.clone(),
        status: RuntimeStatus::Ready,
        status_detail: None,
        detection: Detection::default(),
        git: None,
        processes: vec![],
        ports: vec![],
        declared_ports: vec![],
        services: vec![],
        runs,
        external_running: false,
        can_run: false,
        run_blocked_reason: None,
    };
    match projects::location(project) {
        Location::Unbound => {
            runtime.status = RuntimeStatus::Unbound;
            runtime.status_detail = Some("Projeto ainda não localizado nesta máquina.".into());
            runtime.run_blocked_reason = runtime.status_detail.clone();
            return runtime;
        }
        Location::Missing => {
            runtime.status = RuntimeStatus::Missing;
            runtime.status_detail = Some("A pasta vinculada não existe mais nesta máquina.".into());
            runtime.run_blocked_reason = runtime.status_detail.clone();
            return runtime;
        }
        Location::Available => {}
    }
    let Ok(root) = projects::local_dir(project) else {
        return runtime;
    };
    runtime.detection = detect(&root);
    runtime.git = Some(git::summary(&root));
    runtime.can_run =
        !runtime.detection.scripts.is_empty() && runtime.detection.package_manager.is_some();
    if !runtime.can_run {
        runtime.run_blocked_reason = if runtime.detection.scripts.is_empty() {
            Some("Nenhum script executável encontrado no package.json.".into())
        } else {
            runtime.detection.package_manager_note.clone()
        };
    }

    let processes = system::processes_managed_with(all_projects, managed);
    let mine: Vec<_> = processes
        .iter()
        .filter(|p| p.project_id.as_deref() == Some(project.id.as_str()))
        .collect();
    runtime.processes = mine
        .iter()
        .map(|p| RuntimeProcess {
            pid: p.pid,
            name: p.name.clone(),
            memory: p.memory,
            start_time: p.start_time,
            managed: p.managed,
            evidence: p.confidence.clone(),
        })
        .collect();
    runtime.external_running = mine.iter().any(|p| !p.managed);

    let ports = ports::inspect_with_processes(all_projects, &processes).unwrap_or_default();
    let mut owned = Vec::new();
    for port in &ports {
        if port.project_id.as_deref() == Some(project.id.as_str()) {
            let managed_port = port
                .pid
                .is_some_and(|pid| processes.iter().any(|p| p.pid == pid && p.managed));
            owned.push(RuntimePort {
                port: port.port,
                protocol: port.protocol.clone(),
                pid: port.pid,
                process: port.process.clone(),
                managed: managed_port,
            });
        }
    }
    runtime.declared_ports = project
        .ports
        .iter()
        .map(|declared| {
            let listening: Vec<_> = ports
                .iter()
                .filter(|p| p.port == declared.port && p.protocol == "TCP")
                .collect();
            let state = if listening.is_empty() {
                "free"
            } else if listening
                .iter()
                .any(|p| p.project_id.as_deref() == Some(project.id.as_str()))
            {
                "listening"
            } else {
                "occupied_unverified"
            };
            DeclaredPort {
                name: declared.name.clone(),
                port: declared.port,
                state,
            }
        })
        .collect();
    runtime.services = owned
        .iter()
        .filter(|p| p.protocol == "TCP")
        .map(|p| RuntimeService {
            label: service_label(&runtime.detection, p.port),
            port: Some(p.port),
            pid: p.pid,
            managed: p.managed,
        })
        .collect();
    runtime.ports = owned;

    let active = runtime
        .runs
        .iter()
        .any(|r| r.state.is_active() && r.kind != ScriptKind::Task);
    let failed = runtime
        .runs
        .first()
        .is_some_and(|r| r.state == RunState::Failed && r.kind != ScriptKind::Task);
    let declared_total = runtime.declared_ports.len();
    let declared_up = runtime
        .declared_ports
        .iter()
        .filter(|p| p.state == "listening")
        .count();
    if active || runtime.external_running || !runtime.ports.is_empty() {
        if declared_total > 0 && declared_up < declared_total && declared_up > 0 {
            runtime.status = RuntimeStatus::Partial;
            runtime.status_detail = Some("Alguns serviços declarados não estão ativos.".into());
        } else {
            runtime.status = RuntimeStatus::Running;
            if !active && runtime.external_running {
                runtime.status_detail =
                    Some("Em execução externamente (não iniciado pelo LKR LAB).".into());
            }
        }
    } else if failed {
        runtime.status = RuntimeStatus::Error;
        runtime.status_detail = Some("A última execução terminou com erro.".into());
    }
    runtime
}
