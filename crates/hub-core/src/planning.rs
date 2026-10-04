//! Planejamento do LKR LAB (Concept 09): a fila ordenada de features AINDA NÃO iniciadas de um Project.
//!
//! Responde "o que vem depois?"; o DDAE responde "o que estamos executando agora?".
//! Planning Item != Session != Block != Worktree != issue do Git.
//!
//! Regras canônicas:
//! * Só dois estados são GRAVADOS (`open`, `cancelled`). Planejado / Em execução / Concluído são
//!   DERIVADOS da Session vinculada (`derive_phase`) e nunca persistidos. Por isso não existe um
//!   evento persistido de "concluído": a conclusão é uma função da Session finalizada.
//! * O vínculo mora na SESSION (`ddae_sessions.planning_item_id`): 1 item : 0..1 Session. A Session
//!   nasce do fluxo único de criação do DDAE (`ddae::create_session_in`), na mesma transação.
//! * Sem prioridade: a ordem manual (`position`, com espaçamento) é a prioridade prática. Reordenar
//!   não gera evento (a posição é estado, não fato).
//! * Cancelar só item sem Session; restaurável. Não existe exclusão destrutiva.
//! * Listar é 100% leitura: nada é gravado ao abrir a página.
//! * Estado PORTÁTIL (classe B): sem path, Machine ID, hostname ou IP.
use crate::{
    database::Database,
    ddae::{self, check_payload, check_text, new_id, now, Progress, SessionStatus},
    HubResult,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_TITLE: usize = 120;
pub const MAX_DESCRIPTION: usize = 2_000;
pub const MAX_REASON: usize = 500;
pub const MAX_ITEMS: usize = 10_000;
pub const MAX_EVENTS: usize = 50_000;
const MAX_TIMESTAMP: usize = 40;
/// Espaçamento entre posições: mover um item toca uma única linha na maioria dos casos.
pub const POSITION_GAP: i64 = 1_000;
/// Mensagem exibida quando Iniciar fica indisponível por haver Session ativa.
pub const ACTIVE_SESSION_REASON: &str = "Já existe uma Session ativa neste projeto.";

// ------------------------------------------------------------------ modelo

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredStatus {
    Open,
    Cancelled,
}

impl StoredStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Cancelled => "cancelled",
        }
    }
    fn parse(value: &str) -> HubResult<Self> {
        match value {
            "open" => Ok(Self::Open),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("Estado de planejamento desconhecido ({other}).")),
        }
    }
}

/// O Planning Item como é guardado E como viaja no workspace (o mesmo formato). Não contém Session:
/// o vínculo vive na Session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningItem {
    pub id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub position: i64,
    pub stored_status: StoredStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled_at: Option<String>,
}

/// Eventos semânticos do item (portáteis, append-only). Reordenar não gera evento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanningEventType {
    PlanningItemCreated,
    PlanningItemUpdated,
    PlanningItemCancelled,
    PlanningItemRestored,
    PlanningItemSessionLinked,
}

impl PlanningEventType {
    pub fn as_str(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningEvent {
    pub id: String,
    pub item_id: String,
    #[serde(rename = "type")]
    pub kind: PlanningEventType,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub payload: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub created_at: String,
}

/// Fase EXIBIDA do item. Função pura de (estado gravado, estado da Session): nunca é persistida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Planned,
    Executing,
    Completed,
    Cancelled,
}

/// `cancelled` -> CANCELADO; `open` sem Session -> PLANEJADO; Session `active|frozen|stopped` ->
/// EM EXECUÇÃO (o estado real da Session aparece como subestado); Session `completed` -> CONCLUÍDO.
pub fn derive_phase(stored: StoredStatus, session: Option<SessionStatus>) -> Phase {
    match (stored, session) {
        (StoredStatus::Cancelled, _) => Phase::Cancelled,
        (StoredStatus::Open, None) => Phase::Planned,
        (StoredStatus::Open, Some(SessionStatus::Completed)) => Phase::Completed,
        (StoredStatus::Open, Some(_)) => Phase::Executing,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveTo {
    Up,
    Down,
    Top,
}

// ------------------------------------------------------------------ portabilidade

fn trim_opt(v: &mut Option<String>) {
    *v = v
        .take()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
}

/// Forma canônica: textos aparados; itens por (projeto, posição, id); eventos por (created_at, id) —
/// nunca pela ordem do SQLite.
pub fn normalize(items: &mut [PlanningItem], events: &mut [PlanningEvent]) {
    for i in items.iter_mut() {
        i.title = i.title.trim().to_string();
        i.description = i.description.trim().to_string();
        trim_opt(&mut i.cancel_reason);
        trim_opt(&mut i.cancelled_at);
    }
    items.sort_by(|a, b| {
        (&a.project_id, a.position, &a.id).cmp(&(&b.project_id, b.position, &b.id))
    });
    events.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
}

/// Regras do estado portátil recebido (arquivo versionado, não confiável). Cobre: ids únicos,
/// Project existente, posições únicas por Project, cancelado coerente, eventos de itens existentes,
/// e a relação com as Sessions (mesmo Project, item aberto, 1 item : 0..1 Session).
pub fn validate_portable(
    items: &[PlanningItem],
    events: &[PlanningEvent],
    sessions: &[ddae::Session],
    project_ids: &HashSet<&str>,
) -> HubResult<()> {
    let fail =
        |m: String| -> HubResult<()> { Err(format!("Workspace inválido: planejamento: {m}")) };
    if items.len() > MAX_ITEMS {
        return fail(format!("máximo de {MAX_ITEMS} itens"));
    }
    if events.len() > MAX_EVENTS {
        return fail(format!("máximo de {MAX_EVENTS} eventos"));
    }
    let mut ids: HashSet<&str> = HashSet::new();
    let mut positions: HashSet<(&str, i64)> = HashSet::new();
    for i in items {
        let at = &i.title;
        let map = |e: String| format!("Workspace inválido: planejamento: {at}: {e}");
        if !crate::portable::valid_id(&i.id) {
            return fail(format!("id inválido ({})", i.id));
        }
        if !ids.insert(i.id.as_str()) {
            return fail(format!("id duplicado ({})", i.id));
        }
        if !project_ids.contains(i.project_id.as_str()) {
            return fail(format!(
                "{at}: referencia um projeto que não está no workspace"
            ));
        }
        check_text("título", &i.title, MAX_TITLE, true).map_err(map)?;
        check_text("descrição", &i.description, MAX_DESCRIPTION, false).map_err(map)?;
        if let Some(reason) = &i.cancel_reason {
            check_text("motivo", reason, MAX_REASON, false).map_err(map)?;
        }
        if i.position < 1 || !positions.insert((i.project_id.as_str(), i.position)) {
            return fail(format!("{at}: posição inválida ou repetida no projeto"));
        }
        if i.created_at.len() > MAX_TIMESTAMP
            || i.updated_at.len() > MAX_TIMESTAMP
            || i.cancelled_at
                .as_deref()
                .is_some_and(|c| c.len() > MAX_TIMESTAMP)
        {
            return fail(format!("{at}: data inválida"));
        }
        if (i.stored_status == StoredStatus::Cancelled) != i.cancelled_at.is_some() {
            return fail(format!(
                "{at}: cancelado exige cancelledAt e item aberto não pode tê-lo"
            ));
        }
        if i.cancel_reason.is_some() && i.stored_status != StoredStatus::Cancelled {
            return fail(format!("{at}: só item cancelado tem motivo"));
        }
    }
    let by_id: HashMap<&str, &PlanningItem> = items.iter().map(|i| (i.id.as_str(), i)).collect();
    let mut linked: HashSet<&str> = HashSet::new();
    for s in sessions {
        let Some(item_id) = s.planning_item_id.as_deref() else {
            continue;
        };
        let at = s.label();
        let Some(item) = by_id.get(item_id) else {
            return fail(format!(
                "{at}: referencia um item de planejamento que não está no workspace"
            ));
        };
        if item.project_id != s.project_id {
            return fail(format!("{at}: o item pertence a outro projeto"));
        }
        if item.stored_status == StoredStatus::Cancelled {
            return fail(format!("{at}: item cancelado não pode ter Session"));
        }
        if !linked.insert(item_id) {
            return fail(format!("{at}: o item já está vinculado a outra Session"));
        }
    }
    let mut event_ids: HashSet<&str> = HashSet::new();
    for e in events {
        if !crate::portable::valid_id(&e.id) {
            return fail(format!("id de evento inválido ({})", e.id));
        }
        if !event_ids.insert(e.id.as_str()) || ids.contains(e.id.as_str()) {
            return fail(format!("id de evento duplicado ({})", e.id));
        }
        if !ids.contains(e.item_id.as_str()) {
            return fail(format!("evento {} referencia um item inexistente", e.id));
        }
        if e.created_at.len() > MAX_TIMESTAMP {
            return fail(format!("evento {}: data inválida", e.id));
        }
        check_payload(&e.payload).map_err(|m| format!("Workspace inválido: planejamento: {m}"))?;
    }
    Ok(())
}

// ------------------------------------------------------------------ persistência

const COLUMNS: &str =
    "id,project_id,title,description,position,stored_status,cancel_reason,created_at,updated_at,cancelled_at";

fn json_text<T: Serialize>(v: &T) -> HubResult<String> {
    serde_json::to_string(v).map_err(|e| e.to_string())
}

fn row_to_item(r: &rusqlite::Row) -> rusqlite::Result<PlanningItem> {
    let status: String = r.get(5)?;
    let stored_status = StoredStatus::parse(&status).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::<dyn std::error::Error + Send + Sync>::from(e),
        )
    })?;
    Ok(PlanningItem {
        id: r.get(0)?,
        project_id: r.get(1)?,
        title: r.get(2)?,
        description: r.get(3)?,
        position: r.get(4)?,
        stored_status,
        cancel_reason: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        cancelled_at: r.get(9)?,
    })
}

fn load_items(conn: &Connection, project_id: Option<&str>) -> HubResult<Vec<PlanningItem>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM planning_items {} ORDER BY project_id, position",
        if project_id.is_some() {
            "WHERE project_id=?1"
        } else {
            ""
        }
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = if let Some(id) = project_id {
        stmt.query_map([id], row_to_item)
    } else {
        stmt.query_map([], row_to_item)
    }
    .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn load_item(conn: &Connection, id: &str) -> HubResult<PlanningItem> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM planning_items WHERE id=?1"),
        [id],
        row_to_item,
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Item de planejamento não encontrado.".to_string())
}

fn load_events(conn: &Connection, item_id: Option<&str>) -> HubResult<Vec<PlanningEvent>> {
    let sql = format!(
        "SELECT id,item_id,event_type,payload,created_at FROM planning_events {} ORDER BY created_at, id",
        if item_id.is_some() { "WHERE item_id=?1" } else { "" }
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let map = |r: &rusqlite::Row| -> rusqlite::Result<(String, String, String, String, String)> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    };
    let rows = if let Some(id) = item_id {
        stmt.query_map([id], map)
    } else {
        stmt.query_map([], map)
    }
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(id, item_id, kind, payload, created_at)| {
            Ok(PlanningEvent {
                id,
                item_id,
                kind: serde_json::from_value(serde_json::Value::String(kind))
                    .map_err(|e| e.to_string())?,
                payload: serde_json::from_str(&payload).map_err(|e| e.to_string())?,
                created_at,
            })
        })
        .collect()
}

/// Estado portátil para exportar ao workspace.
pub fn export(conn: &Connection) -> HubResult<(Vec<PlanningItem>, Vec<PlanningEvent>)> {
    Ok((load_items(conn, None)?, load_events(conn, None)?))
}

/// O workspace manda: apaga e reinsere (nenhum estado local de máquina existe aqui). Chamado ANTES do
/// DDAE: as Sessions apontam para os itens, e os gatilhos do banco exigem que eles já existam.
pub fn replace_all(
    tx: &Transaction,
    items: &[PlanningItem],
    events: &[PlanningEvent],
) -> HubResult<()> {
    tx.execute("DELETE FROM planning_items", [])
        .map_err(|e| e.to_string())?;
    let stamp = now(tx)?;
    for i in items {
        let created = if i.created_at.is_empty() {
            stamp.clone()
        } else {
            i.created_at.clone()
        };
        let updated = if i.updated_at.is_empty() {
            created.clone()
        } else {
            i.updated_at.clone()
        };
        tx.execute(
            "INSERT INTO planning_items(id,project_id,title,description,position,stored_status,cancel_reason,created_at,updated_at,cancelled_at) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                i.id,
                i.project_id,
                i.title,
                i.description,
                i.position,
                i.stored_status.as_str(),
                i.cancel_reason,
                created,
                updated,
                i.cancelled_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    for e in events {
        tx.execute(
            "INSERT OR IGNORE INTO planning_events(id,item_id,event_type,payload,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![e.id, e.item_id, e.kind.as_str(), json_text(&e.payload)?, e.created_at],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn record_event(
    tx: &Transaction,
    item_id: &str,
    kind: PlanningEventType,
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
        "INSERT INTO planning_events(id,item_id,event_type,payload,created_at) VALUES(?1,?2,?3,?4,?5)",
        params![new_id(), item_id, kind.as_str(), json_text(&payload)?, now(tx)?],
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

fn session_of_item(conn: &Connection, item_id: &str) -> HubResult<Option<(String, u32)>> {
    conn.query_row(
        "SELECT id,number FROM ddae_sessions WHERE planning_item_id=?1",
        [item_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ leitura agregada

/// A Session vinculada, já derivada (progresso é o da Session, nunca do item).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    pub id: String,
    pub number: u32,
    pub label: String,
    pub title: String,
    pub status: SessionStatus,
    pub progress: Progress,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSession {
    pub id: String,
    pub number: u32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningRow {
    #[serde(flatten)]
    pub item: PlanningItem,
    pub phase: Phase,
    pub session: Option<SessionRef>,
    pub can_start: bool,
    pub disabled_reason: Option<String>,
    pub can_edit: bool,
    pub can_cancel: bool,
    pub can_restore: bool,
    pub can_move_up: bool,
    pub can_move_down: bool,
    pub last_activity_at: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningCounts {
    /// planejados + em execução + concluídos (cancelados ficam de fora).
    pub operational: u32,
    pub planned: u32,
    pub executing: u32,
    pub completed: u32,
    pub cancelled: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub can_start: bool,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningOverview {
    pub project_id: String,
    pub items: Vec<PlanningRow>,
    pub counts: PlanningCounts,
    /// PRÓXIMO do Planejamento: o primeiro item PLANEJADO da fila (mesmo sem poder iniciar).
    pub next: Option<NextItem>,
    /// A Session ACTIVE do Project (qualquer origem), que bloqueia Iniciar.
    pub active_session: Option<ActiveSession>,
}

/// Versão leve para o Project Control Center e a Próxima ação.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningSummary {
    pub counts: PlanningCounts,
    pub next: Option<NextItem>,
    pub active_session: Option<ActiveSession>,
}

/// Item de origem mostrado na lista DDAE e no detalhe da Session.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningRef {
    pub id: String,
    pub title: String,
    pub phase: Phase,
}

/// Rascunho do formulário de Nova Session aberto por Iniciar (nunca cria nada).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDraft {
    pub item_id: String,
    pub title: String,
    pub objective: String,
    pub can_start: bool,
    pub disabled_reason: Option<String>,
}

fn latest<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    values.into_iter().max().unwrap_or_default().to_string()
}

/// Função pura: itens + Sessions do Project -> a leitura agregada. Sem I/O, sem gravar nada.
pub fn build_overview(
    project_id: &str,
    items: Vec<PlanningItem>,
    sessions: &[ddae::Session],
) -> PlanningOverview {
    let active = sessions
        .iter()
        .find(|s| s.status == SessionStatus::Active)
        .map(|s| ActiveSession {
            id: s.id.clone(),
            number: s.number,
            label: s.label(),
        });
    let by_item: HashMap<&str, &ddae::Session> = sessions
        .iter()
        .filter_map(|s| s.planning_item_id.as_deref().map(|id| (id, s)))
        .collect();
    let phase_of = |i: &PlanningItem| {
        derive_phase(
            i.stored_status,
            by_item.get(i.id.as_str()).map(|s| s.status),
        )
    };
    // A fila de PLANEJADOS na ordem manual (mover só reordena entre eles).
    let planned: Vec<&str> = items
        .iter()
        .filter(|i| phase_of(i) == Phase::Planned)
        .map(|i| i.id.as_str())
        .collect();
    let start_block = active.as_ref().map(|_| ACTIVE_SESSION_REASON.to_string());
    let mut counts = PlanningCounts::default();
    let mut rows = Vec::with_capacity(items.len());
    for item in &items {
        let phase = phase_of(item);
        match phase {
            Phase::Planned => counts.planned += 1,
            Phase::Executing => counts.executing += 1,
            Phase::Completed => counts.completed += 1,
            Phase::Cancelled => counts.cancelled += 1,
        }
        let session = by_item.get(item.id.as_str()).map(|s| SessionRef {
            id: s.id.clone(),
            number: s.number,
            label: s.label(),
            title: s.title.clone(),
            status: s.status,
            progress: s.progress(),
        });
        let planned_index = planned.iter().position(|id| *id == item.id);
        let is_planned = phase == Phase::Planned;
        let last_activity_at = latest(
            [
                Some(item.updated_at.as_str()),
                by_item.get(item.id.as_str()).map(|s| s.updated_at.as_str()),
                by_item
                    .get(item.id.as_str())
                    .and_then(|s| s.completed_at.as_deref()),
            ]
            .into_iter()
            .flatten(),
        );
        rows.push(PlanningRow {
            item: item.clone(),
            phase,
            session,
            can_start: is_planned && active.is_none(),
            disabled_reason: if is_planned {
                start_block.clone()
            } else {
                None
            },
            can_edit: is_planned,
            can_cancel: is_planned,
            can_restore: phase == Phase::Cancelled,
            can_move_up: planned_index.is_some_and(|n| n > 0),
            can_move_down: planned_index.is_some_and(|n| n + 1 < planned.len()),
            last_activity_at,
        });
    }
    counts.operational = counts.planned + counts.executing + counts.completed;
    let next = rows
        .iter()
        .find(|r| r.phase == Phase::Planned)
        .map(|r| NextItem {
            id: r.item.id.clone(),
            title: r.item.title.clone(),
            description: r.item.description.clone(),
            can_start: r.can_start,
            disabled_reason: r.disabled_reason.clone(),
        });
    PlanningOverview {
        project_id: project_id.into(),
        items: rows,
        counts,
        next,
        active_session: active,
    }
}

/// Origem de Planejamento das Sessions de um Project (para lista e detalhe do DDAE).
pub(crate) fn refs_for_project(
    conn: &Connection,
    project_id: &str,
    sessions: &[ddae::Session],
) -> HubResult<HashMap<String, PlanningRef>> {
    if !sessions.iter().any(|s| s.planning_item_id.is_some()) {
        return Ok(HashMap::new());
    }
    let items: HashMap<String, PlanningItem> = load_items(conn, Some(project_id))?
        .into_iter()
        .map(|i| (i.id.clone(), i))
        .collect();
    Ok(sessions
        .iter()
        .filter_map(|s| {
            let item = items.get(s.planning_item_id.as_deref()?)?;
            Some((
                s.id.clone(),
                PlanningRef {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    phase: derive_phase(item.stored_status, Some(s.status)),
                },
            ))
        })
        .collect())
}

// ------------------------------------------------------------------ ordenação

/// Posição para `id` logo ANTES/DEPOIS de `anchor`; reescreve a fila inteira só se não houver espaço.
fn place(tx: &Transaction, project_id: &str, id: &str, anchor: &str, after: bool) -> HubResult<()> {
    let mut others: Vec<(String, i64)> = load_items(tx, Some(project_id))?
        .into_iter()
        .filter(|i| i.id != id)
        .map(|i| (i.id, i.position))
        .collect();
    let at = others
        .iter()
        .position(|(other, _)| other == anchor)
        .ok_or_else(|| "Item de referência não encontrado.".to_string())?;
    let insert_at = if after { at + 1 } else { at };
    let low = if insert_at == 0 {
        0
    } else {
        others[insert_at - 1].1
    };
    let fit = if insert_at == others.len() {
        Some(low + POSITION_GAP)
    } else {
        let high = others[insert_at].1;
        (high - low >= 2).then(|| low + (high - low) / 2)
    };
    if let Some(position) = fit {
        tx.execute(
            "UPDATE planning_items SET position=?2 WHERE id=?1",
            params![id, position],
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }
    // Sem espaço: renormaliza (preservando a ordem relativa de TODOS, inclusive cancelados). O
    // deslocamento evita colisão transitória com UNIQUE(project_id, position).
    others.insert(insert_at, (id.to_string(), 0));
    tx.execute(
        "UPDATE planning_items SET position=position+4000000000000 WHERE project_id=?1",
        [project_id],
    )
    .map_err(|e| e.to_string())?;
    for (n, (item, _)) in others.iter().enumerate() {
        tx.execute(
            "UPDATE planning_items SET position=?2 WHERE id=?1",
            params![item, (n as i64 + 1) * POSITION_GAP],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ------------------------------------------------------------------ operações

impl Database {
    /// Leitura agregada do Planejamento de um Project. 100% leitura.
    pub fn planning_overview(&self, project_id: &str) -> HubResult<PlanningOverview> {
        ddae::project_exists(&self.conn, project_id)?;
        let items = load_items(&self.conn, Some(project_id))?;
        let sessions = ddae::load_sessions(&self.conn, Some(project_id))?;
        Ok(build_overview(project_id, items, &sessions))
    }

    /// Só contagens, PRÓXIMO e a Session ativa (Project Control Center / Próxima ação).
    pub fn planning_summary(&self, project_id: &str) -> HubResult<PlanningSummary> {
        let o = self.planning_overview(project_id)?;
        Ok(PlanningSummary {
            counts: o.counts,
            next: o.next,
            active_session: o.active_session,
        })
    }

    pub fn planning_item(&self, id: &str) -> HubResult<PlanningItem> {
        load_item(&self.conn, id)
    }

    pub fn planning_events(&self, item_id: &str) -> HubResult<Vec<PlanningEvent>> {
        load_item(&self.conn, item_id)?;
        load_events(&self.conn, Some(item_id))
    }

    /// Captura rápida: título obrigatório, descrição opcional; vai para o FIM da fila.
    pub fn planning_create_item(
        &mut self,
        project_id: &str,
        title: &str,
        description: &str,
    ) -> HubResult<PlanningItem> {
        let title = check_text("Título", title, MAX_TITLE, true)?;
        let description = check_text("Descrição", description, MAX_DESCRIPTION, false)?;
        ddae::project_exists(&self.conn, project_id)?;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let count: i64 = tx
            .query_row(
                "SELECT count(*) FROM planning_items WHERE project_id=?1",
                [project_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if count as usize >= MAX_ITEMS {
            return Err(format!(
                "Limite de {MAX_ITEMS} itens de planejamento por workspace."
            ));
        }
        let position: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(position),0)+?2 FROM planning_items WHERE project_id=?1",
                params![project_id, POSITION_GAP],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let stamp = now(&tx)?;
        let id = new_id();
        tx.execute(
            "INSERT INTO planning_items(id,project_id,title,description,position,stored_status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'open',?6,?6)",
            params![id, project_id, title, description, position, stamp],
        )
        .map_err(|e| e.to_string())?;
        record_event(
            &tx,
            &id,
            PlanningEventType::PlanningItemCreated,
            &[("title", &title)],
        )?;
        activity(
            &tx,
            project_id,
            &format!("Planejamento: item \"{title}\" criado"),
        )?;
        let item = load_item(&tx, &id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(item)
    }

    /// Edita título/descrição. Só item PLANEJADO: com Session, o título/objetivo passam a viver nela.
    pub fn planning_update_item(
        &mut self,
        id: &str,
        title: &str,
        description: &str,
    ) -> HubResult<PlanningItem> {
        let title = check_text("Título", title, MAX_TITLE, true)?;
        let description = check_text("Descrição", description, MAX_DESCRIPTION, false)?;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let item = load_item(&tx, id)?;
        if item.stored_status == StoredStatus::Cancelled {
            return Err("Item cancelado não pode ser editado; restaure-o antes.".into());
        }
        if session_of_item(&tx, id)?.is_some() {
            return Err("Item com Session vinculada não pode ser editado; o título e o objetivo vivem na Session.".into());
        }
        if item.title == title && item.description == description {
            return Ok(item);
        }
        tx.execute(
            "UPDATE planning_items SET title=?2,description=?3,updated_at=?4 WHERE id=?1",
            params![id, title, description, now(&tx)?],
        )
        .map_err(|e| e.to_string())?;
        record_event(
            &tx,
            id,
            PlanningEventType::PlanningItemUpdated,
            &[("title", &title)],
        )?;
        activity(
            &tx,
            &item.project_id,
            &format!("Planejamento: item \"{title}\" editado"),
        )?;
        let updated = load_item(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(updated)
    }

    /// Cancela (só item aberto SEM Session); restaurável. Não existe exclusão destrutiva.
    pub fn planning_cancel(&mut self, id: &str, reason: Option<&str>) -> HubResult<PlanningItem> {
        let reason = match reason {
            Some(r) => {
                let r = check_text("Motivo", r, MAX_REASON, false)?;
                (!r.is_empty()).then_some(r)
            }
            None => None,
        };
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let item = load_item(&tx, id)?;
        if item.stored_status == StoredStatus::Cancelled {
            return Err("O item já está cancelado.".into());
        }
        if session_of_item(&tx, id)?.is_some() {
            return Err("Item com Session vinculada não pode ser cancelado.".into());
        }
        let stamp = now(&tx)?;
        tx.execute(
            "UPDATE planning_items SET stored_status='cancelled',cancel_reason=?2,cancelled_at=?3,updated_at=?3 WHERE id=?1",
            params![id, reason, stamp],
        )
        .map_err(|e| e.to_string())?;
        record_event(
            &tx,
            id,
            PlanningEventType::PlanningItemCancelled,
            &[("reason", reason.as_deref().unwrap_or(""))],
        )?;
        activity(
            &tx,
            &item.project_id,
            &format!("Planejamento: item \"{}\" cancelado", item.title),
        )?;
        let updated = load_item(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(updated)
    }

    /// Restaura um item cancelado: volta a PLANEJADO na posição que já tinha.
    pub fn planning_restore(&mut self, id: &str) -> HubResult<PlanningItem> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let item = load_item(&tx, id)?;
        if item.stored_status != StoredStatus::Cancelled {
            return Err("Só item cancelado pode ser restaurado.".into());
        }
        tx.execute(
            "UPDATE planning_items SET stored_status='open',cancel_reason=NULL,cancelled_at=NULL,updated_at=?2 WHERE id=?1",
            params![id, now(&tx)?],
        )
        .map_err(|e| e.to_string())?;
        record_event(&tx, id, PlanningEventType::PlanningItemRestored, &[])?;
        activity(
            &tx,
            &item.project_id,
            &format!("Planejamento: item \"{}\" restaurado", item.title),
        )?;
        let updated = load_item(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(updated)
    }

    /// Reordena DENTRO da fila de planejados (cima, baixo, topo). Sem evento: a posição é estado.
    pub fn planning_move(&mut self, id: &str, to: MoveTo) -> HubResult<PlanningItem> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let item = load_item(&tx, id)?;
        if item.stored_status == StoredStatus::Cancelled {
            return Err("Item cancelado não entra na fila.".into());
        }
        if session_of_item(&tx, id)?.is_some() {
            return Err("Só item planejado pode ser reordenado.".into());
        }
        let queue: Vec<String> = load_items(&tx, Some(&item.project_id))?
            .into_iter()
            .filter(|i| i.stored_status == StoredStatus::Open)
            .map(|i| i.id)
            .filter(|other| matches!(session_of_item(&tx, other), Ok(None)))
            .collect();
        let at = queue
            .iter()
            .position(|other| other == id)
            .ok_or_else(|| "Item fora da fila de planejados.".to_string())?;
        let (anchor, after) = match to {
            MoveTo::Up | MoveTo::Top if at == 0 => {
                return Err("O item já é o primeiro da fila.".into())
            }
            MoveTo::Up => (queue[at - 1].clone(), false),
            MoveTo::Top => (queue[0].clone(), false),
            MoveTo::Down if at + 1 == queue.len() => {
                return Err("O item já é o último da fila.".into())
            }
            MoveTo::Down => (queue[at + 1].clone(), true),
        };
        place(&tx, &item.project_id, id, &anchor, after)?;
        tx.execute(
            "UPDATE planning_items SET updated_at=?2 WHERE id=?1",
            params![id, now(&tx)?],
        )
        .map_err(|e| e.to_string())?;
        let updated = load_item(&tx, id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(updated)
    }

    /// O que Iniciar abre: título/objetivo pré-preenchidos e se pode iniciar agora. Não cria nada.
    pub fn planning_prepare_start(&self, id: &str) -> HubResult<StartDraft> {
        let item = load_item(&self.conn, id)?;
        let reason = start_blocker(&self.conn, &item)?;
        Ok(StartDraft {
            item_id: item.id,
            title: item.title,
            objective: item.description,
            can_start: reason.is_none(),
            disabled_reason: reason,
        })
    }

    /// INICIAR (explícito, depois do formulário revisado): cria a Session pelo fluxo real do DDAE,
    /// já vinculada ao item, na MESMA transação. O backend revalida tudo (a UI não é a barreira).
    pub fn planning_start_session(
        &mut self,
        id: &str,
        title: &str,
        objective: &str,
    ) -> HubResult<ddae::Session> {
        let title = check_text("Título", title, ddae::MAX_TITLE, true)?;
        let objective = check_text("Objetivo", objective, ddae::MAX_OBJECTIVE, false)?;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let item = load_item(&tx, id)?;
        if let Some(reason) = start_blocker(&tx, &item)? {
            return Err(reason);
        }
        let session = ddae::create_session_in(&tx, &item.project_id, title, objective, Some(id))?;
        tx.execute(
            "UPDATE planning_items SET updated_at=?2 WHERE id=?1",
            params![id, now(&tx)?],
        )
        .map_err(|e| e.to_string())?;
        record_event(
            &tx,
            id,
            PlanningEventType::PlanningItemSessionLinked,
            &[
                ("sessionId", &session.id),
                ("label", &session.label()),
                ("title", &session.title),
            ],
        )?;
        activity(
            &tx,
            &item.project_id,
            &format!(
                "Planejamento: item \"{}\" iniciado como {}",
                item.title,
                session.label()
            ),
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(session)
    }
}

/// Por que o item não pode ser iniciado agora (None = pode).
fn start_blocker(conn: &Connection, item: &PlanningItem) -> HubResult<Option<String>> {
    if item.stored_status == StoredStatus::Cancelled {
        return Ok(Some(
            "Item cancelado não pode ser iniciado; restaure-o antes.".into(),
        ));
    }
    if let Some((_, number)) = session_of_item(conn, &item.id)? {
        return Ok(Some(format!(
            "O item já foi iniciado como {}.",
            ddae::label(number)
        )));
    }
    let active: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM ddae_sessions WHERE project_id=?1 AND status='active')",
            [&item.project_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(active.then(|| ACTIVE_SESSION_REASON.to_string()))
}
