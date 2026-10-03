//! Workspace portátil (data/workspace.json) — ver docs/STATE.md.
//!
//! IDENTIDADE DO PROJETO != CAMINHO LOCAL: o `id` do projeto é a identidade;
//! a pasta é um vínculo desta máquina (`project_bindings`) e nunca entra aqui.
//!
//! O formato canônico e a validação de publicação vivem em
//! `lkr-lab/core/lkr-workspace.js` (bridge e desktop). Este módulo valida de
//! novo antes de aplicar no SQLite: o arquivo vem do Git e não é confiável.
use crate::{
    locator::RepositoryLocator,
    models::{KnowledgeEntry, Project, ProjectCommand, ProjectPort, Prompt},
    projects::valid_repository,
    HubResult,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// v2 acrescenta `ddae` (sessões, blocos e decisões). v1 continua legível e vira v2 ao normalizar.
pub const SCHEMA_VERSION: u32 = 2;
const MIN_SCHEMA_VERSION: u32 = 1;
/// Prompts criados pela migration 001: não contam como conteúdo do usuário.
const SEED_PROMPT_IDS: [&str; 6] = ["audit", "bug", "pr", "continue", "security", "gate"];
const DENSITIES: [&str; 2] = ["comfortable", "compact"];
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
    /// Opcional: ausente em workspaces antigos e quando o projeto não tem remote reconhecível.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<RepositoryLocator>,
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

/// Preferências do desktop marcadas como `portable` (src/shared/preferences.ts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortablePreferences {
    #[serde(default)]
    pub sidebar_compact: bool,
    #[serde(default = "default_density")]
    pub density: String,
    #[serde(default)]
    pub prompt_favorites: Vec<String>,
}
fn default_density() -> String {
    "comfortable".into()
}
impl Default for PortablePreferences {
    fn default() -> Self {
        Self {
            sidebar_compact: false,
            density: default_density(),
            prompt_favorites: vec![],
        }
    }
}

/// O que o SQLite exporta: dados portáteis, sem vínculos nem atividades.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableWorkspace {
    pub version: u32,
    pub projects: Vec<PortableProject>,
    pub prompts: Vec<Prompt>,
    pub knowledge: Vec<KnowledgeEntry>,
    #[serde(default)]
    pub preferences: PortablePreferences,
    /// DDAE (v2): ausente em workspaces v1 e quando ainda não há sessões.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ddae: Vec<crate::ddae::Session>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplySummary {
    pub projects: usize,
    pub prompts: usize,
    pub knowledge: usize,
    pub sessions: usize,
    pub removed_projects: usize,
}

impl From<&Project> for PortableProject {
    fn from(p: &Project) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            repository: p.repository.clone(),
            locator: p.locator.clone(),
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

/// `host/dono/repo` canônico; vazio quando não é um remote de rede reconhecível.
/// Delega à normalização única de `locator` (HTTPS/SSH/scp-like, sem credencial nem `.git`).
pub fn remote_identity(url: &str) -> String {
    crate::locator::normalize_remote(url)
        .map(|n| n.canonical)
        .unwrap_or_default()
}

/// Forma canônica, igual à de lkr-workspace.js: listas por id, textos aparados,
/// itens vazios de listas descartados, favoritos únicos e ordenados.
pub fn normalize(ws: &mut PortableWorkspace) {
    // Um v1 legível vira v2 (ddae ausente = nenhuma sessão): o mesmo conteúdo, o mesmo hash.
    if ws.version >= MIN_SCHEMA_VERSION && ws.version < SCHEMA_VERSION {
        ws.version = SCHEMA_VERSION;
    }
    crate::ddae::normalize(&mut ws.ddae);
    fn clean(list: &mut Vec<String>) {
        *list = list
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    for p in &mut ws.projects {
        p.name = p.name.trim().to_string();
        p.repository = p.repository.trim().to_string();
        if let Some(l) = &mut p.locator {
            l.remote = l.remote.trim().to_string();
            l.path = l.path.trim().trim_matches('/').to_string();
        }
        clean(&mut p.stack);
        clean(&mut p.tags);
        for port in &mut p.ports {
            port.name = port.name.trim().to_string();
        }
        for c in &mut p.commands {
            c.name = c.name.trim().to_string();
            c.program = c.program.trim().to_string();
        }
    }
    for p in &mut ws.prompts {
        p.title = p.title.trim().to_string();
        p.category = p.category.trim().to_string();
        if p.project_id.as_deref() == Some("") {
            p.project_id = None;
        }
    }
    for k in &mut ws.knowledge {
        k.title = k.title.trim().to_string();
        if k.project_id.as_deref() == Some("") {
            k.project_id = None;
        }
    }
    ws.projects.sort_by(|a, b| a.id.cmp(&b.id));
    ws.prompts.sort_by(|a, b| a.id.cmp(&b.id));
    ws.knowledge.sort_by(|a, b| a.id.cmp(&b.id));
    let favorites = &mut ws.preferences.prompt_favorites;
    favorites.retain(|f| valid_id(f));
    favorites.sort();
    favorites.dedup();
    favorites.truncate(500);
    if !DENSITIES.contains(&ws.preferences.density.as_str()) {
        ws.preferences.density = default_density();
    }
}

/// JSON com chaves ordenadas e sem espaços: mesma entrada, mesmos bytes.
fn write_canonical(value: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap_or_default());
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn strip_ddae_stamps(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for key in ["createdAt", "updatedAt", "completedAt"] {
                map.remove(key);
            }
            for child in map.values_mut() {
                strip_ddae_stamps(child);
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_ddae_stamps),
        _ => {}
    }
}

/// SHA-256 (hex) do workspace canônico. Carimbos de data ficam de fora: salvar
/// sem mudar conteúdo não pode gerar divergência nem commit.
pub fn content_hash(ws: &PortableWorkspace) -> String {
    use sha2::{Digest, Sha256};
    let mut ws = ws.clone();
    normalize(&mut ws);
    let mut value = serde_json::to_value(&ws).unwrap_or_default();
    if let Some(projects) = value["projects"].as_array_mut() {
        for p in projects {
            if let Some(map) = p.as_object_mut() {
                map.remove("createdAt");
                map.remove("updatedAt");
            }
        }
    }
    if let Some(notes) = value["knowledge"].as_array_mut() {
        for k in notes {
            if let Some(map) = k.as_object_mut() {
                map.remove("updatedAt");
            }
        }
    }
    // DDAE: o conteúdo conta (status, blocos, decisões); carimbos de data ficam de fora.
    if let Some(sessions) = value["ddae"].as_array_mut() {
        for s in sessions {
            strip_ddae_stamps(s);
        }
    }
    let mut text = String::new();
    write_canonical(&value, &mut text);
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Máquina nova: sem projetos nem knowledge, só os prompts de fábrica e preferências padrão.
pub fn is_empty(ws: &PortableWorkspace) -> bool {
    ws.projects.is_empty()
        && ws.knowledge.is_empty()
        && ws
            .prompts
            .iter()
            .all(|p| SEED_PROMPT_IDS.contains(&p.id.as_str()))
        && ws.ddae.is_empty()
        && ws.preferences == PortablePreferences::default()
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
/// O locator é portátil: remote canônico (`host/dono/repo`) e caminho RELATIVO dentro do repositório.
/// Nada de caminho absoluto, `..`, barra invertida, credencial ou esquema.
pub fn valid_locator(l: &RepositoryLocator) -> bool {
    let mut segments = l.remote.split('/');
    // ":" só no host (porta); nos demais segmentos não existe.
    let host_ok = segments
        .next()
        .is_some_and(|h| !h.is_empty() && !h.starts_with(['.', '-']));
    let rest: Vec<&str> = segments.collect();
    let remote_ok = host_ok
        && !rest.is_empty()
        && l.remote.len() <= 400
        && !l.remote.contains("://")
        && !l.remote.contains(['@', '\\', '?', '#'])
        && !l
            .remote
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
        && rest
            .iter()
            .all(|s| !s.is_empty() && *s != "." && *s != ".." && !s.contains(':'));
    let path_ok = l.path.is_empty()
        || (l.path.len() <= 500
            && !is_absolute_path(&l.path)
            && !l.path.contains(['\\', ':'])
            && l.path.split('/').all(|s| {
                !s.is_empty() && s != "." && s != ".." && !s.chars().any(|c| c.is_control())
            }));
    remote_ok && path_ok
}

pub fn validate(ws: &PortableWorkspace) -> HubResult<()> {
    check(
        (MIN_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&ws.version),
        || {
            format!(
                "versão {} não suportada (esperada {MIN_SCHEMA_VERSION}–{SCHEMA_VERSION})",
                ws.version
            )
        },
    )?;
    check(ws.version >= 2 || ws.ddae.is_empty(), || {
        "DDAE exige a versão 2 do workspace".into()
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
        if let Some(l) = &p.locator {
            check(valid_locator(l), || {
                format!("projeto {at}: locator inválido (remote canônico e caminho relativo, sem caminho local)")
            })?;
        }
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
    check(DENSITIES.contains(&ws.preferences.density.as_str()), || {
        "preferências: densidade desconhecida".into()
    })?;
    check(
        ws.preferences.prompt_favorites.len() <= 500
            && ws.preferences.prompt_favorites.iter().all(|f| valid_id(f)),
        || "preferências: favoritos inválidos".into(),
    )?;
    crate::ddae::validate_portable(&ws.ddae, &project_ids)?;
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
