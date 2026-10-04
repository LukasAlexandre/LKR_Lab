//! Deterministic Diagnostic Engine (SESSION-002, Block 09): interpreta o que os collectors já
//! observam e decide, por REGRAS, o que merece a atenção do usuário.
//!
//! ```text
//! OBSERVATIONS (snapshots existentes) → FACTS → RULES → FINDINGS → ALERTS (lifecycle) → DIAGNOSTICS
//! ```
//!
//! * **Signal** é o fato bruto; **Finding** é a interpretação determinística de um fato;
//!   **Alert** é um Finding persistido com ciclo de vida local; **Diagnostic** é uma ação explícita
//!   para obter mais evidência (`diagnostic_runner`); **Remediation** NÃO existe aqui.
//! * Nenhuma regra usa modelo de IA, pontuação ou heurística opaca. Cada Finding carrega regra,
//!   evidência, confiança e o motivo, e o estado vem só das regras abaixo.
//! * Os collectors NÃO são duplicados nem relidos: o motor consome os snapshots que eles já têm
//!   (cada um com o próprio TTL). Dado velho demais não gera Finding novo e não resolve o antigo.
//! * `Unknown` não é severidade e NÃO vira alerta: fonte indisponível, que exige administrador ou
//!   sem dado suficiente simplesmente não é avaliada.
//! * Estado de workflow (Session congelada, Worktree parada, Planning pendente) não é alerta.
//! * Alertas são estado da MÁQUINA: ficam em `hub.db` e nunca no workspace portátil, no sync, no
//!   Git, no Planejamento ou no DDAE.
use crate::{
    control_plane::{Context, PortObservation},
    diagnostic_runner::DiagnosticId,
    health::{self, HealthStatus},
    network_security::{
        AvProvider, DefenderState, ListenerScope, NetworkSecuritySnapshot, ProfileKind, WscHealth,
    },
    supervisor::{RunInfo, RunState},
    system::RawProcess,
    telemetry::Telemetry,
    windows_health::{
        Health, Millis, ServiceState, SignalKind, SourceState, StartType, WindowsHealthSnapshot,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

// ------------------------------------------------------------------ parâmetros (documentados)

/// Telemetria é amostrada a cada 1–5 s: mais velha que isto é dado velho.
pub const TELEMETRY_MAX_AGE_MS: Millis = 30_000;
/// Um domínio dos collectors é "velho" depois de 2× o TTL dele.
pub const STALE_TTL_FACTOR: Millis = 2;
/// Um alerta só é resolvido depois de ficar este tempo sem ser visto por uma fonte avaliada: um
/// problema que pisca entre duas leituras não abre e fecha alertas.
pub const RESOLVE_DELAY_MS: Millis = 90_000;
/// Falhas repetidas: 3 ou mais execuções gerenciadas da mesma ação, no mesmo Project, que falharam
/// nesta janela.
pub const REPEATED_FAILURE_COUNT: usize = 3;
pub const REPEATED_FAILURE_WINDOW_MS: Millis = 15 * 60_000;
/// Uma execução que falhou há mais que isto deixa de ser relevante para o alerta.
pub const FAILED_RUN_RELEVANCE_MS: Millis = 24 * 3_600_000;
/// Colisão de porta só vale para Project com execução ativa ou que falhou há pouco.
pub const COLLISION_RECENT_FAILURE_MS: Millis = 10 * 60_000;
/// Assinaturas do Defender mais velhas que isto (dias).
pub const SIGNATURE_STALE_DAYS: i64 = 7;
/// Falhas de atualização do Windows em 7 dias para contar como "repetidas".
pub const UPDATE_REPEATED_FAILURES: u32 = 2;
/// Falhas de serviço em 24 h (a regra já vem filtrada pelo Block 07).
pub const SERVICE_FAILURE_ATTENTION: u32 = 3;
/// Quantas portas aparecem na evidência do listener agregado.
pub const LISTENER_PORTS_SHOWN: usize = 8;
/// Alertas resolvidos continuam visíveis por este tempo; o histórico é podado depois.
pub const RESOLVED_VISIBLE_MS: Millis = 7 * 24 * 3_600_000;
pub const RESOLVED_RECENT_MS: Millis = 24 * 3_600_000;

// ------------------------------------------------------------------ contrato

/// `Unknown` NÃO é severidade (significa dado insuficiente e nunca vira alerta).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Attention,
    Critical,
}
impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Attention => "attention",
            Self::Critical => "critical",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "info" => Some(Self::Info),
            "attention" => Some(Self::Attention),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}
impl From<HealthStatus> for Severity {
    fn from(status: HealthStatus) -> Self {
        match status {
            HealthStatus::Healthy => Self::Info,
            HealthStatus::Attention => Self::Attention,
            HealthStatus::Critical => Self::Critical,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}
impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    Machine,
    Windows,
    Security,
    Network,
    Runtime,
}
impl Domain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Machine => "machine",
            Self::Windows => "windows",
            Self::Security => "security",
            Self::Network => "network",
            Self::Runtime => "runtime",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "machine" => Some(Self::Machine),
            "windows" => Some(Self::Windows),
            "security" => Some(Self::Security),
            "network" => Some(Self::Network),
            "runtime" => Some(Self::Runtime),
            _ => None,
        }
    }
}

/// Fato que sustenta uma conclusão (nunca o snapshot inteiro).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub label: String,
    pub value: String,
    /// Fonte do fato (id da fonte: `machine.disk`, `windows.events`…).
    pub source: String,
}

/// Diagnóstico explícito sugerido (nunca executado automaticamente).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRef {
    pub id: String,
    pub target: Option<String>,
    pub label: String,
}

/// Para onde levar o usuário (navegação). Nunca é uma correção.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cta {
    /// runtime | project | windows_health | network_security | machine | diagnostic
    pub kind: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Estável: igual ao fingerprint (regra + recurso).
    pub id: String,
    pub fingerprint: String,
    pub rule_id: String,
    pub title: String,
    pub summary: String,
    pub severity: Severity,
    pub confidence: Confidence,
    pub domain: Domain,
    /// Fonte avaliada (`machine.disk`…): só ela pode resolver o alerta correspondente.
    pub source: String,
    pub resource: String,
    pub evidence: Vec<Evidence>,
    pub reason: String,
    pub recommended_next_step: String,
    pub diagnostic_action: Option<DiagnosticRef>,
    pub cta: Option<Cta>,
}

pub fn fingerprint(rule_id: &str, resource: &str) -> String {
    format!("{rule_id}@{resource}")
}

struct Rule<'a> {
    rule_id: &'a str,
    resource: &'a str,
    domain: Domain,
    source: &'a str,
    severity: Severity,
    confidence: Confidence,
}
impl Rule<'_> {
    fn finding(
        &self,
        title: impl Into<String>,
        summary: impl Into<String>,
        reason: impl Into<String>,
        next_step: impl Into<String>,
    ) -> Finding {
        let fingerprint = fingerprint(self.rule_id, self.resource);
        Finding {
            id: fingerprint.clone(),
            fingerprint,
            rule_id: self.rule_id.into(),
            title: title.into(),
            summary: summary.into(),
            severity: self.severity,
            confidence: self.confidence,
            domain: self.domain,
            source: self.source.into(),
            resource: self.resource.into(),
            evidence: vec![],
            reason: reason.into(),
            recommended_next_step: next_step.into(),
            diagnostic_action: None,
            cta: None,
        }
    }
}
impl Finding {
    fn ev(mut self, label: &str, value: impl Into<String>) -> Self {
        let source = self.source.clone();
        self.evidence.push(Evidence {
            label: label.into(),
            value: value.into(),
            source,
        });
        self
    }
    fn diagnostic(mut self, id: DiagnosticId, target: Option<&str>) -> Self {
        self.diagnostic_action = Some(DiagnosticRef {
            id: id.as_str().into(),
            target: target.map(String::from),
            label: id.label().into(),
        });
        self
    }
    fn cta(mut self, kind: &str, target: Option<&str>) -> Self {
        self.cta = Some(Cta {
            kind: kind.into(),
            target: target.map(String::from),
        });
        self
    }
}

// ------------------------------------------------------------------ fontes e fatos

pub const SRC_DISK: &str = "machine.disk";
pub const SRC_CPU: &str = "machine.cpu";
pub const SRC_MEMORY: &str = "machine.memory";
pub const SRC_THERMAL: &str = "machine.thermal";
pub const SRC_WIN_RESTART: &str = "windows.restart";
pub const SRC_WIN_SERVICES: &str = "windows.services";
pub const SRC_WIN_DEVICES: &str = "windows.devices";
pub const SRC_WIN_EVENTS: &str = "windows.events";
pub const SRC_WIN_UPDATES: &str = "windows.updates";
pub const SRC_WIN_VOLUMES: &str = "windows.volumes";
pub const SRC_FIREWALL: &str = "security.firewall";
pub const SRC_ANTIVIRUS: &str = "security.antivirus";
pub const SRC_EXPOSURE: &str = "network.exposure";
pub const SRC_RUNS: &str = "runtime.runs";
pub const SRC_PORTS: &str = "runtime.ports";

pub const SOURCES: &[(&str, &str)] = &[
    (SRC_DISK, "Espaço dos volumes"),
    (SRC_CPU, "CPU"),
    (SRC_MEMORY, "Memória"),
    (SRC_THERMAL, "Temperaturas"),
    (SRC_WIN_RESTART, "Reinício pendente"),
    (SRC_WIN_SERVICES, "Serviços do Windows"),
    (SRC_WIN_DEVICES, "Dispositivos"),
    (SRC_WIN_EVENTS, "Eventos do sistema"),
    (SRC_WIN_UPDATES, "Windows Update"),
    (SRC_WIN_VOLUMES, "Integridade dos volumes"),
    (SRC_FIREWALL, "Firewall"),
    (SRC_ANTIVIRUS, "Antivírus"),
    (SRC_EXPOSURE, "Portas em escuta"),
    (SRC_RUNS, "Execuções gerenciadas"),
    (SRC_PORTS, "Portas de Projects"),
];

/// Situação de uma fonte nesta avaliação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceHealth {
    /// Fresca e avaliada pelas regras.
    Evaluated,
    /// Dado velho demais: não gera alerta novo nem resolve o existente.
    Stale,
    /// Sem dado (indisponível, exige administrador, desconhecido): não é problema.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub id: String,
    pub label: String,
    pub state: SourceHealth,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VolumeFact {
    pub mount: String,
    pub total: u64,
    pub available: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MachineAlertFact {
    /// cpu | memory | temperature
    pub source: String,
    pub severity: HealthStatus,
    pub title: String,
    pub detail: String,
    pub resource: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MachineFacts {
    pub volumes: Option<Vec<VolumeFact>>,
    /// CPU/memória/temperatura só são avaliadas depois da primeira amostra completa.
    pub load_ready: bool,
    pub cpu_percent: f32,
    pub memory_percent: f32,
    pub memory_available: u64,
    pub commit_percent: Option<f32>,
    pub alerts: Vec<MachineAlertFact>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceFact {
    pub id: String,
    pub label: String,
    pub state: ServiceState,
    pub start: StartType,
    pub health: Health,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceFact {
    pub name: String,
    pub class: Option<String>,
    pub manufacturer: Option<String>,
    pub problem_code: u32,
    pub problem: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct EventFact {
    pub kind: SignalKind,
    pub label: String,
    pub count_24h: u32,
    pub count_7d: u32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct WinVolumeFact {
    pub mount: String,
    pub dirty: Option<bool>,
    pub read_only: Option<bool>,
    pub status: Health,
    pub reasons: Vec<String>,
}

/// `None` = domínio não avaliado (indisponível, desconhecido ou velho).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowsFacts {
    pub restart_pending: Option<bool>,
    pub services: Option<Vec<ServiceFact>>,
    pub devices: Option<Vec<DeviceFact>>,
    pub events: Option<Vec<EventFact>>,
    pub update_failures_7d: Option<u32>,
    pub volumes: Option<Vec<WinVolumeFact>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirewallFact {
    pub active_profile: Option<ProfileKind>,
    pub active_enabled: Option<bool>,
    pub security_center: Option<WscHealth>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct AntivirusFact {
    pub provider: AvProvider,
    pub defender: DefenderState,
    pub security_center: Option<WscHealth>,
    pub active_threats: Option<u32>,
    pub signature_age_days: Option<i64>,
    pub third_party: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SecurityFacts {
    pub firewall: Option<FirewallFact>,
    pub antivirus: Option<AntivirusFact>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListenerFact {
    pub port: u16,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub project_name: Option<String>,
    pub system: bool,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NetworkFacts {
    /// Só os que escutam em todas as interfaces.
    pub all_interface_listeners: Option<Vec<ListenerFact>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunFact {
    pub run_id: String,
    pub project_id: String,
    pub command_id: String,
    pub command: String,
    pub state: RunState,
    pub exit_code: Option<i32>,
    pub started_at: Millis,
    pub ended_at: Option<Millis>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredPort {
    pub project_id: String,
    pub port: u16,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PortOwner {
    pub port: u16,
    pub pid: u32,
    pub process_name: Option<String>,
    /// Project cuja execução gerenciada possui este processo, se houver.
    pub managed_project_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RuntimeFacts {
    /// As portas em escuta puderam ser lidas (sem isso não há colisão nem resolução dela).
    pub ports_available: bool,
    pub runs: Vec<RunFact>,
    pub declared_ports: Vec<DeclaredPort>,
    pub owners: Vec<PortOwner>,
    pub project_names: HashMap<String, String>,
    pub self_pid: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Facts {
    pub machine: Option<MachineFacts>,
    pub windows: WindowsFacts,
    pub security: SecurityFacts,
    pub network: NetworkFacts,
    pub runtime: Option<RuntimeFacts>,
    pub statuses: Vec<SourceStatus>,
}

fn status(id: &str, state: SourceHealth, reason: Option<&str>) -> SourceStatus {
    let label = SOURCES
        .iter()
        .find(|(source, _)| *source == id)
        .map(|(_, label)| *label)
        .unwrap_or(id);
    SourceStatus {
        id: id.into(),
        label: label.into(),
        state,
        reason: reason.map(String::from),
    }
}

pub fn is_stale(checked_at: Millis, ttl_ms: Millis, now: Millis) -> bool {
    ttl_ms > 0 && now >= checked_at && now - checked_at > ttl_ms * STALE_TTL_FACTOR
}

/// Resultado de olhar uma seção dos collectors: avaliada, velha ou sem dado.
fn gate(
    id: &str,
    statuses: &mut Vec<SourceStatus>,
    now: Millis,
    checked_at: Millis,
    ttl_ms: Millis,
    usable: bool,
    unusable_reason: &str,
) -> bool {
    if is_stale(checked_at, ttl_ms, now) {
        statuses.push(status(
            id,
            SourceHealth::Stale,
            Some(
                "A leitura passou do dobro da validade; nenhum alerta novo é gerado a partir dela.",
            ),
        ));
        false
    } else if !usable {
        statuses.push(status(id, SourceHealth::Unavailable, Some(unusable_reason)));
        false
    } else {
        statuses.push(status(id, SourceHealth::Evaluated, None));
        true
    }
}

pub fn machine_facts(
    telemetry: &Telemetry,
    now: Millis,
    statuses: &mut Vec<SourceStatus>,
) -> Option<MachineFacts> {
    let age = now - telemetry.timestamp;
    if age > TELEMETRY_MAX_AGE_MS {
        for id in [SRC_DISK, SRC_CPU, SRC_MEMORY, SRC_THERMAL] {
            statuses.push(status(
                id,
                SourceHealth::Stale,
                Some("A telemetria está desatualizada; nenhum alerta novo é gerado a partir dela."),
            ));
        }
        return None;
    }
    let volumes = telemetry
        .volumes
        .iter()
        .filter(|v| !v.removable)
        .map(|v| VolumeFact {
            mount: v.mount.clone(),
            total: v.total,
            available: v.available,
        })
        .collect();
    let commit_percent = match (telemetry.memory.commit_used, telemetry.memory.commit_limit) {
        (Some(used), Some(limit)) if limit > 0 => Some(used as f32 * 100.0 / limit as f32),
        _ => None,
    };
    for id in [SRC_DISK, SRC_CPU, SRC_MEMORY, SRC_THERMAL] {
        statuses.push(status(id, SourceHealth::Evaluated, None));
    }
    Some(MachineFacts {
        volumes: Some(volumes),
        load_ready: telemetry.cpu.ready,
        cpu_percent: telemetry.cpu.usage,
        memory_percent: telemetry.memory.percent,
        memory_available: telemetry.memory.available,
        commit_percent,
        alerts: telemetry
            .health
            .alerts
            .iter()
            .map(|a| MachineAlertFact {
                source: a.source.clone(),
                severity: a.severity,
                title: a.title.clone(),
                detail: a.detail.clone(),
                resource: a.resource.clone(),
            })
            .collect(),
    })
}

pub fn windows_facts(
    snapshot: &WindowsHealthSnapshot,
    now: Millis,
    statuses: &mut Vec<SourceStatus>,
) -> WindowsFacts {
    let mut facts = WindowsFacts::default();
    let s = snapshot;
    if gate(
        SRC_WIN_RESTART,
        statuses,
        now,
        s.restart.checked_at,
        s.restart.ttl_ms,
        s.restart.data.pending.is_some(),
        "Nenhuma fonte de reinício pendente respondeu.",
    ) {
        facts.restart_pending = s.restart.data.pending;
    }
    if gate(
        SRC_WIN_SERVICES,
        statuses,
        now,
        s.services.checked_at,
        s.services.ttl_ms,
        !s.services.data.items.is_empty(),
        "Os serviços não puderam ser consultados.",
    ) {
        facts.services = Some(
            s.services
                .data
                .items
                .iter()
                .map(|i| ServiceFact {
                    id: i.id.clone(),
                    label: i.label.clone(),
                    state: i.state,
                    start: i.start,
                    health: i.health,
                    reason: i.reason.clone(),
                })
                .collect(),
        );
    }
    if gate(
        SRC_WIN_DEVICES,
        statuses,
        now,
        s.devices.checked_at,
        s.devices.ttl_ms,
        s.devices.status != Health::Unknown,
        "Os dispositivos não puderam ser enumerados.",
    ) {
        facts.devices = Some(
            s.devices
                .data
                .issues
                .iter()
                .map(|d| DeviceFact {
                    name: d.name.clone(),
                    class: d.class.clone(),
                    manufacturer: d.manufacturer.clone(),
                    problem_code: d.problem_code,
                    problem: d.problem.clone(),
                })
                .collect(),
        );
    }
    if gate(
        SRC_WIN_EVENTS,
        statuses,
        now,
        s.events.checked_at,
        s.events.ttl_ms,
        s.events.status != Health::Unknown,
        "O Event Log não pôde ser consultado.",
    ) {
        facts.events = Some(
            s.events
                .data
                .signals
                .iter()
                .map(|e| EventFact {
                    kind: e.kind,
                    label: e.label.clone(),
                    count_24h: e.count_24h,
                    count_7d: e.count_7d,
                })
                .collect(),
        );
    }
    if gate(
        SRC_WIN_UPDATES,
        statuses,
        now,
        s.updates.checked_at,
        s.updates.ttl_ms,
        s.updates.data.failures_7d.is_some(),
        "As falhas de atualização não puderam ser contadas.",
    ) {
        facts.update_failures_7d = s.updates.data.failures_7d;
    }
    if gate(
        SRC_WIN_VOLUMES,
        statuses,
        now,
        s.volumes.checked_at,
        s.volumes.ttl_ms,
        !s.volumes.data.items.is_empty(),
        "A integridade dos volumes não pôde ser consultada.",
    ) {
        facts.volumes = Some(
            s.volumes
                .data
                .items
                .iter()
                .map(|v| WinVolumeFact {
                    mount: v.mount.clone(),
                    dirty: v.dirty,
                    read_only: v.read_only,
                    status: v.status,
                    reasons: v.reasons.clone(),
                })
                .collect(),
        );
    }
    facts
}

pub fn network_facts(
    snapshot: &NetworkSecuritySnapshot,
    now: Millis,
    statuses: &mut Vec<SourceStatus>,
) -> (SecurityFacts, NetworkFacts) {
    let mut security = SecurityFacts::default();
    let mut network = NetworkFacts::default();
    let fw = &snapshot.firewall;
    if gate(
        SRC_FIREWALL,
        statuses,
        now,
        fw.checked_at,
        fw.ttl_ms,
        fw.status != Health::Unknown,
        "O estado do firewall não pôde ser lido.",
    ) {
        let active = fw.data.active_profile;
        security.firewall = Some(FirewallFact {
            active_profile: active,
            active_enabled: active.and_then(|kind| {
                fw.data
                    .profiles
                    .iter()
                    .find(|p| p.kind == kind)
                    .and_then(|p| p.enabled)
            }),
            security_center: fw.data.security_center,
        });
    }
    let av = &snapshot.antivirus;
    if gate(
        SRC_ANTIVIRUS,
        statuses,
        now,
        av.checked_at,
        av.ttl_ms,
        av.status != Health::Unknown,
        "O antivírus ativo não pôde ser determinado.",
    ) {
        security.antivirus = Some(AntivirusFact {
            provider: av.data.provider,
            defender: av.data.defender.state,
            security_center: av.data.security_center,
            active_threats: av.data.defender.active_threats,
            signature_age_days: av.data.defender.signature_age_days,
            third_party: av.data.third_party_count,
        });
    }
    let ex = &snapshot.exposure;
    let listeners_ok = ex
        .sources
        .iter()
        .any(|n| n.id == "listeners" && n.state == SourceState::Available);
    if gate(
        SRC_EXPOSURE,
        statuses,
        now,
        ex.checked_at,
        ex.ttl_ms,
        listeners_ok,
        "As portas em escuta não puderam ser enumeradas.",
    ) {
        network.all_interface_listeners = Some(
            ex.data
                .listeners
                .iter()
                .filter(|l| l.scope == ListenerScope::AllInterfaces)
                .map(|l| ListenerFact {
                    port: l.port,
                    pid: l.pid,
                    process_name: l.process_name.clone(),
                    project_name: l.project_name.clone(),
                    system: l.system,
                })
                .collect(),
        );
    }
    (security, network)
}

pub fn runtime_facts(
    runs: &[RunInfo],
    ctx: &Context,
    ports: Option<&[PortObservation]>,
    processes: &[RawProcess],
    managed: &HashMap<u32, (String, String)>,
    self_pid: u32,
    statuses: &mut Vec<SourceStatus>,
) -> RuntimeFacts {
    let ports_available = ports.is_some();
    let ports = ports.unwrap_or(&[]);
    statuses.push(status(SRC_RUNS, SourceHealth::Evaluated, None));
    statuses.push(if ports_available {
        status(SRC_PORTS, SourceHealth::Evaluated, None)
    } else {
        status(
            SRC_PORTS,
            SourceHealth::Unavailable,
            Some("As portas em escuta não puderam ser lidas."),
        )
    });
    let names: HashMap<u32, &str> = processes.iter().map(|p| (p.pid, p.name.as_str())).collect();
    RuntimeFacts {
        ports_available,
        runs: runs
            .iter()
            .filter(|r| !r.observer)
            .map(|r| RunFact {
                run_id: r.id.clone(),
                project_id: r.project_id.clone(),
                command_id: r.command_id.clone(),
                command: r.command.clone(),
                state: r.state,
                exit_code: r.exit_code,
                started_at: r.started_at as Millis,
                ended_at: r.ended_at.map(|t| t as Millis),
            })
            .collect(),
        declared_ports: ctx
            .projects
            .iter()
            .flat_map(|p| {
                p.ports.iter().map(|port| DeclaredPort {
                    project_id: p.id.clone(),
                    port: *port,
                })
            })
            .collect(),
        owners: ports
            .iter()
            .filter_map(|p| {
                p.pid.map(|pid| PortOwner {
                    port: p.port,
                    pid,
                    process_name: names.get(&pid).map(|n| n.to_string()),
                    managed_project_id: managed.get(&pid).map(|(project, _)| project.clone()),
                })
            })
            .collect(),
        project_names: ctx
            .projects
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect(),
        self_pid,
    }
}

/// Marca fontes como sem dado (ex.: telemetria ainda sem primeira amostra).
pub fn mark_unavailable(statuses: &mut Vec<SourceStatus>, ids: &[&str], reason: &str) {
    for id in ids {
        statuses.push(status(id, SourceHealth::Unavailable, Some(reason)));
    }
}

// ------------------------------------------------------------------ avaliação

#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    pub findings: Vec<Finding>,
    /// Fontes avaliadas agora: só elas podem resolver alertas.
    pub evaluated: BTreeSet<String>,
    pub statuses: Vec<SourceStatus>,
}

/// Severidade que cada fingerprint tinha na avaliação anterior (histerese). Vazio na primeira.
pub type Previous = HashMap<String, Severity>;

pub fn evaluate(now: Millis, facts: &Facts, previous: &Previous) -> Evaluation {
    let mut findings = Vec::new();
    let mut evaluated = BTreeSet::new();
    if let Some(machine) = &facts.machine {
        machine_rules(machine, previous, &mut findings, &mut evaluated);
    }
    windows_rules(&facts.windows, &mut findings, &mut evaluated);
    security_rules(&facts.security, &mut findings, &mut evaluated);
    network_rules(&facts.network, &mut findings, &mut evaluated);
    if let Some(runtime) = &facts.runtime {
        runtime_rules(now, runtime, &mut findings, &mut evaluated);
    }
    // Um fingerprint nunca aparece duas vezes; fica o mais grave.
    findings.sort_by(|a, b| {
        a.fingerprint
            .cmp(&b.fingerprint)
            .then(b.severity.cmp(&a.severity))
    });
    findings.dedup_by(|b, a| a.fingerprint == b.fingerprint);
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(a.fingerprint.cmp(&b.fingerprint))
    });
    Evaluation {
        findings,
        evaluated,
        statuses: facts.statuses.clone(),
    }
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

fn machine_rules(
    m: &MachineFacts,
    previous: &Previous,
    out: &mut Vec<Finding>,
    evaluated: &mut BTreeSet<String>,
) {
    if let Some(volumes) = &m.volumes {
        evaluated.insert(SRC_DISK.into());
        for v in volumes {
            let fp = fingerprint("machine.disk.low_space", &v.mount);
            let before = previous.get(&fp).map(|s| match s {
                Severity::Critical => HealthStatus::Critical,
                Severity::Attention => HealthStatus::Attention,
                Severity::Info => HealthStatus::Healthy,
            });
            let Some(level) = health::space_severity(v.total, v.available, before) else {
                continue;
            };
            let free = health::free_percent(v.total, v.available);
            let (severity, limits) = match level {
                HealthStatus::Critical => (
                    Severity::Critical,
                    format!(
                        "menos de {:.0}% e menos de {} livres",
                        health::FREE_SPACE_CRITICAL,
                        gib(health::FREE_SPACE_CRITICAL_BYTES)
                    ),
                ),
                _ => (
                    Severity::Attention,
                    format!(
                        "menos de {:.0}% e menos de {} livres",
                        health::FREE_SPACE_ATTENTION,
                        gib(health::FREE_SPACE_ATTENTION_BYTES)
                    ),
                ),
            };
            let rule = Rule {
                rule_id: "machine.disk.low_space",
                resource: &v.mount,
                domain: Domain::Machine,
                source: SRC_DISK,
                severity,
                confidence: Confidence::High,
            };
            out.push(
                rule.finding(
                    format!("Pouco espaço livre em {}", v.mount),
                    format!("O volume {} tem {free:.1}% livre ({}).", v.mount, gib(v.available)),
                    format!("Espaço livre abaixo do limite ({limits}). Os dois limites precisam ser cruzados: só a porcentagem ou só o valor absoluto não geram alerta."),
                    format!("Libere espaço no volume {} antes que o sistema fique sem capacidade.", v.mount),
                )
                .ev("Volume", v.mount.clone())
                .ev("Livre", format!("{free:.1}% ({})", gib(v.available)))
                .ev("Capacidade", gib(v.total))
                .cta("machine", None),
            );
        }
    }
    if !m.load_ready {
        return;
    }
    evaluated.insert(SRC_CPU.into());
    evaluated.insert(SRC_MEMORY.into());
    evaluated.insert(SRC_THERMAL.into());
    for a in &m.alerts {
        match a.source.as_str() {
            "cpu" => {
                let critical = a.severity == HealthStatus::Critical;
                let rule = Rule {
                    rule_id: "machine.cpu.sustained_pressure",
                    resource: "cpu",
                    domain: Domain::Machine,
                    source: SRC_CPU,
                    severity: a.severity.into(),
                    confidence: if critical {
                        Confidence::High
                    } else {
                        Confidence::Medium
                    },
                };
                out.push(
                    rule.finding(
                        a.title.clone(),
                        a.detail.clone(),
                        "A CPU ficou acima do limite durante toda a janela de amostras (um pico isolado não gera alerta).",
                        "Veja em Processos qual aplicação consome a CPU.",
                    )
                    .ev("Uso atual da CPU", format!("{:.0}%", m.cpu_percent))
                    .ev("Condição", a.detail.clone())
                    .cta("machine", None),
                );
            }
            "memory" => {
                // RAM em uso inclui cache: crítico só com o commit (RAM + pagefile) também no limite.
                let commit_high = m.commit_percent.is_some_and(|c| c >= 90.0);
                let (severity, confidence) = match (a.severity, commit_high) {
                    (HealthStatus::Critical, true) => (Severity::Critical, Confidence::High),
                    _ => (Severity::Attention, Confidence::Medium),
                };
                let rule = Rule {
                    rule_id: "machine.memory.sustained_pressure",
                    resource: "memory",
                    domain: Domain::Machine,
                    source: SRC_MEMORY,
                    severity,
                    confidence,
                };
                let mut finding = rule
                    .finding(
                        a.title.clone(),
                        a.detail.clone(),
                        "A RAM ficou acima do limite durante toda a janela de amostras. Uso alto de RAM, sozinho, não é defeito: o Windows usa memória livre como cache.",
                        "Veja em Processos quais aplicações usam mais memória.",
                    )
                    .ev("RAM em uso", format!("{:.0}%", m.memory_percent))
                    .ev("RAM disponível", gib(m.memory_available));
                if let Some(commit) = m.commit_percent {
                    finding = finding.ev("Commit (RAM + pagefile)", format!("{commit:.0}%"));
                }
                out.push(finding.cta("machine", None));
            }
            "temperature" => {
                let label = a.resource.clone().unwrap_or_else(|| a.title.clone());
                let rule = Rule {
                    rule_id: "machine.thermal.over_limit",
                    resource: &label,
                    domain: Domain::Machine,
                    source: SRC_THERMAL,
                    severity: a.severity.into(),
                    confidence: Confidence::High,
                };
                out.push(
                    rule.finding(
                        a.title.clone(),
                        a.detail.clone(),
                        "A leitura ficou acima do limite que o próprio dispositivo declara (ou o limite conhecido da GPU), em toda a janela.",
                        "Verifique a ventilação e a carga de trabalho do componente.",
                    )
                    .ev("Sensor", label.clone())
                    .ev("Leitura", a.detail.clone())
                    .cta("machine", None),
                );
            }
            _ => {}
        }
    }
}

fn windows_rules(w: &WindowsFacts, out: &mut Vec<Finding>, evaluated: &mut BTreeSet<String>) {
    if let Some(pending) = w.restart_pending {
        evaluated.insert(SRC_WIN_RESTART.into());
        if pending {
            let rule = Rule {
                rule_id: "windows.reboot.pending",
                resource: "system",
                domain: Domain::Windows,
                source: SRC_WIN_RESTART,
                severity: Severity::Attention,
                confidence: Confidence::High,
            };
            out.push(
                rule.finding(
                    "Reinício do Windows pendente",
                    "O Windows indica que um reinício é necessário para concluir uma instalação ou atualização.",
                    "Uma fonte forte (Component Based Servicing ou Windows Update) informa reinício pendente.",
                    "Salve o trabalho e reinicie quando for conveniente. Nada é reiniciado automaticamente.",
                )
                .ev("Reinício pendente", "Sim")
                .cta("windows_health", None),
            );
        }
    }
    if let Some(services) = &w.services {
        evaluated.insert(SRC_WIN_SERVICES.into());
        for s in services.iter().filter(|s| s.health >= Health::Attention) {
            let severity = if s.health == Health::Critical {
                Severity::Critical
            } else {
                Severity::Attention
            };
            let rule = Rule {
                rule_id: "windows.service.not_running",
                resource: &s.id,
                domain: Domain::Windows,
                source: SRC_WIN_SERVICES,
                severity,
                confidence: Confidence::High,
            };
            out.push(
                rule.finding(
                    format!("Serviço essencial com problema: {}", s.label),
                    s.reason.clone().unwrap_or_else(|| format!("O serviço {} não está como esperado.", s.label)),
                    "O serviço deveria estar em execução (início automático) e não está, ou está desabilitado.",
                    "Abra o Windows Health para ver o estado dos serviços essenciais.",
                )
                .ev("Serviço", s.id.clone())
                .ev("Estado", format!("{:?}", s.state).to_lowercase())
                .ev("Início", format!("{:?}", s.start).to_lowercase())
                .cta("windows_health", None),
            );
        }
    }
    if let Some(devices) = &w.devices {
        evaluated.insert(SRC_WIN_DEVICES.into());
        for d in devices {
            let resource = format!("{}#{}", d.name, d.problem_code);
            let rule = Rule {
                rule_id: "windows.device.problem",
                resource: &resource,
                domain: Domain::Windows,
                source: SRC_WIN_DEVICES,
                severity: Severity::Attention,
                confidence: Confidence::High,
            };
            let mut finding = rule
                .finding(
                    format!("Dispositivo com problema: {}", d.name),
                    d.problem.clone(),
                    "O Gerenciador de Dispositivos reporta um código de problema para o dispositivo (dispositivos desabilitados de propósito não contam).",
                    "Abra o Windows Health para ver o dispositivo e o código reportado.",
                )
                .ev("Dispositivo", d.name.clone())
                .ev("Código", d.problem_code.to_string());
            if let Some(class) = &d.class {
                finding = finding.ev("Classe", class.clone());
            }
            out.push(finding.cta("windows_health", None));
        }
    }
    if let Some(events) = &w.events {
        evaluated.insert(SRC_WIN_EVENTS.into());
        for e in events {
            event_rule(e, out);
        }
    }
    if let Some(failures) = w.update_failures_7d {
        evaluated.insert(SRC_WIN_UPDATES.into());
        if failures >= UPDATE_REPEATED_FAILURES {
            let rule = Rule {
                rule_id: "windows.update.repeated_failure",
                resource: "windows-update",
                domain: Domain::Windows,
                source: SRC_WIN_UPDATES,
                severity: Severity::Attention,
                confidence: Confidence::High,
            };
            out.push(
                rule.finding(
                    "Falhas repetidas do Windows Update",
                    format!("{failures} falhas de instalação ou download nos últimos 7 dias."),
                    format!("O log do cliente de atualização registra {UPDATE_REPEATED_FAILURES} ou mais falhas em 7 dias (uma falha isolada não gera alerta)."),
                    "Abra o Windows Health; um diagnóstico do component store pode trazer mais evidência.",
                )
                .ev("Falhas (7 dias)", failures.to_string())
                .diagnostic(DiagnosticId::DismCheckHealth, None)
                .cta("windows_health", None),
            );
        }
    }
    if let Some(volumes) = &w.volumes {
        evaluated.insert(SRC_WIN_VOLUMES.into());
        for v in volumes.iter().filter(|v| v.status >= Health::Attention) {
            let rule = Rule {
                rule_id: "windows.volume.problem",
                resource: &v.mount,
                domain: Domain::Windows,
                source: SRC_WIN_VOLUMES,
                severity: Severity::Attention,
                confidence: Confidence::High,
            };
            let mut finding = rule
                .finding(
                    format!("Problema confirmado no volume {}", v.mount),
                    v.reasons.first().cloned().unwrap_or_else(|| "O volume reporta uma condição que merece atenção.".into()),
                    "O Windows marca o volume como sujo, somente leitura ou com verificação de disco agendada.",
                    format!("Um diagnóstico somente leitura do volume {} pode trazer mais evidência.", v.mount),
                )
                .ev("Volume", v.mount.clone());
            if let Some(dirty) = v.dirty {
                finding = finding.ev("Marcado como sujo", if dirty { "Sim" } else { "Não" });
            }
            if let Some(read_only) = v.read_only {
                finding = finding.ev("Somente leitura", if read_only { "Sim" } else { "Não" });
            }
            out.push(
                finding
                    .diagnostic(DiagnosticId::ChkdskScan, Some(&v.mount))
                    .cta("windows_health", None),
            );
        }
    }
}

fn event_rule(e: &EventFact, out: &mut Vec<Finding>) {
    let recent = e.count_24h > 0;
    let seen = e.count_7d > 0 || recent;
    let counts = format!("{} em 24 h · {} em 7 dias", e.count_24h, e.count_7d);
    match e.kind {
        SignalKind::Bugcheck if seen => {
            let severity = if recent {
                Severity::Critical
            } else {
                Severity::Attention
            };
            let rule = Rule {
                rule_id: "windows.event.bugcheck",
                resource: "system",
                domain: Domain::Windows,
                source: SRC_WIN_EVENTS,
                severity,
                confidence: Confidence::High,
            };
            out.push(
                rule.finding(
                    "Tela azul (bugcheck) registrada",
                    if recent {
                        "O Windows registrou uma tela azul nas últimas 24 horas."
                    } else {
                        "O Windows registrou uma tela azul nos últimos 7 dias."
                    },
                    "O Event Log tem um evento de bugcheck (BugCheck 1001) na janela.",
                    "Abra o Windows Health. A causa não é diagnosticada automaticamente.",
                )
                .ev("Bugchecks", counts)
                .diagnostic(DiagnosticId::SfcVerifyOnly, None)
                .cta("windows_health", None),
            );
        }
        SignalKind::UnexpectedShutdown if seen => {
            let severity = if recent {
                Severity::Attention
            } else {
                Severity::Info
            };
            let rule = Rule {
                rule_id: "windows.event.unexpected_shutdown",
                resource: "system",
                domain: Domain::Windows,
                source: SRC_WIN_EVENTS,
                severity,
                confidence: Confidence::High,
            };
            out.push(
                rule.finding(
                    "Desligamento inesperado registrado",
                    "O Windows registrou um desligamento que não foi limpo.",
                    "O Event Log tem Kernel-Power 41 ou EventLog 6008 (contados uma vez por desligamento).",
                    "Verifique alimentação e temperatura se voltar a ocorrer. A causa não é diagnosticada automaticamente.",
                )
                .ev("Desligamentos inesperados", counts)
                .cta("windows_health", None),
            );
        }
        SignalKind::StorageError if seen => {
            let severity = if recent {
                Severity::Attention
            } else {
                Severity::Info
            };
            let rule = Rule {
                rule_id: "windows.event.storage_error",
                resource: "system",
                domain: Domain::Windows,
                source: SRC_WIN_EVENTS,
                severity,
                confidence: Confidence::Medium,
            };
            out.push(
                rule.finding(
                    "Erro de dispositivo de armazenamento registrado",
                    "O Event Log registrou erro de E/S em um dispositivo de armazenamento.",
                    "Há eventos de erro de disco ou de controlador de armazenamento na janela.",
                    "Abra o Windows Health. Um diagnóstico somente leitura de volume pode trazer mais evidência.",
                )
                .ev("Erros de armazenamento", counts)
                .cta("windows_health", None),
            );
        }
        SignalKind::FilesystemError if seen => {
            let severity = if recent {
                Severity::Attention
            } else {
                Severity::Info
            };
            let rule = Rule {
                rule_id: "windows.event.filesystem_error",
                resource: "system",
                domain: Domain::Windows,
                source: SRC_WIN_EVENTS,
                severity,
                confidence: Confidence::Medium,
            };
            out.push(
                rule.finding(
                    "Erro de sistema de arquivos registrado",
                    "O Event Log registrou erro de NTFS.",
                    "Há eventos de erro do sistema de arquivos na janela.",
                    "Abra o Windows Health. Um diagnóstico somente leitura do volume pode trazer mais evidência.",
                )
                .ev("Erros de NTFS", counts)
                .cta("windows_health", None),
            );
        }
        SignalKind::ServiceFailure if e.count_24h >= SERVICE_FAILURE_ATTENTION => {
            let rule = Rule {
                rule_id: "windows.event.service_failures",
                resource: "system",
                domain: Domain::Windows,
                source: SRC_WIN_EVENTS,
                severity: Severity::Attention,
                confidence: Confidence::Medium,
            };
            out.push(
                rule.finding(
                    "Falhas repetidas de serviços do Windows",
                    format!("{} falhas de serviço nas últimas 24 horas.", e.count_24h),
                    format!("{SERVICE_FAILURE_ATTENTION} ou mais falhas de serviço em 24 h (falhas isoladas não geram alerta)."),
                    "Abra o Windows Health para ver o estado dos serviços essenciais.",
                )
                .ev("Falhas de serviço", counts)
                .cta("windows_health", None),
            );
        }
        _ => {}
    }
}

fn security_rules(s: &SecurityFacts, out: &mut Vec<Finding>, evaluated: &mut BTreeSet<String>) {
    if let Some(fw) = &s.firewall {
        evaluated.insert(SRC_FIREWALL.into());
        if let (Some(profile), Some(false)) = (fw.active_profile, fw.active_enabled) {
            // Outro firewall saudável informado pelo Security Center não é problema.
            if fw.security_center != Some(WscHealth::Good) {
                let rule = Rule {
                    rule_id: "security.firewall.active_profile_disabled",
                    resource: profile.label(),
                    domain: Domain::Security,
                    source: SRC_FIREWALL,
                    severity: Severity::Attention,
                    confidence: Confidence::High,
                };
                out.push(
                    rule.finding(
                        format!("Firewall do Windows desativado no perfil {}", profile.label()),
                        "O firewall está desativado no perfil da rede ativa e nenhum outro firewall foi informado pelo Security Center.",
                        "O perfil da rede ativa está desativado e o Security Center não informa outro firewall saudável.",
                        "Abra o Network & Security para ver os perfis. Nada é ativado automaticamente.",
                    )
                    .ev("Perfil ativo", profile.label())
                    .ev("Firewall do perfil", "Desativado")
                    .ev("Security Center (firewall)", wsc_text(fw.security_center))
                    .cta("network_security", None),
                );
            }
        }
    }
    if let Some(av) = &s.antivirus {
        evaluated.insert(SRC_ANTIVIRUS.into());
        if av.provider == AvProvider::None && av.security_center != Some(WscHealth::Good) {
            let rule = Rule {
                rule_id: "security.no_active_antivirus",
                resource: "antivirus",
                domain: Domain::Security,
                source: SRC_ANTIVIRUS,
                severity: Severity::Critical,
                confidence: Confidence::High,
            };
            out.push(
                rule.finding(
                    "Nenhum antivírus ativo",
                    "O Microsoft Defender está desativado e não há antivírus de terceiros registrado.",
                    "Defender desativado, nenhum provedor de terceiros registrado e o Security Center não informa antivírus saudável.",
                    "Abra o Network & Security. Nada é ativado automaticamente.",
                )
                .ev("Microsoft Defender", "Desativado")
                .ev("Antivírus de terceiros", av.third_party.map(|n| n.to_string()).unwrap_or_else(|| "desconhecido".into()))
                .ev("Security Center (antivírus)", wsc_text(av.security_center))
                .cta("network_security", None),
            );
        }
        if let Some(threats) = av.active_threats.filter(|n| *n > 0) {
            let rule = Rule {
                rule_id: "security.threat.active",
                resource: "defender",
                domain: Domain::Security,
                source: SRC_ANTIVIRUS,
                severity: Severity::Attention,
                confidence: Confidence::High,
            };
            out.push(
                rule.finding(
                    "Ameaça ativa reportada pelo Defender",
                    format!("O Microsoft Defender informa {threats} ameaça(s) ativa(s)."),
                    "O Defender reportou ameaças ativas.",
                    "Abra a Segurança do Windows para ver e tratar as ameaças. O LKR LAB não remove ameaças.",
                )
                .ev("Ameaças ativas", threats.to_string())
                .cta("network_security", None),
            );
        }
        if av.provider == AvProvider::Defender {
            if let Some(age) = av.signature_age_days.filter(|d| *d > SIGNATURE_STALE_DAYS) {
                let rule = Rule {
                    rule_id: "security.defender.signatures_stale",
                    resource: "defender",
                    domain: Domain::Security,
                    source: SRC_ANTIVIRUS,
                    severity: Severity::Attention,
                    confidence: Confidence::Medium,
                };
                out.push(
                    rule.finding(
                        "Assinaturas do Defender desatualizadas",
                        format!("As assinaturas do Defender têm {age} dias."),
                        format!("O Defender é o antivírus ativo e as assinaturas têm mais de {SIGNATURE_STALE_DAYS} dias."),
                        "Verifique o Windows Update e a conexão; o LKR LAB não atualiza assinaturas.",
                    )
                    .ev("Idade das assinaturas", format!("{age} dias"))
                    .cta("network_security", None),
                );
            }
        }
    }
}

fn wsc_text(health: Option<WscHealth>) -> String {
    match health {
        Some(WscHealth::Good) => "bom",
        Some(WscHealth::Poor) => "em risco",
        Some(WscHealth::Snooze) => "adiado",
        Some(WscHealth::NotMonitored) => "não monitorado",
        None => "sem resposta",
    }
    .into()
}

fn network_rules(n: &NetworkFacts, out: &mut Vec<Finding>, evaluated: &mut BTreeSet<String>) {
    let Some(listeners) = &n.all_interface_listeners else {
        return;
    };
    evaluated.insert(SRC_EXPOSURE.into());
    // Um achado por processo (não por porta): portas de desenvolvimento sobem e descem.
    let mut by_process: std::collections::BTreeMap<String, Vec<&ListenerFact>> = Default::default();
    for l in listeners.iter().filter(|l| !l.system) {
        if let Some(name) = &l.process_name {
            by_process.entry(name.to_lowercase()).or_default().push(l);
        }
    }
    for (name, group) in by_process {
        let mut ports: Vec<u16> = group.iter().map(|l| l.port).collect();
        ports.sort_unstable();
        ports.dedup();
        let shown: Vec<String> = ports
            .iter()
            .take(LISTENER_PORTS_SHOWN)
            .map(u16::to_string)
            .collect();
        let extra = ports.len().saturating_sub(LISTENER_PORTS_SHOWN);
        let list = if extra > 0 {
            format!("{} (+{extra})", shown.join(", "))
        } else {
            shown.join(", ")
        };
        let project = group.iter().find_map(|l| l.project_name.clone());
        let rule = Rule {
            rule_id: "network.listener.all_interfaces",
            resource: &name,
            domain: Domain::Network,
            source: SRC_EXPOSURE,
            severity: Severity::Info,
            confidence: Confidence::High,
        };
        let mut finding = rule
            .finding(
                format!("{} escuta em todas as interfaces", group[0].process_name.clone().unwrap_or_else(|| name.clone())),
                "O processo aceita conexões em todas as interfaces de rede desta máquina (0.0.0.0 ou ::).",
                "É um fato, não um defeito: depende do firewall e do roteador, que o LKR LAB não testa. Isto não significa exposição à internet.",
                "Se o acesso pela rede local não for necessário, configure o serviço para escutar só em 127.0.0.1.",
            )
            .ev("Processo", group[0].process_name.clone().unwrap_or_default())
            .ev("Portas", list);
        if let Some(project) = project {
            finding = finding.ev("Project", project);
        }
        out.push(finding.cta("network_security", None));
    }
}

struct Collision<'a> {
    project_id: &'a str,
    port: u16,
    owner: &'a PortOwner,
    /// O Project tem uma execução que falhou recentemente.
    after_failure: bool,
}

fn run_ended(r: &RunFact) -> Millis {
    r.ended_at.unwrap_or(r.started_at)
}

/// Project declara a porta X, tem execução gerenciada ativa (ou que falhou há pouco) e um processo
/// FORA da árvore gerenciada dele já possui X.
fn collisions<'a>(now: Millis, rt: &'a RuntimeFacts) -> Vec<Collision<'a>> {
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for declared in &rt.declared_ports {
        let project_runs = || {
            rt.runs
                .iter()
                .filter(|r| r.project_id == declared.project_id)
        };
        let failed_recently = project_runs().any(|r| {
            r.state == RunState::Failed && now - run_ended(r) <= COLLISION_RECENT_FAILURE_MS
        });
        let active = project_runs().any(|r| r.state.is_active());
        if !failed_recently && !active {
            continue;
        }
        let Some(owner) = rt.owners.iter().find(|o| {
            o.port == declared.port
                && o.pid != rt.self_pid
                && o.managed_project_id.as_deref() != Some(declared.project_id.as_str())
        }) else {
            continue;
        };
        if seen.insert((declared.project_id.clone(), declared.port)) {
            found.push(Collision {
                project_id: &declared.project_id,
                port: declared.port,
                owner,
                after_failure: failed_recently,
            });
        }
    }
    found
}

fn project_name<'a>(rt: &'a RuntimeFacts, id: &'a str) -> &'a str {
    rt.project_names.get(id).map(String::as_str).unwrap_or(id)
}

fn runtime_rules(
    now: Millis,
    rt: &RuntimeFacts,
    out: &mut Vec<Finding>,
    evaluated: &mut BTreeSet<String>,
) {
    evaluated.insert(SRC_RUNS.into());
    let collisions = if rt.ports_available {
        evaluated.insert(SRC_PORTS.into());
        collisions(now, rt)
    } else {
        Vec::new()
    };

    // Última execução de cada (Project, ação).
    let mut latest: HashMap<(&str, &str), &RunFact> = HashMap::new();
    for r in &rt.runs {
        let slot = latest.entry((&r.project_id, &r.command_id)).or_insert(r);
        if r.started_at > slot.started_at {
            *slot = r;
        }
    }
    let mut keys: Vec<_> = latest.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        let run = latest[&key];
        let project = project_name(rt, &run.project_id);
        let resource = format!("{}:{}", run.project_id, run.command_id);
        if run.state == RunState::Failed && now - run_ended(run) <= FAILED_RUN_RELEVANCE_MS {
            let correlated = collisions.iter().find(|c| c.project_id == run.project_id);
            let rule = Rule {
                rule_id: "runtime.managed.failed",
                resource: &resource,
                domain: Domain::Runtime,
                source: SRC_RUNS,
                severity: Severity::Attention,
                confidence: Confidence::High,
            };
            let summary = match correlated {
                Some(c) => format!(
                    "A execução \"{}\" falhou e a porta declarada {} está ocupada por outro processo: a falha provavelmente foi causada por porta ocupada.",
                    run.command, c.port
                ),
                None => format!("A execução \"{}\" terminou com falha.", run.command),
            };
            let mut finding = rule
                .finding(
                    format!("Execução falhou: {}", run.command),
                    summary,
                    "O Runtime Supervisor registra a execução gerenciada como falha (saída com erro).",
                    match correlated {
                        Some(c) => format!("Libere a porta {} (ocupada por outro processo) ou mude a porta do Project e execute de novo.", c.port),
                        None => "Abra o Runtime do Project e veja o console da execução.".into(),
                    },
                )
                .ev("Project", project.to_string())
                .ev("Ação", run.command.clone())
                .ev("Código de saída", run.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "desconhecido".into()));
            if let Some(c) = correlated {
                finding = finding
                    .ev("Porta declarada ocupada", c.port.to_string())
                    .ev(
                        "Dono da porta",
                        format!(
                            "{} (PID {})",
                            c.owner
                                .process_name
                                .clone()
                                .unwrap_or_else(|| "processo".into()),
                            c.owner.pid
                        ),
                    );
            }
            out.push(finding.cta("runtime", Some(&run.project_id)));
        }
        let failures = rt
            .runs
            .iter()
            .filter(|r| {
                r.project_id == run.project_id
                    && r.command_id == run.command_id
                    && r.state == RunState::Failed
                    && now - run_ended(r) <= REPEATED_FAILURE_WINDOW_MS
            })
            .count();
        if failures >= REPEATED_FAILURE_COUNT {
            let rule = Rule {
                rule_id: "runtime.managed.repeated_failure",
                resource: &resource,
                domain: Domain::Runtime,
                source: SRC_RUNS,
                severity: Severity::Attention,
                confidence: Confidence::High,
            };
            out.push(
                rule.finding(
                    format!("Falhas repetidas: {}", run.command),
                    format!("{failures} execuções falharam em sequência nos últimos {} minutos.", REPEATED_FAILURE_WINDOW_MS / 60_000),
                    format!("{REPEATED_FAILURE_COUNT} ou mais execuções gerenciadas da mesma ação falharam na janela (reinício em loop)."),
                    "Abra o Runtime do Project e leia o console antes de executar de novo.",
                )
                .ev("Project", project.to_string())
                .ev("Ação", run.command.clone())
                .ev("Falhas na janela", failures.to_string())
                .cta("runtime", Some(&run.project_id)),
            );
        }
    }
    for c in &collisions {
        let project = project_name(rt, c.project_id);
        let resource = format!("{}:{}", c.project_id, c.port);
        let rule = Rule {
            rule_id: "runtime.port.collision",
            resource: &resource,
            domain: Domain::Runtime,
            source: SRC_PORTS,
            severity: Severity::Attention,
            confidence: if c.after_failure {
                Confidence::High
            } else {
                Confidence::Medium
            },
        };
        let owner_name = c
            .owner
            .process_name
            .clone()
            .unwrap_or_else(|| "processo".into());
        out.push(
            rule.finding(
                format!("Porta {} ocupada por outro processo", c.port),
                format!("O Project {project} declara a porta {}, mas {owner_name} (PID {}) já a possui.", c.port, c.owner.pid),
                "O Project tem execução ativa (ou que falhou há pouco) e a porta declarada pertence a um processo fora da árvore gerenciada dele.",
                "Libere a porta ou mude a porta do Project. O LKR LAB não encerra processos automaticamente.",
            )
            .ev("Project", project.to_string())
            .ev("Porta", c.port.to_string())
            .ev("Dono da porta", format!("{owner_name} (PID {})", c.owner.pid))
            .cta("runtime", Some(c.project_id)),
        );
    }
}

// ------------------------------------------------------------------ ciclo de vida

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertStatus {
    /// Condição presente e ainda não vista pelo usuário.
    Active,
    /// O usuário viu. NÃO significa resolvido: a condição continua sendo observada.
    Acknowledged,
    /// A condição deixou de existir (o histórico fica).
    Resolved,
}
impl AlertStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "active" => Some(Self::Active),
            "acknowledged" => Some(Self::Acknowledged),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }
    pub fn is_open(self) -> bool {
        self != Self::Resolved
    }
}

/// Uma ocorrência persistida de um Finding.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRecord {
    /// Identificador desta ocorrência (o do Finding é o fingerprint, que se repete entre ocorrências).
    #[serde(rename = "alertId")]
    pub id: String,
    #[serde(flatten)]
    pub finding: Finding,
    pub status: AlertStatus,
    pub first_seen: Millis,
    pub last_seen: Millis,
    pub acknowledged_at: Option<Millis>,
    pub resolved_at: Option<Millis>,
    /// Quantas vezes este fingerprint abriu (1 = primeira vez; reabrir cria a próxima ocorrência).
    pub occurrence_count: u32,
    /// Quantas avaliações viram a condição nesta ocorrência.
    pub observations: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Insert(AlertRecord),
    Update(AlertRecord),
}

/// Aplica uma avaliação ao histórico. `latest` = a ocorrência mais recente de cada fingerprint
/// (qualquer status). Função pura: o armazenamento só aplica o resultado.
pub fn reconcile(
    latest: &[AlertRecord],
    evaluation: &Evaluation,
    now: Millis,
    new_id: &mut dyn FnMut() -> String,
) -> Vec<Change> {
    let by_fp: HashMap<&str, &AlertRecord> = latest
        .iter()
        .map(|r| (r.finding.fingerprint.as_str(), r))
        .collect();
    let mut changes = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for finding in &evaluation.findings {
        seen.insert(finding.fingerprint.as_str());
        match by_fp.get(finding.fingerprint.as_str()) {
            Some(open) if open.status.is_open() => {
                let mut next = (*open).clone();
                // Piorou depois de reconhecido: volta a chamar a atenção.
                if finding.severity > open.finding.severity
                    && next.status == AlertStatus::Acknowledged
                {
                    next.status = AlertStatus::Active;
                    next.acknowledged_at = None;
                }
                next.finding = finding.clone();
                next.last_seen = now;
                next.observations += 1;
                changes.push(Change::Update(next));
            }
            previous => {
                changes.push(Change::Insert(AlertRecord {
                    id: new_id(),
                    finding: finding.clone(),
                    status: AlertStatus::Active,
                    first_seen: now,
                    last_seen: now,
                    acknowledged_at: None,
                    resolved_at: None,
                    occurrence_count: previous.map(|r| r.occurrence_count + 1).unwrap_or(1),
                    observations: 1,
                }));
            }
        }
    }
    for open in latest.iter().filter(|r| r.status.is_open()) {
        if seen.contains(open.finding.fingerprint.as_str()) {
            continue;
        }
        // Só uma fonte avaliada agora (fresca) pode afirmar que a condição acabou; fonte velha ou
        // indisponível preserva o alerta sem escalar nem resolver. O atraso evita abrir e fechar
        // alertas por uma leitura que pisca.
        if evaluation.evaluated.contains(&open.finding.source)
            && now - open.last_seen >= RESOLVE_DELAY_MS
        {
            let mut next = open.clone();
            next.status = AlertStatus::Resolved;
            next.resolved_at = Some(now);
            changes.push(Change::Update(next));
        }
    }
    changes
}

/// Reconhece um alerta aberto (só o ciclo de vida local; nada muda na máquina).
pub fn acknowledge(record: &AlertRecord, now: Millis) -> Option<AlertRecord> {
    (record.status == AlertStatus::Active).then(|| {
        let mut next = record.clone();
        next.status = AlertStatus::Acknowledged;
        next.acknowledged_at = Some(now);
        next
    })
}

// ------------------------------------------------------------------ visão agregada

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertSummary {
    /// Alertas abertos (ativos + reconhecidos) por severidade.
    pub critical: u32,
    pub attention: u32,
    pub info: u32,
    pub acknowledged: u32,
    pub resolved_recently: u32,
}

pub fn summarize(records: &[AlertRecord], now: Millis) -> AlertSummary {
    let mut summary = AlertSummary::default();
    for r in records {
        match r.status {
            AlertStatus::Resolved => {
                if r.resolved_at
                    .is_some_and(|at| now - at <= RESOLVED_RECENT_MS)
                {
                    summary.resolved_recently += 1;
                }
            }
            status => {
                match r.finding.severity {
                    Severity::Critical => summary.critical += 1,
                    Severity::Attention => summary.attention += 1,
                    Severity::Info => summary.info += 1,
                }
                if status == AlertStatus::Acknowledged {
                    summary.acknowledged += 1;
                }
            }
        }
    }
    summary
}

/// Ordem previsível: abertos antes de resolvidos; Critical > Attention > Info; dentro disso o mais
/// antigo primeiro (`first_seen`), e o fingerprint desempata.
pub fn sort_alerts(records: &mut [AlertRecord]) {
    records.sort_by(|a, b| {
        a.status
            .is_open()
            .cmp(&b.status.is_open())
            .reverse()
            .then(b.finding.severity.cmp(&a.finding.severity))
            .then(a.first_seen.cmp(&b.first_seen))
            .then(a.finding.fingerprint.cmp(&b.finding.fingerprint))
    });
}

/// Estado dos diagnósticos explícitos para a interface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsView {
    /// O app está elevado (só então os diagnósticos podem rodar; nunca se pede UAC).
    pub elevated: bool,
    pub catalog: Vec<crate::diagnostic_runner::DiagnosticInfo>,
    /// O diagnóstico em andamento ou o último que terminou nesta sessão.
    pub current: Option<crate::diagnostic_runner::RunRecord>,
    pub history: Vec<crate::diagnostic_runner::RunRecord>,
}

/// Tudo o que a tela "Alertas e Diagnósticos" precisa.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertsSnapshot {
    pub captured_at: Millis,
    pub summary: AlertSummary,
    pub alerts: Vec<AlertRecord>,
    pub sources: Vec<SourceStatus>,
    pub diagnostics: DiagnosticsView,
}

pub fn new_alert_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Severidade que cada alerta aberto tinha, para a histerese da próxima avaliação.
pub fn previous_of(latest: &[AlertRecord]) -> Previous {
    latest
        .iter()
        .filter(|r| r.status.is_open())
        .map(|r| (r.finding.fingerprint.clone(), r.finding.severity))
        .collect()
}
