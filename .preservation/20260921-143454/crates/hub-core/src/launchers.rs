use crate::{commands, models::Project, HubResult};
use std::process::Command;
fn spawn(mut cmd: Command) -> HubResult<()> {
    cmd.spawn()
        .map_err(|e| format!("Não foi possível abrir o aplicativo: {e}"))?;
    Ok(())
}
pub fn launch(p: &Project, action: &str) -> HubResult<()> {
    let path = crate::projects::canonical(&p.local_path)?;
    match action {
        "folder" => {
            #[cfg(windows)]
            {
                let mut c = Command::new("explorer.exe");
                c.arg(path);
                spawn(c)
            }
            #[cfg(not(windows))]
            {
                let mut c = Command::new("xdg-open");
                c.arg(path);
                spawn(c)
            }
        }
        "terminal" | "claude" => {
            #[cfg(windows)]
            {
                let exe = commands::executable("wt")
                    .ok_or("Windows Terminal não encontrado. Instale-o e reinicie o aplicativo.")?;
                let mut c = Command::new(exe);
                c.arg("-d")
                    .arg(path)
                    .arg("powershell.exe")
                    .arg("-NoLogo")
                    .arg("-NoProfile")
                    .arg("-NoExit");
                if action == "claude" {
                    commands::executable("claude")
                        .ok_or("Claude CLI nativa não encontrada no PATH.")?;
                    c.arg("-Command").arg("& claude.exe");
                }
                spawn(c)
            }
            #[cfg(not(windows))]
            {
                Err("Launcher de terminal/Claude implementado para Windows; não disponível nesta plataforma.".into())
            }
        }
        "vscode" => {
            let exe=commands::executable("code").ok_or("code.exe não encontrado no PATH. Adicione a pasta do executável VS Code ao PATH (o shim code.cmd não é executado).")?;
            let mut c = Command::new(exe);
            c.arg("--new-window").arg(path);
            spawn(c)
        }
        "github" => open_url(&p.repository),
        _ => Err("Ação não permitida.".into()),
    }
}
pub fn open_url(url: &str) -> HubResult<()> {
    let localhost = url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|p| p.parse::<u16>().ok())
        .is_some_and(|p| p > 0);
    if !localhost && !crate::projects::valid_repository(url) {
        return Err("URL não permitida.".into());
    }
    #[cfg(windows)]
    {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler").arg(url);
        spawn(c)
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        spawn(c)
    }
}
