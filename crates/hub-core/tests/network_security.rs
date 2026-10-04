//! Block 08 — Network & Security Visibility. Todas as fontes são falsas: nenhum teste depende do
//! firewall, do Defender, do TPM ou da rede reais (a leitura real fica em um teste ignorado).
use hub_core::control_plane::{Confidence, Context, PortObservation, ProjectRef};
use hub_core::network_security::*;
use hub_core::sensors::{AdapterKind, NetAdapter};
use hub_core::system::RawProcess;
use hub_core::windows_health::{
    Health, RawService, ServiceState, SourceError, SourceState, StartType, DAY_MS,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::net::IpAddr;

const NOW: i64 = 1_800_000_000_000;

fn adapter(
    name: &str,
    kind: AdapterKind,
    ip: &str,
    gateway: Option<&str>,
    metric: u32,
) -> NetAdapter {
    NetAdapter {
        name: name.into(),
        description: format!("{name} adapter"),
        kind,
        up: true,
        link_speed_bps: Some(100_000_000),
        ipv4: if ip.is_empty() {
            vec![]
        } else {
            vec![ip.into()]
        },
        ipv6: vec!["fe80::1".into()],
        if_index: 1,
        mac: Some("AA:BB:CC:DD:EE:FF".into()),
        dhcp_v4: true,
        ipv4_metric: Some(metric),
        ipv4_prefix: Some(24),
        gateways: gateway.map(|g| vec![g.into()]).unwrap_or_default(),
        dns: vec!["1.1.1.1".into(), "8.8.8.8".into()],
        network_guid: Some(format!("{{{name}}}")),
    }
}

fn port(port: u16, address: &str, pid: Option<u32>) -> PortObservation {
    PortObservation {
        port,
        address: address.into(),
        protocol: "TCP",
        ip_version: if address.contains(':') { "v6" } else { "v4" },
        pid,
    }
}

fn process(pid: u32, name: &str, exe: &str, cwd: Option<&str>) -> RawProcess {
    RawProcess {
        pid,
        parent: None,
        name: name.into(),
        executable: Some(exe.into()),
        cmd: vec![exe.into()],
        cwd: cwd.map(String::from),
        start_time: 1_000,
        cpu: 0.0,
        memory: 0,
        read_bytes: 0,
        written_bytes: 0,
    }
}

fn fw(kind: ProfileKind, enabled: Option<bool>) -> FirewallProfileRaw {
    FirewallProfileRaw {
        kind,
        enabled,
        default_inbound: None,
        default_outbound: None,
    }
}

fn all_fw(enabled: bool) -> Vec<FirewallProfileRaw> {
    vec![
        fw(ProfileKind::Domain, Some(enabled)),
        fw(ProfileKind::Private, Some(enabled)),
        fw(ProfileKind::Public, Some(enabled)),
    ]
}

fn defender_raw() -> DefenderRaw {
    DefenderRaw {
        service: Some(RawService {
            state: ServiceState::Running,
            start: StartType::Automatic,
        }),
        service_flag: Some(true),
        disable_antivirus: Some(false),
        disable_antispyware: Some(false),
        engine_version: Some("1.1.26080.3".into()),
        signature_version: Some("1.459.546.0".into()),
        signatures_updated_at: Some(NOW - DAY_MS),
        third_party_av: Some(0),
        ..Default::default()
    }
}

struct Fake {
    adapters: Vec<NetAdapter>,
    route: Option<String>,
    profiles_result: Result<(), SourceError>,
    profiles: HashMap<String, ProfileKind>,
    listeners: Result<Vec<PortObservation>, SourceError>,
    processes: Vec<RawProcess>,
    connections: Result<Vec<RawConnection>, SourceError>,
    firewall: Result<Vec<FirewallProfileRaw>, SourceError>,
    wsc_fw: Option<WscHealth>,
    wsc_av: Option<WscHealth>,
    defender: Result<DefenderRaw, SourceError>,
    bitlocker: Result<Vec<BitlockerVolume>, SourceError>,
    secure_boot: Result<SecureBootRaw, SourceError>,
    tpm: Result<TpmRaw, SourceError>,
    reads: Cell<u32>,
    log: RefCell<Vec<&'static str>>,
}

impl Fake {
    fn healthy() -> Self {
        let wifi = adapter(
            "wifi",
            AdapterKind::Wifi,
            "192.168.1.42",
            Some("192.168.1.1"),
            35,
        );
        let mut profiles = HashMap::new();
        profiles.insert("{wifi}".to_string(), ProfileKind::Private);
        Self {
            adapters: vec![
                adapter("lo", AdapterKind::Loopback, "127.0.0.1", None, 75),
                adapter("eth", AdapterKind::Ethernet, "169.254.3.4", None, 25),
                wifi,
            ],
            route: Some("192.168.1.42".into()),
            profiles_result: Ok(()),
            profiles,
            listeners: Ok(vec![
                port(4317, "127.0.0.1", Some(10)),
                port(3000, "0.0.0.0", Some(11)),
                port(5432, "192.168.1.42", Some(12)),
                port(135, "::", None),
            ]),
            processes: vec![
                process(10, "node.exe", r"C:\Dev\Lab\node.exe", Some(r"C:\Dev\Lab")),
                process(11, "vite.exe", r"C:\tools\vite.exe", None),
                process(12, "postgres.exe", r"C:\pg\postgres.exe", None),
            ],
            connections: Ok(vec![
                RawConnection {
                    local: "192.168.1.42".parse().unwrap(),
                    local_port: 50000,
                    remote: "140.82.112.3".parse().unwrap(),
                    remote_port: 443,
                    pid: Some(11),
                },
                RawConnection {
                    local: "127.0.0.1".parse().unwrap(),
                    local_port: 50001,
                    remote: "127.0.0.1".parse().unwrap(),
                    remote_port: 4317,
                    pid: Some(10),
                },
                RawConnection {
                    local: "192.168.1.42".parse().unwrap(),
                    local_port: 50002,
                    remote: "192.168.1.10".parse().unwrap(),
                    remote_port: 445,
                    pid: None,
                },
            ]),
            firewall: Ok(all_fw(true)),
            wsc_fw: Some(WscHealth::Good),
            wsc_av: Some(WscHealth::Good),
            defender: Ok(defender_raw()),
            bitlocker: Ok(vec![BitlockerVolume {
                mount: "C:".into(),
                system: true,
                state: BitlockerState::Protected,
            }]),
            secure_boot: Ok(SecureBootRaw {
                state: SecureBootState::Enabled,
                uefi: Some(true),
            }),
            tpm: Ok(TpmRaw {
                present: true,
                version: Some("2.0".into()),
            }),
            reads: Cell::new(0),
            log: RefCell::new(vec![]),
        }
    }
    fn note(&self, what: &'static str) {
        self.reads.set(self.reads.get() + 1);
        self.log.borrow_mut().push(what);
    }
}

impl Sources for &Fake {
    fn adapters(&self) -> Vec<NetAdapter> {
        self.note("adapters");
        self.adapters.clone()
    }
    fn route_ipv4(&self) -> Option<String> {
        self.route.clone()
    }
    fn network_profiles(&self, _: &[String]) -> Result<HashMap<String, ProfileKind>, SourceError> {
        self.profiles_result.clone().map(|_| self.profiles.clone())
    }
    fn listeners(&self) -> Result<Vec<PortObservation>, SourceError> {
        self.note("listeners");
        self.listeners.clone()
    }
    fn processes(&self) -> Vec<RawProcess> {
        self.processes.clone()
    }
    fn connections(&self) -> Result<Vec<RawConnection>, SourceError> {
        self.note("connections");
        self.connections.clone()
    }
    fn firewall(&self) -> Result<Vec<FirewallProfileRaw>, SourceError> {
        self.note("firewall");
        self.firewall.clone()
    }
    fn security_center(&self, provider: WscProvider) -> Option<WscHealth> {
        match provider {
            WscProvider::Firewall => self.wsc_fw,
            WscProvider::Antivirus => self.wsc_av,
        }
    }
    fn defender(&self) -> Result<DefenderRaw, SourceError> {
        self.note("defender");
        self.defender.clone()
    }
    fn bitlocker(&self) -> Result<Vec<BitlockerVolume>, SourceError> {
        self.note("bitlocker");
        self.bitlocker.clone()
    }
    fn secure_boot(&self) -> Result<SecureBootRaw, SourceError> {
        self.note("secure_boot");
        self.secure_boot.clone()
    }
    fn tpm(&self) -> Result<TpmRaw, SourceError> {
        self.note("tpm");
        self.tpm.clone()
    }
}

fn ctx() -> Context {
    Context {
        projects: vec![ProjectRef {
            id: "p1".into(),
            name: "LKR_Lab".into(),
            root: r"C:\Dev\Lab".into(),
            ports: vec![4317],
        }],
        worktrees: vec![],
    }
}

fn snap(fake: &Fake) -> NetworkSecuritySnapshot {
    Collector::new(fake).snapshot(NOW, true, &ctx(), &HashMap::new())
}

// ------------------------------------------------------------------ rede

#[test]
fn active_interface_gateway_dns_and_profile() {
    let s = snap(&Fake::healthy());
    assert_eq!(s.network.data.active_interface.as_deref(), Some("wifi"));
    assert_eq!(s.network.data.local_ipv4.as_deref(), Some("192.168.1.42"));
    assert_eq!(s.network.data.gateway.as_deref(), Some("192.168.1.1"));
    assert_eq!(s.network.data.dns, vec!["1.1.1.1", "8.8.8.8"]);
    assert_eq!(s.network.data.profile, Some(ProfileKind::Private));
    // loopback não é listado; link-local sem gateway nunca é a interface ativa.
    assert!(s
        .network
        .data
        .interfaces
        .iter()
        .all(|i| i.kind != "loopback"));
    let eth = s
        .network
        .data
        .interfaces
        .iter()
        .find(|i| i.name == "eth")
        .unwrap();
    assert!(!eth.active);
}

#[test]
fn gateway_prefers_ipv4_and_dns_is_unique_and_ordered() {
    let mut f = Fake::healthy();
    let wifi = f.adapters.iter_mut().find(|a| a.name == "wifi").unwrap();
    wifi.gateways = vec!["fe80::1".into(), "192.168.1.1".into()];
    wifi.dns = vec![
        "2804::2".into(),
        "187.60.148.2".into(),
        "2804::2".into(),
        "187.60.148.2".into(),
        "8.8.8.8".into(),
    ];
    let s = snap(&f);
    assert_eq!(s.network.data.gateway.as_deref(), Some("192.168.1.1"));
    assert_eq!(
        s.network.data.dns,
        vec!["187.60.148.2", "8.8.8.8", "2804::2"]
    );
}

#[test]
fn ipv6_only_gateway_is_still_reported() {
    let mut f = Fake::healthy();
    f.adapters
        .iter_mut()
        .find(|a| a.name == "wifi")
        .unwrap()
        .gateways = vec!["fe80::1".into()];
    assert_eq!(snap(&f).network.data.gateway.as_deref(), Some("fe80::1"));
}

#[test]
fn ipv4_and_ipv6_are_listed() {
    let s = snap(&Fake::healthy());
    let wifi = s.network.data.interfaces.iter().find(|i| i.active).unwrap();
    assert_eq!(wifi.ipv4, vec!["192.168.1.42"]);
    assert_eq!(wifi.ipv6, vec!["fe80::1"]);
    assert_eq!(wifi.prefix, Some(24));
    assert!(wifi.dhcp);
}

#[test]
fn lowest_metric_wins_among_routes() {
    let mut f = Fake::healthy();
    f.adapters.push(adapter(
        "vpn",
        AdapterKind::Tunnel,
        "10.8.0.2",
        Some("10.8.0.1"),
        5,
    ));
    f.route = None;
    let s = snap(&f);
    assert_eq!(s.network.data.active_interface.as_deref(), Some("vpn"));
}

#[test]
fn os_chosen_route_beats_metric_heuristics() {
    // Um adaptador virtual com gateway e métrica menor NÃO vence a rota que o SO realmente usa.
    let mut f = Fake::healthy();
    f.adapters.push(adapter(
        "vpn",
        AdapterKind::Tunnel,
        "26.1.2.3",
        Some("26.0.0.1"),
        5,
    ));
    let s = snap(&f);
    assert_eq!(s.network.data.active_interface.as_deref(), Some("wifi"));
}

#[test]
fn no_default_route_means_no_active_interface() {
    let mut f = Fake::healthy();
    f.adapters = vec![adapter("wifi", AdapterKind::Wifi, "192.168.1.42", None, 35)];
    f.route = None;
    let s = snap(&f);
    assert!(s.network.data.active_interface.is_none());
    assert!(s.network.reasons.iter().any(|r| r.contains("rota padrão")));
}

#[test]
fn public_ip_is_never_queried() {
    let s = snap(&Fake::healthy());
    assert!(!s.network.data.public_ip.queried);
    assert!(s.network.data.public_ip.note.contains("Não consultado"));
}

#[test]
fn network_profile_requires_elevation_is_reported_not_guessed() {
    let mut f = Fake::healthy();
    f.profiles_result = Err(SourceError::RequiresElevation("admin".into()));
    let s = snap(&f);
    assert_eq!(s.network.data.profile, None);
    assert!(s
        .network
        .sources
        .iter()
        .any(|n| n.id == "network_profile" && n.state == SourceState::RequiresElevation));
    // sem a categoria, o firewall avalia todos os perfis lidos (não presume "Público")
    assert_eq!(s.firewall.status, Health::Healthy);
    assert_eq!(s.firewall.data.active_profile, None);
}

#[test]
fn unknown_network_profile_is_partial_not_public() {
    let mut f = Fake::healthy();
    f.profiles.clear();
    let s = snap(&f);
    assert_eq!(s.network.data.profile, None);
    assert!(s
        .network
        .sources
        .iter()
        .any(|n| n.id == "network_profile" && n.state == SourceState::Partial));
}

// ------------------------------------------------------------------ exposição

#[test]
fn listener_scopes() {
    assert_eq!(classify_scope("127.0.0.1"), ListenerScope::Loopback);
    assert_eq!(classify_scope("::1"), ListenerScope::Loopback);
    assert_eq!(classify_scope("0.0.0.0"), ListenerScope::AllInterfaces);
    assert_eq!(classify_scope("::"), ListenerScope::AllInterfaces);
    assert_eq!(classify_scope("192.168.1.42"), ListenerScope::Specific);
}

#[test]
fn exposure_counts_and_wording() {
    let s = snap(&Fake::healthy());
    let c = &s.exposure.data.counts;
    assert_eq!(
        (
            c.total,
            c.loopback,
            c.specific,
            c.all_interfaces,
            c.unidentified
        ),
        (4, 1, 1, 2, 1)
    );
    let all = s
        .exposure
        .data
        .listeners
        .iter()
        .find(|l| l.port == 3000)
        .unwrap();
    assert_eq!(all.scope, ListenerScope::AllInterfaces);
    assert!(all.note.contains("Não significa exposição à internet"));
    // nenhum texto declara vulnerabilidade
    for l in &s.exposure.data.listeners {
        assert!(!l.note.to_lowercase().contains("vulner"));
    }
    // all_interfaces ordenadas primeiro
    assert_eq!(
        s.exposure.data.listeners[0].scope,
        ListenerScope::AllInterfaces
    );
}

#[test]
fn listener_is_attributed_to_pid_and_project() {
    let s = snap(&Fake::healthy());
    let l = s
        .exposure
        .data
        .listeners
        .iter()
        .find(|l| l.port == 4317)
        .unwrap();
    assert_eq!(l.pid, Some(10));
    assert_eq!(l.process_name.as_deref(), Some("node.exe"));
    assert_eq!(l.project_name.as_deref(), Some("LKR_Lab"));
    assert!(l.confidence >= Confidence::Medium);
    let other = s
        .exposure
        .data
        .listeners
        .iter()
        .find(|l| l.port == 5432)
        .unwrap();
    assert_eq!(other.project_name, None);
}

#[test]
fn unidentified_listener_stays_unidentified() {
    let s = snap(&Fake::healthy());
    let l = s
        .exposure
        .data
        .listeners
        .iter()
        .find(|l| l.port == 135)
        .unwrap();
    assert_eq!(l.pid, None);
    assert_eq!(l.project_name, None);
    assert_eq!(l.confidence, Confidence::Unknown);
    assert!(s
        .exposure
        .sources
        .iter()
        .any(|n| n.id == "listener_owners" && n.state == SourceState::Partial));
}

// ------------------------------------------------------------------ conexões

#[test]
fn connections_are_classified_and_filtered() {
    let s = snap(&Fake::healthy());
    let c = &s.connections.data;
    assert_eq!((c.total, c.remote, c.local, c.loopback), (3, 1, 1, 1));
    // remotas primeiro
    assert_eq!(c.items[0].scope, RemoteScope::Remote);
    assert_eq!(c.items[0].process_name.as_deref(), Some("vite.exe"));
    assert!(!c.truncated);
    assert!(c.notes.iter().any(|n| n.contains("Sem resolução reversa")));
}

#[test]
fn remote_scope_rules() {
    let p = |s: &str| remote_scope(&s.parse::<IpAddr>().unwrap());
    assert_eq!(p("127.0.0.1"), RemoteScope::Loopback);
    assert_eq!(p("::1"), RemoteScope::Loopback);
    assert_eq!(p("10.1.2.3"), RemoteScope::Local);
    assert_eq!(p("172.16.0.5"), RemoteScope::Local);
    assert_eq!(p("192.168.0.9"), RemoteScope::Local);
    assert_eq!(p("169.254.1.1"), RemoteScope::Local);
    assert_eq!(p("fd00::1"), RemoteScope::Local);
    assert_eq!(p("fe80::1"), RemoteScope::Local);
    assert_eq!(p("8.8.8.8"), RemoteScope::Remote);
    assert_eq!(p("2606:4700::1111"), RemoteScope::Remote);
}

#[test]
fn many_connections_are_truncated_but_counted() {
    let mut f = Fake::healthy();
    f.connections = Ok((0..(MAX_CONNECTIONS as u16 + 50))
        .map(|i| RawConnection {
            local: "192.168.1.42".parse().unwrap(),
            local_port: 40000 + i,
            remote: "93.184.216.34".parse().unwrap(),
            remote_port: 443,
            pid: None,
        })
        .collect());
    let s = snap(&f);
    assert_eq!(s.connections.data.total, MAX_CONNECTIONS as u32 + 50);
    assert_eq!(s.connections.data.items.len(), MAX_CONNECTIONS);
    assert!(s.connections.data.truncated);
}

#[test]
fn empty_connections_are_empty_not_unknown_error() {
    let mut f = Fake::healthy();
    f.connections = Ok(vec![]);
    let s = snap(&f);
    assert_eq!(s.connections.data.total, 0);
    assert!(s.connections.data.items.is_empty());
}

// ------------------------------------------------------------------ firewall

#[test]
fn firewall_enabled_on_all_profiles_is_healthy() {
    let s = snap(&Fake::healthy());
    assert_eq!(s.firewall.status, Health::Healthy);
    assert_eq!(s.firewall.data.active_profile, Some(ProfileKind::Private));
    assert_eq!(s.firewall.data.profiles.len(), 3);
    assert!(
        s.firewall
            .data
            .profiles
            .iter()
            .find(|p| p.kind == ProfileKind::Private)
            .unwrap()
            .active
    );
}

#[test]
fn profiles_are_domain_private_public() {
    let s = snap(&Fake::healthy());
    let labels: Vec<_> = s.firewall.data.profiles.iter().map(|p| p.label).collect();
    assert_eq!(labels, vec!["Domínio", "Privado", "Público"]);
}

#[test]
fn disabled_active_profile_without_other_firewall_is_critical() {
    let mut f = Fake::healthy();
    f.firewall = Ok(all_fw(false));
    f.wsc_fw = Some(WscHealth::Poor);
    let s = snap(&f);
    assert_eq!(s.firewall.status, Health::Critical);
    assert!(s.firewall.reasons[0].contains("Privado"));
}

#[test]
fn disabled_windows_firewall_with_other_firewall_good_is_not_critical() {
    let mut f = Fake::healthy();
    f.firewall = Ok(all_fw(false));
    f.wsc_fw = Some(WscHealth::Good);
    let s = snap(&f);
    assert_eq!(s.firewall.status, Health::Healthy);
    assert!(!s.firewall.data.notes.is_empty());
}

#[test]
fn inactive_profile_disabled_does_not_alert() {
    let mut f = Fake::healthy();
    f.firewall = Ok(vec![
        fw(ProfileKind::Domain, Some(false)),
        fw(ProfileKind::Private, Some(true)),
        fw(ProfileKind::Public, Some(false)),
    ]);
    let s = snap(&f);
    assert_eq!(s.firewall.status, Health::Healthy);
}

#[test]
fn default_inbound_allow_is_attention() {
    let mut f = Fake::healthy();
    let mut profiles = all_fw(true);
    profiles[1].default_inbound = Some(FirewallAction::Allow);
    f.firewall = Ok(profiles);
    assert_eq!(snap(&f).firewall.status, Health::Attention);
}

#[test]
fn unreadable_firewall_is_unknown_never_healthy() {
    let mut f = Fake::healthy();
    f.firewall = Ok(vec![
        fw(ProfileKind::Domain, None),
        fw(ProfileKind::Private, None),
        fw(ProfileKind::Public, None),
    ]);
    assert_eq!(snap(&f).firewall.status, Health::Unknown);
}

#[test]
fn unknown_active_profile_only_states_what_is_known() {
    let (status, _, _) = evaluate_firewall(&all_fw(true), None, Some(WscHealth::Good));
    assert_eq!(status, Health::Healthy);
    let (status, reasons, _) = evaluate_firewall(
        &[
            fw(ProfileKind::Public, Some(false)),
            fw(ProfileKind::Private, Some(true)),
        ],
        None,
        None,
    );
    assert_eq!(status, Health::Attention);
    assert!(reasons[0].contains("Público"));
}

#[test]
fn firewall_requires_elevation_is_reported() {
    let mut f = Fake::healthy();
    f.firewall = Err(SourceError::RequiresElevation("precisa de admin".into()));
    let s = snap(&f);
    assert_eq!(s.firewall.status, Health::Unknown);
    assert!(s
        .firewall
        .sources
        .iter()
        .any(|n| n.state == SourceState::RequiresElevation));
}

// ------------------------------------------------------------------ antivírus / Defender

#[test]
fn defender_active_and_signature_metadata() {
    let s = snap(&Fake::healthy());
    assert_eq!(s.antivirus.status, Health::Healthy);
    assert_eq!(s.antivirus.data.provider, AvProvider::Defender);
    let d = &s.antivirus.data.defender;
    assert_eq!(d.state, DefenderState::Active);
    assert_eq!(d.signature_version.as_deref(), Some("1.459.546.0"));
    assert_eq!(d.engine_version.as_deref(), Some("1.1.26080.3"));
    assert_eq!(d.signature_age_days, Some(1));
    // ameaças: não consultado, nunca "zero"
    assert_eq!(d.active_threats, None);
    assert!(s
        .antivirus
        .data
        .notes
        .iter()
        .any(|n| n.contains("não consultado")));
}

#[test]
fn old_signatures_are_attention() {
    let mut f = Fake::healthy();
    let mut d = defender_raw();
    d.signatures_updated_at = Some(NOW - 10 * DAY_MS);
    f.defender = Ok(d);
    let s = snap(&f);
    assert_eq!(s.antivirus.status, Health::Attention);
    assert!(s.antivirus.reasons[0].contains("10 dias"));
}

#[test]
fn defender_passive_with_third_party_is_not_a_problem() {
    let mut f = Fake::healthy();
    let mut d = defender_raw();
    d.third_party_av = Some(1);
    d.forced_passive = Some(true);
    d.signatures_updated_at = Some(NOW - 90 * DAY_MS); // assinatura velha do Defender passivo é irrelevante
    f.defender = Ok(d);
    let s = snap(&f);
    assert_eq!(s.antivirus.status, Health::Healthy);
    assert_eq!(s.antivirus.data.provider, AvProvider::ThirdParty);
    assert_eq!(s.antivirus.data.defender.state, DefenderState::Passive);
    assert!(s.antivirus.data.notes.iter().any(|n| n.contains("passivo")));
}

#[test]
fn defender_service_stopped_with_third_party_is_passive_not_critical() {
    let mut f = Fake::healthy();
    let mut d = defender_raw();
    d.service = Some(RawService {
        state: ServiceState::Stopped,
        start: StartType::Manual,
    });
    d.third_party_av = Some(1);
    f.defender = Ok(d);
    let s = snap(&f);
    assert_eq!(s.antivirus.data.defender.state, DefenderState::Passive);
    assert_ne!(s.antivirus.status, Health::Critical);
}

#[test]
fn third_party_with_poor_health_is_attention() {
    let mut f = Fake::healthy();
    let mut d = defender_raw();
    d.third_party_av = Some(1);
    d.forced_passive = Some(true);
    f.defender = Ok(d);
    f.wsc_av = Some(WscHealth::Poor);
    assert_eq!(snap(&f).antivirus.status, Health::Attention);
}

#[test]
fn third_party_without_health_info_is_unknown() {
    let mut f = Fake::healthy();
    let mut d = defender_raw();
    d.third_party_av = Some(1);
    d.forced_passive = Some(true);
    f.defender = Ok(d);
    f.wsc_av = None;
    assert_eq!(snap(&f).antivirus.status, Health::Unknown);
}

#[test]
fn defender_disabled_without_any_av_is_critical() {
    let mut f = Fake::healthy();
    let mut d = defender_raw();
    d.policy_disabled = Some(true);
    f.defender = Ok(d);
    f.wsc_av = Some(WscHealth::NotMonitored);
    let s = snap(&f);
    assert_eq!(s.antivirus.data.defender.state, DefenderState::Disabled);
    assert_eq!(s.antivirus.data.provider, AvProvider::None);
    assert_eq!(s.antivirus.status, Health::Critical);
}

#[test]
fn defender_disabled_but_security_center_good_is_not_critical() {
    let mut f = Fake::healthy();
    let mut d = defender_raw();
    d.disable_antivirus = Some(true);
    f.defender = Ok(d);
    f.wsc_av = Some(WscHealth::Good);
    assert_ne!(snap(&f).antivirus.status, Health::Critical);
}

#[test]
fn realtime_protection_off_without_other_av_is_disabled() {
    let mut f = Fake::healthy();
    let mut d = defender_raw();
    d.realtime_disabled = Some(true);
    f.defender = Ok(d);
    f.wsc_av = Some(WscHealth::Poor);
    let s = snap(&f);
    assert_eq!(s.antivirus.data.defender.realtime_protection, Some(false));
    assert_eq!(s.antivirus.status, Health::Critical);
}

#[test]
fn threat_present_is_critical() {
    let mut f = Fake::healthy();
    let mut d = defender_raw();
    d.active_threats = Some(2);
    f.defender = Ok(d);
    let s = snap(&f);
    assert_eq!(s.antivirus.status, Health::Critical);
    assert!(s.antivirus.reasons.iter().any(|r| r.contains("2 ameaça")));
}

#[test]
fn security_center_poor_with_defender_active_is_attention() {
    let mut f = Fake::healthy();
    f.wsc_av = Some(WscHealth::Poor);
    assert_eq!(snap(&f).antivirus.status, Health::Attention);
}

#[test]
fn defender_unreadable_and_no_security_center_is_unknown() {
    let mut f = Fake::healthy();
    f.defender = Err(SourceError::Unavailable("sem registro".into()));
    f.wsc_av = None;
    let s = snap(&f);
    assert_eq!(s.antivirus.status, Health::Unknown);
    assert!(s
        .antivirus
        .sources
        .iter()
        .any(|n| n.id == "defender" && n.state == SourceState::Unavailable));
}

#[test]
fn signature_age_ignores_future_timestamps() {
    assert_eq!(signature_age_days(Some(NOW + 1), NOW), None);
    assert_eq!(signature_age_days(None, NOW), None);
    assert_eq!(signature_age_days(Some(NOW - 3 * DAY_MS - 5), NOW), Some(3));
}

// ------------------------------------------------------------------ BitLocker / Secure Boot / TPM

fn vol(mount: &str, system: bool, state: BitlockerState) -> BitlockerVolume {
    BitlockerVolume {
        mount: mount.into(),
        system,
        state,
    }
}

#[test]
fn bitlocker_states() {
    assert_eq!(
        evaluate_encryption(&[vol("C:", true, BitlockerState::Protected)]).0,
        Health::Healthy
    );
    let (s, r) = evaluate_encryption(&[vol("C:", true, BitlockerState::Suspended)]);
    assert_eq!(s, Health::Attention);
    assert!(r[0].contains("suspenso"));
    assert_eq!(
        evaluate_encryption(&[vol("C:", true, BitlockerState::Off)]).0,
        Health::Attention
    );
    assert_eq!(
        evaluate_encryption(&[vol("C:", true, BitlockerState::Unknown)]).0,
        Health::Unknown
    );
    assert_eq!(evaluate_encryption(&[]).0, Health::Unknown);
}

#[test]
fn unprotected_data_volume_alone_does_not_alert() {
    let (s, _) = evaluate_encryption(&[
        vol("C:", true, BitlockerState::Protected),
        vol("D:", false, BitlockerState::Off),
    ]);
    assert_eq!(s, Health::Healthy);
}

#[test]
fn bitlocker_unavailable_is_unknown_and_requires_elevation() {
    let mut f = Fake::healthy();
    f.bitlocker = Err(SourceError::RequiresElevation("admin".into()));
    let s = snap(&f);
    assert_eq!(s.encryption.status, Health::Unknown);
    assert!(s
        .encryption
        .sources
        .iter()
        .any(|n| n.state == SourceState::RequiresElevation));
    assert!(s.encryption.data.volumes.is_empty());
}

#[test]
fn secure_boot_states() {
    let on = |state| {
        evaluate_secure_boot(&SecureBootRaw {
            state,
            uefi: Some(true),
        })
    };
    assert_eq!(on(SecureBootState::Enabled).0, Health::Healthy);
    assert_eq!(on(SecureBootState::Disabled).0, Health::Attention);
    assert_eq!(on(SecureBootState::Unsupported).0, Health::Unknown);
    assert_eq!(on(SecureBootState::Unavailable).0, Health::Unknown);
}

#[test]
fn tpm_states() {
    assert_eq!(
        evaluate_tpm(&TpmRaw {
            present: true,
            version: Some("2.0".into())
        })
        .0,
        Health::Healthy
    );
    assert_eq!(
        evaluate_tpm(&TpmRaw {
            present: false,
            version: None
        })
        .0,
        Health::Attention
    );
    let mut f = Fake::healthy();
    f.tpm = Err(SourceError::Unavailable("TBS".into()));
    let s = snap(&f);
    assert_eq!(s.tpm.status, Health::Unknown);
    assert_eq!(s.tpm.data.present, None);
}

// ------------------------------------------------------------------ geral

#[test]
fn healthy_machine_is_healthy_with_all_domains_evaluated() {
    let s = snap(&Fake::healthy());
    assert_eq!(s.overall.status, Health::Healthy);
    assert_eq!((s.overall.evaluated, s.overall.rateable), (5, 5));
    assert!(s.overall.reasons.is_empty());
}

#[test]
fn overall_is_worst_known_domain_with_reasons() {
    let mut f = Fake::healthy();
    f.secure_boot = Ok(SecureBootRaw {
        state: SecureBootState::Disabled,
        uefi: Some(true),
    });
    let s = snap(&f);
    assert_eq!(s.overall.status, Health::Attention);
    assert_eq!(s.overall.reasons[0].domain, "secure_boot");
    f.firewall = Ok(all_fw(false));
    f.wsc_fw = None;
    let s = snap(&f);
    assert_eq!(s.overall.status, Health::Critical);
}

#[test]
fn unknown_domains_do_not_lift_the_overall_to_healthy() {
    let mut f = Fake::healthy();
    f.firewall = Err(SourceError::Unavailable("x".into()));
    f.defender = Err(SourceError::Unavailable("x".into()));
    f.wsc_av = None;
    f.bitlocker = Err(SourceError::RequiresElevation("x".into()));
    f.secure_boot = Err(SourceError::Unavailable("x".into()));
    f.tpm = Err(SourceError::Unavailable("x".into()));
    let s = snap(&f);
    assert_eq!(s.overall.status, Health::Unknown);
    assert_eq!(s.overall.evaluated, 0);
    assert_eq!(s.overall.rateable, 5);
}

#[test]
fn partial_snapshot_one_failure_does_not_break_others() {
    let mut f = Fake::healthy();
    f.listeners = Err(SourceError::Unavailable("netstat".into()));
    f.connections = Err(SourceError::Unavailable("netstat".into()));
    f.tpm = Err(SourceError::Unavailable("tbs".into()));
    let s = snap(&f);
    assert_eq!(s.exposure.status, Health::Unknown);
    assert!(s.exposure.data.listeners.is_empty());
    assert_eq!(s.connections.status, Health::Unknown);
    assert_eq!(s.tpm.status, Health::Unknown);
    assert_eq!(s.firewall.status, Health::Healthy);
    assert_eq!(s.antivirus.status, Health::Healthy);
    assert_eq!(s.overall.evaluated, 4);
    assert!(s
        .capabilities
        .iter()
        .any(|n| n.id == "listeners" && n.state == SourceState::Unavailable));
}

#[test]
fn informational_domains_are_not_rated() {
    let s = snap(&Fake::healthy());
    assert!(!s.network.rated && !s.exposure.rated && !s.connections.rated);
    assert!(
        s.firewall.rated
            && s.antivirus.rated
            && s.encryption.rated
            && s.secure_boot.rated
            && s.tpm.rated
    );
}

#[test]
fn exposure_never_changes_overall_state() {
    let mut f = Fake::healthy();
    f.listeners = Ok((1000..1200).map(|p| port(p, "0.0.0.0", None)).collect());
    let s = snap(&f);
    assert_eq!(s.overall.status, Health::Healthy);
}

// ------------------------------------------------------------------ cache

#[test]
fn cache_by_domain_and_force() {
    let f = Fake::healthy();
    let c = Collector::new(&f);
    c.snapshot(NOW, false, &ctx(), &HashMap::new());
    let first = f.reads.get();
    assert!(first > 0);
    c.snapshot(NOW + 1_000, false, &ctx(), &HashMap::new());
    assert_eq!(f.reads.get(), first, "dentro do TTL nada é relido");
    c.snapshot(NOW + 1_000, true, &ctx(), &HashMap::new());
    assert_eq!(f.reads.get(), first * 2, "force relê tudo");
}

#[test]
fn only_expired_domains_are_reread() {
    let f = Fake::healthy();
    let c = Collector::new(&f);
    c.snapshot(NOW, false, &ctx(), &HashMap::new());
    f.log.borrow_mut().clear();
    c.snapshot(NOW + TTL_NETWORK_MS + 1, false, &ctx(), &HashMap::new());
    let log = f.log.borrow().clone();
    assert!(
        log.contains(&"adapters") && log.contains(&"listeners") && log.contains(&"connections")
    );
    assert!(!log.contains(&"tpm") && !log.contains(&"secure_boot") && !log.contains(&"bitlocker"));
    assert!(!log.contains(&"defender"), "antivírus tem TTL maior");
}

#[test]
fn clock_going_backwards_forces_refresh() {
    let f = Fake::healthy();
    let c = Collector::new(&f);
    c.snapshot(NOW, false, &ctx(), &HashMap::new());
    let before = f.reads.get();
    c.snapshot(NOW - 10_000, false, &ctx(), &HashMap::new());
    assert!(f.reads.get() > before);
}

#[test]
fn section_carries_timestamp_and_ttl() {
    let s = snap(&Fake::healthy());
    assert_eq!(s.captured_at, NOW);
    assert_eq!(s.firewall.checked_at, NOW);
    assert_eq!(s.firewall.ttl_ms, TTL_FIREWALL_MS);
    assert_eq!(s.tpm.ttl_ms, TTL_BOOT_MS);
}

#[test]
fn firewall_follows_active_profile_changes_inside_the_ttl() {
    let mut f = Fake::healthy();
    f.firewall = Ok(vec![
        fw(ProfileKind::Domain, Some(true)),
        fw(ProfileKind::Private, Some(true)),
        fw(ProfileKind::Public, Some(false)),
    ]);
    f.wsc_fw = None;
    let c = Collector::new(&f);
    let a = c.snapshot(NOW, false, &ctx(), &HashMap::new());
    assert_eq!(a.firewall.status, Health::Healthy);
    // troca para uma rede pública dentro do TTL do firewall (as interfaces expiram antes)
    let mut g = Fake::healthy();
    g.firewall = f.firewall.clone();
    g.wsc_fw = None;
    g.profiles.insert("{wifi}".into(), ProfileKind::Public);
    let c2 = Collector::new(&g);
    assert_eq!(
        c2.snapshot(NOW, false, &ctx(), &HashMap::new())
            .firewall
            .status,
        Health::Critical
    );
}

// ------------------------------------------------------------------ serialização e privacidade

#[test]
fn serializes_camel_case_and_snake_case_enums() {
    let s = snap(&Fake::healthy());
    let json = serde_json::to_value(&s).unwrap();
    assert!(json["overall"]["evaluated"].is_number());
    assert_eq!(json["exposure"]["listeners"][0]["scope"], "all_interfaces");
    assert_eq!(json["secureBoot"]["state"], "enabled");
    assert_eq!(json["firewall"]["profiles"][0]["kind"], "domain");
    assert_eq!(json["antivirus"]["securityCenter"], "good");
    assert_eq!(json["network"]["publicIp"]["queried"], false);
    assert!(json["antivirus"]["defender"]["activeThreats"].is_null());
}

#[test]
fn snapshot_contains_no_secret_fields() {
    let s = snap(&Fake::healthy());
    let json = serde_json::to_string(&s).unwrap().to_lowercase();
    for banned in [
        "recovery",
        "password",
        "token",
        "secret",
        "privatekey",
        "psk",
        "passphrase",
    ] {
        assert!(!json.contains(banned), "campo proibido: {banned}");
    }
}

#[test]
fn no_machine_state_in_portable_workspace_or_sync() {
    for file in ["portable.rs", "sync.rs", "snapshot.rs"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("network_security"),
            "{file} não pode tocar o estado de rede/segurança"
        );
        assert!(
            !text.contains("security_native"),
            "{file} não pode tocar o estado de rede/segurança"
        );
    }
}

// ------------------------------------------------------------------ passividade

#[test]
fn sources_never_mutate_the_machine() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let banned = [
        "regsetvalue",
        "regcreatekey",
        "regdeletekey",
        "regdeletevalue",
        "command::new",
        "powershell",
        "netsh",
        "set-mppreference",
        "start-mpscan",
        "remove-mpthreat",
        "manage-bde",
        "enable-bitlocker",
        "terminateprocess",
        "taskkill",
        "controlservice",
        "startservice",
        "setadaptersettings",
        "setipforwardentry",
        "createipforwardentry",
        "deleteipforwardentry",
        "setunicastipaddressentry",
        "dnsflush",
        "shellexecute",
        "tokio::net",
        "reqwest",
        "std::net::tcpstream",
        "connect(",
    ];
    for file in ["network_security.rs", "security_native.rs"] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        let code: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        for word in banned {
            assert!(!code.contains(word), "{file} usa API proibida: {word}");
        }
    }
}

#[test]
fn registry_reads_are_open_for_read_only() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("security_native.rs"),
    )
    .unwrap();
    assert!(
        !text.contains("KEY_WRITE")
            && !text.contains("KEY_SET_VALUE")
            && !text.contains("KEY_ALL_ACCESS")
    );
}

// ------------------------------------------------------------------ leitura real (somente leitura)

/// `cargo test -p hub-core --test network_security -- --ignored --nocapture live_snapshot`
#[test]
#[ignore = "lê a máquina real; imprime só estados agregados"]
fn live_snapshot() {
    let collector = Collector::new(LiveSources);
    let started = std::time::Instant::now();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let s = collector.snapshot(now, true, &Context::default(), &HashMap::new());
    println!("elapsed_ms={}", started.elapsed().as_millis());
    println!(
        "overall={:?} evaluated={}/{}",
        s.overall.status, s.overall.evaluated, s.overall.rateable
    );
    println!(
        "network active={:?} profile={:?} dns={} gateway={}",
        s.network.data.active_interface,
        s.network.data.profile,
        s.network.data.dns.len(),
        s.network.data.gateway.is_some()
    );
    println!("exposure counts={:?}", s.exposure.data.counts);
    println!(
        "connections total={} remote={} local={} loopback={}",
        s.connections.data.total,
        s.connections.data.remote,
        s.connections.data.local,
        s.connections.data.loopback
    );
    println!(
        "firewall={:?} active={:?} reasons={:?}",
        s.firewall.status, s.firewall.data.active_profile, s.firewall.reasons
    );
    for p in &s.firewall.data.profiles {
        println!("  fw {:?} enabled={:?}", p.kind, p.enabled);
    }
    println!(
        "antivirus={:?} provider={:?} wsc={:?} defender={:?} sigAge={:?} thirdParty={:?}",
        s.antivirus.status,
        s.antivirus.data.provider,
        s.antivirus.data.security_center,
        s.antivirus.data.defender.state,
        s.antivirus.data.defender.signature_age_days,
        s.antivirus.data.third_party_count
    );
    println!(
        "encryption={:?} reasons={:?}",
        s.encryption.status, s.encryption.reasons
    );
    println!(
        "secureBoot={:?} {:?}",
        s.secure_boot.status, s.secure_boot.data.state
    );
    println!(
        "tpm={:?} present={:?} version={:?}",
        s.tpm.status, s.tpm.data.present, s.tpm.data.version
    );
    for n in &s.capabilities {
        println!("  source {} {:?} {:?}", n.id, n.state, n.reason);
    }
}
