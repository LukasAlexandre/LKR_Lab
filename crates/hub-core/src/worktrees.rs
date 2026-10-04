//! Worktrees do LKR LAB (Concept 08): a camada OPERACIONAL sobre os Git worktrees reais.
//!
//! Três camadas que nunca se confundem:
//!   1. **Git worktree** (`git::Worktree`): o objeto do Git; lido de `git worktree list --porcelain`.
//!   2. **Managed worktree** (`ManagedWorktree`): metadata PORTÁTIL do LKR LAB — UUID, nome, estado
//!      operacional, vínculo com Session/Block, dicas de locator e eventos.
//!   3. **Binding local** (`worktree_bindings`): o path absoluto desta máquina (classe C).
//!
//! Regras canônicas:
//! * A identidade é o UUID. Path, branch, pasta e HEAD nunca são identidade (a branch é só dica).
//! * O estado operacional (`active|frozen|stopped|completed`) é metadata: não vem de Git nem de
//!   runtime e é independente do estado da Session. `completed` é terminal. Vários `active` são
//!   válidos (Project e Session).
//! * O checkout PRINCIPAL é um Git worktree real mas fica fora do ciclo operacional e não é adotável.
//! * Listar é 100% leitura: nada é gravado, nenhum binding/evento é criado, nenhum comando mutável
//!   do Git roda. A metadata só nasce por ação explícita (adotar/criar).
//! * FINALIZAR é só estado: nunca merge, push, `worktree remove`, `branch -d` nem apagar pasta.
//! * Remover do Git preserva a metadata e os eventos; só o binding local some.
use crate::{
    database::Database,
    ddae::{self, check_payload, check_text, ev, new_id, now, EventType},
    git::{self, NewWorktree},
    inspect,
    locator::RepositoryLocator,
    models::Location,
    overview::{self, Dimension},
    projects, runtime, HubResult,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub const MAX_NAME: usize = 100;
pub const MAX_DESCRIPTION: usize = 1_000;
pub const MAX_REASON: usize = 500;
pub const MAX_RESULT: usize = 2_000;
pub const MAX_HINT: usize = 200;
pub const MAX_WORKTREES: usize = 5_000;
pub const MAX_EVENTS: usize = 20_000;
const MAX_TIMESTAMP: usize = 40;

// ------------------------------------------------------------------ estado operacional

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationalStatus {
    Active,
    Frozen,
    Stopped,
    Completed,
}

impl OperationalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Frozen => "frozen",
            Self::Stopped => "stopped",
            Self::Completed => "completed",
        }
    }
    fn parse(value: &str) -> HubResult<Self> {
        match value {
            "active" => Ok(Self::Active),
            "frozen" => Ok(Self::Frozen),
            "stopped" => Ok(Self::Stopped),
            "completed" => Ok(Self::Completed),
            other => Err(format!("Estado operacional desconhecido ({other}).")),
        }
    }
    /// ACTIVE ↔ FROZEN/STOPPED; qualquer não finalizado → COMPLETED; COMPLETED é terminal.
    pub fn can_become(self, target: Self) -> bool {
        use OperationalStatus::*;
        matches!(
            (self, target),
            (Active, Frozen)
                | (Active, Stopped)
                | (Frozen, Active)
                | (Stopped, Active)
                | (Active, Completed)
                | (Frozen, Completed)
                | (Stopped, Completed)
        )
    }
}

/// Eventos semânticos do worktree (portáteis, append-only). Nunca carregam path local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorktreeEventType {
    WorktreeAdopted,
    WorktreeCreated,
    WorktreeStateChanged,
    WorktreeRenamed,
    WorktreeSessionLinked,
    WorktreeSessionUnlinked,
    WorktreeBlockLinked,
    WorktreeBlockUnlinked,
    WorktreeCompleted,
}

impl WorktreeEventType {
    pub fn as_str(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: WorktreeEventType,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub payload: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub created_at: String,
}

/// A metadata PORTÁTIL de um worktree gerenciado (o mesmo formato no SQLite e no workspace).
/// Não contém path: o binding é local e fica em `worktree_bindings`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedWorktree {
    pub id: String,
    pub project_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub status: OperationalStatus,
    /// Motivo (opcional) de congelado/parado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Dica portátil do repositório (a do Project quando confiável); NÃO é identidade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_locator: Option<RepositoryLocator>,
    /// Dica: a branch pode ser renomeada ou recriada; NÃO é identidade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_hint: Option<String>,
    /// Dica quando o worktree estava em detached HEAD (o commit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detached_head_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Só existe com `session_id`; o bloco precisa ser dessa Session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<WorktreeEvent>,
}

// ------------------------------------------------------------------ portabilidade

/// Forma canônica: textos aparados, vazios viram ausentes, ordenado por id; eventos por
/// (created_at, id) — nunca pela ordem do SQLite.
pub fn normalize(list: &mut [ManagedWorktree]) {
    let trim_opt = |v: &mut Option<String>| {
        *v = v
            .take()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    };
    for w in list.iter_mut() {
        w.display_name = w.display_name.trim().to_string();
        w.description = w.description.trim().to_string();
        trim_opt(&mut w.state_reason);
        trim_opt(&mut w.result);
        trim_opt(&mut w.branch_hint);
        trim_opt(&mut w.detached_head_hint);
        trim_opt(&mut w.session_id);
        trim_opt(&mut w.block_id);
        trim_opt(&mut w.completed_at);
        if let Some(l) = &mut w.repository_locator {
            l.remote = l.remote.trim().to_string();
            l.path = l.path.trim().trim_matches('/').to_string();
        }
        w.events
            .sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
    }
    list.sort_by(|a, b| a.id.cmp(&b.id));
}

/// Regras do estado portátil recebido (arquivo versionado, não confiável).
pub fn validate_portable(
    list: &[ManagedWorktree],
    project_ids: &HashSet<&str>,
    sessions: &[ddae::Session],
) -> HubResult<()> {
    let fail = |m: String| -> HubResult<()> { Err(format!("Workspace inválido: worktrees: {m}")) };
    if list.len() > MAX_WORKTREES {
        return fail(format!("máximo de {MAX_WORKTREES} worktrees"));
    }
    let mut ids = HashSet::new();
    for w in list {
        let at = &w.display_name;
        let map = |e: String| format!("Workspace inválido: worktrees: {at}: {e}");
        crate::portable::valid_id(&w.id)
            .then_some(())
            .ok_or_else(|| map(format!("id inválido ({})", w.id)))?;
        if !ids.insert(w.id.as_str()) {
            return fail(format!("id duplicado ({})", w.id));
        }
        if !project_ids.contains(w.project_id.as_str()) {
            return fail(format!(
                "{at}: referencia um projeto que não está no workspace"
            ));
        }
        check_text("nome", &w.display_name, MAX_NAME, true).map_err(map)?;
        check_text("descrição", &w.description, MAX_DESCRIPTION, false).map_err(map)?;
        for (field, v, max) in [
            ("motivo", &w.state_reason, MAX_REASON),
            ("resultado", &w.result, MAX_RESULT),
            ("branch", &w.branch_hint, MAX_HINT),
            ("HEAD", &w.detached_head_hint, MAX_HINT),
        ] {
            if let Some(t) = v {
                check_text(field, t, max, false).map_err(map)?;
            }
        }
        if let Some(l) = &w.repository_locator {
            if !crate::portable::valid_locator(l) {
                return fail(format!("{at}: locator inválido (sem caminho local)"));
            }
        }
        if w.created_at.len() > MAX_TIMESTAMP
            || w.updated_at.len() > MAX_TIMESTAMP
            || w.completed_at
                .as_deref()
                .is_some_and(|c| c.len() > MAX_TIMESTAMP)
        {
            return fail(format!("{at}: data inválida"));
        }
        if (w.status == OperationalStatus::Completed) != w.completed_at.is_some() {
            return fail(format!(
                "{at}: finalizado exige completedAt e os demais estados não podem tê-lo"
            ));
        }
        if w.block_id.is_some() && w.session_id.is_none() {
            return fail(format!("{at}: bloco exige Session"));
        }
        if let Some(sid) = &w.session_id {
            let Some(session) = sessions.iter().find(|s| &s.id == sid) else {
                return fail(format!(
                    "{at}: referencia uma Session que não está no workspace"
                ));
            };
            if session.project_id != w.project_id {
                return fail(format!("{at}: a Session pertence a outro projeto"));
            }
            if let Some(b) = &w.block_id {
                if !session.blocks.iter().any(|x| &x.id == b) {
                    return fail(format!("{at}: o bloco não pertence à Session vinculada"));
                }
            }
        }
        if w.events.len() > MAX_EVENTS {
            return fail(format!("{at}: excede o limite de eventos"));
        }
        for e in &w.events {
            crate::portable::valid_id(&e.id)
                .then_some(())
                .ok_or_else(|| map(format!("id de evento inválido ({})", e.id)))?;
            if !ids.insert(e.id.as_str()) {
                return fail(format!("id de evento duplicado ({})", e.id));
            }
            if e.created_at.len() > MAX_TIMESTAMP {
                return fail(format!("{at}: data de evento inválida"));
            }
            check_payload(&e.payload).map_err(map)?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ persistência

const COLUMNS: &str = "id,project_id,display_name,description,operational_status,state_reason,result,repository_locator,branch_hint,detached_head_hint,session_id,block_id,created_at,updated_at,completed_at";

fn json_text<T: Serialize>(v: &T) -> HubResult<String> {
    serde_json::to_string(v).map_err(|e| e.to_string())
}

fn load_events(conn: &Connection, id: &str) -> HubResult<Vec<WorktreeEvent>> {
    let mut stmt = conn
        .prepare(
            "SELECT id,event_type,payload,created_at FROM worktree_events WHERE worktree_id=?1 ORDER BY created_at, id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(id, kind, payload, created_at)| {
            Ok(WorktreeEvent {
                id,
                kind: serde_json::from_value(serde_json::Value::String(kind))
                    .map_err(|e| e.to_string())?,
                payload: serde_json::from_str(&payload).map_err(|e| e.to_string())?,
                created_at,
            })
        })
        .collect()
}

fn row_to_managed(r: &rusqlite::Row) -> rusqlite::Result<ManagedWorktree> {
    let status: String = r.get(4)?;
    let status = OperationalStatus::parse(&status).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::<dyn std::error::Error + Send + Sync>::from(e),
        )
    })?;
    let locator: Option<String> = r.get(7)?;
    let repository_locator = match locator {
        None => None,
        Some(text) => Some(serde_json::from_str(&text).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
        })?),
    };
    Ok(ManagedWorktree {
        id: r.get(0)?,
        project_id: r.get(1)?,
        display_name: r.get(2)?,
        description: r.get(3)?,
        status,
        state_reason: r.get(5)?,
        result: r.get(6)?,
        repository_locator,
        branch_hint: r.get(8)?,
        detached_head_hint: r.get(9)?,
        session_id: r.get(10)?,
        block_id: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
        completed_at: r.get(14)?,
        events: vec![],
    })
}

fn load_managed(conn: &Connection, project_id: Option<&str>) -> HubResult<Vec<ManagedWorktree>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM managed_worktrees {} ORDER BY id",
        if project_id.is_some() {
            "WHERE project_id=?1"
        } else {
            ""
        }
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = if let Some(p) = project_id {
        stmt.query_map([p], row_to_managed)
    } else {
        stmt.query_map([], row_to_managed)
    }
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|mut w| {
            w.events = load_events(conn, &w.id)?;
            Ok(w)
        })
        .collect()
}

fn load_one(conn: &Connection, id: &str) -> HubResult<ManagedWorktree> {
    let mut w = conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM managed_worktrees WHERE id=?1"),
            [id],
            row_to_managed,
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Worktree não encontrado.".to_string())?;
    w.events = load_events(conn, id)?;
    Ok(w)
}

/// Estado portátil para exportar ao workspace (sem bindings).
pub fn export(conn: &Connection) -> HubResult<Vec<ManagedWorktree>> {
    load_managed(conn, None)
}

fn upsert(tx: &Transaction, w: &ManagedWorktree) -> HubResult<()> {
    tx.execute(
        "INSERT INTO managed_worktrees(id,project_id,display_name,description,operational_status,state_reason,result,repository_locator,branch_hint,detached_head_hint,session_id,block_id,created_at,updated_at,completed_at) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) \
         ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,display_name=excluded.display_name,description=excluded.description,\
         operational_status=excluded.operational_status,state_reason=excluded.state_reason,result=excluded.result,\
         repository_locator=excluded.repository_locator,branch_hint=excluded.branch_hint,detached_head_hint=excluded.detached_head_hint,\
         session_id=excluded.session_id,block_id=excluded.block_id,created_at=excluded.created_at,updated_at=excluded.updated_at,completed_at=excluded.completed_at",
        params![
            w.id,
            w.project_id,
            w.display_name,
            w.description,
            w.status.as_str(),
            w.state_reason,
            w.result,
            w.repository_locator.as_ref().map(json_text).transpose()?,
            w.branch_hint,
            w.detached_head_hint,
            w.session_id,
            w.block_id,
            w.created_at,
            w.updated_at,
            w.completed_at
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn insert_events(tx: &Transaction, w: &ManagedWorktree) -> HubResult<()> {
    for e in &w.events {
        tx.execute(
            "INSERT OR IGNORE INTO worktree_events(id,worktree_id,event_type,payload,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![e.id, w.id, e.kind.as_str(), json_text(&e.payload)?, e.created_at],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Aplica o workspace: o estado portátil manda (upsert; sai o que não está no arquivo), mas os
/// BINDINGS LOCAIS dos worktrees que continuam existindo são preservados (classe C).
pub fn replace_all(tx: &Transaction, list: &[ManagedWorktree]) -> HubResult<()> {
    let keep: HashSet<&str> = list.iter().map(|w| w.id.as_str()).collect();
    let existing: Vec<String> = tx
        .prepare("SELECT id FROM managed_worktrees")
        .and_then(|mut s| {
            s.query_map([], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()
        })
        .map_err(|e| e.to_string())?;
    for id in existing.iter().filter(|id| !keep.contains(id.as_str())) {
        tx.execute("DELETE FROM managed_worktrees WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
    }
    let stamp = now(tx)?;
    for w in list {
        let mut w = w.clone();
        if w.created_at.is_empty() {
            w.created_at = stamp.clone();
        }
        if w.updated_at.is_empty() {
            w.updated_at = w.created_at.clone();
        }
        upsert(tx, &w)?;
        tx.execute("DELETE FROM worktree_events WHERE worktree_id=?1", [&w.id])
            .map_err(|e| e.to_string())?;
        insert_events(tx, &w)?;
    }
    Ok(())
}

// ------------------------------------------------------------------ leitura agregada

/// O que o card é. NÃO se mistura com o estado operacional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Checkout principal: Git worktree real, sem ciclo operacional, não adotável.
    Primary,
    /// Metadata + binding válido + Git worktree correspondente.
    ManagedAvailable,
    /// Metadata portátil sem binding válido nesta máquina: sem Git nem runtime inventados.
    ManagedMissing,
    /// Git worktree adicional real, sem metadata do LKR LAB (CTA: Adotar).
    Unmanaged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    pub id: String,
    pub label: String,
    pub title: String,
    pub status: ddae::SessionStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRef {
    pub id: String,
    pub title: String,
    pub status: ddae::BlockStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedView {
    #[serde(flatten)]
    pub worktree: ManagedWorktree,
    pub session: Option<SessionRef>,
    pub block: Option<BlockRef>,
    /// Último evento OPERACIONAL do worktree (nunca mtime nem commit).
    pub last_event_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeItem {
    pub kind: ItemKind,
    /// O Git worktree real (path, HEAD, branch...); `None` quando não localizado nesta máquina.
    pub git: Option<git::Worktree>,
    pub managed: Option<ManagedView>,
    /// Estado Git do path (somente leitura); ausente para não localizado.
    pub git_summary: Option<Dimension<git::GitSummary>>,
    /// Incoerências (não corrigidas): `session_completed_worktree_open`,
    /// `worktree_completed_session_active`.
    pub warnings: Vec<&'static str>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCounts {
    /// Git worktrees reais detectados (inclui o principal e os não gerenciados).
    pub git_total: u32,
    /// Total GERENCIADO (a soma active+frozen+stopped+completed fecha com este número).
    pub managed: u32,
    pub active: u32,
    pub frozen: u32,
    pub stopped: u32,
    pub completed: u32,
    /// Gerenciados sem binding válido nesta máquina (subconjunto de `managed`).
    pub missing: u32,
    pub unmanaged: u32,
    /// Worktrees (qualquer tipo) com alterações Git; só quando o Git foi lido.
    pub with_changes: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeOverview {
    pub project_id: String,
    pub project_available: bool,
    pub items: Vec<WorktreeItem>,
    pub counts: WorktreeCounts,
    pub git_error: Option<String>,
}

/// Casamento puro (sem E/S): um Git worktree vira Primary / ManagedAvailable / Unmanaged e cada
/// metadata sem Git correspondente vira ManagedMissing.
pub struct Matched {
    pub kind: ItemKind,
    pub git: Option<git::Worktree>,
    pub managed: Option<usize>,
}

pub fn classify(
    git: &[git::Worktree],
    managed: &[(ManagedWorktree, Option<String>)],
) -> Vec<Matched> {
    let mut used = HashSet::new();
    let mut out = Vec::new();
    for g in git {
        if g.is_primary {
            out.push(Matched {
                kind: ItemKind::Primary,
                git: Some(g.clone()),
                managed: None,
            });
            continue;
        }
        let found = managed.iter().position(|(_, bound)| {
            bound
                .as_deref()
                .is_some_and(|b| inspect::same_folder_path(b, Path::new(&g.path)))
        });
        match found {
            Some(i) if !used.contains(&i) => {
                used.insert(i);
                out.push(Matched {
                    kind: ItemKind::ManagedAvailable,
                    git: Some(g.clone()),
                    managed: Some(i),
                });
            }
            _ => out.push(Matched {
                kind: ItemKind::Unmanaged,
                git: Some(g.clone()),
                managed: None,
            }),
        }
    }
    for i in 0..managed.len() {
        if !used.contains(&i) {
            out.push(Matched {
                kind: ItemKind::ManagedMissing,
                git: None,
                managed: Some(i),
            });
        }
    }
    out
}

impl WorktreeOverview {
    /// Lê o estado Git (somente leitura) de cada path. Fora do lock do banco, porque cada
    /// `git status` pode demorar.
    pub fn attach_git(&mut self) {
        for item in &mut self.items {
            if let Some(g) = &item.git {
                item.git_summary = Some(overview::real_git(Path::new(&g.path)));
            }
        }
        self.counts.with_changes = self
            .items
            .iter()
            .filter(|i| {
                i.git_summary
                    .as_ref()
                    .and_then(|d| d.data.as_ref())
                    .is_some_and(|s| s.changes > 0 || s.conflicts > 0)
            })
            .count() as u32;
    }
}

/// Dados do card de um worktree para a Session (leve: sem Git).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWorktree {
    pub id: String,
    pub display_name: String,
    pub status: OperationalStatus,
    pub branch_hint: Option<String>,
    pub block_id: Option<String>,
    /// Há um binding cujo caminho existe nesta máquina.
    pub available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreateMode {
    NewBranch,
    ExistingBranch,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequest {
    pub mode: CreateMode,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub branch: String,
    /// Só para branch nova: a base é EXPLÍCITA (padrão da interface: HEAD).
    #[serde(default)]
    pub base_ref: String,
    /// Destino LOCAL (vira só o binding; nunca é portátil).
    pub path: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub block_id: Option<String>,
}

fn plain(path: &str) -> String {
    let p = Path::new(path);
    let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    runtime::plain_path(c).to_string_lossy().to_string()
}

fn default_name(g: &git::Worktree) -> String {
    if !g.branch.is_empty() {
        return g.branch.clone();
    }
    Path::new(&g.path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "worktree".into())
}

fn wt_event(
    tx: &Transaction,
    worktree_id: &str,
    kind: WorktreeEventType,
    pairs: &[(&str, &str)],
) -> HubResult<()> {
    let mut payload = serde_json::Map::new();
    for (k, v) in pairs {
        if !v.is_empty() {
            payload.insert((*k).into(), serde_json::Value::String((*v).into()));
        }
    }
    check_payload(&payload)?;
    tx.execute(
        "INSERT INTO worktree_events(id,worktree_id,event_type,payload,created_at) VALUES(?1,?2,?3,?4,?5)",
        params![new_id(), worktree_id, kind.as_str(), json_text(&payload)?, now(tx)?],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn session_event(
    tx: &Transaction,
    session_id: &str,
    kind: EventType,
    block_id: Option<&str>,
    worktree_id: &str,
    name: &str,
) -> HubResult<()> {
    ddae::record_event(
        tx,
        session_id,
        &ev(
            kind,
            block_id,
            &[("worktreeId", worktree_id), ("name", name)],
        ),
    )
}

fn touch(tx: &Transaction, id: &str) -> HubResult<()> {
    tx.execute(
        "UPDATE managed_worktrees SET updated_at=?2 WHERE id=?1",
        params![id, now(tx)?],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn activity(tx: &Transaction, project_id: &str, text: &str) -> HubResult<()> {
    tx.execute(
        "INSERT INTO activities(project_id,action) VALUES(?1,?2)",
        params![project_id, text],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Session do MESMO Project e Block da MESMA Session (mensagens claras; o banco também recusa).
fn check_relation(
    conn: &Connection,
    project_id: &str,
    session_id: Option<&str>,
    block_id: Option<&str>,
) -> HubResult<()> {
    if block_id.is_some() && session_id.is_none() {
        return Err("Um bloco só pode ser vinculado junto com a Session.".into());
    }
    if let Some(s) = session_id {
        let ok: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ddae_sessions WHERE id=?1 AND project_id=?2)",
                params![s, project_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !ok {
            return Err("A Session não existe ou não pertence a este projeto.".into());
        }
    }
    if let (Some(s), Some(b)) = (session_id, block_id) {
        let ok: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ddae_blocks WHERE id=?1 AND session_id=?2)",
                params![b, s],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !ok {
            return Err("O bloco não pertence à Session selecionada.".into());
        }
    }
    Ok(())
}

/// Eventos do worktree E da Session pela diferença de vínculo (nunca com path local).
fn relation_events(
    tx: &Transaction,
    w_id: &str,
    name: &str,
    old: (&Option<String>, &Option<String>),
    new: (&Option<String>, &Option<String>),
) -> HubResult<()> {
    let label_of = |tx: &Transaction, sid: &str| -> String {
        tx.query_row("SELECT number FROM ddae_sessions WHERE id=?1", [sid], |r| {
            r.get::<_, u32>(0)
        })
        .map(ddae::label)
        .unwrap_or_default()
    };
    let block_title = |tx: &Transaction, bid: &str| -> String {
        tx.query_row("SELECT title FROM ddae_blocks WHERE id=?1", [bid], |r| {
            r.get(0)
        })
        .unwrap_or_default()
    };
    if old.1 != new.1 || old.0 != new.0 {
        if let (Some(sid), Some(bid)) = (old.0, old.1) {
            if old.1 != new.1 || old.0 != new.0 {
                let title = block_title(tx, bid);
                wt_event(
                    tx,
                    w_id,
                    WorktreeEventType::WorktreeBlockUnlinked,
                    &[("blockId", bid), ("block", &title)],
                )?;
                session_event(
                    tx,
                    sid,
                    EventType::WorktreeBlockUnlinked,
                    Some(bid),
                    w_id,
                    name,
                )?;
            }
        }
    }
    if old.0 != new.0 {
        if let Some(sid) = old.0 {
            let label = label_of(tx, sid);
            wt_event(
                tx,
                w_id,
                WorktreeEventType::WorktreeSessionUnlinked,
                &[("sessionId", sid), ("session", &label)],
            )?;
            session_event(tx, sid, EventType::WorktreeUnlinked, None, w_id, name)?;
        }
        if let Some(sid) = new.0 {
            let label = label_of(tx, sid);
            wt_event(
                tx,
                w_id,
                WorktreeEventType::WorktreeSessionLinked,
                &[("sessionId", sid), ("session", &label)],
            )?;
            session_event(tx, sid, EventType::WorktreeLinked, None, w_id, name)?;
        }
    }
    if let (Some(sid), Some(bid)) = (new.0, new.1) {
        if old.1 != new.1 || old.0 != new.0 {
            let title = block_title(tx, bid);
            wt_event(
                tx,
                w_id,
                WorktreeEventType::WorktreeBlockLinked,
                &[("blockId", bid), ("block", &title)],
            )?;
            session_event(
                tx,
                sid,
                EventType::WorktreeBlockLinked,
                Some(bid),
                w_id,
                name,
            )?;
        }
    }
    Ok(())
}

/// Campos de uma metadata nova (adotar/criar).
struct NewManaged<'a> {
    name: &'a str,
    description: &'a str,
    branch: &'a str,
    detached_head: Option<&'a str>,
    session_id: Option<&'a str>,
    block_id: Option<&'a str>,
}

impl Database {
    fn worktree_repo_dir(&self, project_id: &str) -> HubResult<PathBuf> {
        projects::local_dir(&self.project(project_id)?)
    }

    fn bindings(&self, project_id: &str) -> HubResult<HashMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT b.worktree_id, b.local_path FROM worktree_bindings b \
                 JOIN managed_worktrees w ON w.id = b.worktree_id WHERE w.project_id=?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([project_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    }

    /// 100% LEITURA: Git worktrees reais + metadata + bindings + (opcional) estado Git de cada
    /// path. Não grava nada, não cria binding nem evento e só roda `git worktree list` e `git status`.
    pub fn project_worktree_overview(
        &self,
        project_id: &str,
        with_git: bool,
    ) -> HubResult<WorktreeOverview> {
        let project = self.project(project_id)?;
        let available = projects::location(&project) == Location::Available;
        let (git_list, git_error) = if available {
            match git::worktrees(&PathBuf::from(&project.local_path)) {
                Ok(list) => (list, None),
                Err(e) => (vec![], Some(e)),
            }
        } else {
            (vec![], None)
        };
        let bindings = self.bindings(project_id)?;
        let managed: Vec<(ManagedWorktree, Option<String>)> =
            load_managed(&self.conn, Some(project_id))?
                .into_iter()
                .map(|w| {
                    let bound = bindings.get(&w.id).cloned();
                    (w, bound)
                })
                .collect();
        let sessions = ddae::load_sessions(&self.conn, Some(project_id))?;
        let matched = classify(&git_list, &managed);

        let mut items: Vec<WorktreeItem> = matched
            .into_iter()
            .map(|m| {
                let view = m.managed.map(|i| {
                    let w = managed[i].0.clone();
                    let session = w
                        .session_id
                        .as_ref()
                        .and_then(|sid| sessions.iter().find(|s| &s.id == sid));
                    let block = session.and_then(|s| {
                        w.block_id
                            .as_ref()
                            .and_then(|bid| s.blocks.iter().find(|b| &b.id == bid))
                    });
                    ManagedView {
                        session: session.map(|s| SessionRef {
                            id: s.id.clone(),
                            label: s.label(),
                            title: s.title.clone(),
                            status: s.status,
                        }),
                        block: block.map(|b| BlockRef {
                            id: b.id.clone(),
                            title: b.title.clone(),
                            status: b.status,
                        }),
                        last_event_at: w.events.last().map(|e| e.created_at.clone()),
                        worktree: w,
                    }
                });
                let mut warnings = Vec::new();
                if let Some(v) = &view {
                    if let Some(s) = &v.session {
                        if s.status == ddae::SessionStatus::Completed
                            && v.worktree.status != OperationalStatus::Completed
                        {
                            warnings.push("session_completed_worktree_open");
                        }
                        if v.worktree.status == OperationalStatus::Completed
                            && s.status == ddae::SessionStatus::Active
                        {
                            warnings.push("worktree_completed_session_active");
                        }
                    }
                }
                let git_summary = match (&m.git, with_git) {
                    (Some(g), true) => Some(overview::real_git(Path::new(&g.path))),
                    _ => None,
                };
                WorktreeItem {
                    kind: m.kind,
                    git: m.git,
                    managed: view,
                    git_summary,
                    warnings,
                }
            })
            .collect();
        let rank = |k: ItemKind| match k {
            ItemKind::Primary => 0,
            ItemKind::ManagedAvailable => 1,
            ItemKind::ManagedMissing => 2,
            ItemKind::Unmanaged => 3,
        };
        items.sort_by(|a, b| {
            let name = |i: &WorktreeItem| {
                i.managed
                    .as_ref()
                    .map(|m| m.worktree.display_name.to_lowercase())
                    .or_else(|| i.git.as_ref().map(|g| g.path.to_lowercase()))
                    .unwrap_or_default()
            };
            (rank(a.kind), name(a)).cmp(&(rank(b.kind), name(b)))
        });

        let mut counts = WorktreeCounts {
            git_total: git_list.len() as u32,
            ..WorktreeCounts::default()
        };
        for item in &items {
            match item.kind {
                ItemKind::Unmanaged => counts.unmanaged += 1,
                ItemKind::ManagedMissing => counts.missing += 1,
                _ => {}
            }
            if let Some(m) = &item.managed {
                counts.managed += 1;
                match m.worktree.status {
                    OperationalStatus::Active => counts.active += 1,
                    OperationalStatus::Frozen => counts.frozen += 1,
                    OperationalStatus::Stopped => counts.stopped += 1,
                    OperationalStatus::Completed => counts.completed += 1,
                }
            }
            if item
                .git_summary
                .as_ref()
                .and_then(|d| d.data.as_ref())
                .is_some_and(|s| s.changes > 0 || s.conflicts > 0)
            {
                counts.with_changes += 1;
            }
        }
        Ok(WorktreeOverview {
            project_id: project_id.into(),
            project_available: available,
            items,
            counts,
            git_error,
        })
    }

    /// Worktrees vinculados a uma Session (leve: banco + existência do binding; sem Git).
    pub fn worktrees_for_session(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> HubResult<Vec<SessionWorktree>> {
        let bindings = self.bindings(project_id)?;
        Ok(load_managed(&self.conn, Some(project_id))?
            .into_iter()
            .filter(|w| w.session_id.as_deref() == Some(session_id))
            .map(|w| SessionWorktree {
                available: bindings.get(&w.id).is_some_and(|p| Path::new(p).exists()),
                id: w.id,
                display_name: w.display_name,
                status: w.status,
                branch_hint: w.branch_hint,
                block_id: w.block_id,
            })
            .collect())
    }

    fn new_managed(&self, project_id: &str, n: NewManaged<'_>) -> HubResult<ManagedWorktree> {
        let NewManaged {
            name,
            description,
            branch,
            detached_head,
            session_id,
            block_id,
        } = n;
        let project = self.project(project_id)?;
        let stamp = now(&self.conn)?;
        Ok(ManagedWorktree {
            id: new_id(),
            project_id: project_id.into(),
            display_name: check_text("Nome", name, MAX_NAME, true)?,
            description: check_text("Descrição", description, MAX_DESCRIPTION, false)?,
            status: OperationalStatus::Active,
            state_reason: None,
            result: None,
            repository_locator: project.locator.clone(),
            branch_hint: (!branch.is_empty()).then(|| branch.to_string()),
            detached_head_hint: detached_head.map(str::to_string),
            session_id: session_id.map(str::to_string),
            block_id: block_id.map(str::to_string),
            created_at: stamp.clone(),
            updated_at: stamp,
            completed_at: None,
            events: vec![],
        })
    }

    /// Grava metadata + binding + eventos numa transação (adotar e criar).
    fn register_managed(
        &mut self,
        w: &ManagedWorktree,
        local_path: &str,
        created: bool,
    ) -> HubResult<()> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        upsert(&tx, w)?;
        tx.execute(
            "INSERT INTO worktree_bindings(worktree_id,local_path) VALUES(?1,?2)",
            params![w.id, local_path],
        )
        .map_err(|_| "Este caminho já está vinculado a outro worktree gerenciado.".to_string())?;
        wt_event(
            &tx,
            &w.id,
            if created {
                WorktreeEventType::WorktreeCreated
            } else {
                WorktreeEventType::WorktreeAdopted
            },
            &[
                ("name", &w.display_name),
                ("branch", w.branch_hint.as_deref().unwrap_or("")),
            ],
        )?;
        relation_events(
            &tx,
            &w.id,
            &w.display_name,
            (&None, &None),
            (&w.session_id, &w.block_id),
        )?;
        activity(
            &tx,
            &w.project_id,
            &format!(
                "Worktree “{}” {}",
                w.display_name,
                if created { "criada" } else { "adotada" }
            ),
        )?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// ADOTAR (ação explícita): cria a metadata de um Git worktree adicional real. Nunca o principal.
    pub fn worktree_adopt(
        &mut self,
        project_id: &str,
        path: &str,
        name: Option<&str>,
        session_id: Option<&str>,
        block_id: Option<&str>,
    ) -> HubResult<ManagedWorktree> {
        let dir = self.worktree_repo_dir(project_id)?;
        let list = git::worktrees(&dir)?;
        let target = list
            .iter()
            .find(|g| inspect::same_folder_path(&g.path, Path::new(path)))
            .ok_or("O Git não lista um worktree neste caminho.")?;
        if target.is_primary {
            return Err(
                "O checkout principal não participa do ciclo operacional e não pode ser adotado."
                    .into(),
            );
        }
        if target.bare {
            return Err("Um repositório bare não é um worktree adotável.".into());
        }
        let local = plain(&target.path);
        let bound: Option<String> = self
            .conn
            .query_row(
                "SELECT worktree_id FROM worktree_bindings WHERE local_path=?1",
                [&local],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if bound.is_some() {
            return Err("Este worktree já é gerenciado pelo LKR LAB.".into());
        }
        check_relation(&self.conn, project_id, session_id, block_id)?;
        let name = name
            .filter(|n| !n.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| default_name(target));
        let head = (target.detached && !target.head.is_empty()).then_some(target.head.as_str());
        let w = self.new_managed(
            project_id,
            NewManaged {
                name: &name,
                description: "",
                branch: &target.branch,
                detached_head: head,
                session_id,
                block_id,
            },
        )?;
        self.register_managed(&w, &local, false)?;
        load_one(&self.conn, &w.id)
    }

    /// NOVO WORKTREE (ação explícita e mutante): `git worktree add` + metadata ACTIVE + binding.
    /// Git e SQLite não têm transação conjunta: se o Git criar mas a metadata falhar, NADA é
    /// desfeito (sem remoção destrutiva); o worktree aparecerá como NÃO GERENCIADO.
    pub fn worktree_create(
        &mut self,
        project_id: &str,
        req: CreateRequest,
    ) -> HubResult<ManagedWorktree> {
        let dir = self.worktree_repo_dir(project_id)?;
        let session = req.session_id.as_deref().filter(|s| !s.is_empty());
        let block = req.block_id.as_deref().filter(|s| !s.is_empty());
        // Antes de tocar no Git: relação e textos válidos (nada de artefato órfão por erro nosso).
        check_relation(&self.conn, project_id, session, block)?;
        let name = if req.display_name.trim().is_empty() {
            req.branch.trim().to_string()
        } else {
            req.display_name.clone()
        };
        let draft = self.new_managed(
            project_id,
            NewManaged {
                name: &name,
                description: &req.description,
                branch: req.branch.trim(),
                detached_head: None,
                session_id: session,
                block_id: block,
            },
        )?;
        let spec = match req.mode {
            CreateMode::NewBranch => NewWorktree::NewBranch {
                branch: req.branch.trim(),
                base: if req.base_ref.trim().is_empty() {
                    "HEAD"
                } else {
                    req.base_ref.trim()
                },
            },
            CreateMode::ExistingBranch => NewWorktree::ExistingBranch {
                branch: req.branch.trim(),
            },
        };
        git::add_worktree(&dir, &req.path, spec)?;
        let local = plain(&req.path);
        self.register_managed(&draft, &local, true).map_err(|e| {
            format!(
                "O Git Worktree foi criado, mas não foi adotado pelo LKR LAB ({e}). Ele aparecerá como NÃO GERENCIADO na próxima leitura; nada foi removido."
            )
        })?;
        load_one(&self.conn, &draft.id)
    }

    pub fn managed_worktree(&self, id: &str) -> HubResult<ManagedWorktree> {
        load_one(&self.conn, id)
    }

    /// Transição operacional (metadata do LKR LAB). Nunca executa Git nem toca no filesystem.
    /// `reason` (opcional) vale para congelar/parar; `result` (opcional) para finalizar.
    pub fn worktree_set_state(
        &mut self,
        id: &str,
        target: OperationalStatus,
        reason: &str,
        result: &str,
    ) -> HubResult<ManagedWorktree> {
        let reason = check_text("Motivo", reason, MAX_REASON, false)?;
        let result = check_text("Resultado", result, MAX_RESULT, false)?;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let w = load_one(&tx, id)?;
        if w.status == OperationalStatus::Completed {
            return Err("Worktree finalizado é terminal e não volta a outro estado.".into());
        }
        if w.status == target {
            return Err("O worktree já está neste estado.".into());
        }
        if !w.status.can_become(target) {
            return Err(format!(
                "Transição não permitida: {} → {}.",
                w.status.as_str(),
                target.as_str()
            ));
        }
        let stamp = now(&tx)?;
        let opt = |s: &str| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        };
        match target {
            OperationalStatus::Completed => tx.execute(
                "UPDATE managed_worktrees SET operational_status='completed', state_reason=NULL, result=?2, completed_at=?3, updated_at=?3 WHERE id=?1",
                params![id, opt(&result), stamp],
            ),
            OperationalStatus::Active => tx.execute(
                "UPDATE managed_worktrees SET operational_status='active', state_reason=NULL, updated_at=?2 WHERE id=?1",
                params![id, stamp],
            ),
            other => tx.execute(
                "UPDATE managed_worktrees SET operational_status=?2, state_reason=?3, updated_at=?4 WHERE id=?1",
                params![id, other.as_str(), opt(&reason), stamp],
            ),
        }
        .map_err(|e| e.to_string())?;
        wt_event(
            &tx,
            id,
            WorktreeEventType::WorktreeStateChanged,
            &[
                ("from", w.status.as_str()),
                ("to", target.as_str()),
                ("reason", &reason),
            ],
        )?;
        if target == OperationalStatus::Completed {
            wt_event(
                &tx,
                id,
                WorktreeEventType::WorktreeCompleted,
                &[("result", &result)],
            )?;
        }
        activity(
            &tx,
            &w.project_id,
            &format!("Worktree “{}” → {}", w.display_name, target.as_str()),
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        load_one(&self.conn, id)
    }

    /// Renomeia e/ou descreve (metadata). Finalizado é histórico e não muda.
    pub fn worktree_update(
        &mut self,
        id: &str,
        display_name: &str,
        description: &str,
    ) -> HubResult<ManagedWorktree> {
        let name = check_text("Nome", display_name, MAX_NAME, true)?;
        let description = check_text("Descrição", description, MAX_DESCRIPTION, false)?;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let w = load_one(&tx, id)?;
        if w.status == OperationalStatus::Completed {
            return Err("Worktree finalizado é histórico e não pode ser alterado.".into());
        }
        if name == w.display_name && description == w.description {
            return Err("Nada foi alterado.".into());
        }
        tx.execute(
            "UPDATE managed_worktrees SET display_name=?2, description=?3 WHERE id=?1",
            params![id, name, description],
        )
        .map_err(|e| e.to_string())?;
        touch(&tx, id)?;
        if name != w.display_name {
            wt_event(
                &tx,
                id,
                WorktreeEventType::WorktreeRenamed,
                &[("from", &w.display_name), ("to", &name)],
            )?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        load_one(&self.conn, id)
    }

    /// Vincula/desvincula Session e Block (nunca altera o estado de nenhum dos dois).
    /// Sem Session o bloco também some; com bloco, a Session é obrigatória.
    pub fn worktree_set_relation(
        &mut self,
        id: &str,
        session_id: Option<&str>,
        block_id: Option<&str>,
    ) -> HubResult<ManagedWorktree> {
        let session_id = session_id.filter(|s| !s.is_empty());
        let block_id = block_id.filter(|s| !s.is_empty());
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let w = load_one(&tx, id)?;
        if w.status == OperationalStatus::Completed {
            return Err("Worktree finalizado é histórico e não pode ser alterado.".into());
        }
        let block_id = if session_id.is_none() { None } else { block_id };
        check_relation(&tx, &w.project_id, session_id, block_id)?;
        let new_s = session_id.map(str::to_string);
        let new_b = block_id.map(str::to_string);
        if new_s == w.session_id && new_b == w.block_id {
            return Err("O vínculo já está assim.".into());
        }
        tx.execute(
            "UPDATE managed_worktrees SET session_id=?2, block_id=?3 WHERE id=?1",
            params![id, new_s, new_b],
        )
        .map_err(|e| e.to_string())?;
        touch(&tx, id)?;
        relation_events(
            &tx,
            id,
            &w.display_name,
            (&w.session_id, &w.block_id),
            (&new_s, &new_b),
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        load_one(&self.conn, id)
    }

    /// LOCALIZAR: casa um Git worktree real desta máquina com a metadata que veio do workspace.
    /// O UUID não muda; nada é criado. Recusa se a branch/HEAD contradiz a dica.
    pub fn worktree_locate(&mut self, id: &str, path: &str) -> HubResult<ManagedWorktree> {
        let w = load_one(&self.conn, id)?;
        let project = self.project(&w.project_id)?;
        let dir = projects::local_dir(&project)?;
        let list = git::worktrees(&dir)?;
        let target = list
            .iter()
            .find(|g| inspect::same_folder_path(&g.path, Path::new(path)))
            .ok_or("O Git não lista um worktree neste caminho.")?;
        if target.is_primary || target.bare {
            return Err(
                "Escolha um worktree adicional (o checkout principal não é gerenciado).".into(),
            );
        }
        if let (Some(want), Some(have)) = (&w.repository_locator, &project.locator) {
            if want != have {
                return Err("Este projeto aponta para outro repositório que o do worktree; nada foi vinculado.".into());
            }
        }
        match (&w.branch_hint, target.branch.as_str()) {
            (Some(hint), have) if !have.is_empty() && hint != have => {
                return Err(format!(
                    "A branch deste worktree é “{have}”, mas o LKR LAB esperava “{hint}”. Nada foi vinculado (um novo UUID não é criado por divergência de branch)."
                ));
            }
            (Some(hint), "") => {
                return Err(format!(
                    "O worktree está em detached HEAD, mas o LKR LAB esperava a branch “{hint}”. Nada foi vinculado."
                ));
            }
            _ => {}
        }
        if let (Some(hint), true) = (&w.detached_head_hint, target.branch.is_empty()) {
            if !target.head.starts_with(hint.as_str()) && !hint.starts_with(&target.head) {
                return Err(
                    "O commit deste worktree detached é diferente do esperado; nada foi vinculado."
                        .into(),
                );
            }
        }
        let local = plain(&target.path);
        let other: Option<String> = self
            .conn
            .query_row(
                "SELECT worktree_id FROM worktree_bindings WHERE local_path=?1",
                [&local],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if other.as_deref().is_some_and(|o| o != id) {
            return Err("Este caminho já está vinculado a outro worktree gerenciado.".into());
        }
        self.conn
            .execute(
                "INSERT INTO worktree_bindings(worktree_id,local_path) VALUES(?1,?2) \
                 ON CONFLICT(worktree_id) DO UPDATE SET local_path=excluded.local_path, updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')",
                params![id, local],
            )
            .map_err(|e| e.to_string())?;
        load_one(&self.conn, id)
    }

    /// REMOVER DO GIT (explícito, separado de finalizar): reaproveita as proteções (não remove o
    /// principal, o atual, bloqueado ou sujo; sem `--force`). Depois some SÓ o binding local: a
    /// metadata e os eventos ficam e o worktree passa a "não localizado". A branch fica.
    pub fn worktree_git_remove(
        &mut self,
        project_id: &str,
        path: &str,
        confirmed: bool,
    ) -> HubResult<()> {
        let dir = self.worktree_repo_dir(project_id)?;
        git::remove_worktree(&dir, path, confirmed)?;
        let bound: Vec<(String, String)> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT b.worktree_id, b.local_path FROM worktree_bindings b \
                     JOIN managed_worktrees w ON w.id=b.worktree_id WHERE w.project_id=?1",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            rows
        };
        for (wid, local) in bound {
            if inspect::same_folder_path(&local, Path::new(path)) {
                self.conn
                    .execute("DELETE FROM worktree_bindings WHERE worktree_id=?1", [wid])
                    .map_err(|e| e.to_string())?;
            }
        }
        self.conn
            .execute(
                "INSERT INTO activities(project_id,action) VALUES(?1,'Worktree removido do Git (metadata preservada)')",
                [project_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
