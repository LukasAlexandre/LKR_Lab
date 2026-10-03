//! DDAE — Sessions, Blocks e Decisions (Concept 06; ver docs/ddae/ e docs/STATE.md).
//!
//! * SQLite é a fonte de verdade em runtime; Markdown é só export/documentação humana.
//! * SESSION = FEATURE e sempre pertence a um Project. Identidade: UUID interno estável + o
//!   `SESSION-NNN` humano (único por Project).
//! * Estados da Session: `active | frozen | stopped | completed` (completed é terminal).
//!   No máximo UMA `active` por Project (índice único parcial no banco + checagem aqui).
//! * Estados do Block: `pending | in_progress | completed`; no máximo UM `in_progress` por Session.
//! * Bloco atual = o `in_progress`; próximo = o primeiro `pending` pela ordem; progresso =
//!   completed / total, sempre derivado (nunca um percentual gravado).
//! * As transições são explícitas (comandos do backend); nada é inferido por chat nem inicia o
//!   próximo Block sozinho.
//! * É estado PORTÁTIL: nunca guarda caminho absoluto, Machine ID, hostname ou IP.
use crate::{database::Database, HubResult};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const MAX_TITLE: usize = 120;
pub const MAX_OBJECTIVE: usize = 4_000;
pub const MAX_BLOCK_TITLE: usize = 200;
pub const MAX_REASON: usize = 500;
pub const MAX_RESULT: usize = 2_000;
pub const MAX_DECISION_TITLE: usize = 200;
pub const MAX_DECISION_BODY: usize = 8_000;
pub const MAX_BLOCKS: usize = 500;
pub const MAX_DECISIONS: usize = 500;
pub const MAX_SESSIONS: usize = 10_000;
pub const MAX_TIMESTAMP: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Frozen,
    Stopped,
    Completed,
}
impl SessionStatus {
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
            other => Err(format!("Estado de sessão desconhecido ({other}).")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockStatus {
    Pending,
    InProgress,
    Completed,
}
impl BlockStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
    fn parse(value: &str) -> HubResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            other => Err(format!("Estado de bloco desconhecido ({other}).")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    pub title: String,
    pub status: BlockStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub created_at: String,
}

/// A Session como é guardada E como viaja no workspace portátil (o mesmo formato).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub project_id: String,
    pub number: u32,
    pub title: String,
    #[serde(default)]
    pub objective: String,
    pub status: SessionStatus,
    /// Por que a Session está congelada/parada (some quando volta a `active`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    /// Resultado da Session finalizada.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub completed: u32,
    pub total: u32,
}

impl Session {
    /// `SESSION-001`
    pub fn label(&self) -> String {
        label(self.number)
    }
    pub fn progress(&self) -> Progress {
        Progress {
            completed: self
                .blocks
                .iter()
                .filter(|b| b.status == BlockStatus::Completed)
                .count() as u32,
            total: self.blocks.len() as u32,
        }
    }
    /// O Block `in_progress` (no máximo um).
    pub fn current_block(&self) -> Option<&Block> {
        self.blocks
            .iter()
            .find(|b| b.status == BlockStatus::InProgress)
    }
    /// O primeiro `pending` pela ordem.
    pub fn next_block(&self) -> Option<&Block> {
        self.blocks
            .iter()
            .find(|b| b.status == BlockStatus::Pending)
    }
    /// Pode ser finalizada: há blocks, todos `completed`, nenhum `in_progress`.
    pub fn can_complete(&self) -> bool {
        !self.blocks.is_empty()
            && self
                .blocks
                .iter()
                .all(|b| b.status == BlockStatus::Completed)
    }
}

pub fn label(number: u32) -> String {
    format!("SESSION-{number:03}")
}

/// Session + o que a lista precisa já derivado no backend (uma fonte de verdade).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    #[serde(flatten)]
    pub session: Session,
    pub label: String,
    pub progress: Progress,
    pub current_block: Option<Block>,
    pub next_block: Option<Block>,
    pub can_complete: bool,
    pub recent_decision: Option<Decision>,
}

impl From<Session> for SessionView {
    fn from(session: Session) -> Self {
        Self {
            label: session.label(),
            progress: session.progress(),
            current_block: session.current_block().cloned(),
            next_block: session.next_block().cloned(),
            can_complete: session.can_complete(),
            recent_decision: session.decisions.last().cloned(),
            session,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub total: u32,
    pub active: u32,
    pub frozen: u32,
    pub stopped: u32,
    pub completed: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DdaeOverview {
    pub project_id: String,
    /// Mais recente primeiro (maior SESSION-NNN).
    pub sessions: Vec<SessionView>,
    pub counts: Counts,
    /// Soma dos blocks de todas as sessões.
    pub blocks_total: u32,
    pub active_session_id: Option<String>,
    pub legacy_import: LegacyImport,
}

pub fn counts_of(sessions: &[Session]) -> Counts {
    let mut counts = Counts {
        total: sessions.len() as u32,
        ..Counts::default()
    };
    for s in sessions {
        match s.status {
            SessionStatus::Active => counts.active += 1,
            SessionStatus::Frozen => counts.frozen += 1,
            SessionStatus::Stopped => counts.stopped += 1,
            SessionStatus::Completed => counts.completed += 1,
        }
    }
    counts
}

/// Resultado da importação da SESSION-001 histórica (Markdown → runtime).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyImport {
    /// O projeto não tem a Session histórica na pasta (ou não está disponível nesta máquina).
    NotApplicable,
    /// Criada agora a partir do Markdown.
    Imported,
    /// Já existia (mesma identidade determinística): nada foi duplicado.
    AlreadyImported,
    /// Não importada para não violar uma regra (número em uso por outra Session ou 2ª ativa).
    Skipped,
}

// ------------------------------------------------------------------ validação de texto

/// Heurística de caminho absoluto de máquina dentro de texto livre (C:\…, \\servidor, /home/…).
/// O estado DDAE é portátil: nada disso pode ser persistido.
pub fn has_machine_path(text: &str) -> bool {
    let b = text.as_bytes();
    for i in 0..b.len() {
        let boundary = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if boundary
            && i + 2 < b.len()
            && b[i].is_ascii_alphabetic()
            && b[i + 1] == b':'
            && (b[i + 2] == b'\\' || b[i + 2] == b'/')
            // "C://" vira "C:/" só quando é letra de unidade isolada, nunca "https://".
            && !(b[i + 2] == b'/' && i + 3 < b.len() && b[i + 3] == b'/')
        {
            return true;
        }
    }
    text.contains("\\\\")
        || text.contains("/Users/")
        || text.contains("/home/")
        || text.contains("~/")
        || text.contains("~\\")
}

fn check_text(field: &str, value: &str, max: usize, required: bool) -> HubResult<String> {
    let value = value.trim().to_string();
    if required && value.is_empty() {
        return Err(format!("{field}: obrigatório."));
    }
    if value.len() > max {
        return Err(format!("{field}: excede {max} bytes."));
    }
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(format!("{field}: contém caracteres de controle."));
    }
    if has_machine_path(&value) {
        return Err(format!(
            "{field}: contém caminho local; DDAE é portátil e não guarda caminhos."
        ));
    }
    Ok(value)
}

fn opt_text(field: &str, value: Option<&str>, max: usize) -> HubResult<Option<String>> {
    match value {
        None => Ok(None),
        Some(v) => {
            let v = check_text(field, v, max, false)?;
            Ok(if v.is_empty() { None } else { Some(v) })
        }
    }
}

fn ensure_id(what: &str, id: &str) -> HubResult<()> {
    if crate::portable::valid_id(id) {
        Ok(())
    } else {
        Err(format!("id de {what} inválido ({id})"))
    }
}

/// Regras do estado portátil (usadas pelo `portable::validate` ao receber o arquivo versionado).
/// Cobre as invariantes: uma ativa por Project, um in_progress por Session, numeração única por
/// Project, finalizada só com todos os blocks concluídos, sem caminho local.
pub fn validate_portable(sessions: &[Session], project_ids: &HashSet<&str>) -> HubResult<()> {
    let fail = |msg: String| -> HubResult<()> { Err(format!("Workspace inválido: DDAE: {msg}")) };
    if sessions.len() > MAX_SESSIONS {
        return fail(format!("máximo de {MAX_SESSIONS} sessões"));
    }
    let mut ids = HashSet::new();
    let mut numbers = HashSet::new();
    let mut active = HashSet::new();
    for s in sessions {
        let at = label(s.number);
        let map = |e: String| format!("Workspace inválido: DDAE: {at}: {e}");
        ensure_id("sessão", &s.id).map_err(map)?;
        if !ids.insert(s.id.as_str()) {
            return fail(format!("id de sessão duplicado ({})", s.id));
        }
        if !project_ids.contains(s.project_id.as_str()) {
            return fail(format!(
                "{at} referencia um projeto que não está no workspace"
            ));
        }
        if s.number == 0 || !numbers.insert((s.project_id.as_str(), s.number)) {
            return fail(format!("{at}: número inválido ou repetido no projeto"));
        }
        check_text("título", &s.title, MAX_TITLE, true).map_err(map)?;
        check_text("objetivo", &s.objective, MAX_OBJECTIVE, false).map_err(map)?;
        opt_text("motivo", s.pause_reason.as_deref(), MAX_REASON).map_err(map)?;
        opt_text("resultado", s.result.as_deref(), MAX_RESULT).map_err(map)?;
        if s.created_at.len() > MAX_TIMESTAMP
            || s.updated_at.len() > MAX_TIMESTAMP
            || s.completed_at
                .as_deref()
                .is_some_and(|c| c.len() > MAX_TIMESTAMP)
        {
            return fail(format!("{at}: data inválida"));
        }
        if s.blocks.len() > MAX_BLOCKS || s.decisions.len() > MAX_DECISIONS {
            return fail(format!("{at}: excede o limite de blocos ou decisões"));
        }
        if s.status == SessionStatus::Active && !active.insert(s.project_id.as_str()) {
            return fail(format!(
                "{at}: o projeto já tem outra sessão ativa (no máximo uma)"
            ));
        }
        let in_progress = s
            .blocks
            .iter()
            .filter(|b| b.status == BlockStatus::InProgress)
            .count();
        if in_progress > 1 {
            return fail(format!("{at}: mais de um bloco em andamento"));
        }
        if s.status == SessionStatus::Completed && !s.can_complete() {
            return fail(format!(
                "{at}: finalizada exige todos os blocos concluídos e nenhum em andamento"
            ));
        }
        for b in &s.blocks {
            ensure_id("bloco", &b.id).map_err(map)?;
            if !ids.insert(b.id.as_str()) {
                return fail(format!("id de bloco duplicado ({})", b.id));
            }
            check_text("bloco", &b.title, MAX_BLOCK_TITLE, true).map_err(map)?;
        }
        for d in &s.decisions {
            ensure_id("decisão", &d.id).map_err(map)?;
            if !ids.insert(d.id.as_str()) {
                return fail(format!("id de decisão duplicado ({})", d.id));
            }
            check_text("decisão", &d.title, MAX_DECISION_TITLE, true).map_err(map)?;
            check_text("decisão", &d.body, MAX_DECISION_BODY, false).map_err(map)?;
            if d.created_at.len() > MAX_TIMESTAMP {
                return fail(format!("{at}: data de decisão inválida"));
            }
        }
    }
    Ok(())
}

/// Forma canônica: aparada e ordenada (projeto, número). Blocks e Decisions mantêm a ordem
/// (a posição tem significado); vazios viram ausentes.
pub fn normalize(sessions: &mut [Session]) {
    let trim_opt = |v: &mut Option<String>| {
        *v = v
            .take()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    };
    for s in sessions.iter_mut() {
        s.title = s.title.trim().to_string();
        s.objective = s.objective.trim().to_string();
        trim_opt(&mut s.pause_reason);
        trim_opt(&mut s.result);
        trim_opt(&mut s.completed_at);
        for b in &mut s.blocks {
            b.title = b.title.trim().to_string();
        }
        for d in &mut s.decisions {
            d.title = d.title.trim().to_string();
            d.body = d.body.trim().to_string();
        }
    }
    sessions.sort_by(|a, b| (&a.project_id, a.number).cmp(&(&b.project_id, b.number)));
}

// ------------------------------------------------------------------ persistência

fn now(conn: &Connection) -> HubResult<String> {
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| {
        r.get(0)
    })
    .map_err(|e| e.to_string())
}

fn load_blocks(conn: &Connection, session_id: &str) -> HubResult<Vec<Block>> {
    let mut stmt = conn
        .prepare("SELECT id,title,status FROM ddae_blocks WHERE session_id=?1 ORDER BY position")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([session_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(id, title, status)| {
            Ok(Block {
                id,
                title,
                status: BlockStatus::parse(&status)?,
            })
        })
        .collect()
}

fn load_decisions(conn: &Connection, session_id: &str) -> HubResult<Vec<Decision>> {
    let mut stmt = conn
        .prepare(
            "SELECT id,title,body,created_at FROM ddae_decisions WHERE session_id=?1 ORDER BY position",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([session_id], |r| {
            Ok(Decision {
                id: r.get(0)?,
                title: r.get(1)?,
                body: r.get(2)?,
                created_at: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

type SessionRow = (
    String,
    String,
    u32,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
);

const SESSION_COLUMNS: &str = "id,project_id,number,title,objective,status,pause_reason,result,created_at,updated_at,completed_at";

fn row_to_tuple(r: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
    ))
}

fn assemble(conn: &Connection, row: SessionRow) -> HubResult<Session> {
    let (
        id,
        project_id,
        number,
        title,
        objective,
        status,
        pause_reason,
        result,
        created_at,
        updated_at,
        completed_at,
    ) = row;
    Ok(Session {
        blocks: load_blocks(conn, &id)?,
        decisions: load_decisions(conn, &id)?,
        id,
        project_id,
        number,
        title,
        objective,
        status: SessionStatus::parse(&status)?,
        pause_reason,
        result,
        created_at,
        updated_at,
        completed_at,
    })
}

fn load_session(conn: &Connection, id: &str) -> HubResult<Session> {
    let row = conn
        .query_row(
            &format!("SELECT {SESSION_COLUMNS} FROM ddae_sessions WHERE id=?1"),
            [id],
            row_to_tuple,
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Sessão não encontrada.".to_string())?;
    assemble(conn, row)
}

fn load_sessions(conn: &Connection, project_id: Option<&str>) -> HubResult<Vec<Session>> {
    let sql = format!(
        "SELECT {SESSION_COLUMNS} FROM ddae_sessions {} ORDER BY project_id, number",
        if project_id.is_some() {
            "WHERE project_id=?1"
        } else {
            ""
        }
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = if let Some(id) = project_id {
        stmt.query_map([id], row_to_tuple)
    } else {
        stmt.query_map([], row_to_tuple)
    }
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;
    rows.into_iter().map(|r| assemble(conn, r)).collect()
}

fn insert_session(tx: &Transaction, s: &Session) -> HubResult<()> {
    tx.execute(
        "INSERT INTO ddae_sessions(id,project_id,number,title,objective,status,pause_reason,result,created_at,updated_at,completed_at) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            s.id,
            s.project_id,
            s.number,
            s.title,
            s.objective,
            s.status.as_str(),
            s.pause_reason,
            s.result,
            s.created_at,
            s.updated_at,
            s.completed_at
        ],
    )
    .map_err(|e| e.to_string())?;
    for (i, b) in s.blocks.iter().enumerate() {
        tx.execute(
            "INSERT INTO ddae_blocks(id,session_id,position,title,status) VALUES(?1,?2,?3,?4,?5)",
            params![b.id, s.id, i as i64, b.title, b.status.as_str()],
        )
        .map_err(|e| e.to_string())?;
    }
    for (i, d) in s.decisions.iter().enumerate() {
        tx.execute(
            "INSERT INTO ddae_decisions(id,session_id,position,title,body,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![d.id, s.id, i as i64, d.title, d.body, d.created_at],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Todas as Sessions (todas as Projects), para exportar ao workspace portátil.
pub fn export(conn: &Connection) -> HubResult<Vec<Session>> {
    load_sessions(conn, None)
}

/// Substitui TODO o DDAE local pelo do workspace (já validado), dentro da transação do apply.
/// Apagar e reinserir evita estados intermediários que violariam os índices únicos parciais
/// (ex.: trocar qual Session está ativa).
pub fn replace_all(tx: &Transaction, sessions: &[Session]) -> HubResult<()> {
    tx.execute("DELETE FROM ddae_sessions", [])
        .map_err(|e| e.to_string())?;
    let now = now(tx)?;
    for s in sessions {
        let mut s = s.clone();
        if s.created_at.is_empty() {
            s.created_at = now.clone();
        }
        if s.updated_at.is_empty() {
            s.updated_at = s.created_at.clone();
        }
        for d in &mut s.decisions {
            if d.created_at.is_empty() {
                d.created_at = now.clone();
            }
        }
        insert_session(tx, &s)?;
    }
    Ok(())
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn project_exists(conn: &Connection, project_id: &str) -> HubResult<()> {
    let found: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
            [project_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if found {
        Ok(())
    } else {
        Err("Projeto não encontrado".into())
    }
}

fn activity(tx: &Transaction, project_id: &str, text: &str) -> HubResult<()> {
    tx.execute(
        "INSERT INTO activities(project_id,action) VALUES(?1,?2)",
        params![project_id, text],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn active_of(conn: &Connection, project_id: &str) -> HubResult<Option<(String, u32)>> {
    conn.query_row(
        "SELECT id,number FROM ddae_sessions WHERE project_id=?1 AND status='active'",
        [project_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(|e| e.to_string())
}

impl Database {
    /// Lista e derivados de um Project (a ordem é a do número, mais recente primeiro).
    pub fn ddae_overview(&self, project_id: &str) -> HubResult<DdaeOverview> {
        project_exists(&self.conn, project_id)?;
        let mut sessions = load_sessions(&self.conn, Some(project_id))?;
        sessions.reverse();
        Ok(build_overview(
            project_id,
            sessions,
            LegacyImport::NotApplicable,
        ))
    }

    pub fn ddae_session(&self, session_id: &str) -> HubResult<Session> {
        load_session(&self.conn, session_id)
    }

    /// Cria uma Session `active` para o Project. Só uma ativa por Project: com outra ativa,
    /// a criação é recusada (congele ou pare a ativa antes) em vez de inventar um estado.
    pub fn ddae_create_session(
        &mut self,
        project_id: &str,
        title: &str,
        objective: &str,
    ) -> HubResult<Session> {
        let title = check_text("Título", title, MAX_TITLE, true)?;
        let objective = check_text("Objetivo", objective, MAX_OBJECTIVE, false)?;
        project_exists(&self.conn, project_id)?;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        if let Some((_, number)) = active_of(&tx, project_id)? {
            return Err(format!(
                "{} já está ativa neste projeto; congele ou pare a sessão ativa antes de criar outra.",
                label(number)
            ));
        }
        let number: u32 = tx
            .query_row(
                "SELECT COALESCE(MAX(number),0)+1 FROM ddae_sessions WHERE project_id=?1",
                [project_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let stamp = now(&tx)?;
        let session = Session {
            id: new_id(),
            project_id: project_id.into(),
            number,
            title,
            objective,
            status: SessionStatus::Active,
            pause_reason: None,
            result: None,
            blocks: vec![],
            decisions: vec![],
            created_at: stamp.clone(),
            updated_at: stamp,
            completed_at: None,
        };
        insert_session(&tx, &session)?;
        activity(
            &tx,
            project_id,
            &format!("DDAE: {} criada", session.label()),
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(session)
    }

    /// Roda uma mudança sobre a Session numa transação: recusa Session finalizada (terminal),
    /// atualiza `updated_at`, registra a atividade e devolve a Session recarregada.
    fn ddae_mutate(
        &mut self,
        session_id: &str,
        change: impl FnOnce(&Transaction, &mut Session) -> HubResult<String>,
    ) -> HubResult<Session> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let mut session = load_session(&tx, session_id)?;
        if session.status == SessionStatus::Completed {
            return Err("Sessão finalizada é terminal e não pode ser alterada.".into());
        }
        // Antes da mudança: depois de finalizar, o gatilho do banco recusa qualquer UPDATE.
        tx.execute(
            "UPDATE ddae_sessions SET updated_at=?2 WHERE id=?1",
            params![session_id, now(&tx)?],
        )
        .map_err(|e| e.to_string())?;
        let text = change(&tx, &mut session)?;
        activity(
            &tx,
            &session.project_id,
            &format!("DDAE: {} {text}", session.label()),
        )?;
        let reloaded = load_session(&tx, session_id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(reloaded)
    }

    /// Acrescenta um Block `pending` ao fim da ordem.
    pub fn ddae_add_block(&mut self, session_id: &str, title: &str) -> HubResult<Session> {
        let title = check_text("Bloco", title, MAX_BLOCK_TITLE, true)?;
        self.ddae_mutate(session_id, |tx, s| {
            if s.blocks.len() >= MAX_BLOCKS {
                return Err(format!("Máximo de {MAX_BLOCKS} blocos por sessão."));
            }
            tx.execute(
                "INSERT INTO ddae_blocks(id,session_id,position,title,status) VALUES(?1,?2,?3,?4,'pending')",
                params![new_id(), s.id, s.blocks.len() as i64, title],
            )
            .map_err(|e| e.to_string())?;
            Ok(format!("ganhou o bloco “{title}”"))
        })
    }

    /// pending → in_progress. Só em Session ativa e sem outro bloco em andamento.
    pub fn ddae_start_block(&mut self, session_id: &str, block_id: &str) -> HubResult<Session> {
        self.ddae_mutate(session_id, |tx, s| {
            if s.status != SessionStatus::Active {
                return Err("Só é possível iniciar blocos em uma sessão ativa.".into());
            }
            if let Some(current) = s.current_block() {
                return Err(format!(
                    "O bloco “{}” já está em andamento; conclua-o antes de iniciar outro.",
                    current.title
                ));
            }
            let block = s
                .blocks
                .iter()
                .find(|b| b.id == block_id)
                .ok_or("Bloco não encontrado nesta sessão.")?;
            if block.status != BlockStatus::Pending {
                return Err("Só um bloco pendente pode ser iniciado.".into());
            }
            tx.execute(
                "UPDATE ddae_blocks SET status='in_progress' WHERE id=?1",
                [block_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(format!("iniciou o bloco “{}”", block.title))
        })
    }

    /// in_progress → completed. Não inicia o próximo bloco.
    pub fn ddae_complete_block(&mut self, session_id: &str, block_id: &str) -> HubResult<Session> {
        self.ddae_mutate(session_id, |tx, s| {
            if s.status != SessionStatus::Active {
                return Err("Só é possível concluir blocos em uma sessão ativa.".into());
            }
            let block = s
                .blocks
                .iter()
                .find(|b| b.id == block_id)
                .ok_or("Bloco não encontrado nesta sessão.")?;
            if block.status != BlockStatus::InProgress {
                return Err("Só o bloco em andamento pode ser concluído.".into());
            }
            tx.execute(
                "UPDATE ddae_blocks SET status='completed' WHERE id=?1",
                [block_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(format!("concluiu o bloco “{}”", block.title))
        })
    }

    /// active → frozen (esperando algo externo). O bloco em andamento continua em andamento.
    pub fn ddae_freeze(&mut self, session_id: &str, reason: &str) -> HubResult<Session> {
        self.pause(session_id, reason, SessionStatus::Frozen)
    }

    /// active → stopped (sem retomada imediata).
    pub fn ddae_stop(&mut self, session_id: &str, reason: &str) -> HubResult<Session> {
        self.pause(session_id, reason, SessionStatus::Stopped)
    }

    fn pause(
        &mut self,
        session_id: &str,
        reason: &str,
        target: SessionStatus,
    ) -> HubResult<Session> {
        let reason = check_text("Motivo", reason, MAX_REASON, true)?;
        self.ddae_mutate(session_id, |tx, s| {
            if s.status != SessionStatus::Active {
                return Err("Só uma sessão ativa pode ser congelada ou parada.".into());
            }
            tx.execute(
                "UPDATE ddae_sessions SET status=?2, pause_reason=?3 WHERE id=?1",
                params![s.id, target.as_str(), reason],
            )
            .map_err(|e| e.to_string())?;
            Ok(match target {
                SessionStatus::Frozen => "congelada",
                _ => "parada",
            }
            .into())
        })
    }

    /// frozen|stopped → active. Recusa se o Project já tem outra ativa.
    pub fn ddae_resume(&mut self, session_id: &str) -> HubResult<Session> {
        self.ddae_mutate(session_id, |tx, s| {
            if s.status == SessionStatus::Active {
                return Err("A sessão já está ativa.".into());
            }
            if let Some((_, number)) = active_of(tx, &s.project_id)? {
                return Err(format!(
                    "{} já está ativa neste projeto; congele ou pare a sessão ativa antes de retomar esta.",
                    label(number)
                ));
            }
            tx.execute(
                "UPDATE ddae_sessions SET status='active', pause_reason=NULL WHERE id=?1",
                [&s.id],
            )
            .map_err(|e| e.to_string())?;
            Ok("retomada".into())
        })
    }

    /// → completed (terminal). Exige todos os blocks concluídos e nenhum em andamento.
    pub fn ddae_complete(&mut self, session_id: &str, result: &str) -> HubResult<Session> {
        let result = check_text("Resultado", result, MAX_RESULT, false)?;
        self.ddae_mutate(session_id, |tx, s| {
            if s.blocks.is_empty() {
                return Err("A sessão não tem blocos; não há o que finalizar.".into());
            }
            if let Some(current) = s.current_block() {
                return Err(format!(
                    "O bloco “{}” ainda está em andamento.",
                    current.title
                ));
            }
            if !s.can_complete() {
                let p = s.progress();
                return Err(format!(
                    "Só é possível finalizar com todos os blocos concluídos ({}/{}).",
                    p.completed, p.total
                ));
            }
            let stamp = now(tx)?;
            tx.execute(
                "UPDATE ddae_sessions SET status='completed', pause_reason=NULL, result=?2, completed_at=?3 WHERE id=?1",
                params![s.id, if result.is_empty() { None } else { Some(&result) }, stamp],
            )
            .map_err(|e| e.to_string())?;
            Ok("finalizada".into())
        })
    }

    /// Registra uma Decision (acrescentada ao fim).
    pub fn ddae_add_decision(
        &mut self,
        session_id: &str,
        title: &str,
        body: &str,
    ) -> HubResult<Session> {
        let title = check_text("Decisão", title, MAX_DECISION_TITLE, true)?;
        let body = check_text("Detalhe da decisão", body, MAX_DECISION_BODY, false)?;
        self.ddae_mutate(session_id, |tx, s| {
            if s.decisions.len() >= MAX_DECISIONS {
                return Err(format!("Máximo de {MAX_DECISIONS} decisões por sessão."));
            }
            tx.execute(
                "INSERT INTO ddae_decisions(id,session_id,position,title,body,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![new_id(), s.id, s.decisions.len() as i64, title, body, now(tx)?],
            )
            .map_err(|e| e.to_string())?;
            Ok(format!("registrou a decisão “{title}”"))
        })
    }

    /// Lista + importação idempotente da SESSION-001 histórica quando o Project tem a pasta aqui.
    pub fn ddae_overview_with_legacy(&mut self, project_id: &str) -> HubResult<DdaeOverview> {
        let legacy = self.ddae_import_legacy(project_id)?;
        let mut overview = self.ddae_overview(project_id)?;
        overview.legacy_import = legacy;
        Ok(overview)
    }

    /// Importa a SESSION-001 de `docs/ddae/sessions/SESSION-001-*.md` da pasta vinculada do Project.
    /// Só a SESSION-001 é histórica e real; qualquer outro arquivo é ignorado. Idempotente: a
    /// identidade (UUID) é derivada de forma determinística do Project, então reimportar — nesta ou
    /// em outra máquina — nunca duplica.
    pub fn ddae_import_legacy(&mut self, project_id: &str) -> HubResult<LegacyImport> {
        let project = self.project(project_id)?;
        if crate::projects::location(&project) != crate::models::Location::Available {
            return Ok(LegacyImport::NotApplicable);
        }
        let dir = std::path::Path::new(&project.local_path).join("docs/ddae/sessions");
        let Some(file) = legacy_file(&dir) else {
            return Ok(LegacyImport::NotApplicable);
        };
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("Não foi possível ler a SESSION-001: {e}"))?;
        let parsed = parse_legacy(&text)?;
        let id = legacy_id(project_id, parsed.number);
        let exists: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ddae_sessions WHERE id=?1)",
                [&id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            return Ok(LegacyImport::AlreadyImported);
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let number_taken: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ddae_sessions WHERE project_id=?1 AND number=?2)",
                params![project_id, parsed.number],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if number_taken
            || (parsed.status == SessionStatus::Active && active_of(&tx, project_id)?.is_some())
        {
            return Ok(LegacyImport::Skipped);
        }
        let stamp = now(&tx)?;
        let created = first_commit_time(&tx, &file).unwrap_or_else(|| stamp.clone());
        let blocks = parsed
            .blocks
            .iter()
            .enumerate()
            .map(|(i, (title, status))| Block {
                id: legacy_child_id(&id, "block", i),
                title: title.clone(),
                status: *status,
            })
            .collect();
        let session = Session {
            id,
            project_id: project_id.into(),
            number: parsed.number,
            title: parsed.title,
            objective: parsed.objective,
            status: parsed.status,
            pause_reason: None,
            result: None,
            blocks,
            decisions: vec![],
            created_at: created,
            updated_at: stamp,
            completed_at: None,
        };
        insert_session(&tx, &session)?;
        activity(
            &tx,
            project_id,
            &format!(
                "DDAE: {} importada do histórico do projeto",
                session.label()
            ),
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(LegacyImport::Imported)
    }
}

fn build_overview(project_id: &str, sessions: Vec<Session>, legacy: LegacyImport) -> DdaeOverview {
    let counts = counts_of(&sessions);
    let blocks_total = sessions.iter().map(|s| s.blocks.len() as u32).sum();
    let active_session_id = sessions
        .iter()
        .find(|s| s.status == SessionStatus::Active)
        .map(|s| s.id.clone());
    DdaeOverview {
        project_id: project_id.into(),
        sessions: sessions.into_iter().map(SessionView::from).collect(),
        counts,
        blocks_total,
        active_session_id,
        legacy_import: legacy,
    }
}

// ------------------------------------------------------------------ SESSION-001 histórica

#[derive(Debug, Clone, PartialEq)]
pub struct LegacySession {
    pub number: u32,
    pub title: String,
    pub objective: String,
    pub status: SessionStatus,
    pub blocks: Vec<(String, BlockStatus)>,
}

/// O arquivo `SESSION-001-*.md` (o único histórico real).
fn legacy_file(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut found: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "md")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("SESSION-001-"))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// UUID (formato v5) derivado do Project + número: o mesmo em qualquer máquina.
fn legacy_id(project_id: &str, number: u32) -> String {
    deterministic_uuid(&format!("lkr-lab:ddae:{project_id}:SESSION-{number:03}"))
}
fn legacy_child_id(session_id: &str, kind: &str, index: usize) -> String {
    deterministic_uuid(&format!("lkr-lab:ddae:{session_id}:{kind}:{index}"))
}
fn deterministic_uuid(name: &str) -> String {
    let digest = Sha256::digest(name.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

/// Data do primeiro commit do arquivo (UTC), quando o Git responde; senão o chamador usa "agora".
fn first_commit_time(conn: &Connection, file: &std::path::Path) -> Option<String> {
    let dir = file.parent()?;
    let name = file.file_name()?.to_str()?;
    let out = crate::commands::run(
        "git",
        &[
            "-c",
            "core.fsmonitor=false",
            "log",
            "--diff-filter=A",
            "--format=%at",
            "--",
            name,
        ],
        Some(dir),
    )
    .ok()?;
    let secs: i64 = out.lines().last()?.trim().parse().ok()?;
    conn.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1, 'unixepoch')",
        [secs],
        |r| r.get(0),
    )
    .ok()
}

fn legacy_session_status(raw: &str) -> Option<SessionStatus> {
    let up = raw.to_uppercase();
    if up.contains("ACTIVE") || up.contains("ATIVA") {
        Some(SessionStatus::Active)
    } else if up.contains("FROZEN") || up.contains("CONGELADA") {
        Some(SessionStatus::Frozen)
    } else if up.contains("STOPPED") || up.contains("PARADA") {
        Some(SessionStatus::Stopped)
    } else if up.contains("FINISHED") || up.contains("FINALIZADA") || up.contains("COMPLETED") {
        Some(SessionStatus::Completed)
    } else {
        None
    }
}

fn legacy_block_status(raw: &str) -> BlockStatus {
    let up = raw.trim().to_uppercase();
    if up.starts_with("CONCLU") {
        BlockStatus::Completed
    } else if up.starts_with("EM ANDAMENTO") {
        BlockStatus::InProgress
    } else {
        BlockStatus::Pending
    }
}

/// Lê o Markdown da SESSION-001: título do `# SESSION-NNN — …`, `**Status:**`, primeiro parágrafo
/// de `## Objetivo` e a tabela de `## Blocos` (# | Bloco | Status | Referência).
pub fn parse_legacy(markdown: &str) -> HubResult<LegacySession> {
    let mut number = None;
    let mut title = String::new();
    let mut status = None;
    let mut objective = String::new();
    let mut blocks = Vec::new();
    let mut section = String::new();
    let mut objective_done = false;
    for raw in markdown.lines() {
        let line = raw.trim_end();
        if let Some(rest) = line.strip_prefix("# SESSION-") {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            number = digits.parse::<u32>().ok();
            title = rest
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start_matches(|c: char| c.is_whitespace() || c == '—' || c == '-' || c == '–')
                .trim()
                .to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            section = rest.trim().to_lowercase();
            continue;
        }
        if let Some(rest) = line.strip_prefix("**Status:**") {
            status = legacy_session_status(rest);
            continue;
        }
        match section.as_str() {
            "objetivo" if !objective_done => {
                if line.trim().is_empty() {
                    objective_done = !objective.is_empty();
                } else {
                    if !objective.is_empty() {
                        objective.push(' ');
                    }
                    objective.push_str(line.trim());
                }
            }
            "blocos" if line.trim_start().starts_with('|') => {
                let cells: Vec<&str> = line
                    .trim()
                    .trim_matches('|')
                    .split('|')
                    .map(str::trim)
                    .collect();
                if cells.len() >= 3
                    && cells[0].chars().all(|c| c.is_ascii_digit())
                    && !cells[0].is_empty()
                {
                    blocks.push((cells[1].to_string(), legacy_block_status(cells[2])));
                }
            }
            _ => {}
        }
    }
    let number = number.ok_or("SESSION-001: cabeçalho “# SESSION-NNN — título” não encontrado.")?;
    if title.is_empty() || blocks.is_empty() {
        return Err("SESSION-001: título ou tabela de blocos não encontrados.".into());
    }
    let status = status.ok_or("SESSION-001: linha “**Status:**” não reconhecida.")?;
    if blocks
        .iter()
        .filter(|(_, s)| *s == BlockStatus::InProgress)
        .count()
        > 1
    {
        return Err("SESSION-001: mais de um bloco em andamento.".into());
    }
    Ok(LegacySession {
        number,
        title: check_text("Título", &title, MAX_TITLE, true)?,
        objective: check_text("Objetivo", &objective, MAX_OBJECTIVE, false)?,
        status,
        blocks: blocks
            .into_iter()
            .map(|(t, s)| Ok((check_text("Bloco", &t, MAX_BLOCK_TITLE, true)?, s)))
            .collect::<HubResult<_>>()?,
    })
}
