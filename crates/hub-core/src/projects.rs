use crate::{
    models::{Location, Project, ProjectEntry, ProjectInput},
    HubResult,
};
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
/// Observação desta máquina: a pasta cadastrada existe aqui? Calculada a cada
/// leitura e nunca persistida, para que um cadastro vindo de outra máquina não
/// pareça válido só porque o caminho foi salvo.
pub fn path_available(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute() && path.is_dir()
}
pub fn location(project: &Project) -> Location {
    if project.local_path.is_empty() {
        Location::Unbound
    } else if path_available(&project.local_path) {
        Location::Available
    } else {
        Location::Missing
    }
}
pub fn entry(project: Project) -> ProjectEntry {
    let location = location(&project);
    ProjectEntry { project, location }
}
/// Única porta para a pasta do projeto: nunca devolve caminho vazio ou inexistente.
pub fn local_dir(project: &Project) -> HubResult<PathBuf> {
    match location(project) {
        Location::Available => Ok(PathBuf::from(&project.local_path)),
        Location::Missing => Err(format!(
            "A pasta vinculada a {} não existe nesta máquina ({}). Use “Localizar”.",
            project.name, project.local_path
        )),
        Location::Unbound => Err(format!(
            "{} faz parte do workspace, mas ainda não foi localizado nesta máquina. Use “Localizar”.",
            project.name
        )),
    }
}
pub fn validate(mut p: ProjectInput) -> HubResult<ProjectInput> {
    p.name = p.name.trim().to_string();
    p.stack.retain(|s| !s.trim().is_empty());
    p.tags.retain(|s| !s.trim().is_empty());
    if p.name.is_empty() || p.name.len() > 100 || p.description.len() > 4000 {
        return Err("Nome obrigatório (até 100 caracteres); descrição até 4000.".into());
    }
    // Vazio só é aceito ao editar (Database::save exige pasta no cadastro novo).
    if !p.local_path.is_empty() {
        p.local_path = canonical(&p.local_path)?.to_string_lossy().to_string();
    }
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
/// Compatível com a API anterior; a detecção é a MESMA da inspeção do cadastro (`inspect`),
/// que por sua vez usa `runtime::detect`: não existe mais uma segunda regra de stack.
pub fn discover(path: &str) -> HubResult<Discovery> {
    let inspection = crate::inspect::inspect_folder(path, &[]);
    if !inspection.valid {
        return Err(inspection.error.unwrap_or_else(|| "Pasta inválida.".into()));
    }
    Ok(Discovery {
        local_path: inspection.folder,
        name: inspection.suggested_name,
        repository: inspection.repository,
        stack: inspection
            .stack
            .iter()
            .map(|s| s.label.to_string())
            .collect(),
        is_git: inspection.git.is_some(),
    })
}
