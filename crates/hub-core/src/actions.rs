//! Ações do projeto: STACK → AÇÕES DISPONÍVEIS → COMANDO.
//!
//! O supervisor é um só (supervisor.rs). Esta camada decide O QUE pode ser executado:
//!
//! * os comandos nascem EXCLUSIVAMENTE de arquivos locais reais da pasta vinculada
//!   (package.json, Cargo.toml, tauri.conf.json, compose.yaml) e da toolchain detectada;
//! * `workspace.json`, `Project.commands` e qualquer metadado portátil são dados, nunca comandos;
//! * cada comando separa `program` e `args` (nada de string de shell) e carrega a sua
//!   disponibilidade: STACK DETECTADA != FERRAMENTA DISPONÍVEL;
//! * a interface só envia `id` (+ seleção). `resolve` recalcula tudo a partir do disco no
//!   momento de executar e rejeita qualquer id/seleção que não esteja na lista atual.
use crate::{
    compose,
    models::Project,
    projects,
    runtime::{self, Detection, LaunchSpec, Script, ScriptKind, TauriInfo},
    rust_project::{RustBin, RustInfo},
    tools::{Needs, ToolStatus, Tools},
    HubResult,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionGroup {
    /// Execução: rodar / subir / dev
    Run,
    /// Qualidade: testar / check / lint
    Quality,
    Build,
    /// Controle: parar / reiniciar
    Control,
}
impl ActionGroup {
    fn rank(self) -> u8 {
        match self {
            Self::Run => 0,
            Self::Quality => 1,
            Self::Build => 2,
            Self::Control => 3,
        }
    }
}

/// Opção de uma ação que precisa de escolha (binário Rust, serviço do Compose).
#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
    /// Argumentos extras. Fixos, derivados da detecção; nunca vindos da interface.
    #[serde(skip)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCommand {
    /// `<origem>:<nome>`: node:dev, cargo:run, tauri:dev, compose:up…
    pub id: String,
    pub label: String,
    /// Como o comando aparece no log ("cargo run --bin app"); nunca é executado como texto.
    pub detail: String,
    /// node | cargo | tauri | compose
    pub source: &'static str,
    pub program: String,
    pub args: Vec<String>,
    /// Relativo à raiz do projeto.
    pub cwd: String,
    pub group: ActionGroup,
    /// Service fica rodando; Task executa e termina.
    pub kind: ScriptKind,
    pub long_running: bool,
    /// Só observa (logs do Compose); não conta como "projeto em execução".
    pub observer: bool,
    pub choices: Vec<Choice>,
    /// Há mais de uma opção e nenhuma é a óbvia: a interface precisa perguntar.
    pub selection_required: bool,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    #[serde(skip)]
    pub program_path: Option<PathBuf>,
    #[serde(skip)]
    pub exclusive: Option<&'static str>,
}

impl RuntimeCommand {
    fn new(
        id: String,
        label: &str,
        source: &'static str,
        program: &str,
        args: Vec<String>,
        group: ActionGroup,
        kind: ScriptKind,
    ) -> Self {
        Self {
            detail: format!("{program} {}", args.join(" ")),
            id,
            label: label.into(),
            source,
            program: program.into(),
            args,
            cwd: ".".into(),
            group,
            kind,
            long_running: kind == ScriptKind::Service,
            observer: false,
            choices: vec![],
            selection_required: false,
            available: true,
            unavailable_reason: None,
            program_path: None,
            exclusive: None,
        }
    }
    fn gate(mut self, gate: Result<PathBuf, String>) -> Self {
        match gate {
            Ok(path) => self.program_path = Some(path),
            Err(reason) => {
                self.available = false;
                self.unavailable_reason = Some(reason);
            }
        }
        self
    }
    fn in_dir(mut self, dir: &str) -> Self {
        self.cwd = dir.into();
        self
    }
}

/// O que o Compose sabe agora (para montar disponibilidade e escolhas).
#[derive(Debug, Clone, Default)]
pub struct ComposeView {
    pub services: Vec<String>,
    /// `None` = não se sabe (daemon fechado): não bloqueia por falta de containers.
    pub containers: Option<usize>,
    pub config_error: Option<String>,
}

pub fn needs(d: &Detection) -> Needs {
    Needs {
        rust: d.rust.is_some(),
        compose: d.docker.as_ref().is_some_and(|x| x.kind == "compose"),
        tauri_cargo_cli: d
            .tauri
            .as_ref()
            .is_some_and(|t| !t.has_script && !t.local_cli),
        package_manager: d.package_manager.as_ref().map(|m| m.name),
    }
}

fn tool_gate(tool: &ToolStatus) -> Result<PathBuf, String> {
    if !tool.available {
        return Err(tool
            .reason
            .clone()
            .unwrap_or_else(|| format!("{} indisponível.", tool.label)));
    }
    tool.path
        .clone()
        .ok_or_else(|| format!("{} sem caminho resolvido.", tool.label))
}

// ------------------------------------------------------------------ Node

fn script_group(script: &Script) -> ActionGroup {
    match script.name.split(':').next().unwrap_or("") {
        "test" | "lint" | "format" | "fmt" | "typecheck" | "check" => ActionGroup::Quality,
        "build" | "clean" | "bundle" | "package" => ActionGroup::Build,
        _ => ActionGroup::Run,
    }
}

fn node_commands(d: &Detection, tools: &Tools) -> Vec<RuntimeCommand> {
    d.scripts
        .iter()
        // `npm run tauri` sozinho só imprime a ajuda da CLI: quem aparece é Tauri Dev / Tauri Build.
        .filter(|s| !(d.tauri.is_some() && s.name == "tauri"))
        .map(|s| {
            let (program, gate) = match (&d.package_manager, &tools.package_manager) {
                (Some(pm), Some(tool)) => (pm.name, tool_gate(tool)),
                (Some(pm), None) => (pm.name, Err(format!("{} não verificado.", pm.name))),
                (None, _) => (
                    "npm",
                    Err(d
                        .package_manager_note
                        .clone()
                        .unwrap_or_else(|| "Gerenciador de pacotes não identificado.".into())),
                ),
            };
            RuntimeCommand::new(
                format!("node:{}", s.name),
                &s.name,
                "node",
                program,
                vec!["run".into(), s.name.clone()],
                script_group(s),
                s.kind,
            )
            .gate(gate)
        })
        .collect()
}

// ------------------------------------------------------------------ Rust / Cargo

fn cargo_run(rust: &RustInfo, cargo: &Result<PathBuf, String>) -> Option<RuntimeCommand> {
    // O app do Tauri não roda com `cargo run` (sem o dev server do frontend): quem roda é o Tauri Dev.
    let candidates: Vec<&RustBin> = rust.bins.iter().filter(|b| !b.is_tauri).collect();
    if candidates.is_empty() {
        return None;
    }
    let mut command = RuntimeCommand::new(
        "cargo:run".into(),
        "Rodar",
        "cargo",
        "cargo",
        vec!["run".into()],
        ActionGroup::Run,
        ScriptKind::Service,
    )
    .in_dir(&rust.dir)
    .gate(cargo.clone());
    let many_packages = rust.packages.len() > 1;
    if candidates.len() == 1 {
        if rust.workspace && (many_packages || rust.virtual_manifest) {
            let bin = candidates[0];
            command.args = vec![
                "run".into(),
                "--package".into(),
                bin.package.clone(),
                "--bin".into(),
                bin.name.clone(),
            ];
        }
    } else if !rust.workspace
        && rust
            .default_run
            .as_ref()
            .is_some_and(|d| candidates.iter().any(|b| &b.name == d))
    {
        // O próprio cargo resolve pelo default-run do Cargo.toml.
    } else {
        command.selection_required = true;
        command.choices = candidates
            .iter()
            .map(|bin| {
                let (id, args) = if many_packages {
                    (
                        format!("{}/{}", bin.package, bin.name),
                        vec![
                            "--package".to_string(),
                            bin.package.clone(),
                            "--bin".into(),
                            bin.name.clone(),
                        ],
                    )
                } else {
                    (
                        bin.name.clone(),
                        vec!["--bin".to_string(), bin.name.clone()],
                    )
                };
                Choice {
                    label: id.replace('/', " › "),
                    id,
                    args,
                }
            })
            .collect();
    }
    command.detail = format!("cargo {}", command.args.join(" "));
    Some(command)
}

fn cargo_commands(rust: &RustInfo, tools: &Tools) -> Vec<RuntimeCommand> {
    let cargo = tool_gate(&tools.cargo);
    let mut list: Vec<RuntimeCommand> = cargo_run(rust, &cargo).into_iter().collect();
    for (name, label, group) in [
        ("build", "Build", ActionGroup::Build),
        ("test", "Testar", ActionGroup::Quality),
        ("check", "Check", ActionGroup::Quality),
        ("clippy", "Clippy", ActionGroup::Quality),
    ] {
        let mut command = RuntimeCommand::new(
            format!("cargo:{name}"),
            label,
            "cargo",
            "cargo",
            vec![name.into()],
            group,
            ScriptKind::Task,
        )
        .in_dir(&rust.dir);
        command = if name == "clippy" {
            // O programa é o cargo; a disponibilidade depende do componente clippy.
            // Sem Cargo, o motivo útil é o Cargo (a causa), não "o Clippy depende do Cargo".
            match (&cargo, tool_gate(&tools.clippy)) {
                (Err(reason), _) => command.gate(Err(reason.clone())),
                (Ok(_), Err(reason)) => command.gate(Err(reason)),
                (Ok(path), Ok(_)) => command.gate(Ok(path.clone())),
            }
        } else {
            command.gate(cargo.clone())
        };
        list.push(command);
    }
    list
}

// ------------------------------------------------------------------ Tauri

struct TauriRoute {
    program: String,
    path: PathBuf,
    /// Argumentos antes do subcomando (`dev` / `build`).
    prefix: Vec<String>,
}

/// Como chamar a CLI do Tauri, em ordem de preferência: script "tauri" do package.json (caminho
/// oficial), CLI local de node_modules e, só se existir de fato, o subcomando `cargo tauri`.
/// Nunca depende de uma CLI global sem que ela tenha sido encontrada.
fn tauri_route(
    d: &Detection,
    t: &TauriInfo,
    tools: &Tools,
    root: &Path,
) -> Result<TauriRoute, String> {
    let mut first_error: Option<String> = None;
    if t.has_script {
        match (&d.package_manager, &tools.package_manager) {
            (Some(pm), Some(tool)) => match tool_gate(tool) {
                Ok(path) => {
                    let script = d.scripts.iter().find(|s| s.name == "tauri");
                    let calls_local_cli =
                        script.is_some_and(|s| s.command.trim_start().starts_with("tauri"));
                    if calls_local_cli && !t.local_cli {
                        first_error = Some(format!(
                            "O Tauri está configurado no package.json, mas a CLI local não está instalada (node_modules ausente). Rode “{} install”.",
                            pm.name
                        ));
                    } else {
                        let prefix = if pm.name == "npm" {
                            vec!["run", "tauri", "--"]
                        } else {
                            vec!["run", "tauri"]
                        };
                        return Ok(TauriRoute {
                            program: pm.name.into(),
                            path,
                            prefix: prefix.into_iter().map(String::from).collect(),
                        });
                    }
                }
                Err(reason) => first_error = Some(reason),
            },
            (None, _) => {
                first_error = Some(
                    d.package_manager_note
                        .clone()
                        .unwrap_or_else(|| "Gerenciador de pacotes não identificado.".into()),
                );
            }
            (Some(pm), None) => first_error = Some(format!("{} não verificado.", pm.name)),
        }
    }
    if t.local_cli {
        let names: &[&str] = if cfg!(windows) {
            &["tauri.cmd", "tauri.exe"]
        } else {
            &["tauri"]
        };
        let found = names
            .iter()
            .map(|n| root.join("node_modules").join(".bin").join(n))
            .find(|p| p.is_file());
        // Um symlink em node_modules não pode apontar a execução para fora do projeto.
        if let Some(path) = found.filter(|p| runtime::inside(root, p)) {
            return Ok(TauriRoute {
                program: "tauri".into(),
                path,
                prefix: vec![],
            });
        }
    }
    if tools.cargo_tauri.available {
        if let Ok(path) = tool_gate(&tools.cargo) {
            return Ok(TauriRoute {
                program: "cargo".into(),
                path,
                prefix: vec!["tauri".into()],
            });
        }
    }
    Err(first_error.unwrap_or_else(|| {
        "Tauri detectado, mas nenhuma CLI utilizável foi encontrada. Instale @tauri-apps/cli no projeto (npm i -D @tauri-apps/cli) ou rode “cargo install tauri-cli”.".into()
    }))
}

fn tauri_commands(d: &Detection, t: &TauriInfo, tools: &Tools, root: &Path) -> Vec<RuntimeCommand> {
    let route = tauri_route(d, t, tools, root);
    [
        ("dev", "Tauri Dev", ActionGroup::Run, ScriptKind::Service),
        ("build", "Tauri Build", ActionGroup::Build, ScriptKind::Task),
    ]
    .into_iter()
    .map(|(sub, label, group, kind)| match &route {
        Ok(route) => {
            let mut args = route.prefix.clone();
            args.push(sub.into());
            RuntimeCommand::new(
                format!("tauri:{sub}"),
                label,
                "tauri",
                &route.program,
                args,
                group,
                kind,
            )
            .gate(Ok(route.path.clone()))
        }
        Err(reason) => RuntimeCommand::new(
            format!("tauri:{sub}"),
            label,
            "tauri",
            "tauri",
            vec![sub.into()],
            group,
            kind,
        )
        .gate(Err(reason.clone())),
    })
    .collect()
}

// ------------------------------------------------------------------ Docker Compose

fn compose_commands(
    d: &Detection,
    tools: &Tools,
    view: Option<&ComposeView>,
) -> Vec<RuntimeCommand> {
    if !d.docker.as_ref().is_some_and(|x| x.kind == "compose") {
        return vec![];
    }
    let base = tool_gate(&tools.docker)
        .and_then(|path| tool_gate(&tools.compose).map(|_| path))
        .and_then(|path| tool_gate(&tools.daemon).map(|_| path))
        .and_then(|path| match view.and_then(|v| v.config_error.clone()) {
            Some(error) => Err(error),
            None => Ok(path),
        });
    let with_containers =
        |gate: Result<PathBuf, String>| match (&gate, view.and_then(|v| v.containers)) {
            (Ok(_), Some(0)) => Err("Nenhum container deste projeto no momento.".to_string()),
            _ => gate,
        };
    let compose = |id: &str, label: &str, args: &[&str], group, kind| {
        let mut command = RuntimeCommand::new(
            format!("compose:{id}"),
            label,
            "compose",
            "docker",
            ["compose"]
                .iter()
                .chain(args)
                .map(|s| s.to_string())
                .collect(),
            group,
            kind,
        );
        command.exclusive = Some("compose");
        command
    };
    let mut logs = compose(
        "logs",
        "Logs ao vivo",
        &["logs", "--follow", "--tail", "200"],
        ActionGroup::Run,
        ScriptKind::Service,
    );
    logs.observer = true;
    logs.exclusive = None;
    logs.choices = view
        .map(|v| {
            v.services
                .iter()
                .filter(|s| compose::valid_service_name(s))
                .map(|s| Choice {
                    id: s.clone(),
                    label: s.clone(),
                    args: vec![s.clone()],
                })
                .collect()
        })
        .unwrap_or_default();
    vec![
        compose(
            "up",
            "Subir containers",
            &["up", "-d"],
            ActionGroup::Run,
            ScriptKind::Task,
        )
        .gate(base.clone()),
        logs.gate(with_containers(base.clone())),
        compose(
            "restart",
            "Reiniciar containers",
            &["restart"],
            ActionGroup::Control,
            ScriptKind::Task,
        )
        .gate(with_containers(base.clone())),
        compose(
            "down",
            "Parar containers",
            &["down"],
            ActionGroup::Control,
            ScriptKind::Task,
        )
        .gate(with_containers(base)),
    ]
}

// ------------------------------------------------------------------ montagem

/// Todas as ações do projeto, já com disponibilidade. Pura: não toca disco nem processos
/// (a detecção, as ferramentas e o estado do Compose chegam por parâmetro), então é testável
/// sem Cargo, Docker ou Tauri instalados.
pub fn build_commands(
    d: &Detection,
    tools: &Tools,
    root: &Path,
    compose: Option<&ComposeView>,
) -> Vec<RuntimeCommand> {
    let mut list = Vec::new();
    if let Some(tauri) = &d.tauri {
        list.extend(tauri_commands(d, tauri, tools, root));
    }
    list.extend(node_commands(d, tools));
    if let Some(rust) = &d.rust {
        list.extend(cargo_commands(rust, tools));
    }
    list.extend(compose_commands(d, tools, compose));
    list.sort_by_key(|c| c.group.rank()); // estável: mantém a ordem de cada origem dentro do grupo
    list
}

/// Ação principal, só quando não há ambiguidade. Tauri manda: o fluxo oficial é `tauri dev`,
/// não o `npm run dev` do frontend. Sem Tauri, só uma origem com ação de execução pode ter
/// principal; com duas (ex.: Node + Rust) o botão vira "Rodar ▾".
pub fn primary(commands: &[RuntimeCommand], d: &Detection) -> Option<String> {
    if d.tauri.is_some() {
        if let Some(command) = commands.iter().find(|c| c.id == "tauri:dev") {
            return Some(command.id.clone());
        }
    }
    let mut candidates: Vec<&RuntimeCommand> = Vec::new();
    candidates.extend(
        commands.iter().find(|c| {
            c.source == "node" && c.group == ActionGroup::Run && c.kind != ScriptKind::Task
        }),
    );
    candidates.extend(commands.iter().find(|c| c.id == "cargo:run"));
    candidates.extend(commands.iter().find(|c| c.id == "compose:up"));
    match candidates.as_slice() {
        [only] if !only.selection_required => Some(only.id.clone()),
        _ => None,
    }
}

// ------------------------------------------------------------------ execução

fn valid_command_id(id: &str) -> bool {
    let Some((source, name)) = id.split_once(':') else {
        return false;
    };
    (2..=10).contains(&source.len())
        && source.chars().all(|c| c.is_ascii_lowercase())
        && name.len() <= 64
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '.' | '-'))
}

/// Valida tudo para executar `command_id` e devolve o comando a rodar.
///
/// Fronteira de confiança: o id e a seleção que chegam da interface são só CHAVES. A lista de
/// comandos é recalculada agora, dos arquivos locais da pasta vinculada; id fora dela, seleção
/// fora das opções detectadas ou ação indisponível são recusados. O programa é sempre um caminho
/// absoluto resolvido no PATH (ou dentro do projeto) e os argumentos são fixos.
pub fn resolve(
    project: &Project,
    command_id: &str,
    selection: Option<&str>,
) -> HubResult<LaunchSpec> {
    let local = projects::local_dir(project)?; // recusa unbound e missing
    let root = runtime::plain_path(
        local
            .canonicalize()
            .map_err(|_| "A pasta do projeto não está acessível.".to_string())?,
    );
    if !valid_command_id(command_id) {
        return Err("Ação inválida.".into());
    }
    let selection = selection.filter(|s| !s.is_empty());
    if selection.is_some_and(|s| s.len() > 128 || s.chars().any(|c| c.is_control())) {
        return Err("Seleção inválida.".into());
    }
    let detection = runtime::detect(&root);
    let tools = Tools::probe(&needs(&detection));
    let view = command_id.starts_with("compose:").then(|| {
        let mut view = ComposeView::default();
        if let (true, Some(docker)) = (
            tools.docker.available && tools.compose.available,
            tools.docker.path.as_ref(),
        ) {
            match compose::config(&root, docker) {
                Ok(config) => view.services = config.services.into_iter().map(|s| s.name).collect(),
                Err(error) => view.config_error = Some(error),
            }
        }
        view
    });
    let commands = build_commands(&detection, &tools, &root, view.as_ref());
    let command = commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| match command_id.strip_prefix("node:") {
            Some(script) => {
                format!("O script “{script}” não existe no package.json deste projeto.")
            }
            None => "Esta ação não está disponível neste projeto.".to_string(),
        })?;
    if !command.available {
        return Err(command
            .unavailable_reason
            .clone()
            .unwrap_or_else(|| "Ação indisponível.".into()));
    }
    let mut args = command.args.clone();
    let mut chosen = None;
    match selection {
        Some(id) => {
            let choice = command
                .choices
                .iter()
                .find(|c| c.id == id)
                .ok_or_else(|| "Seleção desconhecida para esta ação.".to_string())?;
            args.extend(choice.args.iter().cloned());
            chosen = Some(choice.id.clone());
        }
        None if command.selection_required => {
            return Err(format!(
                "Há mais de uma opção para “{}”: escolha qual executar.",
                command.label
            ));
        }
        None => {}
    }
    let program = command
        .program_path
        .clone()
        .ok_or_else(|| "Programa não resolvido.".to_string())?;
    let cwd = if command.cwd == "." {
        root.clone()
    } else {
        let dir = root.join(&command.cwd);
        if !runtime::inside(&root, &dir) {
            return Err("Pasta de execução fora do projeto.".into());
        }
        runtime::plain_path(
            dir.canonicalize()
                .map_err(|_| "Pasta de execução inacessível.".to_string())?,
        )
    };
    Ok(LaunchSpec {
        display: format!("{} {}", command.program, args.join(" ")),
        program,
        args,
        cwd,
        kind: command.kind,
        root,
        command_id: command.id.clone(),
        name: command
            .id
            .split_once(':')
            .map(|(_, n)| n.to_string())
            .unwrap_or_default(),
        label: command.label.clone(),
        source: command.source,
        selection: chosen,
        observer: command.observer,
        exclusive: command.exclusive,
    })
}
