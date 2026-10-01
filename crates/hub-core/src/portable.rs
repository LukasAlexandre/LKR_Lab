//! Workspace portátil (data/workspace.json) — ver docs/STATE.md.
//!
//! IDENTIDADE DO PROJETO != CAMINHO LOCAL: o `id` do projeto é a identidade;
//! a pasta é um vínculo desta máquina (`project_bindings`) e nunca entra aqui.
//!
//! O formato canônico e a validação de publicação vivem em
//! `lkr-lab/core/lkr-workspace.js` (bridge e desktop). Este módulo valida de
//! novo antes de aplicar no SQLite: o arquivo vem do Git e não é confiável.
use crate::{
    models::{KnowledgeEntry, Project, ProjectCommand, ProjectPort, Prompt},
    projects::valid_repository,
    HubResult,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SCHEMA_VERSION: u32 = 1;
const KINDS: [&str; 5] = ["note", "decision", "architecture", "bug", "documentation"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableProject {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub repository: String,
    #[serde(default)]
    pub stack: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub ports: Vec<ProjectPort>,
    #[serde(default)]
    pub commands: Vec<ProjectCommand>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// O que o SQLite exporta: dados portáteis, sem vínculos nem atividades.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableWorkspace {
    pub version: u32,
    pub projects: Vec<PortableProject>,
    pub prompts: Vec<Prompt>,
    pub knowledge: Vec<KnowledgeEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplySummary {
    pub projects: usize,
    pub prompts: usize,
    pub knowledge: usize,
    pub removed_projects: usize,
}

impl From<&Project> for PortableProject {
    fn from(p: &Project) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            repository: p.repository.clone(),
            stack: p.stack.clone(),
            tags: p.tags.clone(),
            ports: p.ports.clone(),
            commands: p.commands.clone(),
            created_at: p.created_at.clone(),
            updated_at: p.updated_at.clone(),
        }
    }
}

/// Caminho absoluto de uma máquina (C:\…, \\servidor, /…, ~/…).
pub fn is_absolute_path(value: &str) -> bool {
    let b = value.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || value.starts_with("\\\\")
        || value.starts_with("//")
        || value.starts_with('/')
        || value.starts_with("~/")
        || value.starts_with("~\\")
}

pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Compara remotes ignorando esquema, credencial, "www.", ".git", barra final e caixa.
/// `git@host:dono/repo` e `https://host/dono/repo` são o mesmo repositório.
pub fn same_repository(a: &str, b: &str) -> bool {
    let a = remote_identity(a);
    !a.is_empty() && a == remote_identity(b)
}

/// `host/dono/repo` em minúsculas, sem esquema nem credenciais (seguro para exibir).
pub fn remote_identity(url: &str) -> String {
    let mut value = url.trim().to_lowercase();
    if let Some(rest) = value.strip_prefix("git@") {
        value = rest.replacen(':', "/", 1);
    }
    for prefix in ["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.to_string();
        }
    }
    if let Some((_, rest)) = value.split_once('@') {
        value = rest.to_string();
    }
    let value = value.strip_prefix("www.").unwrap_or(&value).to_string();
    value
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_end_matches('/')
        .to_string()
}

fn check(ok: bool, message: impl FnOnce() -> String) -> HubResult<()> {
    if ok {
        Ok(())
    } else {
        Err(format!("Workspace inválido: {}", message()))
    }
}

fn unique<'a>(ids: impl Iterator<Item = &'a str>, what: &str) -> HubResult<HashSet<&'a str>> {
    let mut seen = HashSet::new();
    for id in ids {
        check(valid_id(id), || format!("id de {what} inválido ({id})"))?;
        check(seen.insert(id), || format!("id de {what} duplicado ({id})"))?;
    }
    Ok(seen)
}

/// Mesmas regras de lkr-workspace.js (limites em bytes, como no cadastro).
pub fn validate(ws: &PortableWorkspace) -> HubResult<()> {
    check(ws.version == SCHEMA_VERSION, || {
        format!(
            "versão {} não suportada (esperada {SCHEMA_VERSION})",
            ws.version
        )
    })?;
    let project_ids = unique(ws.projects.iter().map(|p| p.id.as_str()), "projeto")?;
    for p in &ws.projects {
        let at = &p.name;
        check(
            !p.name.trim().is_empty() && p.name.len() <= 100 && p.description.len() <= 4000,
            || {
                format!(
                    "projeto {}: nome (até 100) ou descrição (até 4000) inválidos",
                    p.id
                )
            },
        )?;
        check(
            p.repository.is_empty() || valid_repository(&p.repository),
            || format!("projeto {at}: repositório precisa ser HTTPS, sem credenciais"),
        )?;
        check(
            p.stack.len() <= 30
                && p.tags.len() <= 30
                && p.ports.len() <= 30
                && p.commands.len() <= 30,
            || format!("projeto {at}: máximo 30 itens por lista"),
        )?;
        check(
            p.stack.iter().chain(&p.tags).all(|s| s.len() <= 200),
            || format!("projeto {at}: item de stack/tag longo demais"),
        )?;
        let mut ports = HashSet::new();
        check(
            p.ports.iter().all(|port| {
                port.port != 0 && !port.name.trim().is_empty() && ports.insert(port.port)
            }),
            || format!("projeto {at}: portas precisam de nome, valor 1–65535 e sem repetição"),
        )?;
        for c in &p.commands {
            check(
                c.name.len() <= 100
                    && !c.program.trim().is_empty()
                    && c.program.len() <= 200
                    && c.args.len() <= 50
                    && c.args.iter().all(|a| a.len() <= 1000),
                || format!("projeto {at}: comando excede limites"),
            )?;
            check(
                !is_absolute_path(&c.program) && !c.args.iter().any(|a| is_absolute_path(a)),
                || format!("projeto {at}: comando com caminho absoluto de outra máquina"),
            )?;
        }
        check(p.created_at.len() <= 40 && p.updated_at.len() <= 40, || {
            format!("projeto {at}: data inválida")
        })?;
    }
    let reference = |id: &Option<String>| id.as_deref().is_none_or(|id| project_ids.contains(id));
    unique(ws.prompts.iter().map(|p| p.id.as_str()), "prompt")?;
    for p in &ws.prompts {
        check(
            !p.title.trim().is_empty()
                && p.title.len() <= 240
                && p.category.len() <= 100
                && !p.body.trim().is_empty()
                && p.body.len() <= 32_000,
            || {
                format!(
                    "prompt {}: título/corpo obrigatórios e dentro dos limites",
                    p.id
                )
            },
        )?;
        check(reference(&p.project_id), || {
            format!("prompt {} referencia projeto inexistente", p.id)
        })?;
    }
    unique(ws.knowledge.iter().map(|k| k.id.as_str()), "conhecimento")?;
    for k in &ws.knowledge {
        check(
            !k.title.trim().is_empty()
                && k.title.len() <= 240
                && !k.body.trim().is_empty()
                && k.body.len() <= 128_000
                && k.tags.len() <= 2000
                && k.updated_at.len() <= 40,
            || {
                format!(
                    "conhecimento {}: título/corpo obrigatórios e dentro dos limites",
                    k.id
                )
            },
        )?;
        check(KINDS.contains(&k.kind.as_str()), || {
            format!("conhecimento {}: tipo desconhecido", k.id)
        })?;
        check(reference(&k.project_id), || {
            format!("conhecimento {} referencia projeto inexistente", k.id)
        })?;
    }
    Ok(())
}
