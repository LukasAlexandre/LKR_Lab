//! Inspeção PASSIVA de uma pasta (Concept 04) e reconhecimento de Project já conhecido.
//!
//! Uma só fonte de verdade para "o que é esta pasta": stack, package manager e scripts vêm de
//! `runtime::detect` (o mesmo que o Runtime Manager usa); Git e locator vêm de `locator`.
//! O resultado também diz, no backend, se a pasta é um projeto NOVO, já está cadastrada aqui,
//! corresponde a um projeto que o workspace já conhece ou é ambígua — a interface não decide.
//!
//! PASSIVA significa: só lê arquivos pequenos conhecidos e roda consultas Git de LEITURA
//! (`rev-parse`, `config`, `symbolic-ref`, `status`). Nunca executa script, instala, compila,
//! sobe container, busca na rede nem troca de branch; nada do projeto é executado.
use crate::{
    git,
    locator::{self, RepositoryLocator},
    models::{Location, Project},
    projects,
    runtime::{self, Composition, PackageManager, ScriptKind, StackItem},
    HubResult,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const NAME_MAX: usize = 50;
pub const DESCRIPTION_MAX: usize = 200;
const MAX_STRUCTURE: usize = 14;
const MAX_FILES: usize = 12;
const MAX_SCRIPTS: usize = 12;
/// Pastas que só poluem a visão de estrutura.
const HIDDEN_DIRS: [&str; 9] = [
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    "venv",
    "__pycache__",
    ".git",
];
/// Arquivos que dizem algo sobre o projeto, na ordem em que aparecem.
const IMPORTANT_FILES: [&str; 18] = [
    "README.md",
    "package.json",
    "pnpm-workspace.yaml",
    "Cargo.toml",
    "src-tauri/tauri.conf.json",
    "tauri.conf.json",
    "tsconfig.json",
    "vite.config.ts",
    "vite.config.js",
    "pyproject.toml",
    "requirements.txt",
    "Dockerfile",
    "compose.yaml",
    "compose.yml",
    "docker-compose.yml",
    "docker-compose.yaml",
    "AGENTS.md",
    "CLAUDE.md",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInspection {
    pub branch: String,
    pub detached: bool,
    /// Remote escolhido e sua forma canônica (nunca com credencial).
    pub remote_name: Option<String>,
    pub remote: Option<String>,
    /// Nomes dos remotes (para explicar quando há vários).
    pub remotes: Vec<String>,
    pub clean: bool,
    pub changes: u32,
    /// Caminho da pasta dentro do repositório ("" = raiz).
    pub subpath: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptInfo {
    pub name: String,
    pub kind: ScriptKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureEntry {
    pub name: String,
    /// "dir" | "file"
    pub kind: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationStatus {
    /// Nada igual no workspace: pode cadastrar.
    New,
    /// Esta mesma pasta já está vinculada a um projeto nesta máquina.
    AlreadyHere,
    /// Um projeto do workspace tem este mesmo locator: Localizar/Associar, nunca duplicar.
    Known,
    /// Mais de um projeto tem este locator (duplicata antiga): nada é decidido sozinho.
    Ambiguous,
    /// A pasta não pode ser cadastrada.
    Invalid,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchedProject {
    pub id: String,
    pub name: String,
    pub location: Location,
    /// Pasta hoje vinculada a ele nesta máquina (informação local; nunca portátil).
    pub bound_path: Option<String>,
    /// Localizar/associar está liberado (sem vínculo, ou vínculo antigo que não existe mais).
    pub can_locate: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub status: RegistrationStatus,
    pub matches: Vec<MatchedProject>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInspection {
    /// Pasta canônica desta máquina (informação local; só exibida, nunca vira identidade).
    pub folder: String,
    pub valid: bool,
    pub error: Option<String>,
    pub suggested_name: String,
    pub git: Option<GitInspection>,
    pub locator: Option<RepositoryLocator>,
    /// Por que não há locator (sem Git, sem remote, vários remotes…).
    pub locator_note: Option<String>,
    /// URL https do repositório, quando é seguro afirmar uma (exibição).
    pub repository: String,
    pub stack: Vec<StackItem>,
    pub composition: Composition,
    pub package_manager: Option<PackageManager>,
    pub package_manager_note: Option<String>,
    pub scripts: Vec<ScriptInfo>,
    pub important_files: Vec<String>,
    pub structure: Vec<StructureEntry>,
    pub registration: Registration,
    pub warnings: Vec<String>,
}

fn invalid(folder: &str, error: &str) -> ProjectInspection {
    ProjectInspection {
        folder: folder.into(),
        valid: false,
        error: Some(error.into()),
        suggested_name: String::new(),
        git: None,
        locator: None,
        locator_note: None,
        repository: String::new(),
        stack: vec![],
        composition: Composition::default(),
        package_manager: None,
        package_manager_note: None,
        scripts: vec![],
        important_files: vec![],
        structure: vec![],
        registration: Registration {
            status: RegistrationStatus::Invalid,
            matches: vec![],
            message: error.into(),
        },
        warnings: vec![],
    }
}

/// Mensagem útil para cada jeito de a pasta não servir.
fn folder_error(path: &str) -> &'static str {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "Informe o caminho da pasta do projeto."
    } else if !Path::new(trimmed).is_absolute() {
        "Selecione um caminho absoluto (use “Selecionar pasta”)."
    } else if Path::new(trimmed).is_file() {
        "O caminho é um arquivo, não uma pasta."
    } else if !Path::new(trimmed).exists() {
        "A pasta não existe nesta máquina."
    } else {
        "Pasta inacessível: sem permissão de leitura ou não é uma pasta."
    }
}

fn comparable(path: &str) -> String {
    let plain = runtime::plain_path(PathBuf::from(path));
    let text = plain
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string();
    if cfg!(windows) {
        text.to_lowercase().replace('/', "\\")
    } else {
        text
    }
}

fn same_folder(bound: &str, folder: &Path) -> bool {
    !bound.is_empty() && comparable(bound) == comparable(&folder.to_string_lossy())
}

/// A pasta `bound` e `folder` são a mesma (ignora o prefixo \\?\ e, no Windows, a caixa).
pub fn same_folder_path(bound: &str, folder: &Path) -> bool {
    same_folder(bound, folder)
}

fn matched(project: &Project, locate_allowed: bool, reason: Option<String>) -> MatchedProject {
    let location = projects::location(project);
    MatchedProject {
        id: project.id.clone(),
        name: project.name.clone(),
        location,
        bound_path: (!project.local_path.is_empty()).then(|| {
            runtime::plain_path(PathBuf::from(&project.local_path))
                .to_string_lossy()
                .to_string()
        }),
        can_locate: locate_allowed,
        reason,
    }
}

/// O backend decide o que fazer com esta pasta diante dos projetos já conhecidos.
/// Ordem: (1) mesma pasta já vinculada → `AlreadyHere`; (2) locator igual → `Known`/`Ambiguous`;
/// (3) senão `New`. Sem locator (sem Git/remote) nunca há deduplicação além da pasta idêntica.
pub fn classify_registration(
    folder: &Path,
    locator: Option<&RepositoryLocator>,
    known: &[Project],
) -> Registration {
    if let Some(project) = known.iter().find(|p| same_folder(&p.local_path, folder)) {
        return Registration {
            status: RegistrationStatus::AlreadyHere,
            message: format!("Projeto já cadastrado nesta máquina: {}.", project.name),
            matches: vec![matched(project, false, None)],
        };
    }
    let found: Vec<&Project> = match locator {
        Some(l) => known
            .iter()
            .filter(|p| p.locator.as_ref() == Some(l))
            .collect(),
        None => vec![],
    };
    let describe = |p: &Project| -> MatchedProject {
        match projects::location(p) {
            Location::Unbound => matched(p, true, None),
            Location::Missing => matched(p, true, Some("O vínculo antigo aponta para uma pasta que não existe mais; Localizar substitui o vínculo.".into())),
            Location::Available => matched(
                p,
                false,
                Some("Já está vinculado a outra pasta válida nesta máquina; o vínculo não é trocado automaticamente.".into()),
            ),
        }
    };
    match found.as_slice() {
        [] => Registration { status: RegistrationStatus::New, matches: vec![], message: "Pronto para cadastrar.".into() },
        [one] => Registration {
            status: RegistrationStatus::Known,
            message: format!("Projeto já conhecido no workspace: {}.", one.name),
            matches: vec![describe(one)],
        },
        many => Registration {
            status: RegistrationStatus::Ambiguous,
            message: format!("{} projetos do workspace correspondem a este repositório; nenhum é escolhido automaticamente.", many.len()),
            matches: many.iter().map(|p| describe(p)).collect(),
        },
    }
}

fn structure_of(root: &Path) -> Vec<StructureEntry> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut list: Vec<StructureEntry> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let hidden = name.starts_with('.') && name != ".github";
            if hidden || HIDDEN_DIRS.contains(&name.as_str()) || name.len() > 80 {
                return None;
            }
            let is_dir = e.file_type().ok()?.is_dir();
            Some(StructureEntry {
                name,
                kind: if is_dir { "dir" } else { "file" },
            })
        })
        .collect();
    list.sort_by(|a, b| {
        (a.kind != "dir", a.name.to_lowercase()).cmp(&(b.kind != "dir", b.name.to_lowercase()))
    });
    list.truncate(MAX_STRUCTURE);
    list
}

/// Inspeciona `path` (passivo). Nunca falha: pasta inválida volta com `valid = false` e o motivo.
pub fn inspect_folder(path: &str, known: &[Project]) -> ProjectInspection {
    let Ok(canonical) = projects::canonical(path) else {
        return invalid(path.trim(), folder_error(path));
    };
    let root = runtime::plain_path(canonical);
    let folder = root.to_string_lossy().to_string();
    let detection = runtime::detect(&root);
    let facts = locator::read_git_facts(&root);
    let located = locator::locator_from(&facts);
    let summary = facts.is_repo.then(|| git::summary(&root));
    let git = summary.as_ref().map(|s| GitInspection {
        branch: s.branch.clone(),
        detached: s.detached,
        remote_name: located.remote_name.clone(),
        remote: located.locator.as_ref().map(|l| l.remote.clone()),
        remotes: facts.remotes.iter().map(|r| r.name.clone()).collect(),
        clean: s.clean,
        changes: s.changes,
        subpath: facts.prefix.clone(),
    });
    let mut warnings = Vec::new();
    let structure = structure_of(&root);
    if structure.is_empty() {
        warnings.push("A pasta está vazia.".to_string());
    }
    if let Some(note) = facts.warning.clone() {
        warnings.push(note);
    }
    let important_files: Vec<String> = IMPORTANT_FILES
        .iter()
        .filter(|f| root.join(f).is_file())
        .take(MAX_FILES)
        .map(|f| f.to_string())
        .collect();
    let name: String = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
        .chars()
        .take(NAME_MAX)
        .collect();
    ProjectInspection {
        registration: classify_registration(&root, located.locator.as_ref(), known),
        folder,
        valid: true,
        error: None,
        suggested_name: name,
        git,
        locator_note: if located.locator.is_none() && facts.is_repo {
            located.note.clone()
        } else {
            None
        },
        locator: located.locator,
        repository: located.https.unwrap_or_default(),
        stack: detection.stack.clone(),
        composition: detection.composition.clone(),
        package_manager: detection.package_manager.clone(),
        package_manager_note: detection.package_manager_note.clone(),
        scripts: detection
            .scripts
            .iter()
            .take(MAX_SCRIPTS)
            .map(|s| ScriptInfo {
                name: s.name.clone(),
                kind: s.kind,
            })
            .collect(),
        important_files,
        structure,
        warnings,
    }
}

/// Valida os campos editáveis do cadastro (Concept 04): nome obrigatório até 50 caracteres,
/// descrição opcional até 200. Conta caracteres, não bytes.
pub fn validate_registration_fields(name: &str, description: &str) -> HubResult<(String, String)> {
    let name = name.trim().to_string();
    let description = description.trim().to_string();
    if name.is_empty() {
        return Err("Informe o nome do projeto.".into());
    }
    if name.chars().count() > NAME_MAX {
        return Err(format!("O nome pode ter até {NAME_MAX} caracteres."));
    }
    if description.chars().count() > DESCRIPTION_MAX {
        return Err(format!(
            "A descrição pode ter até {DESCRIPTION_MAX} caracteres."
        ));
    }
    Ok((name, description))
}
