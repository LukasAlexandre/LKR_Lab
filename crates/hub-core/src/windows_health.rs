//! Windows Health & Integrity (SESSION-002, Block 07): "o Windows está operacionalmente íntegro?".
//!
//! 100% passivo e somente leitura: nada é reparado, reiniciado, instalado, iniciado, parado ou
//! alterado. Fontes nativas em processo (registro, Service Control Manager, Configuration Manager,
//! Event Log API, volumes): sem PowerShell, sem WMI e sem subprocessos.
//!
//! Cada domínio é uma `Section` independente, com saúde (`Health`), motivos, fontes consultadas e
//! seu próprio instante de leitura:
//!
//! | Domínio | Atualização | Regra de atenção/crítico (objetiva) |
//! |---|---|---|
//! | Reinício pendente | 45 s | Component Based Servicing ou Windows Update exigem reinício |
//! | Serviços essenciais | 45 s | automático e parado/desabilitado; serviço sob demanda só se desabilitado |
//! | Eventos | 3 min | bugcheck, desligamento inesperado, erro de disco/NTFS, falhas de serviço repetidas |
//! | Windows Update | 10 min | falha de instalação recente ou serviço desabilitado |
//! | Dispositivos | 10 min | código de problema no Gerenciador de Dispositivos (desabilitado não conta) |
//! | Volumes | 60 s | volume sujo ou somente leitura |
//!
//! Ausência de informação é `Unknown`, nunca `Healthy`. Evento de nível Erro/Aviso sozinho não muda
//! o estado: só os sinais específicos acima. O estado geral é o pior entre os domínios avaliados.
//! Nada daqui é persistido nem entra no workspace portátil (é estado de MÁQUINA).
use serde::Serialize;
use std::sync::Mutex;

pub type Millis = i64;

pub const DAY_MS: Millis = 86_400_000;
pub const WEEK_MS: Millis = 7 * DAY_MS;

pub const TTL_SYSTEM_MS: Millis = 60_000;
pub const TTL_RESTART_MS: Millis = 45_000;
pub const TTL_SERVICES_MS: Millis = 45_000;
pub const TTL_EVENTS_MS: Millis = 3 * 60_000;
pub const TTL_UPDATES_MS: Millis = 10 * 60_000;
pub const TTL_DEVICES_MS: Millis = 10 * 60_000;
pub const TTL_VOLUMES_MS: Millis = 60_000;

/// Serviços depois do boot recente podem ainda estar subindo (início automático atrasado).
pub const BOOT_GRACE_SECS: u64 = 300;

// ------------------------------------------------------------------ contrato

/// Ordem = gravidade: `Unknown` só vence se NADA foi avaliado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Health {
    Unknown,
    Healthy,
    Attention,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Available,
    Partial,
    Unavailable,
    RequiresElevation,
}

/// Uma fonte de dados consultada e o que aconteceu com ela.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceNote {
    pub id: String,
    pub label: String,
    pub state: SourceState,
    pub reason: Option<String>,
}
impl SourceNote {
    pub fn ok(id: &str, label: &str) -> Self {
        Self::new(id, label, SourceState::Available, None)
    }
    pub fn new(id: &str, label: &str, state: SourceState, reason: Option<&str>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            state,
            reason: reason.map(String::from),
        }
    }
}

/// Falha de uma fonte: nunca vira "saudável" nem derruba as outras.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    Unavailable(String),
    RequiresElevation(String),
}
impl SourceError {
    fn state(&self) -> SourceState {
        match self {
            Self::Unavailable(_) => SourceState::Unavailable,
            Self::RequiresElevation(_) => SourceState::RequiresElevation,
        }
    }
    fn reason(&self) -> &str {
        match self {
            Self::Unavailable(reason) | Self::RequiresElevation(reason) => reason,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section<T> {
    pub status: Health,
    /// Informativo (sem regra objetiva): mostrado, mas fora do estado geral.
    pub rated: bool,
    /// Por que o estado é Atenção/Crítico (ou por que é Desconhecido).
    pub reasons: Vec<String>,
    pub checked_at: Millis,
    pub ttl_ms: Millis,
    pub sources: Vec<SourceNote>,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemData {
    pub product_name: Option<String>,
    pub edition: Option<String>,
    pub version: Option<String>,
    pub build: Option<String>,
    pub architecture: Option<String>,
    pub installed_at: Option<Millis>,
    /// Inicialização do Windows (segundos desde a época Unix) e uptime da MÁQUINA, não do app.
    pub boot_time: Option<u64>,
    pub uptime_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartData {
    /// `None` = não dá para afirmar (fonte indisponível e nenhuma presente).
    pub pending: Option<bool>,
    /// PendingFileRenameOperations: comum e muitas vezes benigno; informativo, nunca sozinho.
    pub file_rename_operations: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Running,
    Stopped,
    StartPending,
    StopPending,
    Paused,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartType {
    Automatic,
    AutomaticDelayed,
    Manual,
    Disabled,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Expectation {
    /// Deve estar rodando quando o início é automático.
    Running,
    /// Sob demanda/por gatilho: parado é normal.
    OnDemand,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceView {
    pub id: String,
    pub label: String,
    pub state: ServiceState,
    pub start: StartType,
    pub expectation: Expectation,
    pub health: Health,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicesData {
    pub items: Vec<ServiceView>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesData {
    pub service: Option<ServiceView>,
    pub last_install_success_at: Option<Millis>,
    pub last_scan_success_at: Option<Millis>,
    /// Falhas de instalação/download nos últimos 7 dias (Event Log do cliente de atualização).
    pub failures_7d: Option<u32>,
    /// Sempre `None`: contar atualizações pendentes exige consultar o serviço de atualização
    /// (busca pesada), o que este Block não faz.
    pub pending_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIssue {
    pub name: String,
    pub class: Option<String>,
    pub manufacturer: Option<String>,
    pub problem_code: u32,
    pub problem: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesData {
    pub issues: Vec<DeviceIssue>,
    /// Desabilitados de propósito (códigos 22/29): não são problema.
    pub disabled: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    UnexpectedShutdown,
    Bugcheck,
    StorageError,
    FilesystemError,
    ServiceFailure,
    UpdateFailure,
    ApplicationCrash,
    ApplicationHang,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelCounts {
    pub critical: u32,
    pub error: u32,
    /// `None` = janela em que avisos não são medidos (nunca vira "0 avisos").
    pub warning: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventSignal {
    pub kind: SignalKind,
    pub label: String,
    pub count_24h: u32,
    pub count_7d: u32,
    pub last_at: Option<Millis>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEntry {
    pub at: Millis,
    pub provider: String,
    pub id: u32,
    pub kind: SignalKind,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsData {
    pub last_24h: LevelCounts,
    pub last_7d: LevelCounts,
    /// Avisos são contados só por informação (e até um teto): nunca mudam o estado.
    pub warnings_capped: bool,
    pub signals: Vec<EventSignal>,
    pub recent: Vec<EventEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeHealth {
    pub mount: String,
    pub filesystem: Option<String>,
    pub read_only: Option<bool>,
    /// `None` = o Windows não deixou ler sem privilégio administrativo.
    pub dirty: Option<bool>,
    pub status: Health,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumesData {
    pub items: Vec<VolumeHealth>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReliabilityData {
    pub app_crashes_7d: Option<u32>,
    pub app_hangs_7d: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegritySignal {
    pub id: String,
    pub label: String,
    pub present: bool,
}

/// Verificação ativa que o Block 09 vai oferecer; aqui só o modelo (nada é executado).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnDemandCheck {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub requires_elevation: bool,
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityData {
    pub signals: Vec<IntegritySignal>,
    pub on_demand: Vec<OnDemandCheck>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverallReason {
    pub domain: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverallHealth {
    pub status: Health,
    pub reasons: Vec<OverallReason>,
    /// Domínios avaliáveis com resultado conhecido / total de domínios avaliáveis.
    pub evaluated: u32,
    pub rateable: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsHealthSnapshot {
    pub captured_at: Millis,
    pub overall: OverallHealth,
    pub system: Section<SystemData>,
    pub restart: Section<RestartData>,
    pub updates: Section<UpdatesData>,
    pub services: Section<ServicesData>,
    pub devices: Section<DevicesData>,
    pub events: Section<EventsData>,
    pub volumes: Section<VolumesData>,
    pub reliability: Section<ReliabilityData>,
    pub integrity: Section<IntegrityData>,
    /// Todas as fontes consultadas, para a interface dizer o que NÃO está disponível.
    pub capabilities: Vec<SourceNote>,
}

// ------------------------------------------------------------------ entradas brutas das fontes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Present,
    Absent,
    /// Sem permissão para olhar (nunca é lido como "ausente").
    Denied,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartProbes {
    pub cbs_reboot_pending: Probe,
    pub wu_reboot_required: Probe,
    pub pending_file_renames: Probe,
}

#[derive(Debug, Clone, Default)]
pub struct SystemRaw {
    pub product_name: Option<String>,
    pub edition: Option<String>,
    pub version: Option<String>,
    pub build: Option<String>,
    pub ubr: Option<u64>,
    pub architecture: Option<String>,
    pub installed_at: Option<Millis>,
    pub boot_time: Option<u64>,
    pub uptime_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawService {
    pub state: ServiceState,
    pub start: StartType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDevice {
    pub name: String,
    pub class: Option<String>,
    pub manufacturer: Option<String>,
    /// 0 = sem problema.
    pub problem_code: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceBatch {
    pub total: u32,
    pub problems: Vec<RawDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvent {
    pub provider: String,
    pub id: u32,
    /// 1 = crítico, 2 = erro, 3 = aviso (níveis do Event Log).
    pub level: u8,
    pub at: Millis,
}

#[derive(Debug, Clone, Default)]
pub struct EventBatch {
    /// Eventos Crítico/Erro do canal System nos últimos 7 dias (com teto).
    pub errors: Vec<RawEvent>,
    /// Eventos dos provedores/IDs específicos (qualquer nível) nos últimos 7 dias.
    pub signals: Vec<RawEvent>,
    pub warnings_24h: u32,
    pub warnings_capped: bool,
    /// `None` = o canal do cliente de atualização não pôde ser lido.
    pub update_failures: Option<Vec<RawEvent>>,
    /// `None` = o canal Application não pôde ser lido.
    pub app_events: Option<Vec<RawEvent>>,
    /// Instante do evento 19 mais recente do cliente de atualização ("instalação bem-sucedida").
    pub last_update_success: Option<Millis>,
    pub truncated: bool,
    pub notes: Vec<SourceNote>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawVolume {
    pub mount: String,
    pub filesystem: Option<String>,
    pub read_only: Option<bool>,
    pub dirty: Option<bool>,
    /// A leitura do bit de "sujo" exigiu privilégio que o app não tem.
    pub dirty_denied: bool,
    /// Verificação de disco agendada para a próxima inicialização (`None` = não foi possível ler).
    pub check_scheduled: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateTimes {
    pub last_install_success_at: Option<Millis>,
    pub last_scan_success_at: Option<Millis>,
    pub notes: Vec<SourceNote>,
}

// ------------------------------------------------------------------ regras puras

pub fn worst(items: impl IntoIterator<Item = Health>) -> Health {
    items.into_iter().max().unwrap_or(Health::Unknown)
}

/// Edição/versão/build. O Windows 11 ainda grava "Windows 10" em `ProductName`: a build ≥ 22000 decide.
pub fn system_data(raw: &SystemRaw) -> SystemData {
    let build_number = raw
        .build
        .as_deref()
        .and_then(|b| b.split('.').next()?.parse::<u32>().ok());
    let product_name = raw.product_name.clone().map(|name| match build_number {
        Some(build) if build >= 22_000 && name.contains("Windows 10") => {
            name.replace("Windows 10", "Windows 11")
        }
        _ => name,
    });
    SystemData {
        product_name,
        edition: raw.edition.clone(),
        version: raw.version.clone(),
        build: raw.build.as_ref().map(|b| match raw.ubr {
            Some(ubr) => format!("{b}.{ubr}"),
            None => b.clone(),
        }),
        architecture: raw.architecture.clone(),
        installed_at: raw.installed_at,
        boot_time: raw.boot_time,
        uptime_secs: raw.uptime_secs,
    }
}

/// Reinício pendente. Duas fontes FORTES (Component Based Servicing, Windows Update); a de
/// renomeações pendentes de arquivo é informativa. Fonte negada/indisponível nunca vira "sem reinício":
/// sem nenhuma fonte forte presente e com alguma não consultada, o resultado é desconhecido.
pub fn evaluate_restart(
    probes: &RestartProbes,
) -> (Health, Vec<String>, RestartData, Vec<SourceNote>) {
    let strong = [
        (
            "cbs",
            "Component Based Servicing",
            probes.cbs_reboot_pending,
            "Component Based Servicing exige reinício",
        ),
        (
            "windows_update",
            "Windows Update",
            probes.wu_reboot_required,
            "O Windows Update exige reinício",
        ),
    ];
    let note = |id: &str, label: &str, probe: Probe| match probe {
        Probe::Present | Probe::Absent => SourceNote::ok(id, label),
        Probe::Denied => SourceNote::new(
            id,
            label,
            SourceState::RequiresElevation,
            Some("Requer privilégio administrativo"),
        ),
        Probe::Unavailable => SourceNote::new(
            id,
            label,
            SourceState::Unavailable,
            Some("Não foi possível consultar esta fonte"),
        ),
    };
    let mut notes: Vec<SourceNote> = strong
        .iter()
        .map(|(id, label, probe, _)| note(id, label, *probe))
        .collect();
    notes.push(note(
        "file_renames",
        "Renomeações pendentes de arquivo",
        probes.pending_file_renames,
    ));
    let present: Vec<&str> = strong
        .iter()
        .filter(|(_, _, p, _)| *p == Probe::Present)
        .map(|(_, _, _, why)| *why)
        .collect();
    let all_absent = strong.iter().all(|(_, _, p, _)| *p == Probe::Absent);
    let data_base = RestartData {
        pending: None,
        file_rename_operations: match probes.pending_file_renames {
            Probe::Present => Some(true),
            Probe::Absent => Some(false),
            _ => None,
        },
    };
    if !present.is_empty() {
        let reasons = present
            .iter()
            .map(|why| format!("Reinicialização pendente: {why}."))
            .collect();
        return (
            Health::Attention,
            reasons,
            RestartData {
                pending: Some(true),
                ..data_base
            },
            notes,
        );
    }
    if all_absent {
        return (
            Health::Healthy,
            vec![],
            RestartData {
                pending: Some(false),
                ..data_base
            },
            notes,
        );
    }
    let unknown: Vec<&str> = strong
        .iter()
        .filter(|(_, _, p, _)| *p != Probe::Absent)
        .map(|(_, label, _, _)| *label)
        .collect();
    (
        Health::Unknown,
        vec![format!(
            "Não foi possível confirmar se há reinício pendente: fonte indisponível ({}).",
            unknown.join(", ")
        )],
        data_base,
        notes,
    )
}

/// Serviço essencial a observar.
pub struct ServiceSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub expectation: Expectation,
    /// Parado/desabilitado compromete o sistema (crítico) em vez de só pedir atenção.
    pub critical: bool,
}

/// Lista curta e objetiva: o que o Windows precisa para registrar eventos, agendar, atualizar e
/// administrar. Serviços de rede/segurança ficam para o Block 08.
pub const ESSENTIAL_SERVICES: [ServiceSpec; 7] = [
    ServiceSpec {
        id: "EventLog",
        label: "Log de Eventos do Windows",
        expectation: Expectation::Running,
        critical: true,
    },
    ServiceSpec {
        id: "RpcSs",
        label: "RPC (Chamada de Procedimento Remoto)",
        expectation: Expectation::Running,
        critical: true,
    },
    ServiceSpec {
        id: "Winmgmt",
        label: "Instrumentação de Gerenciamento (WMI)",
        expectation: Expectation::Running,
        critical: false,
    },
    ServiceSpec {
        id: "Schedule",
        label: "Agendador de Tarefas",
        expectation: Expectation::Running,
        critical: false,
    },
    ServiceSpec {
        id: "CryptSvc",
        label: "Serviços de Criptografia",
        expectation: Expectation::Running,
        critical: false,
    },
    ServiceSpec {
        id: "BITS",
        label: "Transferência Inteligente em Segundo Plano (BITS)",
        expectation: Expectation::OnDemand,
        critical: false,
    },
    ServiceSpec {
        id: "wuauserv",
        label: "Windows Update",
        expectation: Expectation::OnDemand,
        critical: false,
    },
];

/// Regra de um serviço: o que importa é o modo de início e o que se espera dele, não só "parado".
pub fn evaluate_service(
    spec: &ServiceSpec,
    raw: Option<&RawService>,
    uptime_secs: Option<u64>,
) -> ServiceView {
    let view = |state, start, health, reason: Option<String>| ServiceView {
        id: spec.id.into(),
        label: spec.label.into(),
        state,
        start,
        expectation: spec.expectation,
        health,
        reason,
    };
    let Some(raw) = raw else {
        return view(
            ServiceState::Unknown,
            StartType::Unknown,
            Health::Unknown,
            Some("O serviço não pôde ser consultado.".into()),
        );
    };
    let (state, start) = (raw.state, raw.start);
    if start == StartType::Disabled {
        let severity = if spec.critical {
            Health::Critical
        } else {
            Health::Attention
        };
        return view(
            state,
            start,
            severity,
            Some(format!("{} está desabilitado.", spec.label)),
        );
    }
    let automatic = matches!(start, StartType::Automatic | StartType::AutomaticDelayed);
    if spec.expectation == Expectation::Running && automatic && state == ServiceState::Stopped {
        // Início automático atrasado logo após o boot: ainda subindo, não é falha.
        let booting = start == StartType::AutomaticDelayed
            && uptime_secs.is_some_and(|u| u < BOOT_GRACE_SECS);
        if booting {
            return view(state, start, Health::Healthy, None);
        }
        let severity = if spec.critical {
            Health::Critical
        } else {
            Health::Attention
        };
        return view(
            state,
            start,
            severity,
            Some(format!(
                "{} está parado, mas é iniciado automaticamente.",
                spec.label
            )),
        );
    }
    if state == ServiceState::Unknown || start == StartType::Unknown {
        return view(
            state,
            start,
            Health::Unknown,
            Some("Estado do serviço indisponível.".into()),
        );
    }
    view(state, start, Health::Healthy, None)
}

pub fn evaluate_services(views: &[ServiceView]) -> (Health, Vec<String>) {
    let status = worst(views.iter().map(|v| v.health));
    let reasons = views
        .iter()
        .filter(|v| v.health >= Health::Attention)
        .filter_map(|v| v.reason.clone())
        .collect();
    (status, reasons)
}

/// Texto do código de problema do Gerenciador de Dispositivos (CM_PROB_*).
pub fn problem_text(code: u32) -> String {
    let text = match code {
        1 => "dispositivo não configurado",
        3 => "memória insuficiente",
        10 => "o dispositivo não pode iniciar",
        12 => "conflito de recursos",
        14 => "requer reinicialização",
        18 => "reinstale os drivers",
        19 => "configuração do registro inválida",
        21 => "o Windows está removendo o dispositivo",
        24 => "dispositivo ausente ou com defeito",
        28 => "drivers não instalados",
        31 => "o dispositivo não está funcionando corretamente",
        32 => "serviço do driver desabilitado",
        33 => "tradução de recursos falhou",
        34 => "não foi possível determinar as configurações",
        35 => "firmware não informa recursos suficientes",
        37 => "o driver retornou falha na inicialização",
        38 => "não foi possível carregar o driver",
        39 => "driver danificado ou ausente",
        40 => "chave de serviço do driver ausente",
        41 => "driver carregado, mas o dispositivo não foi encontrado",
        42 => "dispositivo duplicado",
        43 => "o Windows parou o dispositivo por ter reportado problemas",
        44 => "um aplicativo ou serviço encerrou o dispositivo",
        45 => "dispositivo não conectado",
        46 => "sem acesso ao dispositivo (desligamento do sistema)",
        47 => "preparado para remoção segura",
        48 => "driver bloqueado por problemas conhecidos",
        49 => "registro excedeu o tamanho máximo",
        52 => "driver sem assinatura digital verificável",
        _ => "problema reportado pelo Gerenciador de Dispositivos",
    };
    format!("código {code}: {text}")
}

/// Códigos de "desabilitado de propósito": não são problema.
pub const fn intentionally_disabled(code: u32) -> bool {
    matches!(code, 22 | 29)
}

pub fn evaluate_devices(batch: &DeviceBatch) -> (Health, Vec<String>, DevicesData) {
    let disabled = batch
        .problems
        .iter()
        .filter(|d| intentionally_disabled(d.problem_code))
        .count() as u32;
    let issues: Vec<DeviceIssue> = batch
        .problems
        .iter()
        .filter(|d| d.problem_code != 0 && !intentionally_disabled(d.problem_code))
        .map(|d| DeviceIssue {
            name: d.name.clone(),
            class: d.class.clone(),
            manufacturer: d.manufacturer.clone(),
            problem_code: d.problem_code,
            problem: problem_text(d.problem_code),
        })
        .collect();
    let data = DevicesData {
        issues: issues.clone(),
        disabled,
        total: batch.total,
    };
    if batch.total == 0 {
        return (
            Health::Unknown,
            vec!["Nenhum dispositivo foi enumerado.".into()],
            data,
        );
    }
    if issues.is_empty() {
        return (Health::Healthy, vec![], data);
    }
    let reason = if issues.len() == 1 {
        "1 dispositivo reporta problema no Gerenciador de Dispositivos.".to_string()
    } else {
        format!(
            "{} dispositivos reportam problema no Gerenciador de Dispositivos.",
            issues.len()
        )
    };
    (Health::Attention, vec![reason], data)
}

/// Sinal específico de um evento, ou `None` para todo o ruído restante.
pub fn classify_event(provider: &str, id: u32) -> Option<SignalKind> {
    match (provider, id) {
        ("Microsoft-Windows-Kernel-Power", 41) | ("EventLog", 6008) => {
            Some(SignalKind::UnexpectedShutdown)
        }
        ("Microsoft-Windows-WER-SystemErrorReporting", 1001) => Some(SignalKind::Bugcheck),
        ("disk", 7 | 11 | 15 | 51 | 52 | 153) => Some(SignalKind::StorageError),
        ("Ntfs" | "Microsoft-Windows-Ntfs", 55 | 137 | 140) => Some(SignalKind::FilesystemError),
        ("Service Control Manager", 7000 | 7001 | 7009 | 7011 | 7031 | 7034) => {
            Some(SignalKind::ServiceFailure)
        }
        ("Microsoft-Windows-WindowsUpdateClient", 20 | 31) => Some(SignalKind::UpdateFailure),
        ("Application Error", 1000) => Some(SignalKind::ApplicationCrash),
        ("Application Hang", 1002) => Some(SignalKind::ApplicationHang),
        _ => None,
    }
}

/// Erro de armazenamento que indica falha real do dispositivo (os demais são frequentes e brandos).
fn hard_storage(event: &RawEvent) -> bool {
    event.provider == "disk" && matches!(event.id, 7 | 11 | 52)
}

pub fn signal_label(kind: SignalKind) -> &'static str {
    match kind {
        SignalKind::UnexpectedShutdown => "Desligamento inesperado",
        SignalKind::Bugcheck => "Tela azul (bugcheck)",
        SignalKind::StorageError => "Erro de armazenamento",
        SignalKind::FilesystemError => "Erro de sistema de arquivos",
        SignalKind::ServiceFailure => "Falha de serviço",
        SignalKind::UpdateFailure => "Falha do Windows Update",
        SignalKind::ApplicationCrash => "Falha de aplicativo",
        SignalKind::ApplicationHang => "Aplicativo sem resposta",
    }
}

fn level_counts(events: &[RawEvent], now: Millis, window: Millis) -> LevelCounts {
    let mut counts = LevelCounts::default();
    for event in events
        .iter()
        .filter(|e| now - e.at <= window && e.at <= now + 60_000)
    {
        match event.level {
            1 => counts.critical += 1,
            2 => counts.error += 1,
            _ => {}
        }
    }
    counts
}

/// Mescla eventos do MESMO desligamento (41 e 6008 chegam juntos): janela de 10 minutos.
fn dedupe_shutdowns(mut events: Vec<&RawEvent>) -> Vec<&RawEvent> {
    events.sort_by_key(|e| e.at);
    let mut out: Vec<&RawEvent> = Vec::new();
    for event in events {
        if out
            .last()
            .is_none_or(|last| event.at - last.at > 10 * 60_000)
        {
            out.push(event);
        }
    }
    out
}

fn signal_events(batch: &EventBatch, now: Millis, kind: SignalKind) -> Vec<&RawEvent> {
    let pool = batch
        .signals
        .iter()
        .chain(batch.update_failures.iter().flatten())
        .chain(batch.app_events.iter().flatten());
    let mut events: Vec<&RawEvent> = pool
        .filter(|e| {
            classify_event(&e.provider, e.id) == Some(kind)
                && now - e.at <= WEEK_MS
                && e.at <= now + 60_000
        })
        .collect();
    if kind == SignalKind::UnexpectedShutdown {
        events = dedupe_shutdowns(events);
    }
    events.sort_by_key(|e| std::cmp::Reverse(e.at));
    events
}

/// Eventos: contagens por nível (informativas) + sinais específicos (regras objetivas).
pub fn evaluate_events(batch: &EventBatch, now: Millis) -> (Health, Vec<String>, EventsData) {
    let mut last_24h = level_counts(&batch.errors, now, DAY_MS);
    let last_7d = level_counts(&batch.errors, now, WEEK_MS);
    last_24h.warning = Some(batch.warnings_24h);
    let kinds = [
        SignalKind::UnexpectedShutdown,
        SignalKind::Bugcheck,
        SignalKind::StorageError,
        SignalKind::FilesystemError,
        SignalKind::ServiceFailure,
        SignalKind::UpdateFailure,
        SignalKind::ApplicationCrash,
        SignalKind::ApplicationHang,
    ];
    let mut signals = Vec::new();
    let mut recent: Vec<EventEntry> = Vec::new();
    for kind in kinds {
        let events = signal_events(batch, now, kind);
        if events.is_empty() {
            continue;
        }
        signals.push(EventSignal {
            kind,
            label: signal_label(kind).into(),
            count_24h: events.iter().filter(|e| now - e.at <= DAY_MS).count() as u32,
            count_7d: events.len() as u32,
            last_at: events.first().map(|e| e.at),
        });
        // Aplicativos que travam e falhas de atualização têm painel próprio: o "recente" é do sistema.
        if !matches!(
            kind,
            SignalKind::ApplicationCrash | SignalKind::ApplicationHang | SignalKind::UpdateFailure
        ) {
            recent.extend(events.iter().take(5).map(|e| EventEntry {
                at: e.at,
                provider: e.provider.clone(),
                id: e.id,
                kind,
            }));
        }
    }
    recent.sort_by_key(|e| std::cmp::Reverse(e.at));
    recent.truncate(10);
    let data = EventsData {
        last_24h,
        last_7d,
        warnings_capped: batch.warnings_capped,
        signals,
        recent,
        truncated: batch.truncated,
    };

    let mut reasons = Vec::new();
    let mut status = Health::Healthy;
    let mut raise = |level: Health, reason: String| {
        status = status.max(level);
        reasons.push(reason);
    };
    let bugchecks = signal_events(batch, now, SignalKind::Bugcheck);
    if bugchecks.iter().any(|e| now - e.at <= DAY_MS) {
        raise(
            Health::Critical,
            "Tela azul (bugcheck) registrada nas últimas 24 horas.".into(),
        );
    } else if !bugchecks.is_empty() {
        raise(
            Health::Attention,
            format!(
                "{} tela(s) azul(is) (bugcheck) nos últimos 7 dias.",
                bugchecks.len()
            ),
        );
    }
    let shutdowns = signal_events(batch, now, SignalKind::UnexpectedShutdown);
    if !shutdowns.is_empty() {
        raise(
            Health::Attention,
            format!(
                "{} desligamento(s) inesperado(s) nos últimos 7 dias.",
                shutdowns.len()
            ),
        );
    }
    let storage = signal_events(batch, now, SignalKind::StorageError);
    let hard = storage.iter().filter(|e| hard_storage(e)).count();
    if hard > 0 {
        raise(
            Health::Attention,
            format!("{hard} erro(s) de dispositivo de armazenamento nos últimos 7 dias."),
        );
    } else if storage.len() >= 5 {
        raise(
            Health::Attention,
            format!(
                "{} avisos de E/S de disco repetidos nos últimos 7 dias.",
                storage.len()
            ),
        );
    }
    let filesystem = signal_events(batch, now, SignalKind::FilesystemError);
    if !filesystem.is_empty() {
        raise(
            Health::Attention,
            format!(
                "{} erro(s) de sistema de arquivos (NTFS) nos últimos 7 dias.",
                filesystem.len()
            ),
        );
    }
    let services_24h = signal_events(batch, now, SignalKind::ServiceFailure)
        .iter()
        .filter(|e| now - e.at <= DAY_MS)
        .count();
    if services_24h >= 3 {
        raise(
            Health::Attention,
            format!("{services_24h} falhas de serviço nas últimas 24 horas."),
        );
    }
    (status, reasons, data)
}

/// "2026-10-04 13:45:12" (UTC, formato do registro do Windows Update) → ms desde a época Unix.
pub fn parse_wu_time(text: &str) -> Option<Millis> {
    let text = text.trim();
    let (date, time) = text.split_once(' ')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|p| p.parse::<i64>().ok());
    let (hour, minute, second) = (t.next()??, t.next()??, t.next()??);
    if !(1970..=2200).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Dias desde 1970-01-01 (algoritmo civil de Howard Hinnant).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000)
}

pub fn evaluate_updates(
    service: Option<&ServiceView>,
    failures: Option<u32>,
) -> (Health, Vec<String>) {
    let mut reasons = Vec::new();
    let mut status = Health::Healthy;
    if let Some(view) = service {
        if view.start == StartType::Disabled {
            status = Health::Attention;
            reasons.push("O serviço do Windows Update está desabilitado: atualizações não podem ser instaladas.".into());
        }
    }
    match failures {
        Some(n) if n > 0 => {
            status = Health::Attention;
            reasons.push(format!(
                "{n} falha(s) de instalação ou download do Windows Update nos últimos 7 dias."
            ));
        }
        Some(_) => {}
        None if status == Health::Healthy => {
            return (
                Health::Unknown,
                vec!["O histórico de falhas do Windows Update não pôde ser lido.".into()],
            );
        }
        None => {}
    }
    if service.is_none() && status == Health::Healthy {
        return (
            Health::Unknown,
            vec!["O serviço do Windows Update não pôde ser consultado.".into()],
        );
    }
    (status, reasons)
}

pub fn evaluate_volume(raw: &RawVolume) -> VolumeHealth {
    let mut reasons = Vec::new();
    let mut status = Health::Healthy;
    if raw.dirty == Some(true) {
        status = Health::Attention;
        reasons.push(format!("O volume {} está marcado como sujo: o Windows verificará o disco na próxima inicialização.", raw.mount));
    }
    if raw.read_only == Some(true) {
        status = Health::Attention;
        reasons.push(format!(
            "O volume {} está montado como somente leitura.",
            raw.mount
        ));
    }
    if raw.check_scheduled == Some(true) {
        status = Health::Attention;
        reasons.push(format!(
            "Há uma verificação de disco agendada para {} na próxima inicialização.",
            raw.mount
        ));
    }
    if status == Health::Healthy && (raw.dirty.is_none() || raw.read_only.is_none()) {
        status = Health::Unknown;
        reasons.push(if raw.dirty_denied {
            format!("Estado de integridade do volume {} indisponível: requer privilégio administrativo.", raw.mount)
        } else {
            format!("Estado do volume {} não pôde ser lido por completo.", raw.mount)
        });
    }
    VolumeHealth {
        mount: raw.mount.clone(),
        filesystem: raw.filesystem.clone(),
        read_only: raw.read_only,
        dirty: raw.dirty,
        status,
        reasons,
    }
}

pub fn evaluate_volumes(items: &[VolumeHealth]) -> (Health, Vec<String>) {
    let status = worst(items.iter().map(|v| v.status));
    let reasons = items
        .iter()
        .filter(|v| v.status >= Health::Attention)
        .flat_map(|v| v.reasons.clone())
        .collect();
    (status, reasons)
}

/// Integridade PASSIVA: só reúne sinais que as outras leituras já têm. SFC, DISM e CHKDSK não são
/// executados aqui; sem verificação ativa a ausência de sinais não prova integridade (fica `Unknown`).
pub fn evaluate_integrity(
    restart: &RestartData,
    events: &EventsData,
    devices: &DevicesData,
    volumes: &VolumesData,
    update_failures: Option<u32>,
) -> (Health, Vec<String>, IntegrityData) {
    let signal = |id: &str, label: &str, present: bool| IntegritySignal {
        id: id.into(),
        label: label.into(),
        present,
    };
    let count = |kind: SignalKind| {
        events
            .signals
            .iter()
            .find(|s| s.kind == kind)
            .map_or(0, |s| s.count_7d)
    };
    let signals = vec![
        signal(
            "servicing_pending",
            "Reinício pendente de servicing/atualização",
            restart.pending == Some(true),
        ),
        signal(
            "update_failures",
            "Falhas de instalação do Windows Update",
            update_failures.is_some_and(|n| n > 0),
        ),
        signal(
            "filesystem_errors",
            "Erros de sistema de arquivos (NTFS)",
            count(SignalKind::FilesystemError) > 0,
        ),
        signal(
            "crash",
            "Tela azul ou desligamento inesperado",
            count(SignalKind::Bugcheck) + count(SignalKind::UnexpectedShutdown) > 0,
        ),
        signal(
            "device_problems",
            "Dispositivos com problema no driver",
            !devices.issues.is_empty(),
        ),
        signal(
            "dirty_volume",
            "Volume marcado como sujo",
            volumes.items.iter().any(|v| v.dirty == Some(true)),
        ),
    ];
    let present: Vec<String> = signals
        .iter()
        .filter(|s| s.present)
        .map(|s| s.label.clone())
        .collect();
    let on_demand = [
        (
            "sfc_verify",
            "Verificação de arquivos do sistema (SFC /verifynow)",
        ),
        (
            "dism_scan_health",
            "Verificação do component store (DISM /ScanHealth)",
        ),
        (
            "chkdsk_scan",
            "Verificação de volume somente leitura (CHKDSK /scan)",
        ),
    ]
    .into_iter()
    .map(|(id, label)| OnDemandCheck {
        id: id.into(),
        label: label.into(),
        available: false,
        requires_elevation: true,
        note: "Diagnóstico sob demanda: não é executado automaticamente e fica para o Block 09."
            .into(),
    })
    .collect();
    let data = IntegrityData { signals, on_demand };
    if present.is_empty() {
        return (
            Health::Unknown,
            vec!["Nenhum sinal passivo de problema. A integridade completa só é comprovada por verificação sob demanda (SFC/DISM), que não foi executada.".into()],
            data,
        );
    }
    let reasons = present
        .into_iter()
        .map(|label| format!("Sinal passivo: {label}."))
        .collect();
    (Health::Attention, reasons, data)
}

pub fn overall(sections: &[(&str, bool, Health, &[String])]) -> OverallHealth {
    let rateable = sections.iter().filter(|(_, rated, _, _)| *rated).count() as u32;
    let known: Vec<_> = sections
        .iter()
        .filter(|(_, rated, status, _)| *rated && *status != Health::Unknown)
        .collect();
    let status = worst(known.iter().map(|(_, _, status, _)| *status));
    let reasons = known
        .iter()
        .filter(|(_, _, status, _)| *status >= Health::Attention)
        .flat_map(|(domain, _, _, reasons)| {
            reasons.iter().map(|text| OverallReason {
                domain: (*domain).into(),
                text: text.clone(),
            })
        })
        .collect();
    OverallHealth {
        status,
        reasons,
        evaluated: known.len() as u32,
        rateable,
    }
}

// ------------------------------------------------------------------ fontes e coletor

/// De onde vêm os dados. A implementação real (`WindowsSources`) só lê; os testes usam fontes falsas.
pub trait Sources {
    fn system(&self) -> Result<SystemRaw, SourceError>;
    fn restart(&self) -> RestartProbes;
    fn services(&self, ids: &[&str]) -> Vec<(String, Result<RawService, SourceError>)>;
    fn devices(&self) -> Result<DeviceBatch, SourceError>;
    fn events(&self) -> Result<EventBatch, SourceError>;
    fn volumes(&self) -> Result<Vec<RawVolume>, SourceError>;
    fn update_times(&self) -> UpdateTimes;
}

fn failed<T: Default>(
    now: Millis,
    ttl: Millis,
    rated: bool,
    id: &str,
    label: &str,
    error: &SourceError,
) -> Section<T> {
    Section {
        status: Health::Unknown,
        rated,
        reasons: vec![format!("{label}: {}", error.reason())],
        checked_at: now,
        ttl_ms: ttl,
        sources: vec![SourceNote::new(
            id,
            label,
            error.state(),
            Some(error.reason()),
        )],
        data: T::default(),
    }
}

#[derive(Default)]
struct Cache {
    system: Option<Section<SystemData>>,
    restart: Option<Section<RestartData>>,
    services: Option<Section<ServicesData>>,
    devices: Option<Section<DevicesData>>,
    events: Option<(Section<EventsData>, Option<u32>, Vec<SourceNote>)>,
    last_update_success: Option<Millis>,
    update_times: Option<(Millis, UpdateTimes)>,
    volumes: Option<Section<VolumesData>>,
}

fn stale(checked_at: Millis, ttl: Millis, now: Millis, force: bool) -> bool {
    force || now < checked_at || now - checked_at >= ttl
}

/// Agrega os domínios, cada um com o próprio cache. Não faz nada além de ler.
pub struct Collector<S: Sources> {
    sources: S,
    cache: Mutex<Cache>,
}

impl<S: Sources> Collector<S> {
    pub fn new(sources: S) -> Self {
        Self {
            sources,
            cache: Mutex::new(Cache::default()),
        }
    }

    /// Snapshot atual. Cada domínio só é relido quando o cache dele expirou (ou com `force`).
    pub fn snapshot(&self, now: Millis, force: bool) -> WindowsHealthSnapshot {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let system = self.system(&mut cache, now, force);
        let uptime = system.data.uptime_secs;
        let restart = self.restart(&mut cache, now, force);
        let services = self.services(&mut cache, now, force, uptime);
        let devices = self.devices(&mut cache, now, force);
        let (events, update_failures, event_notes) = self.events(&mut cache, now, force);
        let volumes = self.volumes(&mut cache, now, force);
        let updates = self.updates(
            &mut cache,
            now,
            force,
            &services,
            update_failures,
            &event_notes,
        );
        let reliability = reliability(&events, &event_notes, now);
        let integrity = integrity(&restart, &events, &devices, &volumes, update_failures, now);
        let rated: [(&str, bool, Health, &[String]); 6] = [
            ("restart", true, restart.status, &restart.reasons),
            ("updates", true, updates.status, &updates.reasons),
            ("services", true, services.status, &services.reasons),
            ("devices", true, devices.status, &devices.reasons),
            ("events", true, events.status, &events.reasons),
            ("volumes", true, volumes.status, &volumes.reasons),
        ];
        let overall = overall(&rated);
        let capabilities = [
            &system.sources,
            &restart.sources,
            &services.sources,
            &devices.sources,
            &events.sources,
            &updates.sources,
            &volumes.sources,
            &reliability.sources,
        ]
        .iter()
        .flat_map(|sources| sources.iter().cloned())
        .collect();
        WindowsHealthSnapshot {
            captured_at: now,
            overall,
            system,
            restart,
            updates,
            services,
            devices,
            events,
            volumes,
            reliability,
            integrity,
            capabilities,
        }
    }

    fn system(&self, cache: &mut Cache, now: Millis, force: bool) -> Section<SystemData> {
        if let Some(section) = cache
            .system
            .as_ref()
            .filter(|s| !stale(s.checked_at, s.ttl_ms, now, force))
        {
            return section.clone();
        }
        let section = match self.sources.system() {
            Ok(raw) => Section {
                status: Health::Unknown,
                rated: false,
                reasons: vec![],
                checked_at: now,
                ttl_ms: TTL_SYSTEM_MS,
                sources: vec![SourceNote::ok("system", "Versão do Windows")],
                data: system_data(&raw),
            },
            Err(error) => failed(
                now,
                TTL_SYSTEM_MS,
                false,
                "system",
                "Versão do Windows",
                &error,
            ),
        };
        cache.system = Some(section.clone());
        section
    }

    fn restart(&self, cache: &mut Cache, now: Millis, force: bool) -> Section<RestartData> {
        if let Some(section) = cache
            .restart
            .as_ref()
            .filter(|s| !stale(s.checked_at, s.ttl_ms, now, force))
        {
            return section.clone();
        }
        let (status, reasons, data, sources) = evaluate_restart(&self.sources.restart());
        let section = Section {
            status,
            rated: true,
            reasons,
            checked_at: now,
            ttl_ms: TTL_RESTART_MS,
            sources,
            data,
        };
        cache.restart = Some(section.clone());
        section
    }

    fn services(
        &self,
        cache: &mut Cache,
        now: Millis,
        force: bool,
        uptime: Option<u64>,
    ) -> Section<ServicesData> {
        if let Some(section) = cache
            .services
            .as_ref()
            .filter(|s| !stale(s.checked_at, s.ttl_ms, now, force))
        {
            return section.clone();
        }
        let ids: Vec<&str> = ESSENTIAL_SERVICES.iter().map(|s| s.id).collect();
        let raw = self.sources.services(&ids);
        let mut sources = Vec::new();
        let items: Vec<ServiceView> = ESSENTIAL_SERVICES
            .iter()
            .map(|spec| {
                let found = raw
                    .iter()
                    .find(|(id, _)| id == spec.id)
                    .map(|(_, result)| result);
                let value = match found {
                    Some(Ok(service)) => Some(service),
                    Some(Err(error)) => {
                        sources.push(SourceNote::new(
                            &format!("service:{}", spec.id),
                            spec.label,
                            error.state(),
                            Some(error.reason()),
                        ));
                        None
                    }
                    None => None,
                };
                evaluate_service(spec, value, uptime)
            })
            .collect();
        if sources.is_empty() {
            sources.push(SourceNote::ok(
                "services",
                "Serviços essenciais (Service Control Manager)",
            ));
        }
        let (status, reasons) = evaluate_services(&items);
        let section = Section {
            status,
            rated: true,
            reasons,
            checked_at: now,
            ttl_ms: TTL_SERVICES_MS,
            sources,
            data: ServicesData { items },
        };
        cache.services = Some(section.clone());
        section
    }

    fn devices(&self, cache: &mut Cache, now: Millis, force: bool) -> Section<DevicesData> {
        if let Some(section) = cache
            .devices
            .as_ref()
            .filter(|s| !stale(s.checked_at, s.ttl_ms, now, force))
        {
            return section.clone();
        }
        let section = match self.sources.devices() {
            Ok(batch) => {
                let (status, reasons, data) = evaluate_devices(&batch);
                Section {
                    status,
                    rated: true,
                    reasons,
                    checked_at: now,
                    ttl_ms: TTL_DEVICES_MS,
                    sources: vec![SourceNote::ok("devices", "Gerenciador de Dispositivos")],
                    data,
                }
            }
            Err(error) => failed(
                now,
                TTL_DEVICES_MS,
                true,
                "devices",
                "Gerenciador de Dispositivos",
                &error,
            ),
        };
        cache.devices = Some(section.clone());
        section
    }

    fn events(
        &self,
        cache: &mut Cache,
        now: Millis,
        force: bool,
    ) -> (Section<EventsData>, Option<u32>, Vec<SourceNote>) {
        if let Some(entry) = cache
            .events
            .as_ref()
            .filter(|(s, _, _)| !stale(s.checked_at, s.ttl_ms, now, force))
        {
            return entry.clone();
        }
        let entry = match self.sources.events() {
            Ok(batch) => {
                cache.last_update_success = batch.last_update_success;
                let (status, reasons, data) = evaluate_events(&batch, now);
                // Sem o canal do cliente de atualização não há contagem de falhas (e não vira zero).
                let failures = batch
                    .update_failures
                    .as_ref()
                    .map(|_| signal_events(&batch, now, SignalKind::UpdateFailure).len() as u32);
                let sources = if batch.notes.is_empty() {
                    vec![SourceNote::ok("events", "Registro de Eventos do Windows")]
                } else {
                    batch.notes.clone()
                };
                (
                    Section {
                        status,
                        rated: true,
                        reasons,
                        checked_at: now,
                        ttl_ms: TTL_EVENTS_MS,
                        sources: sources.clone(),
                        data,
                    },
                    failures,
                    sources,
                )
            }
            Err(error) => {
                let section = failed::<EventsData>(
                    now,
                    TTL_EVENTS_MS,
                    true,
                    "events",
                    "Registro de Eventos do Windows",
                    &error,
                );
                let notes = section.sources.clone();
                (section, None, notes)
            }
        };
        cache.events = Some(entry.clone());
        entry
    }

    fn volumes(&self, cache: &mut Cache, now: Millis, force: bool) -> Section<VolumesData> {
        if let Some(section) = cache
            .volumes
            .as_ref()
            .filter(|s| !stale(s.checked_at, s.ttl_ms, now, force))
        {
            return section.clone();
        }
        let section = match self.sources.volumes() {
            Ok(raw) => {
                let items: Vec<VolumeHealth> = raw.iter().map(evaluate_volume).collect();
                let (status, reasons) = evaluate_volumes(&items);
                let elevated = raw.iter().any(|v| v.dirty_denied);
                let sources = vec![if elevated {
                    SourceNote::new(
                        "volumes",
                        "Volumes e sistema de arquivos",
                        SourceState::Partial,
                        Some("O bit de volume sujo requer privilégio administrativo"),
                    )
                } else {
                    SourceNote::ok("volumes", "Volumes e sistema de arquivos")
                }];
                Section {
                    status,
                    rated: true,
                    reasons,
                    checked_at: now,
                    ttl_ms: TTL_VOLUMES_MS,
                    sources,
                    data: VolumesData { items },
                }
            }
            Err(error) => failed(
                now,
                TTL_VOLUMES_MS,
                true,
                "volumes",
                "Volumes e sistema de arquivos",
                &error,
            ),
        };
        cache.volumes = Some(section.clone());
        section
    }

    fn updates(
        &self,
        cache: &mut Cache,
        now: Millis,
        force: bool,
        services: &Section<ServicesData>,
        failures: Option<u32>,
        event_notes: &[SourceNote],
    ) -> Section<UpdatesData> {
        if cache
            .update_times
            .as_ref()
            .is_none_or(|(at, _)| stale(*at, TTL_UPDATES_MS, now, force))
        {
            cache.update_times = Some((now, self.sources.update_times()));
        }
        let (checked_at, times) = cache
            .update_times
            .clone()
            .unwrap_or((now, UpdateTimes::default()));
        let service = services
            .data
            .items
            .iter()
            .find(|s| s.id == "wuauserv")
            .cloned();
        let (status, reasons) = evaluate_updates(service.as_ref(), failures);
        let mut sources = times.notes.clone();
        if sources.is_empty() {
            sources.push(SourceNote::ok(
                "update_times",
                "Histórico do Windows Update (registro)",
            ));
        }
        sources.extend(
            event_notes
                .iter()
                .filter(|n| n.id.starts_with("events:update"))
                .cloned(),
        );
        Section {
            status,
            rated: true,
            reasons,
            checked_at,
            ttl_ms: TTL_UPDATES_MS,
            sources,
            data: UpdatesData {
                service,
                // Registro (versões antigas) ou, na falta dele, o evento 19 do cliente de atualização.
                last_install_success_at: times
                    .last_install_success_at
                    .or(cache.last_update_success),
                last_scan_success_at: times.last_scan_success_at,
                failures_7d: failures,
                pending_count: None,
            },
        }
    }
}

fn reliability(
    events: &Section<EventsData>,
    notes: &[SourceNote],
    now: Millis,
) -> Section<ReliabilityData> {
    let count = |kind: SignalKind| {
        events
            .data
            .signals
            .iter()
            .find(|s| s.kind == kind)
            .map_or(0, |s| s.count_7d)
    };
    let app_available = !notes
        .iter()
        .any(|n| n.id == "events:application" && n.state != SourceState::Available);
    let mut sources = vec![SourceNote::new(
        "reliability_monitor",
        "Monitor de Confiabilidade (WMI)",
        SourceState::Unavailable,
        Some("Não consultado: exige WMI, que é caro para uma leitura periódica. Sem pontuação inventada."),
    )];
    sources.extend(
        notes
            .iter()
            .filter(|n| n.id == "events:application")
            .cloned(),
    );
    Section {
        status: Health::Unknown,
        rated: false,
        reasons: vec![],
        checked_at: now,
        ttl_ms: TTL_EVENTS_MS,
        sources,
        data: ReliabilityData {
            app_crashes_7d: app_available.then(|| count(SignalKind::ApplicationCrash)),
            app_hangs_7d: app_available.then(|| count(SignalKind::ApplicationHang)),
        },
    }
}

fn integrity(
    restart: &Section<RestartData>,
    events: &Section<EventsData>,
    devices: &Section<DevicesData>,
    volumes: &Section<VolumesData>,
    update_failures: Option<u32>,
    now: Millis,
) -> Section<IntegrityData> {
    let (status, reasons, data) = evaluate_integrity(
        &restart.data,
        &events.data,
        &devices.data,
        &volumes.data,
        update_failures,
    );
    Section {
        status,
        rated: false,
        reasons,
        checked_at: now,
        ttl_ms: 0,
        sources: vec![SourceNote::new(
            "integrity_active",
            "Verificação ativa de integridade (SFC/DISM)",
            SourceState::Unavailable,
            Some("Não executada: diagnóstico sob demanda, fora deste Block"),
        )],
        data,
    }
}

// ------------------------------------------------------------------ fontes reais

/// Fontes reais do Windows (somente leitura, em processo). Fora do Windows tudo é indisponível.
pub struct WindowsSources;

impl Sources for WindowsSources {
    fn system(&self) -> Result<SystemRaw, SourceError> {
        crate::windows_native::system()
    }
    fn restart(&self) -> RestartProbes {
        crate::windows_native::restart_probes()
    }
    fn services(&self, ids: &[&str]) -> Vec<(String, Result<RawService, SourceError>)> {
        ids.iter()
            .map(|id| ((*id).to_string(), crate::windows_native::service(id)))
            .collect()
    }
    fn devices(&self) -> Result<DeviceBatch, SourceError> {
        crate::windows_native::devices()
    }
    fn events(&self) -> Result<EventBatch, SourceError> {
        crate::windows_native::events()
    }
    fn volumes(&self) -> Result<Vec<RawVolume>, SourceError> {
        crate::windows_native::volumes()
    }
    fn update_times(&self) -> UpdateTimes {
        crate::windows_native::update_times()
    }
}
