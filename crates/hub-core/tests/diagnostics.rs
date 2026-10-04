//! Block 09 — Deterministic Diagnostic Engine. Fatos sintéticos: nenhum teste depende da máquina real.
use hub_core::diagnostics::*;
use hub_core::health::HealthStatus;
use hub_core::network_security::{AvProvider, DefenderState, ProfileKind, WscHealth};
use hub_core::supervisor::RunState;
use hub_core::windows_health::{Health, ServiceState, SignalKind, StartType};
use std::collections::HashMap;

const NOW: i64 = 1_800_000_000_000;
const GIB: u64 = 1024 * 1024 * 1024;
const MIN: i64 = 60_000;

fn volume(mount: &str, total_gib: f64, available_gib: f64) -> VolumeFact {
    VolumeFact {
        mount: mount.into(),
        total: (total_gib * GIB as f64) as u64,
        available: (available_gib * GIB as f64) as u64,
    }
}

fn machine(volumes: Vec<VolumeFact>) -> MachineFacts {
    MachineFacts {
        volumes: Some(volumes),
        load_ready: true,
        cpu_percent: 12.0,
        memory_percent: 55.0,
        memory_available: 8 * GIB,
        commit_percent: Some(60.0),
        alerts: vec![],
    }
}

fn healthy_facts() -> Facts {
    Facts {
        machine: Some(machine(vec![
            volume("C:", 500.0, 200.0),
            volume("D:", 1000.0, 600.0),
        ])),
        windows: WindowsFacts {
            restart_pending: Some(false),
            services: Some(vec![ServiceFact {
                id: "EventLog".into(),
                label: "Log de Eventos".into(),
                state: ServiceState::Running,
                start: StartType::Automatic,
                health: Health::Healthy,
                reason: None,
            }]),
            devices: Some(vec![]),
            events: Some(vec![]),
            update_failures_7d: Some(0),
            volumes: Some(vec![WinVolumeFact {
                mount: "C:".into(),
                dirty: Some(false),
                read_only: Some(false),
                status: Health::Healthy,
                reasons: vec![],
            }]),
        },
        security: SecurityFacts {
            firewall: Some(FirewallFact {
                active_profile: Some(ProfileKind::Private),
                active_enabled: Some(true),
                security_center: Some(WscHealth::Good),
            }),
            antivirus: Some(AntivirusFact {
                provider: AvProvider::Defender,
                defender: DefenderState::Active,
                security_center: Some(WscHealth::Good),
                active_threats: None,
                signature_age_days: Some(0),
                third_party: Some(0),
            }),
        },
        network: NetworkFacts {
            all_interface_listeners: Some(vec![]),
        },
        runtime: Some(RuntimeFacts {
            ports_available: true,
            runs: vec![],
            declared_ports: vec![],
            owners: vec![],
            project_names: HashMap::from([("p1".to_string(), "LKR_Lab".to_string())]),
            self_pid: 1,
        }),
        statuses: vec![],
    }
}

fn eval(facts: &Facts) -> Evaluation {
    evaluate(NOW, facts, &Previous::new())
}

fn rule<'a>(e: &'a Evaluation, id: &str) -> Vec<&'a Finding> {
    e.findings.iter().filter(|f| f.rule_id == id).collect()
}

fn one<'a>(e: &'a Evaluation, id: &str) -> &'a Finding {
    let found = rule(e, id);
    assert_eq!(
        found.len(),
        1,
        "esperava 1 finding de {id}, vieram {}: {:?}",
        found.len(),
        e.findings
            .iter()
            .map(|f| &f.fingerprint)
            .collect::<Vec<_>>()
    );
    found[0]
}

// ------------------------------------------------------------------ máquina saudável / unknown

#[test]
fn no_findings_on_a_healthy_machine() {
    let e = eval(&healthy_facts());
    assert!(e.findings.is_empty(), "{:?}", e.findings);
    for source in [
        SRC_DISK,
        SRC_CPU,
        SRC_MEMORY,
        SRC_WIN_EVENTS,
        SRC_FIREWALL,
        SRC_ANTIVIRUS,
        SRC_EXPOSURE,
        SRC_RUNS,
    ] {
        assert!(
            e.evaluated.contains(source),
            "{source} deveria estar avaliada"
        );
    }
}

#[test]
fn unknown_does_not_become_an_alert_and_is_not_evaluated() {
    let e = eval(&Facts::default());
    assert!(e.findings.is_empty());
    assert!(
        e.evaluated.is_empty(),
        "fonte sem dado não é avaliada (nem resolve nada)"
    );
}

#[test]
fn missing_sources_never_create_findings() {
    // BitLocker, categoria da rede e afins nem existem nos fatos: nada de avalanche por elevação.
    let mut facts = healthy_facts();
    facts.security = SecurityFacts::default();
    facts.windows.volumes = None;
    assert!(eval(&facts).findings.is_empty());
}

// ------------------------------------------------------------------ disco

#[test]
fn disk_attention_needs_percent_and_absolute() {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 500.0, 40.0)])); // 8%, 40 GiB → só a porcentagem
    assert!(rule(&eval(&facts), "machine.disk.low_space").is_empty());
    facts.machine = Some(machine(vec![volume("C:", 2000.0, 100.0)])); // 5%, 100 GiB → só a porcentagem
    assert!(rule(&eval(&facts), "machine.disk.low_space").is_empty());
    facts.machine = Some(machine(vec![volume("C:", 500.0, 15.0)])); // 3%, 15 GiB: percentual crítico, absoluto só atenção
    let f = eval(&facts);
    let f = one(&f, "machine.disk.low_space");
    assert_eq!(f.severity, Severity::Attention);
    assert_eq!(f.confidence, Confidence::High);
    assert_eq!(f.resource, "C:");
}

#[test]
fn disk_attention_example() {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 200.0, 18.0)])); // 9%, 18 GiB
    let e = eval(&facts);
    let f = one(&e, "machine.disk.low_space");
    assert_eq!(f.severity, Severity::Attention);
    assert!(f.recommended_next_step.contains("C:"));
    assert!(f
        .evidence
        .iter()
        .any(|ev| ev.label == "Livre" && ev.value.contains("9.0%")));
}

#[test]
fn disk_critical_needs_both_limits() {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 200.0, 4.0)])); // 2%, 4 GiB
    let e = eval(&facts);
    assert_eq!(
        one(&e, "machine.disk.low_space").severity,
        Severity::Critical
    );
    facts.machine = Some(machine(vec![volume("C:", 60.0, 2.5)])); // 4.2%, 2.5 GiB
    assert_eq!(
        one(&eval(&facts), "machine.disk.low_space").severity,
        Severity::Critical
    );
}

#[test]
fn small_volumes_are_covered_separately() {
    let mut facts = healthy_facts();
    // 8 GiB com 0,6 GiB livres (7,5%): atenção; com 0,2 GiB (2,5%): crítico. 20 GiB nunca existiria nele.
    facts.machine = Some(machine(vec![volume("E:", 8.0, 0.6)]));
    assert_eq!(
        one(&eval(&facts), "machine.disk.low_space").severity,
        Severity::Attention
    );
    facts.machine = Some(machine(vec![volume("E:", 8.0, 0.2)]));
    assert_eq!(
        one(&eval(&facts), "machine.disk.low_space").severity,
        Severity::Critical
    );
    // Menor que 1 GiB (recuperação/EFI) não tem o que liberar: não é avaliado.
    facts.machine = Some(machine(vec![volume("R:", 0.5, 0.01)]));
    assert!(rule(&eval(&facts), "machine.disk.low_space").is_empty());
}

#[test]
fn disk_hysteresis_keeps_the_alert_until_there_is_margin() {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 200.0, 20.4)])); // 10.2%, 20.4 GiB: logo acima do limite
    let fp = fingerprint("machine.disk.low_space", "C:");
    assert!(
        rule(&eval(&facts), "machine.disk.low_space").is_empty(),
        "sem histórico não alerta"
    );
    let previous: Previous = HashMap::from([(fp, Severity::Attention)]);
    let e = evaluate(NOW, &facts, &previous);
    assert_eq!(
        one(&e, "machine.disk.low_space").severity,
        Severity::Attention,
        "já estava em alerta: segura"
    );
    facts.machine = Some(machine(vec![volume("C:", 200.0, 30.0)])); // 15%, 30 GiB
    let e = evaluate(
        NOW,
        &facts,
        &HashMap::from([(
            fingerprint("machine.disk.low_space", "C:"),
            Severity::Attention,
        )]),
    );
    assert!(
        rule(&e, "machine.disk.low_space").is_empty(),
        "com folga o alerta sai"
    );
}

#[test]
fn each_volume_has_its_own_fingerprint() {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![
        volume("C:", 200.0, 4.0),
        volume("D:", 200.0, 4.0),
    ]));
    let e = eval(&facts);
    let fps: Vec<_> = rule(&e, "machine.disk.low_space")
        .iter()
        .map(|f| f.fingerprint.clone())
        .collect();
    assert_eq!(fps.len(), 2);
    assert_ne!(fps[0], fps[1]);
}

// ------------------------------------------------------------------ CPU / memória / temperatura

fn alert_fact(source: &str, severity: HealthStatus, resource: Option<&str>) -> MachineAlertFact {
    MachineAlertFact {
        source: source.into(),
        severity,
        title: format!("alerta {source}"),
        detail: "detalhe".into(),
        resource: resource.map(String::from),
    }
}

#[test]
fn single_cpu_spike_is_not_an_alert() {
    let mut facts = healthy_facts();
    let m = facts.machine.as_mut().unwrap();
    m.cpu_percent = 100.0; // pico isolado: a regra sustentada de saúde não gerou alerta
    assert!(eval(&facts).findings.is_empty());
}

#[test]
fn sustained_cpu_pressure_alerts_with_confidence() {
    let mut facts = healthy_facts();
    facts.machine.as_mut().unwrap().alerts = vec![alert_fact("cpu", HealthStatus::Attention, None)];
    let e = eval(&facts);
    let f = one(&e, "machine.cpu.sustained_pressure");
    assert_eq!(
        (f.severity, f.confidence),
        (Severity::Attention, Confidence::Medium)
    );
    facts.machine.as_mut().unwrap().alerts = vec![alert_fact("cpu", HealthStatus::Critical, None)];
    let e = eval(&facts);
    assert_eq!(
        (
            one(&e, "machine.cpu.sustained_pressure").severity,
            one(&e, "machine.cpu.sustained_pressure").confidence
        ),
        (Severity::Critical, Confidence::High)
    );
}

#[test]
fn transient_memory_is_not_an_alert_even_at_high_percent() {
    let mut facts = healthy_facts();
    let m = facts.machine.as_mut().unwrap();
    m.memory_percent = 97.0;
    m.memory_available = GIB / 2;
    assert!(eval(&facts).findings.is_empty());
}

#[test]
fn memory_pressure_is_attention_and_critical_needs_commit_pressure() {
    let mut facts = healthy_facts();
    facts.machine.as_mut().unwrap().alerts =
        vec![alert_fact("memory", HealthStatus::Attention, None)];
    assert_eq!(
        one(&eval(&facts), "machine.memory.sustained_pressure").severity,
        Severity::Attention
    );
    // Crítico da regra de saúde, mas o commit está folgado (RAM é cache): fica Atenção.
    facts.machine.as_mut().unwrap().alerts =
        vec![alert_fact("memory", HealthStatus::Critical, None)];
    let e = eval(&facts);
    assert_eq!(
        one(&e, "machine.memory.sustained_pressure").severity,
        Severity::Attention
    );
    assert_eq!(
        one(&e, "machine.memory.sustained_pressure").confidence,
        Confidence::Medium
    );
    facts.machine.as_mut().unwrap().commit_percent = Some(93.0);
    let e = eval(&facts);
    assert_eq!(
        one(&e, "machine.memory.sustained_pressure").severity,
        Severity::Critical
    );
    assert!(one(&e, "machine.memory.sustained_pressure")
        .evidence
        .iter()
        .any(|ev| ev.label.contains("Commit")));
    // Sem a medida de commit, nunca crítico.
    facts.machine.as_mut().unwrap().commit_percent = None;
    assert_eq!(
        one(&eval(&facts), "machine.memory.sustained_pressure").severity,
        Severity::Attention
    );
}

#[test]
fn load_is_not_evaluated_before_the_first_complete_sample() {
    let mut facts = healthy_facts();
    let m = facts.machine.as_mut().unwrap();
    m.load_ready = false;
    m.alerts = vec![alert_fact("cpu", HealthStatus::Critical, None)];
    let e = eval(&facts);
    assert!(rule(&e, "machine.cpu.sustained_pressure").is_empty());
    assert!(!e.evaluated.contains(SRC_CPU));
}

#[test]
fn thermal_alerts_use_the_known_limit_and_sensor_identity() {
    let mut facts = healthy_facts();
    facts.machine.as_mut().unwrap().alerts = vec![
        alert_fact("temperature", HealthStatus::Critical, Some("GPU - NVIDIA")),
        alert_fact("temperature", HealthStatus::Attention, Some("NVMe - SSD")),
    ];
    let e = eval(&facts);
    let thermal = rule(&e, "machine.thermal.over_limit");
    assert_eq!(thermal.len(), 2);
    assert!(thermal
        .iter()
        .any(|f| f.resource == "GPU - NVIDIA" && f.severity == Severity::Critical));
    assert!(thermal.iter().all(|f| f.confidence == Confidence::High));
}

#[test]
fn no_temperature_means_no_thermal_finding() {
    // Sensor sem limite conhecido nunca chega como alerta de saúde.
    assert!(rule(&eval(&healthy_facts()), "machine.thermal.over_limit").is_empty());
}

// ------------------------------------------------------------------ Windows

#[test]
fn pending_reboot() {
    let mut facts = healthy_facts();
    facts.windows.restart_pending = Some(true);
    let e = eval(&facts);
    let f = one(&e, "windows.reboot.pending");
    assert_eq!(f.severity, Severity::Attention);
    assert!(f
        .recommended_next_step
        .contains("Nada é reiniciado automaticamente"));
    // Desconhecido (nenhuma fonte respondeu) não vira alerta.
    facts.windows.restart_pending = None;
    assert!(rule(&eval(&facts), "windows.reboot.pending").is_empty());
}

#[test]
fn critical_service_down() {
    let mut facts = healthy_facts();
    facts.windows.services = Some(vec![
        ServiceFact {
            id: "EventLog".into(),
            label: "Log de Eventos".into(),
            state: ServiceState::Stopped,
            start: StartType::Automatic,
            health: Health::Critical,
            reason: Some("parado".into()),
        },
        ServiceFact {
            id: "BITS".into(),
            label: "BITS".into(),
            state: ServiceState::Stopped,
            start: StartType::Manual,
            health: Health::Healthy,
            reason: None,
        },
        ServiceFact {
            id: "wuauserv".into(),
            label: "Windows Update".into(),
            state: ServiceState::Stopped,
            start: StartType::Manual,
            health: Health::Unknown,
            reason: None,
        },
    ]);
    let e = eval(&facts);
    let f = one(&e, "windows.service.not_running");
    assert_eq!(
        (f.severity, f.resource.as_str()),
        (Severity::Critical, "EventLog")
    );
}

#[test]
fn device_problem() {
    let mut facts = healthy_facts();
    facts.windows.devices = Some(vec![DeviceFact {
        name: "Adaptador X".into(),
        class: Some("Net".into()),
        manufacturer: None,
        problem_code: 43,
        problem: "O Windows parou o dispositivo".into(),
    }]);
    let e = eval(&facts);
    let f = one(&e, "windows.device.problem");
    assert_eq!(f.severity, Severity::Attention);
    assert!(f
        .evidence
        .iter()
        .any(|ev| ev.label == "Código" && ev.value == "43"));
}

fn event(kind: SignalKind, c24: u32, c7: u32) -> EventFact {
    EventFact {
        kind,
        label: format!("{kind:?}"),
        count_24h: c24,
        count_7d: c7,
    }
}

#[test]
fn bugcheck_recent_is_critical_and_old_is_attention() {
    let mut facts = healthy_facts();
    facts.windows.events = Some(vec![event(SignalKind::Bugcheck, 1, 1)]);
    let e = eval(&facts);
    let f = one(&e, "windows.event.bugcheck");
    assert_eq!(
        (f.severity, f.confidence),
        (Severity::Critical, Confidence::High)
    );
    assert!(f.diagnostic_action.is_some());
    facts.windows.events = Some(vec![event(SignalKind::Bugcheck, 0, 2)]);
    assert_eq!(
        one(&eval(&facts), "windows.event.bugcheck").severity,
        Severity::Attention
    );
}

#[test]
fn unexpected_shutdown_recent_is_attention_and_old_is_info() {
    let mut facts = healthy_facts();
    facts.windows.events = Some(vec![event(SignalKind::UnexpectedShutdown, 1, 1)]);
    assert_eq!(
        one(&eval(&facts), "windows.event.unexpected_shutdown").severity,
        Severity::Attention
    );
    facts.windows.events = Some(vec![event(SignalKind::UnexpectedShutdown, 0, 1)]);
    assert_eq!(
        one(&eval(&facts), "windows.event.unexpected_shutdown").severity,
        Severity::Info
    );
}

#[test]
fn event_noise_never_becomes_critical() {
    let mut facts = healthy_facts();
    facts.windows.events = Some(vec![
        event(SignalKind::ApplicationCrash, 9, 20),
        event(SignalKind::ApplicationHang, 4, 9),
        event(SignalKind::ServiceFailure, 2, 2), // abaixo do limite
        event(SignalKind::UpdateFailure, 3, 3),  // coberto pela regra de update
    ]);
    assert!(eval(&facts).findings.is_empty());
    facts.windows.events = Some(vec![event(SignalKind::ServiceFailure, 3, 3)]);
    assert_eq!(
        one(&eval(&facts), "windows.event.service_failures").severity,
        Severity::Attention
    );
}

#[test]
fn storage_and_filesystem_events() {
    let mut facts = healthy_facts();
    facts.windows.events = Some(vec![
        event(SignalKind::StorageError, 1, 1),
        event(SignalKind::FilesystemError, 0, 3),
    ]);
    let e = eval(&facts);
    assert_eq!(
        one(&e, "windows.event.storage_error").severity,
        Severity::Attention
    );
    assert_eq!(
        one(&e, "windows.event.filesystem_error").severity,
        Severity::Info
    );
}

#[test]
fn update_failure_must_repeat() {
    let mut facts = healthy_facts();
    facts.windows.update_failures_7d = Some(1);
    assert!(rule(&eval(&facts), "windows.update.repeated_failure").is_empty());
    facts.windows.update_failures_7d = Some(2);
    let e = eval(&facts);
    let f = one(&e, "windows.update.repeated_failure");
    assert_eq!(f.severity, Severity::Attention);
    assert_eq!(f.diagnostic_action.as_ref().unwrap().id, "dism_checkhealth");
}

#[test]
fn confirmed_volume_problem_offers_a_read_only_scan_for_that_volume() {
    let mut facts = healthy_facts();
    facts.windows.volumes = Some(vec![WinVolumeFact {
        mount: "D:".into(),
        dirty: Some(true),
        read_only: Some(false),
        status: Health::Attention,
        reasons: vec!["Marcado como sujo".into()],
    }]);
    let e = eval(&facts);
    let f = one(&e, "windows.volume.problem");
    assert_eq!(f.resource, "D:");
    let action = f.diagnostic_action.as_ref().unwrap();
    assert_eq!(
        (action.id.as_str(), action.target.as_deref()),
        ("chkdsk_scan", Some("D:"))
    );
    // Volume cujo "sujo" é ilegível sem administrador não alerta.
    facts.windows.volumes = Some(vec![WinVolumeFact {
        mount: "C:".into(),
        dirty: None,
        read_only: Some(false),
        status: Health::Unknown,
        reasons: vec![],
    }]);
    assert!(rule(&eval(&facts), "windows.volume.problem").is_empty());
}

// ------------------------------------------------------------------ segurança

#[test]
fn firewall_active_profile_disabled() {
    let mut facts = healthy_facts();
    facts.security.firewall = Some(FirewallFact {
        active_profile: Some(ProfileKind::Public),
        active_enabled: Some(false),
        security_center: Some(WscHealth::Poor),
    });
    let e = eval(&facts);
    let f = one(&e, "security.firewall.active_profile_disabled");
    assert_eq!(
        (f.severity, f.confidence),
        (Severity::Attention, Confidence::High)
    );
    assert_eq!(f.resource, "Público");
}

#[test]
fn firewall_rules_do_not_fire_without_evidence() {
    let mut facts = healthy_facts();
    // Outro firewall saudável informado pelo Security Center.
    facts.security.firewall = Some(FirewallFact {
        active_profile: Some(ProfileKind::Private),
        active_enabled: Some(false),
        security_center: Some(WscHealth::Good),
    });
    assert!(eval(&facts).findings.is_empty());
    // Perfil ativo desconhecido (categoria da rede exige administrador): nada a afirmar.
    facts.security.firewall = Some(FirewallFact {
        active_profile: None,
        active_enabled: None,
        security_center: None,
    });
    assert!(eval(&facts).findings.is_empty());
}

#[test]
fn no_active_antivirus_is_critical_only_with_strong_evidence() {
    let mut facts = healthy_facts();
    facts.security.antivirus = Some(AntivirusFact {
        provider: AvProvider::None,
        defender: DefenderState::Disabled,
        security_center: Some(WscHealth::NotMonitored),
        active_threats: None,
        signature_age_days: None,
        third_party: Some(0),
    });
    let e = eval(&facts);
    let f = one(&e, "security.no_active_antivirus");
    assert_eq!(
        (f.severity, f.confidence),
        (Severity::Critical, Confidence::High)
    );
    // O Security Center informa um antivírus saudável: não é crítico.
    facts.security.antivirus.as_mut().unwrap().security_center = Some(WscHealth::Good);
    assert!(rule(&eval(&facts), "security.no_active_antivirus").is_empty());
}

#[test]
fn third_party_antivirus_with_passive_defender_is_valid() {
    let mut facts = healthy_facts();
    facts.security.antivirus = Some(AntivirusFact {
        provider: AvProvider::ThirdParty,
        defender: DefenderState::Passive,
        security_center: Some(WscHealth::Good),
        active_threats: None,
        signature_age_days: Some(400),
        third_party: Some(1),
    });
    assert!(
        eval(&facts).findings.is_empty(),
        "Defender passivo com antivírus de terceiros não é problema (nem assinatura velha dele)"
    );
}

#[test]
fn active_threat_and_stale_signatures() {
    let mut facts = healthy_facts();
    facts.security.antivirus.as_mut().unwrap().active_threats = Some(2);
    let e = eval(&facts);
    assert_eq!(
        one(&e, "security.threat.active").severity,
        Severity::Attention
    );
    facts.security.antivirus.as_mut().unwrap().active_threats = None;
    facts
        .security
        .antivirus
        .as_mut()
        .unwrap()
        .signature_age_days = Some(10);
    let e = eval(&facts);
    let f = one(&e, "security.defender.signatures_stale");
    assert_eq!(
        (f.severity, f.confidence),
        (Severity::Attention, Confidence::Medium)
    );
    facts.security.antivirus.as_mut().unwrap().active_threats = Some(0);
    facts
        .security
        .antivirus
        .as_mut()
        .unwrap()
        .signature_age_days = Some(3);
    assert!(eval(&facts).findings.is_empty());
}

// ------------------------------------------------------------------ exposição de portas

fn listener(port: u16, process: &str, system: bool, project: Option<&str>) -> ListenerFact {
    ListenerFact {
        port,
        pid: Some(100 + port as u32),
        process_name: Some(process.into()),
        project_name: project.map(String::from),
        system,
    }
}

#[test]
fn all_interfaces_listener_is_info_never_attention_or_exposed() {
    let mut facts = healthy_facts();
    facts.network.all_interface_listeners = Some(vec![
        listener(3000, "vite.exe", false, Some("LKR_Lab")),
        listener(5173, "vite.exe", false, None),
        listener(135, "svchost.exe", true, None),
    ]);
    let e = eval(&facts);
    let f = one(&e, "network.listener.all_interfaces");
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.resource, "vite.exe");
    assert!(f
        .evidence
        .iter()
        .any(|ev| ev.label == "Portas" && ev.value == "3000, 5173"));
    assert!(f
        .evidence
        .iter()
        .any(|ev| ev.label == "Project" && ev.value == "LKR_Lab"));
    let text = format!(
        "{} {} {} {}",
        f.title, f.summary, f.reason, f.recommended_next_step
    )
    .to_lowercase();
    assert!(!text.contains("vulner"));
    assert!(text.contains("não significa exposição à internet"));
    // processo do sistema não gera Info
    assert!(rule(&e, "network.listener.all_interfaces")
        .iter()
        .all(|f| f.resource != "svchost.exe"));
}

#[test]
fn listener_ports_are_not_part_of_the_fingerprint() {
    let mut facts = healthy_facts();
    facts.network.all_interface_listeners = Some(vec![listener(3000, "vite.exe", false, None)]);
    let a = eval(&facts).findings[0].fingerprint.clone();
    facts.network.all_interface_listeners = Some(vec![
        listener(3001, "vite.exe", false, None),
        listener(3002, "vite.exe", false, None),
    ]);
    assert_eq!(eval(&facts).findings[0].fingerprint, a);
}

#[test]
fn many_ports_are_summarized() {
    let mut facts = healthy_facts();
    facts.network.all_interface_listeners = Some(
        (1000..1012)
            .map(|p| listener(p, "svc.exe", false, None))
            .collect(),
    );
    let e = eval(&facts);
    let ports = &one(&e, "network.listener.all_interfaces")
        .evidence
        .iter()
        .find(|ev| ev.label == "Portas")
        .unwrap()
        .value;
    assert!(ports.ends_with("(+4)"), "{ports}");
}

#[test]
fn listener_info_does_not_raise_overall_attention() {
    let mut facts = healthy_facts();
    facts.network.all_interface_listeners = Some(vec![listener(3000, "vite.exe", false, None)]);
    let e = eval(&facts);
    assert!(e.findings.iter().all(|f| f.severity == Severity::Info));
}

// ------------------------------------------------------------------ runtime

fn run(id: &str, command_id: &str, state: RunState, started: i64, ended: Option<i64>) -> RunFact {
    RunFact {
        run_id: id.into(),
        project_id: "p1".into(),
        command_id: command_id.into(),
        command: format!("npm run {command_id}"),
        state,
        exit_code: if state == RunState::Failed {
            Some(1)
        } else {
            None
        },
        started_at: started,
        ended_at: ended,
    }
}

fn runtime(runs: Vec<RunFact>) -> Facts {
    let mut facts = healthy_facts();
    let rt = facts.runtime.as_mut().unwrap();
    rt.runs = runs;
    facts
}

#[test]
fn managed_runtime_failed() {
    let facts = runtime(vec![run(
        "r1",
        "lab",
        RunState::Failed,
        NOW - 5 * MIN,
        Some(NOW - 4 * MIN),
    )]);
    let e = eval(&facts);
    let f = one(&e, "runtime.managed.failed");
    assert_eq!(
        (f.severity, f.confidence, f.domain),
        (Severity::Attention, Confidence::High, Domain::Runtime)
    );
    assert_eq!(f.cta.as_ref().unwrap().kind, "runtime");
    assert_eq!(f.cta.as_ref().unwrap().target.as_deref(), Some("p1"));
    assert!(f
        .evidence
        .iter()
        .any(|ev| ev.label == "Project" && ev.value == "LKR_Lab"));
}

#[test]
fn workflow_and_normal_exit_states_are_not_alerts() {
    let facts = runtime(vec![
        run(
            "r1",
            "lab",
            RunState::Stopped,
            NOW - 9 * MIN,
            Some(NOW - 8 * MIN),
        ),
        run(
            "r2",
            "dev",
            RunState::Completed,
            NOW - 9 * MIN,
            Some(NOW - 8 * MIN),
        ),
        run("r3", "up", RunState::Running, NOW - 9 * MIN, None),
    ]);
    assert!(eval(&facts).findings.is_empty());
}

#[test]
fn failure_resolves_when_a_newer_run_of_the_same_action_succeeds() {
    let facts = runtime(vec![
        run(
            "r1",
            "lab",
            RunState::Failed,
            NOW - 20 * MIN,
            Some(NOW - 19 * MIN),
        ),
        run("r2", "lab", RunState::Running, NOW - 2 * MIN, None),
    ]);
    assert!(rule(&eval(&facts), "runtime.managed.failed").is_empty());
}

#[test]
fn old_failures_are_not_relevant() {
    let facts = runtime(vec![run(
        "r1",
        "lab",
        RunState::Failed,
        NOW - 30 * 3_600_000,
        Some(NOW - 30 * 3_600_000),
    )]);
    assert!(rule(&eval(&facts), "runtime.managed.failed").is_empty());
}

#[test]
fn crash_loop_is_detected_after_three_failures_in_the_window() {
    let two = runtime(vec![
        run(
            "r1",
            "lab",
            RunState::Failed,
            NOW - 9 * MIN,
            Some(NOW - 8 * MIN),
        ),
        run(
            "r2",
            "lab",
            RunState::Failed,
            NOW - 6 * MIN,
            Some(NOW - 5 * MIN),
        ),
    ]);
    assert!(rule(&eval(&two), "runtime.managed.repeated_failure").is_empty());
    let three = runtime(vec![
        run(
            "r1",
            "lab",
            RunState::Failed,
            NOW - 9 * MIN,
            Some(NOW - 8 * MIN),
        ),
        run(
            "r2",
            "lab",
            RunState::Failed,
            NOW - 6 * MIN,
            Some(NOW - 5 * MIN),
        ),
        run(
            "r3",
            "lab",
            RunState::Failed,
            NOW - 3 * MIN,
            Some(NOW - 2 * MIN),
        ),
    ]);
    let e = eval(&three);
    let f = one(&e, "runtime.managed.repeated_failure");
    assert!(f
        .evidence
        .iter()
        .any(|ev| ev.label == "Falhas na janela" && ev.value == "3"));
    // falhas antigas (fora da janela) não contam
    let old = runtime(vec![
        run(
            "r1",
            "lab",
            RunState::Failed,
            NOW - 60 * MIN,
            Some(NOW - 59 * MIN),
        ),
        run(
            "r2",
            "lab",
            RunState::Failed,
            NOW - 50 * MIN,
            Some(NOW - 49 * MIN),
        ),
        run(
            "r3",
            "lab",
            RunState::Failed,
            NOW - 40 * MIN,
            Some(NOW - 39 * MIN),
        ),
    ]);
    assert!(rule(&eval(&old), "runtime.managed.repeated_failure").is_empty());
}

fn with_port(mut facts: Facts, owner_managed_by: Option<&str>, pid: u32) -> Facts {
    let rt = facts.runtime.as_mut().unwrap();
    rt.declared_ports = vec![DeclaredPort {
        project_id: "p1".into(),
        port: 4317,
    }];
    rt.owners = vec![PortOwner {
        port: 4317,
        pid,
        process_name: Some("node.exe".into()),
        managed_project_id: owner_managed_by.map(String::from),
    }];
    facts
}

#[test]
fn port_collision_shows_port_owner_pid_and_project() {
    let facts = with_port(
        runtime(vec![run("r1", "lab", RunState::Running, NOW - MIN, None)]),
        None,
        4242,
    );
    let e = eval(&facts);
    let f = one(&e, "runtime.port.collision");
    assert_eq!(
        f.confidence,
        Confidence::Medium,
        "execução ainda ativa: indício, não prova"
    );
    assert!(
        f.summary.contains("4317")
            && f.summary.contains("node.exe")
            && f.summary.contains("4242")
            && f.summary.contains("LKR_Lab")
    );
    assert!(f.recommended_next_step.contains("não encerra processos"));
}

#[test]
fn port_collision_requires_a_foreign_owner_and_a_relevant_run() {
    // dono é a própria árvore gerenciada do Project: normal
    let own = with_port(
        runtime(vec![run("r1", "lab", RunState::Running, NOW - MIN, None)]),
        Some("p1"),
        4242,
    );
    assert!(rule(&eval(&own), "runtime.port.collision").is_empty());
    // o próprio LKR LAB
    let me = with_port(
        runtime(vec![run("r1", "lab", RunState::Running, NOW - MIN, None)]),
        None,
        1,
    );
    assert!(rule(&eval(&me), "runtime.port.collision").is_empty());
    // Project sem execução: servidor iniciado fora do app é normal
    let idle = with_port(runtime(vec![]), None, 4242);
    assert!(rule(&eval(&idle), "runtime.port.collision").is_empty());
    // dono gerenciado por OUTRO Project é colisão
    let other = with_port(
        runtime(vec![run("r1", "lab", RunState::Running, NOW - MIN, None)]),
        Some("p2"),
        4242,
    );
    assert_eq!(rule(&eval(&other), "runtime.port.collision").len(), 1);
}

#[test]
fn failure_plus_port_collision_is_correlated_with_both_pieces_of_evidence() {
    let facts = with_port(
        runtime(vec![run(
            "r1",
            "lab",
            RunState::Failed,
            NOW - 3 * MIN,
            Some(NOW - 2 * MIN),
        )]),
        None,
        4242,
    );
    let e = eval(&facts);
    let failed = one(&e, "runtime.managed.failed");
    assert!(failed
        .summary
        .contains("provavelmente foi causada por porta ocupada"));
    assert!(failed
        .evidence
        .iter()
        .any(|ev| ev.label == "Código de saída"));
    assert!(failed
        .evidence
        .iter()
        .any(|ev| ev.label == "Porta declarada ocupada" && ev.value == "4317"));
    assert!(failed
        .evidence
        .iter()
        .any(|ev| ev.label == "Dono da porta" && ev.value.contains("4242")));
    let collision = one(&e, "runtime.port.collision");
    assert_eq!(
        collision.confidence,
        Confidence::High,
        "depois de uma falha a colisão é confirmada"
    );
}

#[test]
fn failure_without_collision_has_no_cause_claimed() {
    let facts = runtime(vec![run(
        "r1",
        "lab",
        RunState::Failed,
        NOW - 3 * MIN,
        Some(NOW - 2 * MIN),
    )]);
    let e = eval(&facts);
    assert!(!one(&e, "runtime.managed.failed").summary.contains("porta"));
}

// ------------------------------------------------------------------ fingerprint / ordenação / dados velhos

#[test]
fn fingerprints_are_stable_and_unique_findings() {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 200.0, 4.0)]));
    let a = eval(&facts);
    let b = eval(&facts);
    assert_eq!(a.findings[0].fingerprint, "machine.disk.low_space@C:");
    assert_eq!(a.findings[0].id, a.findings[0].fingerprint);
    assert_eq!(a.findings, b.findings);
}

#[test]
fn findings_are_sorted_critical_first() {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 200.0, 4.0)]));
    facts.windows.restart_pending = Some(true);
    facts.network.all_interface_listeners = Some(vec![listener(3000, "vite.exe", false, None)]);
    let order: Vec<Severity> = eval(&facts).findings.iter().map(|f| f.severity).collect();
    assert_eq!(
        order,
        vec![Severity::Critical, Severity::Attention, Severity::Info]
    );
}

#[test]
fn staleness_uses_twice_the_ttl_and_never_flags_zero_ttl() {
    assert!(!is_stale(NOW - 59 * MIN, 30 * MIN, NOW));
    assert!(is_stale(NOW - 61 * MIN, 30 * MIN, NOW));
    assert!(!is_stale(NOW - 999 * MIN, 0, NOW));
    assert!(!is_stale(NOW + 5, 1000, NOW), "relógio voltou: não é velho");
}

#[test]
fn every_finding_is_explainable() {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 200.0, 4.0)]));
    facts.windows.restart_pending = Some(true);
    facts.windows.events = Some(vec![event(SignalKind::Bugcheck, 1, 1)]);
    facts.security.antivirus = Some(AntivirusFact {
        provider: AvProvider::None,
        defender: DefenderState::Disabled,
        security_center: None,
        active_threats: None,
        signature_age_days: None,
        third_party: Some(0),
    });
    facts.network.all_interface_listeners = Some(vec![listener(3000, "vite.exe", false, None)]);
    let facts = {
        let mut f = runtime(vec![run(
            "r1",
            "lab",
            RunState::Failed,
            NOW - 3 * MIN,
            Some(NOW - 2 * MIN),
        )]);
        f.machine = facts.machine;
        f.windows = facts.windows;
        f.security = facts.security;
        f.network = facts.network;
        f
    };
    let e = eval(&facts);
    assert!(e.findings.len() >= 5);
    for f in &e.findings {
        assert!(!f.rule_id.is_empty() && !f.title.is_empty() && !f.summary.is_empty());
        assert!(!f.reason.is_empty() && !f.recommended_next_step.is_empty());
        assert!(!f.evidence.is_empty(), "{} sem evidência", f.rule_id);
        assert!(f.evidence.iter().all(|ev| ev.source == f.source));
        assert!(f.source.contains('.'));
        assert!(f.cta.is_some());
        // Nenhuma ação de correção.
        let text = format!("{} {}", f.title, f.recommended_next_step).to_lowercase();
        assert!(!text.contains("corrigir automaticamente"));
    }
    // Crítico exige evidência forte: confiança alta.
    assert!(e
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Critical)
        .all(|f| f.confidence == Confidence::High));
}

// ------------------------------------------------------------------ ciclo de vida

fn ids() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("alert-{n}")
    }
}

fn disk_finding(severity_free_gib: f64) -> Evaluation {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 200.0, severity_free_gib)]));
    eval(&facts)
}

fn apply(
    latest: &mut Vec<AlertRecord>,
    evaluation: &Evaluation,
    now: i64,
    next: &mut dyn FnMut() -> String,
) {
    for change in reconcile(latest, evaluation, now, next) {
        match change {
            Change::Insert(r) => latest.push(r),
            Change::Update(r) => {
                let slot = latest.iter_mut().find(|x| x.id == r.id).unwrap();
                *slot = r;
            }
        }
    }
}

#[test]
fn a_persistent_problem_is_one_alert_not_one_per_snapshot() {
    let mut latest = vec![];
    let mut next = ids();
    for i in 0..5 {
        apply(&mut latest, &disk_finding(4.0), NOW + i * 30_000, &mut next);
    }
    assert_eq!(latest.len(), 1);
    let r = &latest[0];
    assert_eq!(
        (r.status, r.occurrence_count, r.observations),
        (AlertStatus::Active, 1, 5)
    );
    assert_eq!((r.first_seen, r.last_seen), (NOW, NOW + 4 * 30_000));
    assert!(r.resolved_at.is_none() && r.acknowledged_at.is_none());
}

#[test]
fn acknowledge_only_changes_the_local_lifecycle() {
    let mut latest = vec![];
    let mut next = ids();
    apply(&mut latest, &disk_finding(4.0), NOW, &mut next);
    let acked = acknowledge(&latest[0], NOW + 1000).unwrap();
    assert_eq!(acked.status, AlertStatus::Acknowledged);
    assert_eq!(acked.acknowledged_at, Some(NOW + 1000));
    assert_eq!(acked.finding, latest[0].finding, "a condição não mudou");
    latest[0] = acked;
    // continua sendo visto; reconhecido não é resolvido
    apply(&mut latest, &disk_finding(4.0), NOW + 30_000, &mut next);
    assert_eq!(latest[0].status, AlertStatus::Acknowledged);
    assert_eq!(latest[0].observations, 2);
    assert!(
        acknowledge(&latest[0], NOW).is_none(),
        "só ativo pode ser reconhecido"
    );
}

#[test]
fn escalation_after_acknowledge_calls_for_attention_again() {
    let mut latest = vec![];
    let mut next = ids();
    apply(&mut latest, &disk_finding(15.0), NOW, &mut next); // Attention
    latest[0] = acknowledge(&latest[0], NOW + 1000).unwrap();
    apply(&mut latest, &disk_finding(4.0), NOW + 60_000, &mut next); // Critical
    assert_eq!(latest[0].finding.severity, Severity::Critical);
    assert_eq!(latest[0].status, AlertStatus::Active);
    assert!(latest[0].acknowledged_at.is_none());
}

#[test]
fn de_escalation_keeps_the_acknowledgement() {
    let mut latest = vec![];
    let mut next = ids();
    apply(&mut latest, &disk_finding(4.0), NOW, &mut next);
    latest[0] = acknowledge(&latest[0], NOW + 1000).unwrap();
    apply(&mut latest, &disk_finding(15.0), NOW + 60_000, &mut next);
    assert_eq!(latest[0].finding.severity, Severity::Attention);
    assert_eq!(latest[0].status, AlertStatus::Acknowledged);
}

#[test]
fn resolves_only_after_the_delay_and_when_the_source_was_evaluated() {
    let mut latest = vec![];
    let mut next = ids();
    apply(&mut latest, &disk_finding(4.0), NOW, &mut next);
    let healthy = eval(&healthy_facts());
    // ainda dentro do atraso: pisca, não resolve
    apply(&mut latest, &healthy, NOW + RESOLVE_DELAY_MS - 1, &mut next);
    assert_eq!(latest[0].status, AlertStatus::Active);
    apply(&mut latest, &healthy, NOW + RESOLVE_DELAY_MS, &mut next);
    assert_eq!(latest[0].status, AlertStatus::Resolved);
    assert_eq!(latest[0].resolved_at, Some(NOW + RESOLVE_DELAY_MS));
}

#[test]
fn stale_or_unavailable_source_neither_resolves_nor_escalates() {
    let mut latest = vec![];
    let mut next = ids();
    apply(&mut latest, &disk_finding(15.0), NOW, &mut next);
    let before = latest[0].clone();
    // telemetria velha: nenhuma fonte de máquina foi avaliada
    let mut facts = healthy_facts();
    facts.machine = None;
    apply(&mut latest, &eval(&facts), NOW + 10 * MIN, &mut next);
    assert_eq!(
        latest[0], before,
        "alerta preservado, sem escalar nem resolver"
    );
}

#[test]
fn a_resolved_condition_that_returns_opens_a_new_occurrence_preserving_history() {
    let mut latest = vec![];
    let mut next = ids();
    apply(&mut latest, &disk_finding(4.0), NOW, &mut next);
    apply(
        &mut latest,
        &eval(&healthy_facts()),
        NOW + 5 * MIN,
        &mut next,
    );
    assert_eq!(latest[0].status, AlertStatus::Resolved);
    let first_id = latest[0].id.clone();
    // volta
    let back = reconcile(&latest, &disk_finding(4.0), NOW + 20 * MIN, &mut next);
    assert_eq!(back.len(), 1);
    let Change::Insert(second) = &back[0] else {
        panic!("deveria abrir nova ocorrência")
    };
    assert_ne!(
        second.id, first_id,
        "a ocorrência anterior continua no histórico"
    );
    assert_eq!(
        (second.occurrence_count, second.status, second.first_seen),
        (2, AlertStatus::Active, NOW + 20 * MIN)
    );
    assert_eq!(second.finding.fingerprint, latest[0].finding.fingerprint);
}

#[test]
fn only_one_open_alert_per_fingerprint() {
    let mut latest = vec![];
    let mut next = ids();
    for i in 0..3 {
        apply(&mut latest, &disk_finding(4.0), NOW + i * 1000, &mut next);
    }
    assert_eq!(latest.iter().filter(|r| r.status.is_open()).count(), 1);
}

#[test]
fn info_findings_follow_the_same_lifecycle() {
    let mut latest = vec![];
    let mut next = ids();
    let mut facts = healthy_facts();
    facts.network.all_interface_listeners = Some(vec![listener(3000, "vite.exe", false, None)]);
    apply(&mut latest, &eval(&facts), NOW, &mut next);
    assert_eq!(latest[0].finding.severity, Severity::Info);
    apply(
        &mut latest,
        &eval(&healthy_facts()),
        NOW + 2 * MIN,
        &mut next,
    );
    assert_eq!(latest[0].status, AlertStatus::Resolved);
}

#[test]
fn summary_counts_open_by_severity_and_recent_resolutions() {
    let mut latest = vec![];
    let mut next = ids();
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![
        volume("C:", 200.0, 4.0),
        volume("D:", 200.0, 15.0),
    ]));
    facts.windows.restart_pending = Some(true);
    facts.network.all_interface_listeners = Some(vec![listener(3000, "vite.exe", false, None)]);
    apply(&mut latest, &eval(&facts), NOW, &mut next);
    let summary = summarize(&latest, NOW);
    assert_eq!(
        (summary.critical, summary.attention, summary.info),
        (1, 2, 1)
    );
    assert_eq!((summary.acknowledged, summary.resolved_recently), (0, 0));
    let idx = latest
        .iter()
        .position(|r| r.finding.rule_id == "windows.reboot.pending")
        .unwrap();
    latest[idx] = acknowledge(&latest[idx], NOW).unwrap();
    // o disco D: se resolve
    apply(
        &mut latest,
        &eval(&healthy_facts()),
        NOW + 5 * MIN,
        &mut next,
    );
    let summary = summarize(&latest, NOW + 5 * MIN);
    assert_eq!(
        (summary.critical, summary.attention, summary.info),
        (0, 0, 0)
    );
    assert_eq!(summary.resolved_recently, 4);
    // fora da janela de "recentemente"
    assert_eq!(
        summarize(&latest, NOW + 5 * MIN + RESOLVED_RECENT_MS + 1).resolved_recently,
        0
    );
}

#[test]
fn sorting_is_predictable() {
    let mut latest = vec![];
    let mut next = ids();
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 200.0, 4.0)]));
    apply(&mut latest, &eval(&facts), NOW, &mut next);
    facts.windows.restart_pending = Some(true);
    facts.network.all_interface_listeners = Some(vec![listener(3000, "vite.exe", false, None)]);
    apply(&mut latest, &eval(&facts), NOW + MIN, &mut next);
    let mut flipped = facts.clone();
    flipped.windows.devices = Some(vec![DeviceFact {
        name: "Dev".into(),
        class: None,
        manufacturer: None,
        problem_code: 10,
        problem: "x".into(),
    }]);
    apply(&mut latest, &eval(&flipped), NOW + 3 * MIN, &mut next); // resolve os demais? (fora do atraso, não)
    let mut sorted = latest.clone();
    sort_alerts(&mut sorted);
    let severities: Vec<_> = sorted.iter().map(|r| r.finding.severity).collect();
    let mut expected = severities.clone();
    expected.sort_by(|a, b| b.cmp(a));
    assert_eq!(severities, expected, "Critical > Attention > Info");
    // dentro da mesma severidade: o mais antigo primeiro
    let attention: Vec<_> = sorted
        .iter()
        .filter(|r| r.finding.severity == Severity::Attention)
        .map(|r| r.first_seen)
        .collect();
    assert!(attention.windows(2).all(|w| w[0] <= w[1]));
    // resolvidos por último
    let mut with_resolved = sorted.clone();
    with_resolved[0].status = AlertStatus::Resolved;
    sort_alerts(&mut with_resolved);
    assert_eq!(with_resolved.last().unwrap().status, AlertStatus::Resolved);
    // determinístico
    let mut again = latest.clone();
    sort_alerts(&mut again);
    assert_eq!(again, sorted);
}

#[test]
fn previous_severity_map_only_has_open_alerts() {
    let mut latest = vec![];
    let mut next = ids();
    apply(&mut latest, &disk_finding(4.0), NOW, &mut next);
    assert_eq!(previous_of(&latest).len(), 1);
    apply(
        &mut latest,
        &eval(&healthy_facts()),
        NOW + 5 * MIN,
        &mut next,
    );
    assert!(previous_of(&latest).is_empty());
}

// ------------------------------------------------------------------ privacidade e passividade

#[test]
fn findings_never_carry_secrets_or_raw_paths() {
    let mut facts = healthy_facts();
    facts.windows.devices = Some(vec![DeviceFact {
        name: "Dispositivo".into(),
        class: None,
        manufacturer: None,
        problem_code: 43,
        problem: "falha".into(),
    }]);
    facts.network.all_interface_listeners = Some(vec![listener(3000, "vite.exe", false, None)]);
    let facts = {
        let mut f = runtime(vec![run(
            "r1",
            "lab",
            RunState::Failed,
            NOW - 3 * MIN,
            Some(NOW - 2 * MIN),
        )]);
        f.windows = facts.windows;
        f.network = facts.network;
        f
    };
    let json = serde_json::to_string(&eval(&facts).findings)
        .unwrap()
        .to_lowercase();
    for banned in ["password", "token", "secret", "c:\\\\users", "recovery"] {
        assert!(!json.contains(banned), "{banned}");
    }
}

#[test]
fn serialization_is_camel_case_with_lowercase_enums() {
    let mut facts = healthy_facts();
    facts.machine = Some(machine(vec![volume("C:", 200.0, 4.0)]));
    let json = serde_json::to_value(&eval(&facts).findings[0]).unwrap();
    assert_eq!(json["severity"], "critical");
    assert_eq!(json["confidence"], "high");
    assert_eq!(json["domain"], "machine");
    assert!(json["recommendedNextStep"].is_string());
    assert!(json["ruleId"].is_string());
}

#[test]
fn engine_and_store_never_touch_portable_workspace_or_sync() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in [
        "portable.rs",
        "sync.rs",
        "snapshot.rs",
        "planning.rs",
        "ddae.rs",
    ] {
        let text = std::fs::read_to_string(src.join(file)).unwrap();
        for word in [
            "machine_alerts",
            "machine_diagnostic_runs",
            "diagnostics::",
            "alert_store",
            "diagnostic_runner",
        ] {
            assert!(!text.contains(word), "{file} não pode tocar {word}");
        }
    }
}

#[test]
fn the_engine_has_no_model_calls_no_score_and_no_mutation() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("diagnostics.rs"),
    )
    .unwrap();
    let code: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    for banned in [
        "reqwest",
        "anthropic",
        "openai",
        "llm",
        "score",
        "command::new",
        "std::process",
        "remove_file",
        "kill(",
        "taskkill",
        "regsetvalue",
        "netsh",
    ] {
        assert!(
            !code.contains(banned),
            "diagnostics.rs não pode conter {banned}"
        );
    }
}

#[test]
fn unavailable_ports_neither_collide_nor_resolve() {
    let mut facts = with_port(
        runtime(vec![run("r1", "lab", RunState::Running, NOW - MIN, None)]),
        None,
        4242,
    );
    facts.runtime.as_mut().unwrap().ports_available = false;
    let e = eval(&facts);
    assert!(rule(&e, "runtime.port.collision").is_empty());
    assert!(
        !e.evaluated.contains(SRC_PORTS),
        "sem leitura de portas, a colisão não pode ser resolvida"
    );
    assert!(e.evaluated.contains(SRC_RUNS));
}
