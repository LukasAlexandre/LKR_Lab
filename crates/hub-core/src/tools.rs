//! Disponibilidade das ferramentas que o Runtime usa (cargo, rustc, clippy, docker, compose,
//! daemon do Docker, gerenciador de pacotes).
//!
//! STACK DETECTADA != FERRAMENTA DISPONÍVEL: o projeto pode ser Rust sem cargo no PATH, ou ter
//! Compose com o Docker Desktop fechado. Cada ferramenta tem estado próprio, motivo pronto para
//! a interface e resultado em cache (versões não mudam a cada 5 s). As sondas rodam sem shell,
//! com limite de tempo e de saída, e SEMPRE fora da pasta do projeto: o rustup respeita
//! `rust-toolchain.toml` do diretório atual e poderia baixar um toolchain só por causa da sondagem.
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};
use wait_timeout::ChildExt;

const PRESENCE_TTL: Duration = Duration::from_secs(60);
const DAEMON_TTL: Duration = Duration::from_secs(6);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub available: bool,
    pub version: Option<String>,
    /// Por que não está disponível, em linguagem de produto.
    pub reason: Option<String>,
    /// Caminho absoluto resolvido no PATH (nunca exposto à interface).
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl ToolStatus {
    pub fn ok(
        id: &'static str,
        label: &'static str,
        version: Option<String>,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            id,
            label,
            available: true,
            version,
            reason: None,
            path,
        }
    }
    pub fn missing(id: &'static str, label: &'static str, reason: impl Into<String>) -> Self {
        Self {
            id,
            label,
            available: false,
            version: None,
            reason: Some(reason.into()),
            path: None,
        }
    }
    /// Ferramenta que o projeto atual não precisa: não é sondada.
    pub fn not_needed(id: &'static str, label: &'static str) -> Self {
        Self::missing(
            id,
            label,
            "Não verificado: o projeto não usa esta ferramenta.",
        )
    }
}

/// Foto das ferramentas relevantes para um projeto.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    pub cargo: ToolStatus,
    pub rustc: ToolStatus,
    pub clippy: ToolStatus,
    pub docker: ToolStatus,
    pub compose: ToolStatus,
    pub daemon: ToolStatus,
    pub package_manager: Option<ToolStatus>,
    pub cargo_tauri: ToolStatus,
}

impl Tools {
    /// Tudo "não verificado". Base para sondagem real e para fixtures de teste.
    pub fn unchecked() -> Self {
        Self {
            cargo: ToolStatus::not_needed("cargo", "Cargo"),
            rustc: ToolStatus::not_needed("rustc", "rustc"),
            clippy: ToolStatus::not_needed("clippy", "Clippy"),
            docker: ToolStatus::not_needed("docker", "Docker CLI"),
            compose: ToolStatus::not_needed("compose", "Docker Compose"),
            daemon: ToolStatus::not_needed("daemon", "Docker Desktop"),
            package_manager: None,
            cargo_tauri: ToolStatus::not_needed("cargo-tauri", "cargo-tauri"),
        }
    }
    /// Todas as ferramentas de Rust e Docker disponíveis (fixtures determinísticos).
    pub fn all_available() -> Self {
        let ok =
            |id, label| ToolStatus::ok(id, label, Some("teste".into()), Some(PathBuf::from(id)));
        Self {
            cargo: ok("cargo", "Cargo"),
            rustc: ok("rustc", "rustc"),
            clippy: ok("clippy", "Clippy"),
            docker: ok("docker", "Docker CLI"),
            compose: ok("compose", "Docker Compose"),
            daemon: ok("daemon", "Docker Desktop"),
            package_manager: None,
            cargo_tauri: ToolStatus::missing(
                "cargo-tauri",
                "cargo-tauri",
                "cargo-tauri não instalado.",
            ),
        }
    }
    /// Define o gerenciador de pacotes (fixtures determinísticos).
    pub fn with_package_manager(mut self, status: ToolStatus) -> Self {
        self.package_manager = Some(status);
        self
    }
    /// Só as ferramentas que importam para este projeto, na ordem em que a interface as mostra.
    pub fn relevant(&self, needs: &Needs) -> Vec<ToolStatus> {
        let mut list = Vec::new();
        if let Some(pm) = &self.package_manager {
            list.push(pm.clone());
        }
        if needs.rust {
            list.extend([self.cargo.clone(), self.rustc.clone(), self.clippy.clone()]);
        }
        if needs.tauri_cargo_cli {
            list.push(self.cargo_tauri.clone());
        }
        if needs.compose {
            list.extend([
                self.docker.clone(),
                self.compose.clone(),
                self.daemon.clone(),
            ]);
        }
        list
    }
    /// Sonda só o que o projeto usa (resultado em cache).
    pub fn probe(needs: &Needs) -> Self {
        let mut tools = Self::unchecked();
        if needs.rust {
            tools.cargo = cached("cargo", PRESENCE_TTL, probe_cargo);
            tools.rustc = cached("rustc", PRESENCE_TTL, probe_rustc);
            if tools.cargo.available {
                tools.clippy = cached("clippy", PRESENCE_TTL, probe_clippy);
            } else {
                tools.clippy = ToolStatus::missing(
                    "clippy",
                    "Clippy",
                    "Depende do Cargo, que não está disponível.",
                );
            }
        }
        if needs.tauri_cargo_cli {
            tools.cargo_tauri = cached("cargo-tauri", PRESENCE_TTL, probe_cargo_tauri);
        }
        if needs.compose {
            tools.docker = cached("docker", PRESENCE_TTL, probe_docker);
            if tools.docker.available {
                tools.compose = cached("compose", PRESENCE_TTL, probe_compose);
                tools.daemon = cached("daemon", DAEMON_TTL, probe_daemon);
            } else {
                let why = "Depende da Docker CLI, que não está disponível.";
                tools.compose = ToolStatus::missing("compose", "Docker Compose", why);
                tools.daemon = ToolStatus::missing("daemon", "Docker Desktop", why);
            }
        }
        if let Some(name) = needs.package_manager {
            tools.package_manager = Some(cached_pm(name));
        }
        tools
    }
}

/// O que o projeto precisa; vem da detecção passiva.
#[derive(Debug, Clone, Default)]
pub struct Needs {
    pub rust: bool,
    pub compose: bool,
    pub tauri_cargo_cli: bool,
    pub package_manager: Option<&'static str>,
}

// ---------------------------------------------------------------- cache

type Cache = Mutex<HashMap<String, (Instant, ToolStatus)>>;
fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
fn cached(id: &'static str, ttl: Duration, probe: fn() -> ToolStatus) -> ToolStatus {
    if let Some((at, status)) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(id) {
        if at.elapsed() < ttl {
            return status.clone();
        }
    }
    let status = probe();
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_string(), (Instant::now(), status.clone()));
    status
}
fn cached_pm(name: &'static str) -> ToolStatus {
    let key = format!("pm:{name}");
    if let Some((at, status)) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        if at.elapsed() < PRESENCE_TTL {
            return status.clone();
        }
    }
    let status = match crate::commands::resolve_tool(name) {
        Some(path) => ToolStatus::ok(name, name, None, Some(path)),
        None => ToolStatus::missing(
            name,
            name,
            format!("{name} não foi encontrado no PATH. Instale-o e reinicie o aplicativo."),
        ),
    };
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, (Instant::now(), status.clone()));
    status
}
/// Esquece as sondagens (usado depois de instalar/abrir uma ferramenta e nos testes).
pub fn forget() {
    cache().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

// ---------------------------------------------------------------- sondas

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(120).collect())
}
fn neutral_dir() -> PathBuf {
    std::env::temp_dir()
}
fn probe_with(
    id: &'static str,
    label: &'static str,
    tool: &str,
    args: &[&str],
    missing: &str,
    failed: &str,
) -> ToolStatus {
    let Some(path) = crate::commands::resolve_tool(tool) else {
        return ToolStatus::missing(id, label, missing);
    };
    match capture(&path, args, &neutral_dir(), PROBE_TIMEOUT, 16 * 1024) {
        Ok(out) if out.code == Some(0) => {
            ToolStatus::ok(id, label, first_line(&out.stdout), Some(path))
        }
        Ok(out) => ToolStatus::missing(
            id,
            label,
            format!("{failed} {}", first_line(&out.stderr).unwrap_or_default())
                .trim()
                .to_string(),
        ),
        Err(error) => ToolStatus::missing(id, label, format!("{failed} {error}")),
    }
}
fn probe_cargo() -> ToolStatus {
    probe_with(
        "cargo",
        "Cargo",
        "cargo",
        &["--version"],
        "Cargo não encontrado no PATH. Instale o Rust (rustup.rs) e reinicie o aplicativo.",
        "Cargo não respondeu:",
    )
}
fn probe_rustc() -> ToolStatus {
    probe_with(
        "rustc",
        "rustc",
        "rustc",
        &["--version"],
        "rustc não encontrado no PATH. Instale o Rust (rustup.rs) e reinicie o aplicativo.",
        "rustc não respondeu:",
    )
}
fn probe_clippy() -> ToolStatus {
    let Some(cargo) = crate::commands::resolve_tool("cargo") else {
        return ToolStatus::missing(
            "clippy",
            "Clippy",
            "Depende do Cargo, que não está disponível.",
        );
    };
    match capture(
        &cargo,
        &["clippy", "--version"],
        &neutral_dir(),
        PROBE_TIMEOUT,
        16 * 1024,
    ) {
        Ok(out) if out.code == Some(0) => {
            ToolStatus::ok("clippy", "Clippy", first_line(&out.stdout), Some(cargo))
        }
        _ => ToolStatus::missing(
            "clippy",
            "Clippy",
            "Clippy não está instalado neste toolchain (rustup component add clippy).",
        ),
    }
}
fn probe_cargo_tauri() -> ToolStatus {
    // Subcomando do cargo = executável `cargo-tauri` no PATH (inclui ~/.cargo/bin).
    match crate::commands::resolve_tool("cargo-tauri") {
        Some(path) => ToolStatus::ok("cargo-tauri", "cargo-tauri", None, Some(path)),
        None => ToolStatus::missing(
            "cargo-tauri",
            "cargo-tauri",
            "cargo-tauri não está instalado (cargo install tauri-cli).",
        ),
    }
}
fn probe_docker() -> ToolStatus {
    probe_with(
        "docker",
        "Docker CLI",
        "docker",
        &["--version"],
        "Docker CLI não encontrado no PATH. Instale o Docker Desktop e reinicie o aplicativo.",
        "A Docker CLI não respondeu:",
    )
}
fn probe_compose() -> ToolStatus {
    probe_with(
        "compose",
        "Docker Compose",
        "docker",
        &["compose", "version", "--short"],
        "Docker CLI não encontrado no PATH.",
        "O plugin Docker Compose (v2) não está disponível.",
    )
}
fn probe_daemon() -> ToolStatus {
    let Some(path) = crate::commands::resolve_tool("docker") else {
        return ToolStatus::missing(
            "daemon",
            "Docker Desktop",
            "Docker CLI não encontrado no PATH.",
        );
    };
    match capture(&path, &["version", "--format", "{{.Server.Version}}"], &neutral_dir(), Duration::from_secs(4), 4 * 1024) {
        Ok(out) if out.code == Some(0) && !out.stdout.trim().is_empty() => ToolStatus::ok("daemon", "Docker Desktop", first_line(&out.stdout), Some(path)),
        _ => ToolStatus::missing("daemon", "Docker Desktop", "O Docker Desktop não está disponível (o daemon não respondeu). Abra o Docker Desktop e tente de novo."),
    }
}

// ---------------------------------------------------------------- execução limitada

#[derive(Debug)]
pub struct Captured {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Executa `program` sem shell, com argumentos separados, tempo e saída limitados.
/// A saída volta ao chamador, que decide o que pode ser exibido: nunca é registrada aqui.
pub fn capture(
    program: &Path,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
    max: usize,
) -> Result<Captured, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1")
        .env("DOCKER_CLI_HINTS", "false")
        .env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("não foi possível iniciar: {e}"))?;
    let read = |stream: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(stream) = stream {
                let _ = stream.take(max as u64).read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let status = match child.wait_timeout(timeout).map_err(|e| e.to_string())? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("tempo limite de {} s excedido", timeout.as_secs()));
        }
    };
    let text = |handle: thread::JoinHandle<Vec<u8>>| {
        String::from_utf8_lossy(&handle.join().unwrap_or_default()).to_string()
    };
    Ok(Captured {
        code: status.code(),
        stdout: text(out),
        stderr: text(err),
    })
}
