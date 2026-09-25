use crate::{commands, models::ProjectInput, HubResult};
use serde::Serialize;
use std::path::{Path, PathBuf};
pub fn slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
pub fn canonical(path: &str) -> HubResult<PathBuf> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("Selecione um caminho absoluto.".into());
    }
    let path = path
        .canonicalize()
        .map_err(|_| "Pasta não encontrada ou sem acesso.".to_string())?;
    if !path.is_dir() {
        return Err("O caminho precisa ser uma pasta.".into());
    }
    Ok(path)
}
pub fn validate(mut p: ProjectInput) -> HubResult<ProjectInput> {
    p.name = p.name.trim().to_string();
    p.stack.retain(|s| !s.trim().is_empty());
    p.tags.retain(|s| !s.trim().is_empty());
    if p.name.is_empty() || p.name.len() > 100 || p.description.len() > 4000 {
        return Err("Nome obrigatório (até 100 caracteres); descrição até 4000.".into());
    }
    p.local_path = canonical(&p.local_path)?.to_string_lossy().to_string();
    if !p.repository.is_empty() && !valid_repository(&p.repository) {
        return Err("Use URL HTTPS de repositório, sem credenciais ou parâmetros.".into());
    }
    if p.ports
        .iter()
        .any(|p| p.port == 0 || p.name.trim().is_empty())
    {
        return Err("Portas devem estar entre 1 e 65535 e possuir nome.".into());
    }
    let mut seen = std::collections::HashSet::new();
    if p.ports.iter().any(|p| !seen.insert(p.port)) {
        return Err("Portas duplicadas no projeto.".into());
    }
    if p.commands.len() > 30 || p.stack.len() > 30 || p.tags.len() > 30 || p.ports.len() > 30 {
        return Err("Máximo 30 itens por lista.".into());
    }
    if p.commands.iter().any(|c| {
        c.name.len() > 100
            || c.program.len() > 200
            || c.args.len() > 50
            || c.args.iter().any(|a| a.len() > 1000)
    }) {
        return Err("Comando excede limite de tamanho.".into());
    }
    Ok(p)
}
pub fn valid_repository(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let Some((host, path)) = rest.split_once('/') else {
        return false;
    };
    !host.is_empty()
        && !path.is_empty()
        && !url.contains(['@', '?', '#', '\\'])
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub local_path: String,
    pub name: String,
    pub repository: String,
    pub stack: Vec<String>,
    pub is_git: bool,
}
pub fn discover(path: &str) -> HubResult<Discovery> {
    let path = canonical(path)?;
    let mut stack = Vec::new();
    if path.join("package.json").is_file() {
        stack.push("Node.js".into());
        let package = path.join("package.json");
        if let Ok(text) = std::fs::metadata(&package)
            .ok()
            .filter(|m| m.len() < 1_000_000)
            .ok_or(())
            .and_then(|_| std::fs::read_to_string(&package).map_err(|_| ()))
        {
            if text.len() < 1_000_000 {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    for (key, label) in [
                        ("react", "React"),
                        ("typescript", "TypeScript"),
                        ("next", "Next.js"),
                    ] {
                        if v["dependencies"][key].is_string()
                            || v["devDependencies"][key].is_string()
                        {
                            stack.push(label.into());
                        }
                    }
                }
            }
        }
    }
    for (file, label) in [
        ("Cargo.toml", "Rust"),
        ("requirements.txt", "Python"),
        ("pyproject.toml", "Python"),
        ("docker-compose.yml", "Docker"),
        ("compose.yaml", "Docker"),
        ("appsscript.json", "Apps Script"),
        ("foundry.toml", "Solidity"),
    ] {
        if path.join(file).is_file() && !stack.contains(&label.to_string()) {
            stack.push(label.into());
        }
    }
    let remote =
        commands::run("git", &["remote", "get-url", "origin"], Some(&path)).unwrap_or_default();
    let remote = remote
        .strip_prefix("git@github.com:")
        .map(|r| format!("https://github.com/{r}"))
        .unwrap_or(remote);
    let repository = if valid_repository(&remote) {
        remote.trim_end_matches(".git").to_string()
    } else {
        String::new()
    };
    let is_git = commands::run("git", &["rev-parse", "--is-inside-work-tree"], Some(&path)).is_ok();
    Ok(Discovery {
        local_path: path.to_string_lossy().to_string(),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        repository,
        stack,
        is_git,
    })
}
