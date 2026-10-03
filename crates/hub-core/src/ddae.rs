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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub status: BlockStatus,
}

/// Critério de conclusão marcável. NÃO é progresso de execução (isso são os Blocks).
/// Lê também o formato antigo (uma string): vira `{ id, text, completed: false }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Criterion {
    pub id: String,
    pub text: String,
    pub completed: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CriterionRepr {
    Text(String),
    Full {
        #[serde(default)]
        id: String,
        text: String,
        #[serde(default)]
        completed: bool,
    },
}

impl<'de> Deserialize<'de> for Criterion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match CriterionRepr::deserialize(d)? {
            CriterionRepr::Text(text) => Criterion {
                id: String::new(),
                text,
                completed: false,
            },
            CriterionRepr::Full {
                id,
                text,
                completed,
            } => Criterion {
                id,
                text,
                completed,
            },
        })
    }
}

impl From<&str> for Criterion {
    fn from(text: &str) -> Self {
        Criterion {
            id: String::new(),
            text: text.into(),
            completed: false,
        }
    }
}

/// Id determinístico de um critério vindo do formato antigo: o mesmo em qualquer máquina,
/// então o workspace canônico (e o hash) não muda entre PCs.
fn assign_criterion_ids(session_id: &str, list: &mut [Criterion]) {
    for (i, c) in list.iter_mut().enumerate() {
        if c.id.is_empty() {
            c.id = deterministic_uuid(&format!(
                "lkr-lab:ddae:{session_id}:criterion:{i}:{}",
                c.text
            ));
        }
    }
}

/// Tipos de evento do histórico semântico da Session (portátil).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventType {
    SessionCreated,
    SessionFrozen,
    SessionStopped,
    SessionResumed,
    SessionCompleted,
    /// Session importada do histórico legado (o que veio antes dos eventos não é inventado).
    LegacyImported,
    BlockAdded,
    BlockStarted,
    BlockCompleted,
    BlockRenamed,
    BlockRemoved,
    CriterionAdded,
    CriterionCompleted,
    CriterionReopened,
    CriterionRemoved,
    DecisionAdded,
    NoteAdded,
    NoteRemoved,
    DetailsUpdated,
}

impl EventType {
    pub fn as_str(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    }
}

/// Evento append-only da Session. Só fatos da feature (nunca caminho, Machine ID, host, IP, PID).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: EventType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub payload: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub created_at: String,
}

/// Evento a gravar na MESMA transação da mudança.
pub struct NewEvent {
    kind: EventType,
    block_id: Option<String>,
    payload: serde_json::Map<String, serde_json::Value>,
}

fn ev(kind: EventType, block_id: Option<&str>, pairs: &[(&str, &str)]) -> NewEvent {
    let mut payload = serde_json::Map::new();
    for (k, v) in pairs {
        if !v.is_empty() {
            payload.insert((*k).into(), serde_json::Value::String((*v).into()));
        }
    }
    NewEvent {
        kind,
        block_id: block_id.map(str::to_string),
        payload,
    }
}

pub const MAX_PAYLOAD: usize = 2_000;
pub const MAX_EVENTS: usize = 20_000;
pub const MAX_BLOCK_DESCRIPTION: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub id: String,
    /// Bloco associado (opcional); some se o bloco pendente for removido.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// Caminho RELATIVO ao Project (nunca absoluto).
    ProjectPath,
    /// URL https sem credenciais.
    Url,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub kind: ReferenceKind,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
    /// Resultado desejado (o que deve existir ao final). Vazio = não informado.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub desired_outcome: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    /// Critérios de conclusão.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub criteria: Vec<Criterion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
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
    /// Histórico semântico, append-only; ordem canônica (created_at, id).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
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
    /// Blocks: há ao menos um e todos `completed` (a regra que um workspace finalizado precisa cumprir).
    pub fn blocks_complete(&self) -> bool {
        !self.blocks.is_empty()
            && self
                .blocks
                .iter()
                .all(|b| b.status == BlockStatus::Completed)
    }
    /// O que impede finalizar (códigos estáveis): nenhum bloco, bloco em andamento, blocos
    /// pendentes, critérios pendentes. Vazio = elegível. Critérios só bloqueiam se existirem.
    pub fn completion_blockers(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.blocks.is_empty() {
            out.push("no_blocks");
        }
        if self.current_block().is_some() {
            out.push("block_in_progress");
        }
        if self.next_block().is_some() {
            out.push("blocks_pending");
        }
        if self.criteria.iter().any(|c| !c.completed) {
            out.push("criteria_pending");
        }
        out
    }
    /// Pode ser finalizada: todos os blocks concluídos E, se houver critérios, todos concluídos.
    pub fn can_complete(&self) -> bool {
        self.completion_blockers().is_empty()
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
    pub completion_blockers: Vec<&'static str>,
    pub recent_decision: Option<Decision>,
    /// Derivado (nunca gravado): a Session tem o necessário para um agente continuá-la?
    pub ready_for_ai: ReadyForAi,
}

impl From<Session> for SessionView {
    fn from(session: Session) -> Self {
        Self {
            label: session.label(),
            progress: session.progress(),
            current_block: session.current_block().cloned(),
            next_block: session.next_block().cloned(),
            can_complete: session.can_complete(),
            completion_blockers: session.completion_blockers(),
            recent_decision: session.decisions.last().cloned(),
            ready_for_ai: ready_for_ai(&session),
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

// ------------------------------------------------------------------ Ready for AI

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextState {
    /// Tem o necessário para um agente continuar.
    Ready,
    /// Faltam campos (veja `missing`).
    Incomplete,
    /// Sessão finalizada: o contexto continua gerável, mas não há o que "continuar".
    Available,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyForAi {
    pub state: ContextState,
    pub ready: bool,
    /// Campos ausentes, por código estável: objective, desired_outcome, blocks, criteria,
    /// actionable_block.
    pub missing: Vec<&'static str>,
}

/// Ready for AI é DERIVADO (nenhum boolean é gravado). Canônico: objetivo, resultado desejado,
/// ≥1 bloco, ≥1 critério de conclusão e (bloco atual OU bloco pendente). Fatos que o histórico
/// não traz ficam ausentes: nada é completado por inferência.
pub fn ready_for_ai(s: &Session) -> ReadyForAi {
    let mut missing = Vec::new();
    if s.objective.trim().is_empty() {
        missing.push("objective");
    }
    if s.desired_outcome.trim().is_empty() {
        missing.push("desired_outcome");
    }
    if s.blocks.is_empty() {
        missing.push("blocks");
    }
    if s.criteria.is_empty() {
        missing.push("criteria");
    }
    let completed = s.status == SessionStatus::Completed;
    if !completed && !s.blocks.is_empty() && s.current_block().is_none() && s.next_block().is_none()
    {
        missing.push("actionable_block");
    }
    let state = if completed {
        ContextState::Available
    } else if missing.is_empty() {
        ContextState::Ready
    } else {
        ContextState::Incomplete
    };
    ReadyForAi {
        state,
        ready: state == ContextState::Ready,
        missing,
    }
}

// ------------------------------------------------------------------ detalhes (campos de contexto)

pub const MAX_OUTCOME: usize = 2_000;
pub const MAX_ITEM: usize = 500;
pub const MAX_ITEMS: usize = 50;

/// O que o usuário pode definir além do título: objetivo, resultado desejado, restrições,
/// critérios de conclusão, notas e referências.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    /// `None` = título inalterado (número e UUID nunca mudam).
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub desired_outcome: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub criteria: Vec<Criterion>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub references: Vec<Reference>,
}

fn check_list(field: &str, items: &[String]) -> HubResult<Vec<String>> {
    if items.len() > MAX_ITEMS {
        return Err(format!("{field}: máximo de {MAX_ITEMS} itens."));
    }
    items
        .iter()
        .map(|i| check_text(field, i, MAX_ITEM, false))
        .filter(|r| r.as_ref().map_or(true, |v| !v.is_empty()))
        .collect()
}

/// Caminho relativo ao Project: sem raiz, unidade, `..`, barra invertida ou caracteres de controle.
fn valid_project_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ITEM
        && !crate::portable::is_absolute_path(value)
        && !value.contains(['\\', ':'])
        && !value.chars().any(|c| c.is_control())
        && value
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

fn check_references(items: &[Reference]) -> HubResult<Vec<Reference>> {
    if items.len() > MAX_ITEMS {
        return Err(format!("Referências: máximo de {MAX_ITEMS} itens."));
    }
    items
        .iter()
        .map(|r| {
            let value = r.value.trim().to_string();
            let ok = match r.kind {
                ReferenceKind::ProjectPath => valid_project_path(&value),
                ReferenceKind::Url => {
                    value.len() <= MAX_ITEM && crate::projects::valid_repository(&value)
                }
            };
            if ok {
                let label = opt_text("Rótulo", r.label.as_deref(), 100)?;
                Ok(Reference {
                    kind: r.kind,
                    value,
                    label,
                })
            } else {
                Err(match r.kind {
                    ReferenceKind::ProjectPath => format!(
                        "Referência “{value}”: use um caminho RELATIVO ao projeto (sem caminho local nem “..”)."
                    ),
                    ReferenceKind::Url => {
                        format!("Referência “{value}”: use uma URL https sem credenciais.")
                    }
                })
            }
        })
        .collect()
}

fn check_criteria(items: &[Criterion]) -> HubResult<Vec<Criterion>> {
    if items.len() > MAX_ITEMS {
        return Err(format!(
            "Critérios de conclusão: máximo de {MAX_ITEMS} itens."
        ));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for c in items {
        let text = check_text("Critérios de conclusão", &c.text, MAX_ITEM, false)?;
        if text.is_empty() {
            continue;
        }
        if !c.id.is_empty() {
            ensure_id("critério", &c.id)?;
            if !seen.insert(c.id.clone()) {
                return Err(format!("id de critério duplicado ({})", c.id));
            }
        }
        out.push(Criterion {
            id: c.id.clone(),
            text,
            completed: c.completed,
        });
    }
    Ok(out)
}

/// Valida e aplica a forma canônica (aparada, sem itens vazios).
pub fn check_details(d: &Details) -> HubResult<Details> {
    Ok(Details {
        title: match &d.title {
            None => None,
            Some(t) => Some(check_text("Título", t, MAX_TITLE, true)?),
        },
        objective: check_text("Objetivo", &d.objective, MAX_OBJECTIVE, false)?,
        desired_outcome: check_text("Resultado desejado", &d.desired_outcome, MAX_OUTCOME, false)?,
        constraints: check_list("Restrições", &d.constraints)?,
        criteria: check_criteria(&d.criteria)?,
        notes: check_list("Notas", &d.notes)?,
        references: check_references(&d.references)?,
    })
}

fn check_payload(p: &serde_json::Map<String, serde_json::Value>) -> HubResult<()> {
    let text = serde_json::to_string(p).map_err(|e| e.to_string())?;
    if text.len() > MAX_PAYLOAD {
        return Err("payload de evento grande demais".into());
    }
    for v in p.values() {
        match v {
            serde_json::Value::String(t) => {
                if has_machine_path(t) {
                    return Err("evento contém caminho local".into());
                }
            }
            serde_json::Value::Number(_) | serde_json::Value::Bool(_) => {}
            _ => return Err("payload de evento só aceita texto, número ou booleano".into()),
        }
    }
    Ok(())
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
        check_details(&Details {
            title: None,
            objective: s.objective.clone(),
            desired_outcome: s.desired_outcome.clone(),
            constraints: s.constraints.clone(),
            criteria: s.criteria.clone(),
            notes: s.notes.clone(),
            references: s.references.clone(),
        })
        .map_err(map)?;
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
        // Uma Session finalizada é terminal e confiável: exige os blocos, não revalida critérios
        // (um workspace antigo legítimo, com critérios em texto, continua válido).
        if s.status == SessionStatus::Completed && !s.blocks_complete() {
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
            check_text(
                "descrição do bloco",
                &b.description,
                MAX_BLOCK_DESCRIPTION,
                false,
            )
            .map_err(map)?;
        }
        for e in &s.events {
            ensure_id("evento", &e.id).map_err(map)?;
            if !ids.insert(e.id.as_str()) {
                return fail(format!("id de evento duplicado ({})", e.id));
            }
            if e.created_at.len() > MAX_TIMESTAMP {
                return fail(format!("{at}: data de evento inválida"));
            }
            if let Some(b) = &e.block_id {
                ensure_id("bloco do evento", b).map_err(map)?;
            }
            check_payload(&e.payload).map_err(map)?;
        }
        if s.events.len() > MAX_EVENTS {
            return fail(format!("{at}: excede o limite de eventos"));
        }
        for d in &s.decisions {
            if let Some(b) = &d.block_id {
                if !s.blocks.iter().any(|x| &x.id == b) {
                    return fail(format!(
                        "{at}: decisão referencia um bloco que não existe nesta sessão"
                    ));
                }
            }
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
        s.desired_outcome = s.desired_outcome.trim().to_string();
        for c in &mut s.criteria {
            c.text = c.text.trim().to_string();
        }
        s.criteria.retain(|c| !c.text.is_empty());
        assign_criterion_ids(&s.id, &mut s.criteria);
        for r in &mut s.references {
            r.label = r
                .label
                .take()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty());
        }
        for b in &mut s.blocks {
            b.description = b.description.trim().to_string();
        }
        s.events
            .sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
        for list in [&mut s.constraints, &mut s.notes] {
            *list = list
                .iter()
                .map(|i| i.trim().to_string())
                .filter(|i| !i.is_empty())
                .collect();
        }
        for r in &mut s.references {
            r.value = r.value.trim().to_string();
        }
        s.references.retain(|r| !r.value.is_empty());
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
        .prepare("SELECT id,title,status,description FROM ddae_blocks WHERE session_id=?1 ORDER BY position")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([session_id], |r| {
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
        .map(|(id, title, status, description)| {
            Ok(Block {
                id,
                title,
                description,
                status: BlockStatus::parse(&status)?,
            })
        })
        .collect()
}

fn load_events(conn: &Connection, session_id: &str) -> HubResult<Vec<Event>> {
    let mut stmt = conn
        .prepare(
            "SELECT id,event_type,block_id,payload,created_at FROM ddae_events WHERE session_id=?1 ORDER BY created_at, id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([session_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(id, kind, block_id, payload, created_at)| {
            Ok(Event {
                id,
                kind: serde_json::from_value(serde_json::Value::String(kind))
                    .map_err(|e| e.to_string())?,
                block_id,
                payload: serde_json::from_str(&payload).map_err(|e| e.to_string())?,
                created_at,
            })
        })
        .collect()
}

fn load_decisions(conn: &Connection, session_id: &str) -> HubResult<Vec<Decision>> {
    let mut stmt = conn
        .prepare(
            "SELECT id,title,body,created_at,block_id FROM ddae_decisions WHERE session_id=?1 ORDER BY position",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([session_id], |r| {
            Ok(Decision {
                id: r.get(0)?,
                title: r.get(1)?,
                body: r.get(2)?,
                created_at: r.get(3)?,
                block_id: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

const SESSION_COLUMNS: &str = "id,project_id,number,title,objective,desired_outcome,constraints,criteria,notes,refs,status,pause_reason,result,created_at,updated_at,completed_at";

fn json_column<T: serde::de::DeserializeOwned>(
    r: &rusqlite::Row,
    index: usize,
) -> rusqlite::Result<T> {
    let text: String = r.get(index)?;
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Uma linha de `ddae_sessions` (sem blocks nem decisions, que `assemble` carrega).
fn row_to_session(r: &rusqlite::Row) -> rusqlite::Result<Session> {
    let status: String = r.get(10)?;
    let status = SessionStatus::parse(&status).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            10,
            rusqlite::types::Type::Text,
            Box::<dyn std::error::Error + Send + Sync>::from(e),
        )
    })?;
    let mut session = Session {
        id: r.get(0)?,
        project_id: r.get(1)?,
        number: r.get(2)?,
        title: r.get(3)?,
        objective: r.get(4)?,
        desired_outcome: r.get(5)?,
        constraints: json_column(r, 6)?,
        criteria: json_column(r, 7)?,
        notes: json_column(r, 8)?,
        references: json_column(r, 9)?,
        status,
        pause_reason: r.get(11)?,
        result: r.get(12)?,
        blocks: vec![],
        decisions: vec![],
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
        completed_at: r.get(15)?,
        events: vec![],
    };
    let id = session.id.clone();
    assign_criterion_ids(&id, &mut session.criteria);
    Ok(session)
}

fn assemble(conn: &Connection, mut session: Session) -> HubResult<Session> {
    session.blocks = load_blocks(conn, &session.id)?;
    session.decisions = load_decisions(conn, &session.id)?;
    session.events = load_events(conn, &session.id)?;
    Ok(session)
}

fn load_session(conn: &Connection, id: &str) -> HubResult<Session> {
    let row = conn
        .query_row(
            &format!("SELECT {SESSION_COLUMNS} FROM ddae_sessions WHERE id=?1"),
            [id],
            row_to_session,
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
        stmt.query_map([id], row_to_session)
    } else {
        stmt.query_map([], row_to_session)
    }
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;
    rows.into_iter().map(|r| assemble(conn, r)).collect()
}

fn json_text<T: Serialize>(value: &T) -> HubResult<String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

fn insert_session(tx: &Transaction, s: &Session) -> HubResult<()> {
    tx.execute(
        "INSERT INTO ddae_sessions(id,project_id,number,title,objective,desired_outcome,constraints,criteria,notes,refs,status,pause_reason,result,created_at,updated_at,completed_at) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            s.id,
            s.project_id,
            s.number,
            s.title,
            s.objective,
            s.desired_outcome,
            json_text(&s.constraints)?,
            json_text(&s.criteria)?,
            json_text(&s.notes)?,
            json_text(&s.references)?,
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
            "INSERT INTO ddae_blocks(id,session_id,position,title,status,description) VALUES(?1,?2,?3,?4,?5,?6)",
            params![b.id, s.id, i as i64, b.title, b.status.as_str(), b.description],
        )
        .map_err(|e| e.to_string())?;
    }
    for (i, d) in s.decisions.iter().enumerate() {
        tx.execute(
            "INSERT INTO ddae_decisions(id,session_id,position,title,body,created_at,block_id) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![d.id, s.id, i as i64, d.title, d.body, d.created_at, d.block_id],
        )
        .map_err(|e| e.to_string())?;
    }
    for e in &s.events {
        tx.execute(
            "INSERT OR IGNORE INTO ddae_events(id,session_id,block_id,event_type,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![e.id, s.id, e.block_id, e.kind.as_str(), json_text(&e.payload)?, e.created_at],
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
        let sid = s.id.clone();
        assign_criterion_ids(&sid, &mut s.criteria);
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

/// Grava um evento na transação corrente (UUID próprio; `created_at` do banco).
fn record_event(tx: &Transaction, session_id: &str, e: &NewEvent) -> HubResult<()> {
    check_payload(&e.payload)?;
    tx.execute(
        "INSERT INTO ddae_events(id,session_id,block_id,event_type,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
        params![new_id(), session_id, e.block_id, e.kind.as_str(), json_text(&e.payload)?, now(tx)?],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn legacy_event_id(session_id: &str) -> String {
    deterministic_uuid(&format!("lkr-lab:ddae:{session_id}:event:legacy_imported"))
}

/// Sessions importadas ANTES de existirem eventos ganham um único evento LEGACY_IMPORTED (com o
/// instante da atividade de importação já registrada). Nenhum histórico detalhado é inventado.
/// Id determinístico: o mesmo em qualquer máquina, sem duplicar ao sincronizar.
pub fn backfill_legacy_events(conn: &Connection) -> HubResult<()> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.number, s.project_id FROM ddae_sessions s \
             WHERE NOT EXISTS (SELECT 1 FROM ddae_events e WHERE e.session_id = s.id)",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for (session_id, number, project_id) in rows {
        let at: Option<String> = conn
            .query_row(
                "SELECT created_at FROM activities WHERE project_id=?1 AND action=?2 ORDER BY id LIMIT 1",
                params![
                    project_id,
                    format!(
                        "DDAE: {} importada do histórico do projeto",
                        label(number)
                    )
                ],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(created_at) = at {
            conn.execute(
                "INSERT OR IGNORE INTO ddae_events(id,session_id,block_id,event_type,payload,created_at) VALUES(?1,?2,NULL,'LEGACY_IMPORTED','{}',?3)",
                params![legacy_event_id(&session_id), session_id, created_at],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
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

    /// Detalhe de UMA Session, validando o PAR (Project, Session): uma Session de outro Project
    /// é tratada como inexistente (a rota nunca depende só do UUID global).
    pub fn ddae_session_detail(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> HubResult<SessionView> {
        project_exists(&self.conn, project_id)?;
        let session = load_session(&self.conn, session_id)
            .map_err(|_| "Sessão não encontrada neste projeto.".to_string())?;
        if session.project_id != project_id {
            return Err("Sessão não encontrada neste projeto.".into());
        }
        Ok(SessionView::from(session))
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
            desired_outcome: String::new(),
            constraints: vec![],
            criteria: vec![],
            notes: vec![],
            references: vec![],
            events: vec![],
        };
        insert_session(&tx, &session)?;
        record_event(
            &tx,
            &session.id,
            &ev(
                EventType::SessionCreated,
                None,
                &[("title", &session.title)],
            ),
        )?;
        activity(
            &tx,
            project_id,
            &format!("DDAE: {} criada", session.label()),
        )?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(session)
    }

    /// Roda uma mudança sobre a Session numa transação: recusa Session finalizada (terminal),
    /// atualiza `updated_at`, grava os eventos e a atividade NA MESMA transação e devolve a Session
    /// recarregada. Se qualquer parte falhar, nada muda.
    fn ddae_mutate(
        &mut self,
        session_id: &str,
        change: impl FnOnce(&Transaction, &mut Session) -> HubResult<(String, Vec<NewEvent>)>,
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
        let (text, events) = change(&tx, &mut session)?;
        for e in &events {
            record_event(&tx, session_id, e)?;
        }
        activity(
            &tx,
            &session.project_id,
            &format!("DDAE: {} {text}", session.label()),
        )?;
        let reloaded = load_session(&tx, session_id)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(reloaded)
    }

    /// Define título, objetivo, resultado desejado, restrições, critérios, notas e referências.
    /// Substitui esses campos (não toca em número, estado, blocos nem decisões); `title: None`
    /// mantém o título. Os eventos (critério adicionado/concluído/reaberto/removido, nota
    /// adicionada/removida, detalhes alterados) saem da diferença entre o estado antigo e o novo.
    pub fn ddae_update_details(
        &mut self,
        session_id: &str,
        details: Details,
    ) -> HubResult<Session> {
        let details = check_details(&details)?;
        self.ddae_mutate(session_id, |tx, s| {
            let mut criteria = details.criteria.clone();
            for c in &mut criteria {
                if c.id.is_empty() {
                    c.id = new_id();
                }
            }
            let title = details.title.clone().unwrap_or_else(|| s.title.clone());
            let events = details_events(s, &title, &details, &criteria);
            tx.execute(
                "UPDATE ddae_sessions SET title=?8,objective=?2,desired_outcome=?3,constraints=?4,criteria=?5,notes=?6,refs=?7 WHERE id=?1",
                params![
                    s.id,
                    details.objective,
                    details.desired_outcome,
                    json_text(&details.constraints)?,
                    json_text(&criteria)?,
                    json_text(&details.notes)?,
                    json_text(&details.references)?,
                    title
                ],
            )
            .map_err(|e| e.to_string())?;
            Ok(("teve os detalhes atualizados".into(), events))
        })
    }

    /// Adiciona uma referência. Para `project_path` aceita também o caminho ABSOLUTO de um arquivo
    /// escolhido: o backend o converte em caminho RELATIVO ao Project (e recusa o que está fora).
    pub fn ddae_add_reference(
        &mut self,
        session_id: &str,
        kind: ReferenceKind,
        value: &str,
        label: Option<&str>,
    ) -> HubResult<Session> {
        let session = load_session(&self.conn, session_id)?;
        let value = match kind {
            ReferenceKind::ProjectPath => {
                let project = self.project(&session.project_id)?;
                relativize(&project.local_path, value)?
            }
            ReferenceKind::Url => value.trim().to_string(),
        };
        let mut references = session.references.clone();
        let new_ref = Reference {
            kind,
            value,
            label: label.map(str::to_string),
        };
        if references
            .iter()
            .any(|r| r.kind == new_ref.kind && r.value == new_ref.value)
        {
            return Err("Esta referência já está na sessão.".into());
        }
        references.push(new_ref);
        let references = check_references(&references)?;
        self.ddae_mutate(session_id, |tx, s| {
            tx.execute(
                "UPDATE ddae_sessions SET refs=?2 WHERE id=?1",
                params![s.id, json_text(&references)?],
            )
            .map_err(|e| e.to_string())?;
            Ok((
                "ganhou uma referência".into(),
                vec![ev(
                    EventType::DetailsUpdated,
                    None,
                    &[("fields", "references")],
                )],
            ))
        })
    }

    /// Contexto DETERMINÍSTICO da Session (Markdown): só dados reais da Session e o nome do Project.
    /// Sem LLM, sem timestamps, sem caminho absoluto, Machine ID, hostname, IP, PID ou segredo.
    pub fn ddae_generate_context(&self, session_id: &str) -> HubResult<SessionContext> {
        let session = load_session(&self.conn, session_id)?;
        let project = self.project(&session.project_id)?;
        let markdown = render_context(&project.name, &session)?;
        Ok(SessionContext {
            ready_for_ai: ready_for_ai(&session),
            markdown,
        })
    }

    /// Acrescenta um Block `pending` ao fim da ordem (descrição opcional).
    pub fn ddae_add_block(
        &mut self,
        session_id: &str,
        title: &str,
        description: &str,
    ) -> HubResult<Session> {
        let title = check_text("Bloco", title, MAX_BLOCK_TITLE, true)?;
        let description = check_text(
            "Descrição do bloco",
            description,
            MAX_BLOCK_DESCRIPTION,
            false,
        )?;
        self.ddae_mutate(session_id, |tx, s| {
            if s.blocks.len() >= MAX_BLOCKS {
                return Err(format!("Máximo de {MAX_BLOCKS} blocos por sessão."));
            }
            let id = new_id();
            tx.execute(
                "INSERT INTO ddae_blocks(id,session_id,position,title,status,description) VALUES(?1,?2,?3,?4,'pending',?5)",
                params![id, s.id, s.blocks.len() as i64, title, description],
            )
            .map_err(|e| e.to_string())?;
            Ok((
                format!("ganhou o bloco “{title}”"),
                vec![ev(EventType::BlockAdded, Some(&id), &[("title", &title)])],
            ))
        })
    }

    /// Renomeia um Block pendente ou em andamento. Um bloco concluído é histórico e não muda.
    pub fn ddae_rename_block(
        &mut self,
        session_id: &str,
        block_id: &str,
        title: &str,
    ) -> HubResult<Session> {
        let title = check_text("Bloco", title, MAX_BLOCK_TITLE, true)?;
        self.ddae_mutate(session_id, |tx, s| {
            let block = s
                .blocks
                .iter()
                .find(|b| b.id == block_id)
                .ok_or("Bloco não encontrado nesta sessão.")?;
            if block.status == BlockStatus::Completed {
                return Err("Um bloco concluído é histórico e não pode ser renomeado.".into());
            }
            if block.title == title {
                return Err("O bloco já tem este nome.".into());
            }
            let old = block.title.clone();
            tx.execute(
                "UPDATE ddae_blocks SET title=?2 WHERE id=?1",
                params![block_id, title],
            )
            .map_err(|e| e.to_string())?;
            Ok((
                format!("renomeou o bloco “{old}”"),
                vec![ev(
                    EventType::BlockRenamed,
                    Some(block_id),
                    &[("from", &old), ("to", &title)],
                )],
            ))
        })
    }

    /// Remove um Block SOMENTE se estiver pendente (em andamento e concluídos ficam).
    pub fn ddae_remove_block(&mut self, session_id: &str, block_id: &str) -> HubResult<Session> {
        self.ddae_mutate(session_id, |tx, s| {
            let index = s
                .blocks
                .iter()
                .position(|b| b.id == block_id)
                .ok_or("Bloco não encontrado nesta sessão.")?;
            let block = &s.blocks[index];
            if block.status != BlockStatus::Pending {
                return Err("Só um bloco pendente pode ser removido.".into());
            }
            let title = block.title.clone();
            tx.execute("DELETE FROM ddae_blocks WHERE id=?1", [block_id])
                .map_err(|e| e.to_string())?;
            // Reempacota as posições (uma a uma, em ordem, para não violar UNIQUE(session, posição)).
            for (offset, later) in s.blocks[index + 1..].iter().enumerate() {
                tx.execute(
                    "UPDATE ddae_blocks SET position=?2 WHERE id=?1",
                    params![later.id, (index + offset) as i64],
                )
                .map_err(|e| e.to_string())?;
            }
            Ok((
                format!("perdeu o bloco “{title}”"),
                vec![ev(
                    EventType::BlockRemoved,
                    Some(block_id),
                    &[("title", &title)],
                )],
            ))
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
            Ok((
                format!("iniciou o bloco “{}”", block.title),
                vec![ev(
                    EventType::BlockStarted,
                    Some(block_id),
                    &[("title", &block.title)],
                )],
            ))
        })
    }

    /// in_progress → completed. Não inicia o próximo bloco NEM finaliza a Session.
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
            Ok((
                format!("concluiu o bloco “{}”", block.title),
                vec![ev(
                    EventType::BlockCompleted,
                    Some(block_id),
                    &[("title", &block.title)],
                )],
            ))
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

    /// O motivo é OPCIONAL no backend (a interface o incentiva, mas não bloqueia por vazio).
    fn pause(
        &mut self,
        session_id: &str,
        reason: &str,
        target: SessionStatus,
    ) -> HubResult<Session> {
        let reason = check_text("Motivo", reason, MAX_REASON, false)?;
        let reason_value: Option<&str> = if reason.is_empty() {
            None
        } else {
            Some(&reason)
        };
        self.ddae_mutate(session_id, |tx, s| {
            if s.status != SessionStatus::Active {
                return Err("Só uma sessão ativa pode ser congelada ou parada.".into());
            }
            tx.execute(
                "UPDATE ddae_sessions SET status=?2, pause_reason=?3 WHERE id=?1",
                params![s.id, target.as_str(), reason_value],
            )
            .map_err(|e| e.to_string())?;
            let (text, kind) = match target {
                SessionStatus::Frozen => ("congelada", EventType::SessionFrozen),
                _ => ("parada", EventType::SessionStopped),
            };
            Ok((text.into(), vec![ev(kind, None, &[("reason", &reason)])]))
        })
    }

    /// frozen|stopped → active. Recusa se o Project já tem outra ativa (nunca mexe na outra).
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
            Ok(("retomada".into(), vec![ev(EventType::SessionResumed, None, &[])]))
        })
    }

    /// → completed (terminal), SEMPRE explícito. Exige todos os blocks concluídos e, se existirem
    /// critérios, todos concluídos. Sem critérios, não bloqueia por critérios.
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
            if s.next_block().is_some() {
                let p = s.progress();
                return Err(format!(
                    "Só é possível finalizar com todos os blocos concluídos ({}/{}).",
                    p.completed, p.total
                ));
            }
            let open = s.criteria.iter().filter(|c| !c.completed).count();
            if open > 0 {
                return Err(format!(
                    "Só é possível finalizar com todos os critérios de conclusão concluídos ({} pendente{}).",
                    open,
                    if open == 1 { "" } else { "s" }
                ));
            }
            let stamp = now(tx)?;
            tx.execute(
                "UPDATE ddae_sessions SET status='completed', pause_reason=NULL, result=?2, completed_at=?3 WHERE id=?1",
                params![s.id, if result.is_empty() { None } else { Some(&result) }, stamp],
            )
            .map_err(|e| e.to_string())?;
            Ok((
                "finalizada".into(),
                vec![ev(EventType::SessionCompleted, None, &[("result", &result)])],
            ))
        })
    }

    /// Registra uma Decision (acrescentada ao fim; é registro histórico: sem editar nem apagar).
    /// `block_id` opcional precisa ser um bloco desta Session.
    pub fn ddae_add_decision(
        &mut self,
        session_id: &str,
        title: &str,
        body: &str,
        block_id: Option<&str>,
    ) -> HubResult<Session> {
        let title = check_text("Decisão", title, MAX_DECISION_TITLE, true)?;
        let body = check_text("Detalhe da decisão", body, MAX_DECISION_BODY, false)?;
        self.ddae_mutate(session_id, |tx, s| {
            if s.decisions.len() >= MAX_DECISIONS {
                return Err(format!("Máximo de {MAX_DECISIONS} decisões por sessão."));
            }
            if let Some(b) = block_id {
                if !s.blocks.iter().any(|x| x.id == b) {
                    return Err("O bloco da decisão não pertence a esta sessão.".into());
                }
            }
            let id = new_id();
            tx.execute(
                "INSERT INTO ddae_decisions(id,session_id,position,title,body,created_at,block_id) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![id, s.id, s.decisions.len() as i64, title, body, now(tx)?, block_id],
            )
            .map_err(|e| e.to_string())?;
            Ok((
                format!("registrou a decisão “{title}”"),
                vec![ev(EventType::DecisionAdded, block_id, &[("title", &title)])],
            ))
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
                description: String::new(),
                status: *status,
            })
            .collect();
        let session = Session {
            id: id.clone(),
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
            updated_at: stamp.clone(),
            completed_at: None,
            desired_outcome: String::new(),
            constraints: vec![],
            criteria: vec![],
            notes: vec![],
            references: vec![],
            events: vec![Event {
                id: legacy_event_id(&id),
                kind: EventType::LegacyImported,
                block_id: None,
                payload: serde_json::Map::new(),
                created_at: stamp.clone(),
            }],
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

/// Eventos da edição de detalhes, pela diferença entre o estado antigo e o novo.
fn details_events(
    old: &Session,
    title: &str,
    new: &Details,
    criteria: &[Criterion],
) -> Vec<NewEvent> {
    let mut out = Vec::new();
    let mut fields: Vec<&str> = Vec::new();
    if title != old.title {
        fields.push("title");
    }
    if new.objective != old.objective {
        fields.push("objective");
    }
    if new.desired_outcome != old.desired_outcome {
        fields.push("desired_outcome");
    }
    if new.constraints != old.constraints {
        fields.push("constraints");
    }
    if new.references != old.references {
        fields.push("references");
    }
    for c in criteria {
        match old.criteria.iter().find(|o| o.id == c.id) {
            None => out.push(ev(EventType::CriterionAdded, None, &[("text", &c.text)])),
            Some(o) => {
                if o.completed != c.completed {
                    let kind = if c.completed {
                        EventType::CriterionCompleted
                    } else {
                        EventType::CriterionReopened
                    };
                    out.push(ev(kind, None, &[("text", &c.text)]));
                }
                if o.text != c.text && !fields.contains(&"criteria") {
                    fields.push("criteria");
                }
            }
        }
    }
    for o in &old.criteria {
        if !criteria.iter().any(|c| c.id == o.id) {
            out.push(ev(EventType::CriterionRemoved, None, &[("text", &o.text)]));
        }
    }
    let mut remaining = old.notes.clone();
    for n in &new.notes {
        if let Some(i) = remaining.iter().position(|o| o == n) {
            remaining.remove(i);
        } else {
            out.push(ev(EventType::NoteAdded, None, &[("text", n)]));
        }
    }
    for removed in remaining {
        out.push(ev(EventType::NoteRemoved, None, &[("text", &removed)]));
    }
    if !fields.is_empty() {
        out.insert(
            0,
            ev(
                EventType::DetailsUpdated,
                None,
                &[("fields", &fields.join(","))],
            ),
        );
    }
    out
}

/// Caminho RELATIVO ao Project a partir do que o usuário informou: relativo passa (validado);
/// absoluto só é aceito se estiver DENTRO da pasta do Project (e é convertido). Nunca persiste
/// caminho absoluto.
fn relativize(project_path: &str, input: &str) -> HubResult<String> {
    let input = input.trim();
    if !crate::portable::is_absolute_path(input) {
        return Ok(input.replace('\\', "/"));
    }
    if project_path.is_empty() {
        return Err(
            "O projeto não está localizado nesta máquina; informe um caminho relativo.".into(),
        );
    }
    let root = crate::runtime::plain_path(crate::projects::canonical(project_path)?);
    let given = std::path::Path::new(input);
    let given =
        crate::runtime::plain_path(given.canonicalize().unwrap_or_else(|_| given.to_path_buf()));
    let rest = given.strip_prefix(&root).map_err(|_| {
        "O arquivo está fora da pasta do projeto; escolha um arquivo dentro do projeto.".to_string()
    })?;
    let parts: Vec<String> = rest
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    if parts.is_empty() || parts.iter().any(|p| p == ".." || p == ".") {
        return Err("Caminho inválido para uma referência do projeto.".into());
    }
    Ok(parts.join("/"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContext {
    pub markdown: String,
    pub ready_for_ai: ReadyForAi,
}

const NOT_INFORMED: &str = "_Não informado._";

fn list_section(out: &mut String, title: &str, items: &[String]) {
    out.push_str(&format!("## {title}\n\n"));
    if items.is_empty() {
        out.push_str(NOT_INFORMED);
        out.push('\n');
    } else {
        for item in items {
            out.push_str(&format!("- {item}\n"));
        }
    }
    out.push('\n');
}

fn text_section(out: &mut String, title: &str, text: &str) {
    out.push_str(&format!(
        "## {title}\n\n{}\n\n",
        if text.is_empty() { NOT_INFORMED } else { text }
    ));
}

const MISSING_LABEL: [(&str, &str); 5] = [
    ("objective", "objetivo"),
    ("desired_outcome", "resultado desejado"),
    ("blocks", "blocos"),
    ("criteria", "critérios de conclusão"),
    ("actionable_block", "bloco atual ou pendente"),
];

/// Markdown estável: a mesma Session produz exatamente os mesmos bytes.
fn render_context(project_name: &str, s: &Session) -> HubResult<String> {
    let ready = ready_for_ai(s);
    let state = match ready.state {
        ContextState::Ready => "Pronto para continuar".to_string(),
        ContextState::Available => "Contexto disponível (sessão finalizada)".to_string(),
        ContextState::Incomplete => format!(
            "Incompleto — falta: {}",
            ready
                .missing
                .iter()
                .map(|m| MISSING_LABEL
                    .iter()
                    .find(|(k, _)| k == m)
                    .map_or(*m, |(_, l)| *l))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let progress = s.progress();
    let mut out = format!(
        "# DDAE — {}: {}\n\n- Projeto: {}\n- Estado: {} ({})\n- Contexto IA: {}\n- Progresso: {} / {} blocos concluídos\n",
        s.label(),
        s.title,
        project_name,
        s.status.as_str(),
        crate::ddae::status_pt(s.status),
        state,
        progress.completed,
        progress.total
    );
    if let Some(reason) = &s.pause_reason {
        out.push_str(&format!("- Motivo da pausa: {reason}\n"));
    }
    if let Some(result) = &s.result {
        out.push_str(&format!("- Resultado: {result}\n"));
    }
    out.push('\n');
    text_section(&mut out, "Objetivo", &s.objective);
    text_section(&mut out, "Resultado desejado", &s.desired_outcome);
    list_section(&mut out, "Restrições", &s.constraints);
    out.push_str("## Critérios de conclusão\n\n");
    if s.criteria.is_empty() {
        out.push_str(NOT_INFORMED);
        out.push('\n');
    }
    for c in &s.criteria {
        out.push_str(&format!(
            "- [{}] {}\n",
            if c.completed { "x" } else { " " },
            c.text
        ));
    }
    out.push('\n');
    out.push_str("## Blocos\n\n");
    if s.blocks.is_empty() {
        out.push_str(NOT_INFORMED);
        out.push('\n');
    }
    for (i, b) in s.blocks.iter().enumerate() {
        let mark = match b.status {
            BlockStatus::Completed => "x",
            BlockStatus::InProgress => "~",
            BlockStatus::Pending => " ",
        };
        out.push_str(&format!("{}. [{mark}] {}\n", i + 1, b.title));
    }
    out.push_str(&format!(
        "\n- Bloco atual: {}\n- Próximo bloco: {}\n\n",
        s.current_block().map_or("nenhum", |b| b.title.as_str()),
        s.next_block().map_or("nenhum", |b| b.title.as_str())
    ));
    out.push_str("## Decisões\n\n");
    if s.decisions.is_empty() {
        out.push_str(NOT_INFORMED);
        out.push('\n');
    }
    for d in &s.decisions {
        let block = d
            .block_id
            .as_ref()
            .and_then(|id| s.blocks.iter().find(|b| &b.id == id))
            .map(|b| format!(" (bloco: {})", b.title))
            .unwrap_or_default();
        if d.body.is_empty() {
            out.push_str(&format!("- {}{block}\n", d.title));
        } else {
            out.push_str(&format!("- {}{block} — {}\n", d.title, d.body));
        }
    }
    out.push('\n');
    list_section(&mut out, "Notas", &s.notes);
    out.push_str("## Referências\n\n");
    if s.references.is_empty() {
        out.push_str(NOT_INFORMED);
        out.push('\n');
    }
    for r in &s.references {
        let kind = match r.kind {
            ReferenceKind::ProjectPath => "project_path",
            ReferenceKind::Url => "url",
        };
        out.push_str(&format!("- {kind}: {}\n", r.value));
    }
    // Última barreira: nada de caminho local no contexto (os campos já são validados na escrita).
    if has_machine_path(&out) {
        return Err("O contexto da sessão contém um caminho local e não foi gerado.".into());
    }
    Ok(out)
}

fn status_pt(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "ativa",
        SessionStatus::Frozen => "congelada",
        SessionStatus::Stopped => "parada",
        SessionStatus::Completed => "finalizada",
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
