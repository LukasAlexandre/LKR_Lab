//! Network & Security Visibility (SESSION-002, Block 08): leitura PASSIVA da rede e da postura de
//! segurança da MÁQUINA. Nada aqui altera configuração: não ativa firewall, não muda regra, não
//! bloqueia porta, não encerra processo, não inicia varredura do Defender, não mexe em BitLocker,
//! DNS, rota, adaptador ou perfil de rede, e não faz varredura de portas nem consulta externa.
//!
//! Princípios:
//! - **Não inventar insegurança.** Defender inativo por causa de antivírus de terceiros NÃO é crítico;
//!   escutar em `0.0.0.0` NÃO é vulnerabilidade nem "exposto à internet" (isso depende do firewall e do
//!   roteador, que o app não testa); BitLocker não consultável é `unknown`; Secure Boot desligado é um fato.
//! - **Ausência de informação é `unknown`, nunca `healthy`.** Fonte que exige administrador vira
//!   `requires_elevation`; não há UAC automático.
//! - **Sem segredos:** nenhuma chave de recuperação, senha, token ou segredo de Wi-Fi é lido.
//! - **Estado de máquina:** endpoints remotos e conexões nunca vão ao workspace portátil, Git, sync,
//!   Planejamento, DDAE ou contexto de IA. Sem reverse DNS e sem IP público consultado.
//! - **Sem security score.** Só estado e motivo objetivos por domínio.
//!
//! Estrutura: contrato + regras puras + `Collector<S: Sources>` com cache por domínio. Os testes usam
//! fontes falsas; `LiveSources` (em `security_native.rs`) só lê.
use crate::control_plane::{self, Confidence, Context, PortObservation};
use crate::sensors::{AdapterKind, NetAdapter};
use crate::system::RawProcess;
use crate::windows_health::{
    overall, worst, Health, Millis, OverallHealth, RawService, Section, ServiceState, SourceError,
    SourceNote, SourceState, DAY_MS,
};
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

pub const TTL_NETWORK_MS: Millis = 15_000;
pub const TTL_EXPOSURE_MS: Millis = 15_000;
pub const TTL_CONNECTIONS_MS: Millis = 10_000;
pub const TTL_FIREWALL_MS: Millis = 60_000;
pub const TTL_ANTIVIRUS_MS: Millis = 60_000;
pub const TTL_ENCRYPTION_MS: Millis = 5 * 60_000;
pub const TTL_BOOT_MS: Millis = 10 * 60_000;

/// Teto de conexões devolvidas (a contagem total continua exata).
pub const MAX_CONNECTIONS: usize = 300;
/// Assinaturas do Defender mais velhas que isso são "desatualizadas".
pub const SIGNATURE_STALE_DAYS: i64 = 7;
/// GUID do próprio Microsoft Defender em `Security Center\Provider\Av` (os demais são de terceiros).
pub const DEFENDER_PROVIDER_GUID: &str = "{D68DDC3A-831F-4fae-9E44-DA132C1ACF46}";

// ------------------------------------------------------------------ tipos brutos das fontes

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    Domain,
    Private,
    Public,
}
impl ProfileKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Domain => "Domínio",
            Self::Private => "Privado",
            Self::Public => "Público",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallAction {
    Allow,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirewallProfileRaw {
    pub kind: ProfileKind,
    /// `None` = não lido (nunca é lido como desligado nem como ligado).
    pub enabled: Option<bool>,
    pub default_inbound: Option<FirewallAction>,
    pub default_outbound: Option<FirewallAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WscProvider {
    Antivirus,
    Firewall,
}

/// Saúde agregada que o Windows Security Center dá a uma categoria de proteção.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WscHealth {
    Good,
    NotMonitored,
    Poor,
    Snooze,
}

#[derive(Debug, Clone, Default)]
pub struct DefenderRaw {
    pub service: Option<RawService>,
    pub service_flag: Option<bool>,
    pub disable_antivirus: Option<bool>,
    pub disable_antispyware: Option<bool>,
    pub policy_disabled: Option<bool>,
    pub realtime_disabled: Option<bool>,
    pub forced_passive: Option<bool>,
    pub signature_version: Option<String>,
    pub engine_version: Option<String>,
    pub signatures_updated_at: Option<Millis>,
    /// Provedores de antivírus registrados além do Defender; `None` = não foi possível contar.
    pub third_party_av: Option<u32>,
    /// Ameaças ativas informadas pelo Defender; `None` = não consultado.
    pub active_threats: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BitlockerState {
    Protected,
    Suspended,
    Off,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BitlockerVolume {
    pub mount: String,
    /// Volume do sistema operacional.
    pub system: bool,
    pub state: BitlockerState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecureBootState {
    Enabled,
    Disabled,
    /// Firmware legado (BIOS): o recurso não existe.
    Unsupported,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecureBootRaw {
    pub state: SecureBootState,
    pub uefi: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TpmRaw {
    pub present: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawConnection {
    pub local: IpAddr,
    pub local_port: u16,
    pub remote: IpAddr,
    pub remote_port: u16,
    pub pid: Option<u32>,
}

// ------------------------------------------------------------------ contrato (saída)

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceView {
    pub name: String,
    pub description: String,
    pub kind: &'static str,
    pub up: bool,
    /// Interface que o Windows usa para sair (rota padrão).
    pub active: bool,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub prefix: Option<u8>,
    pub gateways: Vec<String>,
    pub dns: Vec<String>,
    pub dhcp: bool,
    pub mac: Option<String>,
    pub link_speed_bps: Option<u64>,
    pub profile: Option<ProfileKind>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicIpView {
    /// Sempre `false`: o app não faz requisição externa para descobrir o IP público.
    pub queried: bool,
    pub note: String,
}
impl Default for PublicIpView {
    fn default() -> Self {
        Self {
            queried: false,
            note: "Não consultado: o LKR LAB não faz requisições externas para descobrir o IP público.".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkData {
    pub interfaces: Vec<InterfaceView>,
    pub active_interface: Option<String>,
    pub local_ipv4: Option<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub profile: Option<ProfileKind>,
    pub public_ip: PublicIpView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenerScope {
    /// 127.0.0.0/8 ou ::1: só esta máquina.
    Loopback,
    /// Endereço específico de uma interface.
    Specific,
    /// 0.0.0.0 ou ::: todas as interfaces. NÃO significa exposição à internet.
    AllInterfaces,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenerView {
    pub port: u16,
    pub address: String,
    pub ip_version: &'static str,
    pub scope: ListenerScope,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub executable: Option<String>,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub confidence: Confidence,
    /// Processo do sistema operacional.
    pub system: bool,
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposureCounts {
    pub total: u32,
    pub loopback: u32,
    pub specific: u32,
    pub all_interfaces: u32,
    pub unidentified: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposureData {
    pub listeners: Vec<ListenerView>,
    pub counts: ExposureCounts,
    /// O que NÃO é coberto, dito explicitamente.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteScope {
    Loopback,
    /// Rede local/privada (RFC 1918, link-local, ULA).
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    pub local_address: String,
    pub local_port: u16,
    pub remote_address: String,
    pub remote_port: u16,
    pub scope: RemoteScope,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub project_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsData {
    pub total: u32,
    pub loopback: u32,
    pub local: u32,
    pub remote: u32,
    pub items: Vec<ConnectionView>,
    pub truncated: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallProfileView {
    pub kind: ProfileKind,
    pub label: &'static str,
    pub enabled: Option<bool>,
    pub default_inbound: Option<FirewallAction>,
    pub default_outbound: Option<FirewallAction>,
    /// Perfil da rede ativa agora.
    pub active: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallData {
    pub profiles: Vec<FirewallProfileView>,
    pub active_profile: Option<ProfileKind>,
    pub security_center: Option<WscHealth>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefenderState {
    Active,
    /// Ativo, mas deixando outro antivírus protegendo. NÃO é problema.
    Passive,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AvProvider {
    Defender,
    ThirdParty,
    None,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefenderView {
    pub state: DefenderState,
    pub service_running: Option<bool>,
    pub realtime_protection: Option<bool>,
    pub engine_version: Option<String>,
    pub signature_version: Option<String>,
    pub signatures_updated_at: Option<Millis>,
    pub signature_age_days: Option<i64>,
    /// `None` = não consultado (a leitura passiva não pergunta ameaças ao Defender).
    pub active_threats: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AntivirusData {
    pub provider: AvProvider,
    pub third_party_count: Option<u32>,
    pub security_center: Option<WscHealth>,
    pub defender: DefenderView,
    pub notes: Vec<String>,
}
impl Default for AntivirusData {
    fn default() -> Self {
        Self {
            provider: AvProvider::Unknown,
            third_party_count: None,
            security_center: None,
            defender: DefenderView {
                state: DefenderState::Unknown,
                service_running: None,
                realtime_protection: None,
                engine_version: None,
                signature_version: None,
                signatures_updated_at: None,
                signature_age_days: None,
                active_threats: None,
            },
            notes: vec![],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionData {
    pub volumes: Vec<BitlockerVolume>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecureBootData {
    pub state: SecureBootState,
    pub uefi: Option<bool>,
}
impl Default for SecureBootData {
    fn default() -> Self {
        Self {
            state: SecureBootState::Unavailable,
            uefi: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TpmData {
    pub present: Option<bool>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSecuritySnapshot {
    pub captured_at: Millis,
    pub overall: OverallHealth,
    pub network: Section<NetworkData>,
    pub exposure: Section<ExposureData>,
    pub connections: Section<ConnectionsData>,
    pub firewall: Section<FirewallData>,
    pub antivirus: Section<AntivirusData>,
    pub encryption: Section<EncryptionData>,
    pub secure_boot: Section<SecureBootData>,
    pub tpm: Section<TpmData>,
    /// Todas as fontes consultadas, para a interface dizer o que NÃO está disponível.
    pub capabilities: Vec<SourceNote>,
}

// ------------------------------------------------------------------ regras puras

/// Escopo do endereço de escuta.
pub fn classify_scope(address: &str) -> ListenerScope {
    match address.parse::<IpAddr>() {
        Ok(ip) if ip.is_loopback() => ListenerScope::Loopback,
        Ok(ip) if ip.is_unspecified() => ListenerScope::AllInterfaces,
        _ => ListenerScope::Specific,
    }
}

pub fn remote_scope(ip: &IpAddr) -> RemoteScope {
    if ip.is_loopback() {
        return RemoteScope::Loopback;
    }
    let local = match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
        }
    };
    if local {
        RemoteScope::Local
    } else {
        RemoteScope::Remote
    }
}

fn listener_note(scope: ListenerScope) -> &'static str {
    match scope {
        ListenerScope::Loopback => "Somente esta máquina (loopback).",
        ListenerScope::Specific => {
            "Escuta em um endereço específico de uma interface; alcançável pela rede dessa interface se o firewall permitir."
        }
        ListenerScope::AllInterfaces => {
            "Escuta em todas as interfaces. Não significa exposição à internet: depende do firewall e do roteador, que o app não testa."
        }
    }
}

fn is_link_local_v4(ip: &str) -> bool {
    ip.starts_with("169.254.")
}

/// Interface de saída. O IP que o próprio SO escolhe para sair (`route_ip`) decide; sem ele, vale a
/// interface ativa, não loopback, com IPv4 utilizável e gateway de menor métrica.
pub fn select_active_adapter(adapters: &[NetAdapter], route_ip: Option<&str>) -> Option<usize> {
    if let Some(ip) = route_ip {
        if let Some(index) = adapters
            .iter()
            .position(|a| a.up && a.kind != AdapterKind::Loopback && a.ipv4.iter().any(|x| x == ip))
        {
            return Some(index);
        }
    }
    adapters
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            a.up && a.kind != AdapterKind::Loopback
                && !a.gateways.is_empty()
                && a.ipv4.iter().any(|ip| !is_link_local_v4(ip))
        })
        .min_by_key(|(_, a)| a.ipv4_metric.unwrap_or(u32::MAX))
        .map(|(index, _)| index)
}

/// Endereços sem repetição, IPv4 antes de IPv6 (a ordem original é mantida dentro de cada família).
pub fn ordered_addresses(addresses: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<&String> = addresses
        .iter()
        .filter(|a| seen.insert(a.as_str()))
        .collect();
    let (v4, v6): (Vec<&String>, Vec<&String>) = unique.into_iter().partition(|a| !a.contains(':'));
    v4.into_iter().chain(v6).cloned().collect()
}

pub fn build_network(
    adapters: &[NetAdapter],
    profiles: &HashMap<String, ProfileKind>,
    route_ip: Option<&str>,
) -> NetworkData {
    let active = select_active_adapter(adapters, route_ip);
    let views: Vec<InterfaceView> = adapters
        .iter()
        .enumerate()
        .filter(|(_, a)| a.kind != AdapterKind::Loopback)
        .map(|(index, a)| InterfaceView {
            name: a.name.clone(),
            description: a.description.clone(),
            kind: a.kind.as_str(),
            up: a.up,
            active: active == Some(index),
            ipv4: a.ipv4.clone(),
            ipv6: a.ipv6.clone(),
            prefix: a.ipv4_prefix,
            gateways: ordered_addresses(&a.gateways),
            dns: ordered_addresses(&a.dns),
            dhcp: a.dhcp_v4,
            mac: a.mac.clone(),
            link_speed_bps: a.link_speed_bps,
            profile: a
                .network_guid
                .as_ref()
                .and_then(|g| profiles.get(g))
                .copied(),
        })
        .collect();
    let current = views.iter().find(|v| v.active);
    NetworkData {
        active_interface: current.map(|v| v.name.clone()),
        local_ipv4: current.and_then(|v| v.ipv4.iter().find(|ip| !is_link_local_v4(ip)).cloned()),
        gateway: current.and_then(|v| ordered_addresses(&v.gateways).into_iter().next()),
        dns: current
            .map(|v| ordered_addresses(&v.dns))
            .unwrap_or_default(),
        profile: current.and_then(|v| v.profile),
        interfaces: views,
        public_ip: PublicIpView::default(),
    }
}

pub fn build_exposure(
    ports: &[PortObservation],
    processes: &[RawProcess],
    ctx: &Context,
    managed: &HashMap<u32, (String, String)>,
) -> ExposureData {
    let attribution = control_plane::attribute(processes, ports, ctx, managed);
    let by_pid: HashMap<u32, &RawProcess> = processes.iter().map(|p| (p.pid, p)).collect();
    let mut listeners: Vec<ListenerView> = ports
        .iter()
        .map(|port| {
            let scope = classify_scope(&port.address);
            let process = port.pid.and_then(|pid| by_pid.get(&pid).copied());
            let association = port.pid.and_then(|pid| attribution.get(&pid));
            ListenerView {
                port: port.port,
                address: port.address.clone(),
                ip_version: port.ip_version,
                scope,
                pid: port.pid,
                process_name: process.map(|p| p.name.clone()),
                executable: process.and_then(|p| p.executable.clone()),
                project_id: association.and_then(|a| a.project_id.clone()),
                project_name: association.and_then(|a| a.project_name.clone()),
                confidence: association.map(|a| a.confidence).unwrap_or_default(),
                system: process.is_some_and(control_plane::is_system),
                note: listener_note(scope).into(),
            }
        })
        .collect();
    let rank = |scope: ListenerScope| match scope {
        ListenerScope::AllInterfaces => 0,
        ListenerScope::Specific => 1,
        ListenerScope::Loopback => 2,
    };
    listeners.sort_by_key(|l| (rank(l.scope), l.port));
    let mut counts = ExposureCounts {
        total: listeners.len() as u32,
        ..Default::default()
    };
    for l in &listeners {
        match l.scope {
            ListenerScope::Loopback => counts.loopback += 1,
            ListenerScope::Specific => counts.specific += 1,
            ListenerScope::AllInterfaces => counts.all_interfaces += 1,
        }
        if l.pid.is_none() {
            counts.unidentified += 1;
        }
    }
    ExposureData {
        listeners,
        counts,
        notes: vec![
            "Somente portas TCP em escuta; UDP não é listado.".into(),
            "Alcançabilidade externa não é testada: não há varredura de portas nem consulta de rede externa.".into(),
        ],
    }
}

pub fn build_connections(
    raw: &[RawConnection],
    processes: &[RawProcess],
    ports: &[PortObservation],
    ctx: &Context,
    managed: &HashMap<u32, (String, String)>,
) -> ConnectionsData {
    let attribution = control_plane::attribute(processes, ports, ctx, managed);
    let by_pid: HashMap<u32, &RawProcess> = processes.iter().map(|p| (p.pid, p)).collect();
    let mut data = ConnectionsData {
        total: raw.len() as u32,
        ..Default::default()
    };
    let mut items: Vec<ConnectionView> = raw
        .iter()
        .map(|c| {
            let scope = remote_scope(&c.remote);
            match scope {
                RemoteScope::Loopback => data.loopback += 1,
                RemoteScope::Local => data.local += 1,
                RemoteScope::Remote => data.remote += 1,
            }
            ConnectionView {
                local_address: c.local.to_string(),
                local_port: c.local_port,
                remote_address: c.remote.to_string(),
                remote_port: c.remote_port,
                scope,
                pid: c.pid,
                process_name: c
                    .pid
                    .and_then(|pid| by_pid.get(&pid))
                    .map(|p| p.name.clone()),
                project_name: c
                    .pid
                    .and_then(|pid| attribution.get(&pid))
                    .and_then(|a| a.project_name.clone()),
            }
        })
        .collect();
    let rank = |scope: RemoteScope| match scope {
        RemoteScope::Remote => 0,
        RemoteScope::Local => 1,
        RemoteScope::Loopback => 2,
    };
    items.sort_by(|a, b| {
        (rank(a.scope), &a.process_name, a.remote_port).cmp(&(
            rank(b.scope),
            &b.process_name,
            b.remote_port,
        ))
    });
    data.truncated = items.len() > MAX_CONNECTIONS;
    items.truncate(MAX_CONNECTIONS);
    data.items = items;
    data.notes = vec![
        "Somente conexões TCP estabelecidas, lidas agora. Dados desta máquina: não são sincronizados nem enviados a lugar nenhum.".into(),
        "Sem resolução reversa de DNS: endereços aparecem como o sistema os informa.".into(),
    ];
    data
}

/// Firewall: avalia o perfil da rede ativa (os demais perfis são informativos).
pub fn evaluate_firewall(
    profiles: &[FirewallProfileRaw],
    active: Option<ProfileKind>,
    wsc: Option<WscHealth>,
) -> (Health, Vec<String>, FirewallData) {
    let views: Vec<FirewallProfileView> = profiles
        .iter()
        .map(|p| FirewallProfileView {
            kind: p.kind,
            label: p.kind.label(),
            enabled: p.enabled,
            default_inbound: p.default_inbound,
            default_outbound: p.default_outbound,
            active: active == Some(p.kind),
        })
        .collect();
    let mut data = FirewallData {
        profiles: views,
        active_profile: active,
        security_center: wsc,
        notes: vec![],
    };
    if profiles.iter().all(|p| p.enabled.is_none()) {
        return (
            Health::Unknown,
            vec!["O estado do firewall não pôde ser lido.".into()],
            data,
        );
    }
    let Some(kind) = active else {
        // Sem perfil ativo conhecido: só dá para afirmar o que todos os perfis dizem.
        let off: Vec<_> = profiles
            .iter()
            .filter(|p| p.enabled == Some(false))
            .collect();
        if off.is_empty() && profiles.iter().all(|p| p.enabled == Some(true)) {
            return (Health::Healthy, vec![], data);
        }
        if off.is_empty() {
            return (
                Health::Unknown,
                vec!["Perfil de rede ativo desconhecido e nem todos os perfis foram lidos.".into()],
                data,
            );
        }
        let names: Vec<_> = off.iter().map(|p| p.kind.label()).collect();
        return (
            Health::Attention,
            vec![format!(
                "Firewall do Windows desativado no perfil {}; o perfil ativo não pôde ser determinado.",
                names.join(", ")
            )],
            data,
        );
    };
    let Some(profile) = profiles.iter().find(|p| p.kind == kind) else {
        return (
            Health::Unknown,
            vec![format!("Perfil {} do firewall não foi lido.", kind.label())],
            data,
        );
    };
    match profile.enabled {
        None => (
            Health::Unknown,
            vec![format!("Estado do perfil {} não foi lido.", kind.label())],
            data,
        ),
        Some(false) if wsc == Some(WscHealth::Good) => {
            data.notes.push(format!(
                "Firewall do Windows desativado no perfil {}, mas o Security Center informa outro firewall protegendo.",
                kind.label()
            ));
            (Health::Healthy, vec![], data)
        }
        Some(false) => (
            Health::Critical,
            vec![format!(
                "Firewall do Windows desativado no perfil ativo ({}) e nenhum outro firewall informado pelo Security Center.",
                kind.label()
            )],
            data,
        ),
        Some(true) => {
            if profile.default_inbound == Some(FirewallAction::Allow) {
                return (
                    Health::Attention,
                    vec![format!(
                        "Perfil {}: conexões de entrada permitidas por padrão.",
                        kind.label()
                    )],
                    data,
                );
            }
            (Health::Healthy, vec![], data)
        }
    }
}

pub fn signature_age_days(updated_at: Option<Millis>, now: Millis) -> Option<i64> {
    updated_at
        .filter(|at| *at <= now)
        .map(|at| (now - at) / DAY_MS)
}

pub fn defender_view(raw: &DefenderRaw, now: Millis) -> DefenderView {
    let running = raw
        .service
        .as_ref()
        .map(|s| s.state == ServiceState::Running)
        .or(raw.service_flag);
    let disabled_flag = raw.policy_disabled == Some(true)
        || raw.disable_antispyware == Some(true)
        || raw.disable_antivirus == Some(true);
    let realtime_off = raw.realtime_disabled == Some(true);
    let third_party = raw.third_party_av.unwrap_or(0) > 0;
    let inactive = running == Some(false) || disabled_flag || realtime_off;
    let state = if third_party && (inactive || raw.forced_passive == Some(true)) {
        DefenderState::Passive
    } else if inactive {
        DefenderState::Disabled
    } else if running == Some(true) {
        DefenderState::Active
    } else {
        DefenderState::Unknown
    };
    DefenderView {
        state,
        service_running: running,
        realtime_protection: raw.realtime_disabled.map(|off| !off),
        engine_version: raw.engine_version.clone(),
        signature_version: raw.signature_version.clone(),
        signatures_updated_at: raw.signatures_updated_at,
        signature_age_days: signature_age_days(raw.signatures_updated_at, now),
        active_threats: raw.active_threats,
    }
}

fn wsc_problem(health: WscHealth) -> Option<&'static str> {
    match health {
        WscHealth::Good => None,
        WscHealth::Poor => Some("o Security Center reporta a proteção como em risco"),
        WscHealth::Snooze => Some("o Security Center reporta a proteção como adiada"),
        WscHealth::NotMonitored => Some("o Security Center não monitora a proteção"),
    }
}

/// Antivírus: Defender passivo por causa de antivírus de terceiros NÃO é problema.
pub fn evaluate_antivirus(
    defender: Option<&DefenderRaw>,
    wsc: Option<WscHealth>,
    now: Millis,
) -> (Health, Vec<String>, AntivirusData) {
    let mut data = AntivirusData {
        security_center: wsc,
        ..Default::default()
    };
    if let Some(raw) = defender {
        data.defender = defender_view(raw, now);
        data.third_party_count = raw.third_party_av;
    }
    let state = data.defender.state;
    data.provider = match (state, data.third_party_count) {
        (DefenderState::Active, _) => AvProvider::Defender,
        (_, Some(n)) if n > 0 => AvProvider::ThirdParty,
        (_, Some(0)) if state != DefenderState::Unknown => AvProvider::None,
        _ => AvProvider::Unknown,
    };
    data.defender_notes();
    let mut reasons = Vec::new();
    let mut status = Health::Healthy;
    let mut raise = |level: Health, text: String, reasons: &mut Vec<String>| {
        status = status.max(level);
        reasons.push(text);
    };
    if let Some(threats) = data.defender.active_threats.filter(|n| *n > 0) {
        raise(
            Health::Critical,
            format!("O Microsoft Defender informa {threats} ameaça(s) ativa(s)."),
            &mut reasons,
        );
    }
    match data.provider {
        AvProvider::Defender => {
            if data
                .defender
                .signature_age_days
                .is_some_and(|d| d > SIGNATURE_STALE_DAYS)
            {
                raise(
                    Health::Attention,
                    format!(
                        "Assinaturas do Defender com {} dias (mais de {SIGNATURE_STALE_DAYS}).",
                        data.defender.signature_age_days.unwrap_or(0)
                    ),
                    &mut reasons,
                );
            }
            if let Some(text) = wsc.and_then(wsc_problem) {
                raise(
                    Health::Attention,
                    format!("Antivírus: {text}."),
                    &mut reasons,
                );
            }
        }
        AvProvider::ThirdParty => match wsc {
            Some(h) => {
                if let Some(text) = wsc_problem(h) {
                    raise(
                        Health::Attention,
                        format!("Antivírus de terceiros: {text}."),
                        &mut reasons,
                    );
                }
            }
            None => {
                if status == Health::Healthy {
                    status = Health::Unknown;
                    reasons.push("Antivírus de terceiros registrado, mas o Security Center não informa a saúde dele.".into());
                }
            }
        },
        AvProvider::None => {
            if wsc == Some(WscHealth::Good) {
                data.notes.push("O Security Center informa um antivírus saudável que não pôde ser identificado.".into());
            } else {
                raise(
                    Health::Critical,
                    "Nenhum antivírus ativo: o Defender está desativado e não há antivírus de terceiros registrado.".into(),
                    &mut reasons,
                );
            }
        }
        AvProvider::Unknown => {
            if wsc == Some(WscHealth::Good) {
                data.notes
                    .push("O Security Center informa um antivírus saudável.".into());
            } else if status == Health::Healthy {
                status = Health::Unknown;
                reasons.push("Não foi possível determinar o antivírus ativo.".into());
            }
        }
    }
    (status, reasons, data)
}

impl AntivirusData {
    fn defender_notes(&mut self) {
        match self.defender.state {
            DefenderState::Passive => self.notes.push(
                "O Defender está passivo porque outro antivírus está registrado. Isso é esperado, não um problema.".into(),
            ),
            DefenderState::Disabled if self.third_party_count.unwrap_or(0) > 0 => {}
            _ => {}
        }
        if self.defender.active_threats.is_none() {
            self.notes.push(
                "Ameaças ativas: não consultado (a leitura passiva não pergunta ao Defender)."
                    .into(),
            );
        }
    }
}

/// BitLocker: indisponível é `unknown`. Off no volume do sistema e suspenso são fatos a observar.
pub fn evaluate_encryption(volumes: &[BitlockerVolume]) -> (Health, Vec<String>) {
    if volumes.is_empty() {
        return (
            Health::Unknown,
            vec!["Nenhum volume com estado de BitLocker conhecido.".into()],
        );
    }
    let mut reasons = Vec::new();
    let mut levels = Vec::new();
    for v in volumes {
        match v.state {
            BitlockerState::Protected => levels.push(Health::Healthy),
            BitlockerState::Suspended => {
                levels.push(Health::Attention);
                reasons.push(format!("BitLocker suspenso em {}.", v.mount));
            }
            BitlockerState::Off if v.system => {
                levels.push(Health::Attention);
                reasons.push(format!(
                    "BitLocker desligado no volume do sistema ({}).",
                    v.mount
                ));
            }
            // Volume de dados sem BitLocker é só informação.
            BitlockerState::Off => {}
            BitlockerState::Unknown => {
                if v.system {
                    levels.push(Health::Unknown);
                    reasons.push(format!("Estado do BitLocker em {} desconhecido.", v.mount));
                }
            }
        }
    }
    let status = if levels.is_empty() {
        Health::Healthy
    } else if levels.iter().any(|h| *h >= Health::Attention) {
        worst(levels)
    } else if levels.contains(&Health::Unknown) {
        Health::Unknown
    } else {
        Health::Healthy
    };
    (status, reasons)
}

pub fn evaluate_secure_boot(raw: &SecureBootRaw) -> (Health, Vec<String>) {
    match raw.state {
        SecureBootState::Enabled => (Health::Healthy, vec![]),
        SecureBootState::Disabled => (
            Health::Attention,
            vec!["Secure Boot está desativado.".into()],
        ),
        SecureBootState::Unsupported => (
            Health::Unknown,
            vec![
                "Firmware legado (BIOS): Secure Boot não existe neste modo de inicialização."
                    .into(),
            ],
        ),
        SecureBootState::Unavailable => (
            Health::Unknown,
            vec!["Estado do Secure Boot indisponível.".into()],
        ),
    }
}

pub fn evaluate_tpm(raw: &TpmRaw) -> (Health, Vec<String>) {
    if raw.present {
        (Health::Healthy, vec![])
    } else {
        (Health::Attention, vec!["Nenhum TPM detectado.".into()])
    }
}

// ------------------------------------------------------------------ fontes e coletor

/// De onde vêm os dados. A implementação real (`LiveSources`) só lê; os testes usam fontes falsas.
pub trait Sources {
    fn adapters(&self) -> Vec<NetAdapter>;
    /// IPv4 que o SO escolhe para sair (rota padrão). Não envia nenhum pacote.
    fn route_ipv4(&self) -> Option<String>;
    fn network_profiles(
        &self,
        guids: &[String],
    ) -> Result<HashMap<String, ProfileKind>, SourceError>;
    fn listeners(&self) -> Result<Vec<PortObservation>, SourceError>;
    fn processes(&self) -> Vec<RawProcess>;
    fn connections(&self) -> Result<Vec<RawConnection>, SourceError>;
    fn firewall(&self) -> Result<Vec<FirewallProfileRaw>, SourceError>;
    fn security_center(&self, provider: WscProvider) -> Option<WscHealth>;
    fn defender(&self) -> Result<DefenderRaw, SourceError>;
    fn bitlocker(&self) -> Result<Vec<BitlockerVolume>, SourceError>;
    fn secure_boot(&self) -> Result<SecureBootRaw, SourceError>;
    fn tpm(&self) -> Result<TpmRaw, SourceError>;
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

fn stale(checked_at: Millis, ttl: Millis, now: Millis, force: bool) -> bool {
    force || now < checked_at || now - checked_at >= ttl
}

#[derive(Default)]
struct Cache {
    network: Option<Section<NetworkData>>,
    exposure: Option<Section<ExposureData>>,
    connections: Option<Section<ConnectionsData>>,
    firewall: Option<Section<FirewallData>>,
    antivirus: Option<Section<AntivirusData>>,
    encryption: Option<Section<EncryptionData>>,
    secure_boot: Option<Section<SecureBootData>>,
    tpm: Option<Section<TpmData>>,
}

fn fresh<T: Clone>(slot: &Option<Section<T>>, now: Millis, force: bool) -> Option<Section<T>> {
    slot.as_ref()
        .filter(|s| !stale(s.checked_at, s.ttl_ms, now, force))
        .cloned()
}

fn info<T>(
    now: Millis,
    ttl: Millis,
    sources: Vec<SourceNote>,
    reasons: Vec<String>,
    data: T,
) -> Section<T> {
    Section {
        status: Health::Unknown,
        rated: false,
        reasons,
        checked_at: now,
        ttl_ms: ttl,
        sources,
        data,
    }
}

fn rated<T>(
    now: Millis,
    ttl: Millis,
    status: Health,
    reasons: Vec<String>,
    sources: Vec<SourceNote>,
    data: T,
) -> Section<T> {
    Section {
        status,
        rated: true,
        reasons,
        checked_at: now,
        ttl_ms: ttl,
        sources,
        data,
    }
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

    /// `ctx` e `managed` vêm do banco e do Supervisor só para atribuir listeners a Projects.
    pub fn snapshot(
        &self,
        now: Millis,
        force: bool,
        ctx: &Context,
        managed: &HashMap<u32, (String, String)>,
    ) -> NetworkSecuritySnapshot {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let network = self.network(&mut cache, now, force);
        let exposure = self.exposure(&mut cache, now, force, ctx, managed);
        let connections = self.connections(&mut cache, now, force, ctx, managed);
        let firewall = self.firewall(&mut cache, now, force, &network);
        let antivirus = self.antivirus(&mut cache, now, force);
        let encryption = self.encryption(&mut cache, now, force);
        let secure_boot = self.secure_boot(&mut cache, now, force);
        let tpm = self.tpm(&mut cache, now, force);
        let rated_sections: [(&str, bool, Health, &[String]); 5] = [
            ("firewall", true, firewall.status, &firewall.reasons),
            ("antivirus", true, antivirus.status, &antivirus.reasons),
            ("encryption", true, encryption.status, &encryption.reasons),
            (
                "secure_boot",
                true,
                secure_boot.status,
                &secure_boot.reasons,
            ),
            ("tpm", true, tpm.status, &tpm.reasons),
        ];
        let overall = overall(&rated_sections);
        let capabilities = [
            &network.sources,
            &exposure.sources,
            &connections.sources,
            &firewall.sources,
            &antivirus.sources,
            &encryption.sources,
            &secure_boot.sources,
            &tpm.sources,
        ]
        .iter()
        .flat_map(|s| s.iter().cloned())
        .collect();
        NetworkSecuritySnapshot {
            captured_at: now,
            overall,
            network,
            exposure,
            connections,
            firewall,
            antivirus,
            encryption,
            secure_boot,
            tpm,
            capabilities,
        }
    }

    fn network(&self, cache: &mut Cache, now: Millis, force: bool) -> Section<NetworkData> {
        if let Some(section) = fresh(&cache.network, now, force) {
            return section;
        }
        let adapters = self.sources.adapters();
        let guids: Vec<String> = adapters
            .iter()
            .filter_map(|a| a.network_guid.clone())
            .collect();
        let profiles_result = self.sources.network_profiles(&guids);
        let profiles = profiles_result.clone().unwrap_or_default();
        let route = self.sources.route_ipv4();
        let data = build_network(&adapters, &profiles, route.as_deref());
        let mut sources = vec![SourceNote::ok("adapters", "Interfaces de rede")];
        let mut reasons = vec![];
        if adapters.is_empty() {
            sources[0] = SourceNote::new(
                "adapters",
                "Interfaces de rede",
                SourceState::Unavailable,
                Some("Nenhuma interface retornada pelo sistema."),
            );
        }
        if data.active_interface.is_none() && !adapters.is_empty() {
            reasons
                .push("Nenhuma interface com rota padrão ativa (sem conexão com gateway).".into());
        }
        if let Err(error) = &profiles_result {
            sources.push(SourceNote::new(
                "network_profile",
                "Categoria da rede (Público/Privado/Domínio)",
                error.state(),
                Some(error.reason()),
            ));
        } else if data.active_interface.is_some() && data.profile.is_none() {
            sources.push(SourceNote::new(
                "network_profile",
                "Categoria da rede (Público/Privado/Domínio)",
                SourceState::Partial,
                Some("O Windows não informou a categoria da rede ativa."),
            ));
        } else {
            sources.push(SourceNote::ok(
                "network_profile",
                "Categoria da rede (Público/Privado/Domínio)",
            ));
        }
        let section = info(now, TTL_NETWORK_MS, sources, reasons, data);
        cache.network = Some(section.clone());
        section
    }

    fn exposure(
        &self,
        cache: &mut Cache,
        now: Millis,
        force: bool,
        ctx: &Context,
        managed: &HashMap<u32, (String, String)>,
    ) -> Section<ExposureData> {
        if let Some(section) = fresh(&cache.exposure, now, force) {
            return section;
        }
        let section = match self.sources.listeners() {
            Ok(ports) => {
                let processes = self.sources.processes();
                let data = build_exposure(&ports, &processes, ctx, managed);
                let mut sources = vec![SourceNote::ok("listeners", "Portas TCP em escuta")];
                if data.counts.unidentified > 0 {
                    sources.push(SourceNote::new(
                        "listener_owners",
                        "Processo dono da porta",
                        SourceState::Partial,
                        Some("O sistema não informou o dono de algumas portas (permissão)."),
                    ));
                }
                info(now, TTL_EXPOSURE_MS, sources, vec![], data)
            }
            Err(error) => failed(
                now,
                TTL_EXPOSURE_MS,
                false,
                "listeners",
                "Portas TCP em escuta",
                &error,
            ),
        };
        cache.exposure = Some(section.clone());
        section
    }

    fn connections(
        &self,
        cache: &mut Cache,
        now: Millis,
        force: bool,
        ctx: &Context,
        managed: &HashMap<u32, (String, String)>,
    ) -> Section<ConnectionsData> {
        if let Some(section) = fresh(&cache.connections, now, force) {
            return section;
        }
        let section = match self.sources.connections() {
            Ok(raw) => {
                let processes = self.sources.processes();
                let ports = self.sources.listeners().unwrap_or_default();
                let data = build_connections(&raw, &processes, &ports, ctx, managed);
                info(
                    now,
                    TTL_CONNECTIONS_MS,
                    vec![SourceNote::ok("connections", "Conexões TCP estabelecidas")],
                    vec![],
                    data,
                )
            }
            Err(error) => failed(
                now,
                TTL_CONNECTIONS_MS,
                false,
                "connections",
                "Conexões TCP estabelecidas",
                &error,
            ),
        };
        cache.connections = Some(section.clone());
        section
    }

    fn firewall(
        &self,
        cache: &mut Cache,
        now: Millis,
        force: bool,
        network: &Section<NetworkData>,
    ) -> Section<FirewallData> {
        if let Some(section) = fresh(&cache.firewall, now, force) {
            // O perfil ativo pode mudar (troca de rede) antes de o cache expirar.
            if section.data.active_profile == network.data.profile {
                return section;
            }
        }
        let wsc = self.sources.security_center(WscProvider::Firewall);
        let mut sources = vec![if wsc.is_some() {
            SourceNote::ok("wsc_firewall", "Security Center (firewall)")
        } else {
            SourceNote::new(
                "wsc_firewall",
                "Security Center (firewall)",
                SourceState::Unavailable,
                Some("O Security Center não respondeu."),
            )
        }];
        let section = match self.sources.firewall() {
            Ok(profiles) => {
                sources.insert(
                    0,
                    SourceNote::ok("firewall_policy", "Política do Firewall do Windows"),
                );
                let (status, reasons, data) =
                    evaluate_firewall(&profiles, network.data.profile, wsc);
                rated(now, TTL_FIREWALL_MS, status, reasons, sources, data)
            }
            Err(error) => {
                let mut section: Section<FirewallData> = failed(
                    now,
                    TTL_FIREWALL_MS,
                    true,
                    "firewall_policy",
                    "Política do Firewall do Windows",
                    &error,
                );
                section.sources.extend(sources);
                section
            }
        };
        cache.firewall = Some(section.clone());
        section
    }

    fn antivirus(&self, cache: &mut Cache, now: Millis, force: bool) -> Section<AntivirusData> {
        if let Some(section) = fresh(&cache.antivirus, now, force) {
            return section;
        }
        let wsc = self.sources.security_center(WscProvider::Antivirus);
        let mut sources = vec![if wsc.is_some() {
            SourceNote::ok("wsc_antivirus", "Security Center (antivírus)")
        } else {
            SourceNote::new(
                "wsc_antivirus",
                "Security Center (antivírus)",
                SourceState::Unavailable,
                Some("O Security Center não respondeu."),
            )
        }];
        let defender = match self.sources.defender() {
            Ok(raw) => {
                sources.insert(
                    0,
                    SourceNote::ok("defender", "Microsoft Defender (registro e serviço)"),
                );
                Some(raw)
            }
            Err(error) => {
                sources.insert(
                    0,
                    SourceNote::new(
                        "defender",
                        "Microsoft Defender (registro e serviço)",
                        error.state(),
                        Some(error.reason()),
                    ),
                );
                None
            }
        };
        if defender
            .as_ref()
            .is_some_and(|d| d.third_party_av.is_none())
        {
            sources.push(SourceNote::new(
                "av_providers",
                "Antivírus de terceiros registrados",
                SourceState::Partial,
                Some("Não foi possível contar os provedores registrados."),
            ));
        }
        let (status, reasons, data) = evaluate_antivirus(defender.as_ref(), wsc, now);
        let section = rated(now, TTL_ANTIVIRUS_MS, status, reasons, sources, data);
        cache.antivirus = Some(section.clone());
        section
    }

    fn encryption(&self, cache: &mut Cache, now: Millis, force: bool) -> Section<EncryptionData> {
        if let Some(section) = fresh(&cache.encryption, now, force) {
            return section;
        }
        let section = match self.sources.bitlocker() {
            Ok(volumes) => {
                let (status, reasons) = evaluate_encryption(&volumes);
                rated(
                    now,
                    TTL_ENCRYPTION_MS,
                    status,
                    reasons,
                    vec![SourceNote::ok("bitlocker", "BitLocker por volume")],
                    EncryptionData { volumes },
                )
            }
            Err(error) => failed(
                now,
                TTL_ENCRYPTION_MS,
                true,
                "bitlocker",
                "BitLocker por volume",
                &error,
            ),
        };
        cache.encryption = Some(section.clone());
        section
    }

    fn secure_boot(&self, cache: &mut Cache, now: Millis, force: bool) -> Section<SecureBootData> {
        if let Some(section) = fresh(&cache.secure_boot, now, force) {
            return section;
        }
        let section = match self.sources.secure_boot() {
            Ok(raw) => {
                let (status, reasons) = evaluate_secure_boot(&raw);
                rated(
                    now,
                    TTL_BOOT_MS,
                    status,
                    reasons,
                    vec![SourceNote::ok("secure_boot", "Secure Boot")],
                    SecureBootData {
                        state: raw.state,
                        uefi: raw.uefi,
                    },
                )
            }
            Err(error) => failed(now, TTL_BOOT_MS, true, "secure_boot", "Secure Boot", &error),
        };
        cache.secure_boot = Some(section.clone());
        section
    }

    fn tpm(&self, cache: &mut Cache, now: Millis, force: bool) -> Section<TpmData> {
        if let Some(section) = fresh(&cache.tpm, now, force) {
            return section;
        }
        let section = match self.sources.tpm() {
            Ok(raw) => {
                let (status, reasons) = evaluate_tpm(&raw);
                rated(
                    now,
                    TTL_BOOT_MS,
                    status,
                    reasons,
                    vec![SourceNote::ok("tpm", "TPM")],
                    TpmData {
                        present: Some(raw.present),
                        version: raw.version,
                    },
                )
            }
            Err(error) => failed(now, TTL_BOOT_MS, true, "tpm", "TPM", &error),
        };
        cache.tpm = Some(section.clone());
        section
    }
}

/// Fontes reais (somente leitura). As leituras nativas ficam em `security_native`.
pub struct LiveSources;

impl Sources for LiveSources {
    fn adapters(&self) -> Vec<NetAdapter> {
        crate::sensors::network_adapters()
    }
    fn route_ipv4(&self) -> Option<String> {
        crate::machine::route_ipv4().map(|ip| ip.to_string())
    }
    fn network_profiles(
        &self,
        guids: &[String],
    ) -> Result<HashMap<String, ProfileKind>, SourceError> {
        crate::security_native::network_profiles(guids)
    }
    fn listeners(&self) -> Result<Vec<PortObservation>, SourceError> {
        control_plane::listening_ports().map_err(|e| SourceError::Unavailable(e.to_string()))
    }
    fn processes(&self) -> Vec<RawProcess> {
        crate::system::inventory()
    }
    fn connections(&self) -> Result<Vec<RawConnection>, SourceError> {
        crate::security_native::connections()
    }
    fn firewall(&self) -> Result<Vec<FirewallProfileRaw>, SourceError> {
        crate::security_native::firewall_policy()
    }
    fn security_center(&self, provider: WscProvider) -> Option<WscHealth> {
        crate::security_native::wsc_health(provider)
    }
    fn defender(&self) -> Result<DefenderRaw, SourceError> {
        crate::security_native::defender()
    }
    fn bitlocker(&self) -> Result<Vec<BitlockerVolume>, SourceError> {
        crate::security_native::bitlocker()
    }
    fn secure_boot(&self) -> Result<SecureBootRaw, SourceError> {
        crate::security_native::secure_boot()
    }
    fn tpm(&self) -> Result<TpmRaw, SourceError> {
        crate::security_native::tpm()
    }
}
