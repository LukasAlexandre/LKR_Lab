//! Bounded, shell-free, read-only subprocesses. Output never enters logs automatically.
use crate::HubResult;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use wait_timeout::ChildExt;
const LIMIT: u64 = 512 * 1024;
pub fn executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path).filter(|p| p.is_absolute()) {
        let candidate = dir.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        });
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
/// Ferramenta de desenvolvimento no PATH (só diretórios absolutos). No Windows aceita o
/// shim .cmd do npm/pnpm/yarn; o chamador garante que os argumentos são fixos e validados.
pub fn resolve_tool(name: &str) -> Option<PathBuf> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    let suffixes: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd"]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(&path).filter(|p| p.is_absolute()) {
        for suffix in suffixes {
            let candidate = dir.join(format!("{name}{suffix}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
pub fn run(program: &str, args: &[&str], cwd: Option<&Path>) -> HubResult<String> {
    let exe = executable(program).ok_or_else(|| {
        format!("{program} não encontrado no PATH. Instale a CLI e reinicie o aplicativo.")
    })?;
    let mut cmd = Command::new(exe);
    if program == "git" {
        cmd.args(["-c", "core.fsmonitor=false"]);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_OPTIONAL_LOCKS", "0");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|_| format!("Falha ao iniciar {program}."))?;
    let stdout = child.stdout.take().ok_or("Saída indisponível")?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let status = match child
        .wait_timeout(Duration::from_secs(12))
        .map_err(|e| e.to_string())?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{program}: tempo limite de 12 segundos excedido."));
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "Leitura interrompida")?
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT as usize {
        return Err(format!("{program}: saída excedeu o limite de segurança."));
    }
    if !status.success() {
        return Err(format!("{program}: comando indisponível ou falhou (código {:?}). Verifique instalação, autenticação e repositório.", status.code()));
    }
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}
