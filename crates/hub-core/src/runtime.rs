//! Runtime do projeto: o que a máquina REAL mostra sobre ele agora.
//!
//! Nada daqui vai para o workspace portátil. A detecção é passiva (arquivos
//! conhecidos na raiz; nada de npm install/cargo build) e os scripts que podem
//! ser executados vêm EXCLUSIVAMENTE do package.json local — nunca do workspace
//! portátil, do Git remoto do LKR LAB nem de `Project.commands`.
use crate::{
    actions::{self, ComposeView, RuntimeCommand},
    compose::{self, ComposeConfig, ComposePort, Container, DockerInfo},
    git::{self, GitSummary},
    models::{Location, Project},
    ports, projects,
    rust_project::{self, RustInfo},
    supervisor::{RunInfo, RunState},
    system,
    tools::Tools,
    HubResult,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

const MAX_PACKAGE_JSON: u64 = 1_000_000;
const MAX_SCRIPTS: usize = 50;
/// Mesmo sem mudança nos arquivos observados, a detecção é refeita depois deste tempo
/// (membros de workspace Cargo e outros arquivos fora da lista não entram na impressão digital).
const DETECTION_TTL: Duration = Duration::from_secs(20);

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

/// Tauri do projeto, lido dos arquivos (sem executar a CLI).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TauriInfo {
    /// Versão principal (1 ou 2) e de onde ela veio.
    pub version: Option<u8>,
    pub version_evidence: Option<String>,
    /// Pasta do tauri.conf.json relativa à raiz: "src-tauri" ou ".".
    pub conf_dir: String,
    /// Porta do dev server declarada em `build.devUrl` (ou `devPath` no v1), se houver.
    pub dev_url_port: Option<u16>,
    /// O package.json tem o script "tauri" (caminho oficial: `npm run tauri dev`).
    pub has_script: bool,
    /// A CLI local está instalada em node_modules/.bin.
    pub local_cli: bool,
}

/// Uma peça de uma stack composta (ex.: Tauri = shell + frontend + backend).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StackPart {
    pub role: &'static str,
    pub label: String,
}
/// A stack lida como UM sistema, não como runtimes soltos.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Composition {
    /// "Tauri · Vite · Rust"
    pub headline: String,
    pub parts: Vec<StackPart>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub stack: Vec<StackItem>,
    pub package_manager: Option<PackageManager>,
    /// Por que não há gerenciador (sem lockfile, ambíguo…).
    pub package_manager_note: Option<String>,
    pub scripts: Vec<Script>,
    pub rust: Option<RustInfo>,
    pub tauri: Option<TauriInfo>,
    pub docker: Option<DockerInfo>,
    pub composition: Composition,
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

/// Serviço do Compose como a interface o mostra: configuração (YAML resolvido) + estado real.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeServiceRuntime {
    pub name: String,
    /// running | restarting | paused | exited | created | dead | absent (sem container)
    pub state: String,
    pub health: Option<String>,
    pub exit_code: Option<i32>,
    pub ports: Vec<ComposePort>,
    pub profiles: Vec<String>,
}

/// Projeto Compose gerenciado: o estado vem do `compose ps`, nunca do PID do cliente `docker`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeRuntime {
    pub file: String,
    pub override_files: Vec<String>,
    /// Mais de um arquivo Compose na pasta: qual vale (precedência do próprio Compose).
    pub note: Option<String>,
    pub project_name: Option<String>,
    pub services: Vec<ComposeServiceRuntime>,
    /// Containers do projeto (qualquer estado).
    pub containers: usize,
    /// Serviços padrão (sem profile) em execução / total.
    pub running: usize,
    pub expected: usize,
    /// O LKR LAB subiu o projeto nesta sessão (último `up` concluído, sem `down` depois).
    pub started_here: bool,
    /// Config inválida ou `ps` indisponível: a interface mostra o motivo, sem inventar estado.
    pub error: Option<String>,
}

/// Resultado da última tarefa (build, teste, check…), só em memória.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResult {
    pub command: String,
    pub state: RunState,
    pub exit_code: Option<i32>,
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
    /// Ferramentas relevantes para este projeto e se estão disponíveis.
    pub tools: Vec<crate::tools::ToolStatus>,
    /// Tudo que dá para executar, com disponibilidade e motivo (vem só de arquivos locais).
    pub commands: Vec<RuntimeCommand>,
    /// Ação principal quando não há ambiguidade; ausente = "Rodar ▾".
    pub primary_command: Option<String>,
    pub compose: Option<ComposeRuntime>,
    pub last_task: Option<TaskResult>,
}

/// O que o supervisor pode executar, já validado.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub display: String,
    pub kind: ScriptKind,
    /// Raiz do projeto (já limpa do prefixo verbatim do Windows).
    pub root: PathBuf,
    /// "node:dev", "cargo:run", "tauri:dev", "compose:up"…
    pub command_id: String,
    /// Parte do id depois da origem ("dev", "run", "up"…).
    pub name: String,
    pub label: String,
    /// node | cargo | tauri | compose
    pub source: &'static str,
    pub selection: Option<String>,
    /// Só observa (logs do Compose): não conta como "projeto em execução".
    pub observer: bool,
    /// Operações do mesmo grupo não rodam em paralelo no projeto.
    pub exclusive: Option<&'static str>,
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
pub(crate) fn inside(root: &Path, file: &Path) -> bool {
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
type DetectionCache = Mutex<HashMap<PathBuf, (Instant, Fingerprint, Detection)>>;
const WATCHED: [&str; 28] = [
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "Cargo.toml",
    "src-tauri/Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "Pipfile",
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
    "compose.override.yml",
    "compose.override.yaml",
    "docker-compose.override.yml",
    "docker-compose.override.yaml",
    "tauri.conf.json",
    "src-tauri/tauri.conf.json",
    "vite.config.ts",
    // A CLI local aparece quando as dependências são instaladas.
    "node_modules/.bin/tauri",
    "node_modules/.bin/tauri.cmd",
    "src/main.rs",
    "src-tauri/src/main.rs",
    "rust-toolchain.toml",
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
    if let Some((at, cached, detection)) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(root)
    {
        if *cached == print && at.elapsed() < DETECTION_TTL {
            return detection.clone();
        }
    }
    let detection = detect_uncached(root);
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(
        root.to_path_buf(),
        (Instant::now(), print, detection.clone()),
    );
    detection
}

// ------------------------------------------------------------------ Tauri

fn major_of(version: &str) -> Option<u8> {
    let digits: String = version
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok().filter(|v| (1..=9).contains(v))
}

/// Porta de uma URL de dev local (`http://localhost:1420`); outros hosts não valem como evidência.
fn local_url_port(url: &str) -> Option<u16> {
    let rest = url.split("://").nth(1)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = authority.rsplit_once(':')?;
    if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        return None;
    }
    port.parse::<u16>().ok().filter(|p| *p != 0)
}

fn detect_tauri(root: &Path, pkg: &serde_json::Value, scripts: &[Script]) -> TauriInfo {
    let conf_dir = if root.join("src-tauri").join("tauri.conf.json").is_file() {
        "src-tauri"
    } else {
        "."
    };
    let conf_path = root.join(conf_dir).join("tauri.conf.json");
    let conf = if conf_path.is_file() && inside(root, &conf_path) {
        read_small(&conf_path).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    } else {
        None
    };
    let mut info = TauriInfo {
        conf_dir: conf_dir.into(),
        has_script: scripts.iter().any(|s| s.name == "tauri"),
        local_cli: ["tauri", "tauri.cmd", "tauri.exe"]
            .iter()
            .any(|name| root.join("node_modules").join(".bin").join(name).is_file()),
        ..TauriInfo::default()
    };
    // Versão: Cargo.toml do app (dependência `tauri`) > formato do tauri.conf.json > pacote npm.
    let cargo = root.join(conf_dir).join("Cargo.toml");
    let from_cargo = read_small(&cargo)
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|m| {
            let dep = m.get("dependencies")?.as_table()?.get("tauri")?;
            let version = dep
                .as_str()
                .or_else(|| dep.as_table()?.get("version")?.as_str())?;
            major_of(version)
        });
    if let Some(v) = from_cargo {
        info.version = Some(v);
        info.version_evidence = Some(format!("{}/Cargo.toml › tauri", conf_dir).replace("./", ""));
    } else if let Some(conf) = &conf {
        if conf["$schema"]
            .as_str()
            .is_some_and(|s| s.contains("/config/2"))
            || conf["app"].is_object()
            || conf["identifier"].is_string()
        {
            info.version = Some(2);
            info.version_evidence = Some("tauri.conf.json › formato v2".into());
        } else if conf["tauri"].is_object() {
            info.version = Some(1);
            info.version_evidence = Some("tauri.conf.json › formato v1".into());
        }
    }
    if info.version.is_none() {
        for dep in ["@tauri-apps/api", "@tauri-apps/cli"] {
            let version = ["dependencies", "devDependencies"]
                .iter()
                .find_map(|k| pkg[*k][dep].as_str());
            if let Some(v) = version.and_then(major_of) {
                info.version = Some(v);
                info.version_evidence = Some(format!("package.json › {dep}"));
                break;
            }
        }
    }
    if let Some(conf) = &conf {
        let url = conf["build"]["devUrl"]
            .as_str()
            .or_else(|| conf["build"]["devPath"].as_str());
        info.dev_url_port = url.and_then(local_url_port);
    }
    info
}

// ------------------------------------------------------------------ composição

fn compose_composition(d: &Detection) -> Composition {
    let has = |id: &str| d.stack.iter().any(|s| s.id == id);
    let label = |id: &str| d.stack.iter().find(|s| s.id == id).map(|s| s.label);
    let front = ["next", "astro", "vite"]
        .iter()
        .find_map(|id| label(id))
        .or_else(|| label("react"))
        .or_else(|| label("node"));
    let manager = d.package_manager.as_ref().map(|m| m.name);
    let mut headline = Vec::new();
    let mut parts = Vec::new();
    if let Some(info) = &d.tauri {
        headline.push("Tauri".to_string());
        parts.push(StackPart {
            role: "Shell",
            label: match info.version {
                Some(v) => format!("Tauri v{v}"),
                None => "Tauri".into(),
            },
        });
    }
    if let Some(front) = front {
        headline.push(front.to_string());
        let role = if d.tauri.is_some() { "Frontend" } else { "App" };
        parts.push(StackPart {
            role,
            label: match manager {
                Some(m) => format!("{front} / {m}"),
                None => front.to_string(),
            },
        });
    }
    if has("rust") {
        headline.push("Rust".into());
        let role = if d.tauri.is_some() { "Backend" } else { "Rust" };
        parts.push(StackPart {
            role,
            label: "Rust / Cargo".into(),
        });
    }
    if has("python") {
        headline.push("Python".into());
        parts.push(StackPart {
            role: "Python",
            label: "Python".into(),
        });
    }
    if let Some(docker) = &d.docker {
        headline.push(
            if docker.kind == "compose" {
                "Docker Compose"
            } else {
                "Dockerfile"
            }
            .into(),
        );
        parts.push(StackPart {
            role: "Containers",
            label: if docker.kind == "compose" {
                "Docker Compose".into()
            } else {
                "Dockerfile (sem Compose)".into()
            },
        });
    }
    Composition {
        headline: headline.join(" · "),
        parts,
    }
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
    } else if root.join("src-tauri").join("Cargo.toml").is_file() {
        add("rust", "Rust", "src-tauri/Cargo.toml".into());
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
    if detection.stack.iter().any(|s| s.id == "rust") {
        detection.rust = rust_project::detect(root);
    }
    if detection.stack.iter().any(|s| s.id == "tauri") {
        detection.tauri = Some(detect_tauri(root, pkg, &detection.scripts));
    }
    if detection.stack.iter().any(|s| s.id == "docker") {
        detection.docker = compose::detect_files(root);
    }
    detection.composition = compose_composition(&detection);
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

/// Executa um script do package.json local (API anterior, mantida): equivale a `node:<script>`.
/// Fronteira de confiança: o nome precisa existir no package.json LOCAL da pasta vinculada;
/// o programa é o gerenciador detectado (caminho absoluto do PATH); os argumentos são fixos.
pub fn launch_spec(project: &Project, script: &str) -> HubResult<LaunchSpec> {
    if !valid_script_name(script) {
        projects::local_dir(project)?; // unbound/missing explicam o motivo real primeiro
        return Err("Nome de script inválido.".into());
    }
    actions::resolve(project, &format!("node:{script}"), None)
}

// ------------------------------------------------------------------ snapshot

/// Nome do serviço numa porta. Só afirma um framework quando há evidência: ele está na stack E
/// o script de serviço do projeto cita a porta (`--port 1420`) ou a porta é a padrão do framework
/// sem porta explícita no script. Caso contrário o nome é genérico (nunca um palpite).
fn service_label(detection: &Detection, port: u16) -> String {
    // A porta que o próprio tauri.conf.json declara como dev server do frontend.
    if detection.tauri.as_ref().and_then(|t| t.dev_url_port) == Some(port) {
        return "Frontend (devUrl do Tauri)".into();
    }
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

// ------------------------------------------------------------------ Compose

/// Melhor estado entre as réplicas de um serviço (running > restarting > paused > created > exited).
fn best_container<'a>(containers: &[&'a Container]) -> Option<&'a Container> {
    let rank = |c: &Container| match c.state.as_str() {
        "running" => 0,
        "restarting" => 1,
        "paused" => 2,
        "created" => 3,
        _ => 4,
    };
    containers.iter().min_by_key(|c| rank(c)).copied()
}

/// O LKR LAB subiu o projeto nesta sessão e ninguém derrubou depois?
fn started_here(runs: &[RunInfo]) -> bool {
    runs.iter()
        .find(|r| {
            !r.observer
                && matches!(
                    r.command_id.as_str(),
                    "compose:up" | "compose:down" | "compose:restart"
                )
                && !r.state.is_active()
        })
        .is_some_and(|r| r.command_id != "compose:down" && r.state == RunState::Completed)
}

/// Estado do projeto Compose: serviços do `config` × containers reais do `ps`.
/// Sem Docker utilizável (ou daemon fechado) não há estado — a interface mostra o motivo,
/// nunca um estado deduzido do YAML.
fn compose_runtime(
    root: &Path,
    detection: &Detection,
    tools: &Tools,
    runs: &[RunInfo],
) -> Option<(ComposeRuntime, ComposeView)> {
    let docker = detection.docker.as_ref().filter(|d| d.kind == "compose")?;
    let file = docker.compose_files.first()?.clone();
    let mut runtime = ComposeRuntime {
        note: (docker.compose_files.len() > 1).then(|| {
            format!(
                "{} arquivos Compose na pasta; vale {file} (precedência do próprio Compose).",
                docker.compose_files.len()
            )
        }),
        file,
        override_files: docker.override_files.clone(),
        project_name: None,
        services: vec![],
        containers: 0,
        running: 0,
        expected: 0,
        started_here: started_here(runs),
        error: None,
    };
    let mut view = ComposeView::default();
    let docker_path = tools
        .docker
        .path
        .as_ref()
        .filter(|_| tools.docker.available && tools.compose.available);
    let Some(docker_path) = docker_path else {
        return Some((runtime, view));
    };
    let config: Option<ComposeConfig> = match compose::config(root, docker_path) {
        Ok(config) => Some(config),
        Err(error) => {
            runtime.error = Some(error.clone());
            view.config_error = Some(error);
            None
        }
    };
    let containers: Option<Vec<Container>> = if tools.daemon.available {
        match compose::ps(root, docker_path) {
            Ok(list) => Some(list),
            Err(error) => {
                runtime.error.get_or_insert(error);
                None
            }
        }
    } else {
        None
    };
    runtime.project_name = config.as_ref().and_then(|c| c.project_name.clone());
    // Sem config válida, o `ps` ainda diz o que existe de verdade (serviços sem portas/profiles).
    let mut declared: Vec<(String, Vec<ComposePort>, Vec<String>)> = match &config {
        Some(c) => c
            .services
            .iter()
            .map(|s| (s.name.clone(), s.ports.clone(), s.profiles.clone()))
            .collect(),
        None => vec![],
    };
    if config.is_none() {
        let mut names: Vec<&str> = containers
            .iter()
            .flatten()
            .map(|c| c.service.as_str())
            .collect();
        names.sort();
        names.dedup();
        declared = names
            .into_iter()
            .map(|n| (n.to_string(), vec![], vec![]))
            .collect();
    }
    for (name, ports, profiles) in declared {
        let mine: Vec<&Container> = containers
            .iter()
            .flatten()
            .filter(|c| c.service == name)
            .collect();
        let best = best_container(&mine);
        runtime.services.push(ComposeServiceRuntime {
            state: best
                .map(|c| c.state.clone())
                .unwrap_or_else(|| "absent".into()),
            health: best.and_then(|c| c.health.clone()),
            exit_code: best.and_then(|c| c.exit_code),
            name,
            ports,
            profiles,
        });
    }
    runtime.containers = containers.as_ref().map(Vec::len).unwrap_or(0);
    runtime.expected = runtime
        .services
        .iter()
        .filter(|s| s.profiles.is_empty())
        .count();
    runtime.running = runtime
        .services
        .iter()
        .filter(|s| s.profiles.is_empty() && s.state == "running")
        .count();
    view.services = runtime.services.iter().map(|s| s.name.clone()).collect();
    view.containers = containers.as_ref().map(Vec::len);
    Some((runtime, view))
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
        tools: vec![],
        commands: vec![],
        primary_command: None,
        compose: None,
        last_task: None,
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

    // STACK DETECTADA != FERRAMENTA DISPONÍVEL: sondas em cache, só do que o projeto usa.
    let needs = actions::needs(&runtime.detection);
    let tools = Tools::probe(&needs);
    let compose_view = compose_runtime(&root, &runtime.detection, &tools, &runtime.runs);
    let (compose, view) = match compose_view {
        Some((compose, view)) => (Some(compose), Some(view)),
        None => (None, None),
    };
    runtime.compose = compose;
    runtime.commands = actions::build_commands(&runtime.detection, &tools, &root, view.as_ref());
    runtime.primary_command = actions::primary(&runtime.commands, &runtime.detection);
    runtime.tools = tools.relevant(&needs);
    runtime.can_run = runtime.commands.iter().any(|c| c.available && !c.observer);
    if !runtime.can_run {
        runtime.run_blocked_reason = runtime
            .commands
            .iter()
            .find_map(|c| c.unavailable_reason.clone())
            .or_else(|| runtime.detection.package_manager_note.clone())
            .or_else(|| {
                Some(
                    if runtime.detection.scripts.is_empty()
                        && runtime.detection.package_manager.is_some()
                    {
                        "Nenhum script executável encontrado no package.json.".into()
                    } else {
                        "Nenhuma ação executável encontrada neste projeto.".into()
                    },
                )
            });
    }
    runtime.last_task = runtime
        .runs
        .iter()
        .find(|r| r.kind == ScriptKind::Task && !r.observer && !r.state.is_active())
        .map(|r| TaskResult {
            command: r.command.clone(),
            state: r.state,
            exit_code: r.exit_code,
        });

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
        .any(|r| r.state.is_active() && r.kind != ScriptKind::Task && !r.observer);
    let compose_up = runtime.compose.as_ref().is_some_and(|c| c.running > 0);
    let failed = runtime
        .runs
        .iter()
        .find(|r| !r.observer)
        .is_some_and(|r| r.state == RunState::Failed && r.kind != ScriptKind::Task);
    let declared_total = runtime.declared_ports.len();
    let declared_up = runtime
        .declared_ports
        .iter()
        .filter(|p| p.state == "listening")
        .count();
    let compose_partial = runtime
        .compose
        .as_ref()
        .is_some_and(|c| c.running > 0 && c.running < c.expected);
    if active || compose_up || runtime.external_running || !runtime.ports.is_empty() {
        if compose_partial {
            runtime.status = RuntimeStatus::Partial;
            runtime.status_detail = Some("Alguns serviços do Compose não estão ativos.".into());
        } else if declared_total > 0 && declared_up < declared_total && declared_up > 0 {
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
