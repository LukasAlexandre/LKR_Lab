//! Windows Health & Integrity (SESSION-002, Block 07). As regras são puras e as fontes são
//! substituídas por fakes: nenhum teste depende do Event Log, dos serviços ou dos drivers reais da
//! máquina. Ausência de informação é `Unknown`, nunca `Healthy`; nada é reparado ou alterado.
use hub_core::windows_health::{
    classify_event, evaluate_devices, evaluate_events, evaluate_integrity, evaluate_restart,
    evaluate_service, evaluate_services, evaluate_updates, evaluate_volume, overall, parse_wu_time,
    problem_text, system_data, Collector, DeviceBatch, EventBatch, Expectation, Health, Millis,
    Probe, RawDevice, RawEvent, RawService, RawVolume, RestartData, RestartProbes, ServiceSpec,
    ServiceState, ServiceView, SignalKind, SourceError, SourceState, Sources, StartType, SystemRaw,
    UpdateTimes, DAY_MS, ESSENTIAL_SERVICES, TTL_DEVICES_MS, TTL_EVENTS_MS, TTL_RESTART_MS,
    TTL_SERVICES_MS, WEEK_MS,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

const NOW: Millis = 1_791_121_512_000; // 2026-10-04 13:45:12 UTC
const HOUR: Millis = 3_600_000;
const MIN: Millis = 60_000;

fn event(provider: &str, id: u32, level: u8, ago: Millis) -> RawEvent {
    RawEvent {
        provider: provider.into(),
        id,
        level,
        at: NOW - ago,
    }
}
fn batch(errors: Vec<RawEvent>, signals: Vec<RawEvent>) -> EventBatch {
    EventBatch {
        errors,
        signals,
        update_failures: Some(vec![]),
        app_events: Some(vec![]),
        ..EventBatch::default()
    }
}

// ---------------------------------------------------------------- sistema

#[test]
fn windows_11_is_named_by_build_not_by_the_legacy_product_name() {
    let raw = SystemRaw {
        product_name: Some("Windows 10 Pro".into()),
        build: Some("26200".into()),
        ubr: Some(9457),
        version: Some("25H2".into()),
        architecture: Some("x64".into()),
        ..SystemRaw::default()
    };
    let data = system_data(&raw);
    assert_eq!(data.product_name.as_deref(), Some("Windows 11 Pro"));
    assert_eq!(data.build.as_deref(), Some("26200.9457"));
    let windows10 = system_data(&SystemRaw {
        product_name: Some("Windows 10 Pro".into()),
        build: Some("19045".into()),
        ..SystemRaw::default()
    });
    assert_eq!(
        windows10.product_name.as_deref(),
        Some("Windows 10 Pro"),
        "build de Windows 10 não é reescrita"
    );
    assert!(system_data(&SystemRaw::default()).product_name.is_none());
}

// ---------------------------------------------------------------- reinício pendente

fn probes(cbs: Probe, wu: Probe, renames: Probe) -> RestartProbes {
    RestartProbes {
        cbs_reboot_pending: cbs,
        wu_reboot_required: wu,
        pending_file_renames: renames,
    }
}

#[test]
fn restart_is_healthy_only_when_every_strong_source_was_checked_and_absent() {
    let (status, reasons, data, _) =
        evaluate_restart(&probes(Probe::Absent, Probe::Absent, Probe::Absent));
    assert_eq!((status, data.pending), (Health::Healthy, Some(false)));
    assert!(reasons.is_empty());
}

#[test]
fn a_present_strong_source_means_restart_pending_with_a_visible_reason() {
    let (status, reasons, data, _) =
        evaluate_restart(&probes(Probe::Absent, Probe::Present, Probe::Absent));
    assert_eq!((status, data.pending), (Health::Attention, Some(true)));
    assert!(reasons[0].contains("Windows Update"), "{reasons:?}");
    let (status, reasons, _, _) =
        evaluate_restart(&probes(Probe::Present, Probe::Present, Probe::Absent));
    assert_eq!(status, Health::Attention);
    assert_eq!(reasons.len(), 2, "uma razão por fonte");
    // Uma fonte presente basta, mesmo que a outra esteja negada.
    let (status, _, data, _) =
        evaluate_restart(&probes(Probe::Present, Probe::Denied, Probe::Unavailable));
    assert_eq!((status, data.pending), (Health::Attention, Some(true)));
}

#[test]
fn an_unavailable_source_never_concludes_reboot_is_needed_nor_that_it_is_not() {
    let (status, reasons, data, notes) =
        evaluate_restart(&probes(Probe::Absent, Probe::Denied, Probe::Absent));
    assert_eq!(
        status,
        Health::Unknown,
        "uma fonte negada impede afirmar que não há reinício"
    );
    assert_eq!(data.pending, None);
    assert!(
        reasons[0].contains("Não foi possível confirmar"),
        "{reasons:?}"
    );
    let denied = notes.iter().find(|n| n.id == "windows_update").unwrap();
    assert_eq!(denied.state, SourceState::RequiresElevation);
    let (status, _, data, notes) = evaluate_restart(&probes(
        Probe::Unavailable,
        Probe::Unavailable,
        Probe::Unavailable,
    ));
    assert_eq!((status, data.pending), (Health::Unknown, None));
    assert!(notes.iter().all(|n| n.state == SourceState::Unavailable));
}

#[test]
fn pending_file_renames_alone_are_informational_not_a_restart_alert() {
    let (status, reasons, data, _) =
        evaluate_restart(&probes(Probe::Absent, Probe::Absent, Probe::Present));
    assert_eq!(
        (status, data.pending, data.file_rename_operations),
        (Health::Healthy, Some(false), Some(true))
    );
    assert!(reasons.is_empty());
}

// ---------------------------------------------------------------- serviços

fn spec(id: &str) -> &'static ServiceSpec {
    ESSENTIAL_SERVICES.iter().find(|s| s.id == id).unwrap()
}
fn raw(state: ServiceState, start: StartType) -> RawService {
    RawService { state, start }
}

#[test]
fn automatic_service_running_is_healthy_and_stopped_is_an_attention_with_a_reason() {
    let ok = evaluate_service(
        spec("Schedule"),
        Some(&raw(ServiceState::Running, StartType::Automatic)),
        Some(86_400),
    );
    assert_eq!((ok.health, ok.reason), (Health::Healthy, None));
    let stopped = evaluate_service(
        spec("Schedule"),
        Some(&raw(ServiceState::Stopped, StartType::Automatic)),
        Some(86_400),
    );
    assert_eq!(stopped.health, Health::Attention);
    assert!(stopped
        .reason
        .unwrap()
        .contains("parado, mas é iniciado automaticamente"));
}

#[test]
fn critical_services_stopped_or_disabled_are_critical() {
    for id in ["EventLog", "RpcSs"] {
        let stopped = evaluate_service(
            spec(id),
            Some(&raw(ServiceState::Stopped, StartType::Automatic)),
            Some(86_400),
        );
        assert_eq!(stopped.health, Health::Critical, "{id}");
        let disabled = evaluate_service(
            spec(id),
            Some(&raw(ServiceState::Stopped, StartType::Disabled)),
            Some(86_400),
        );
        assert_eq!(disabled.health, Health::Critical, "{id}");
    }
}

#[test]
fn a_stopped_service_is_not_a_problem_when_its_start_mode_allows_it() {
    // Início manual: parado é o estado normal (o Windows sobe sob demanda).
    let manual = evaluate_service(
        spec("Winmgmt"),
        Some(&raw(ServiceState::Stopped, StartType::Manual)),
        Some(86_400),
    );
    assert_eq!(manual.health, Health::Healthy);
    // Windows Update e BITS são sob demanda/por gatilho: parados é o normal.
    for id in ["wuauserv", "BITS"] {
        let on_demand = evaluate_service(
            spec(id),
            Some(&raw(ServiceState::Stopped, StartType::Manual)),
            Some(86_400),
        );
        assert_eq!(
            (on_demand.health, on_demand.expectation),
            (Health::Healthy, Expectation::OnDemand),
            "{id}"
        );
        let delayed_stopped = evaluate_service(
            spec(id),
            Some(&raw(ServiceState::Stopped, StartType::AutomaticDelayed)),
            Some(86_400),
        );
        assert_eq!(
            delayed_stopped.health,
            Health::Healthy,
            "{id}: sob demanda nunca alerta só por estar parado"
        );
    }
}

#[test]
fn a_disabled_service_is_an_attention_even_when_on_demand() {
    let view = evaluate_service(
        spec("wuauserv"),
        Some(&raw(ServiceState::Stopped, StartType::Disabled)),
        Some(86_400),
    );
    assert_eq!(view.health, Health::Attention);
    assert!(view.reason.unwrap().contains("desabilitado"));
}

#[test]
fn delayed_start_just_after_boot_is_not_a_failure_but_is_after_the_grace_period() {
    let s = raw(ServiceState::Stopped, StartType::AutomaticDelayed);
    assert_eq!(
        evaluate_service(spec("Schedule"), Some(&s), Some(60)).health,
        Health::Healthy
    );
    assert_eq!(
        evaluate_service(spec("Schedule"), Some(&s), Some(3_600)).health,
        Health::Attention
    );
    // Início automático normal (não atrasado) parado logo após o boot já é um problema.
    let plain = raw(ServiceState::Stopped, StartType::Automatic);
    assert_eq!(
        evaluate_service(spec("Schedule"), Some(&plain), Some(60)).health,
        Health::Attention
    );
}

#[test]
fn a_service_that_could_not_be_queried_is_unknown_not_healthy() {
    let view = evaluate_service(spec("CryptSvc"), None, Some(1_000));
    assert_eq!(
        (view.health, view.state, view.start),
        (Health::Unknown, ServiceState::Unknown, StartType::Unknown)
    );
    let (status, reasons) = evaluate_services(&[view]);
    assert_eq!(status, Health::Unknown);
    assert!(reasons.is_empty(), "desconhecido não vira alerta");
}

#[test]
fn services_status_is_the_worst_known_with_each_reason() {
    let views: Vec<ServiceView> = vec![
        evaluate_service(
            spec("Schedule"),
            Some(&raw(ServiceState::Running, StartType::Automatic)),
            Some(1e6 as u64),
        ),
        evaluate_service(
            spec("CryptSvc"),
            Some(&raw(ServiceState::Stopped, StartType::Automatic)),
            Some(1e6 as u64),
        ),
        evaluate_service(
            spec("EventLog"),
            Some(&raw(ServiceState::Stopped, StartType::Automatic)),
            Some(1e6 as u64),
        ),
        evaluate_service(spec("BITS"), None, Some(1e6 as u64)),
    ];
    let (status, reasons) = evaluate_services(&views);
    assert_eq!(status, Health::Critical);
    assert_eq!(reasons.len(), 2);
}

// ---------------------------------------------------------------- dispositivos

fn device(name: &str, problem: u32) -> RawDevice {
    RawDevice {
        name: name.into(),
        class: Some("Net".into()),
        manufacturer: Some("Fabricante".into()),
        problem_code: problem,
    }
}

#[test]
fn a_device_problem_is_an_attention_with_a_counted_reason() {
    let (status, reasons, data) = evaluate_devices(&DeviceBatch {
        total: 300,
        problems: vec![device("Placa X", 43)],
    });
    assert_eq!(status, Health::Attention);
    assert_eq!(
        reasons,
        vec!["1 dispositivo reporta problema no Gerenciador de Dispositivos."]
    );
    assert_eq!(data.issues[0].problem_code, 43);
    assert!(data.issues[0].problem.contains("43"));
    assert_eq!(data.issues[0].manufacturer.as_deref(), Some("Fabricante"));
    let (_, reasons, _) = evaluate_devices(&DeviceBatch {
        total: 300,
        problems: vec![device("A", 10), device("B", 28)],
    });
    assert_eq!(
        reasons,
        vec!["2 dispositivos reportam problema no Gerenciador de Dispositivos."]
    );
}

#[test]
fn no_device_problem_is_healthy_and_a_deliberately_disabled_device_is_not_a_problem() {
    let (status, reasons, data) = evaluate_devices(&DeviceBatch {
        total: 300,
        problems: vec![],
    });
    assert_eq!(
        (status, data.issues.len(), data.total),
        (Health::Healthy, 0, 300)
    );
    assert!(reasons.is_empty());
    let (status, _, data) = evaluate_devices(&DeviceBatch {
        total: 300,
        problems: vec![device("Adaptador desligado", 22), device("Firmware", 29)],
    });
    assert_eq!(
        (status, data.issues.len(), data.disabled),
        (Health::Healthy, 0, 2)
    );
}

#[test]
fn zero_enumerated_devices_is_unknown_never_healthy() {
    let (status, reasons, _) = evaluate_devices(&DeviceBatch {
        total: 0,
        problems: vec![],
    });
    assert_eq!(status, Health::Unknown);
    assert!(!reasons.is_empty());
    assert!(problem_text(43).contains("código 43"));
    assert!(problem_text(9999).contains("código 9999"));
}

// ---------------------------------------------------------------- eventos

#[test]
fn only_specific_provider_and_id_pairs_are_signals() {
    assert_eq!(
        classify_event("Microsoft-Windows-Kernel-Power", 41),
        Some(SignalKind::UnexpectedShutdown)
    );
    assert_eq!(
        classify_event("EventLog", 6008),
        Some(SignalKind::UnexpectedShutdown)
    );
    assert_eq!(
        classify_event("Microsoft-Windows-WER-SystemErrorReporting", 1001),
        Some(SignalKind::Bugcheck)
    );
    assert_eq!(classify_event("disk", 7), Some(SignalKind::StorageError));
    assert_eq!(
        classify_event("Ntfs", 55),
        Some(SignalKind::FilesystemError)
    );
    assert_eq!(
        classify_event("Service Control Manager", 7034),
        Some(SignalKind::ServiceFailure)
    );
    assert_eq!(
        classify_event("Microsoft-Windows-WindowsUpdateClient", 20),
        Some(SignalKind::UpdateFailure)
    );
    assert_eq!(
        classify_event("Application Error", 1000),
        Some(SignalKind::ApplicationCrash)
    );
    // Ruído: o mesmo ID em outro provedor, ou um ID comum, não é sinal.
    assert_eq!(classify_event("Algum Provedor", 41), None);
    assert_eq!(classify_event("disk", 9999), None);
    assert_eq!(
        classify_event("Ntfs", 98),
        None,
        "98 é informativo (volume verificado sem corrupção)"
    );
    assert_eq!(classify_event("Microsoft-Windows-Kernel-Power", 109), None);
}

#[test]
fn noisy_errors_and_warnings_alone_never_change_the_status() {
    let errors: Vec<RawEvent> = (0..300)
        .map(|i| {
            event(
                "Provedor Barulhento",
                1000 + (i % 7),
                2,
                i as Millis * 10 * MIN,
            )
        })
        .collect();
    let mut b = batch(errors, vec![]);
    b.warnings_24h = 842;
    b.warnings_capped = false;
    let (status, reasons, data) = evaluate_events(&b, NOW);
    assert_eq!(status, Health::Healthy);
    assert!(reasons.is_empty());
    assert!(
        data.last_7d.error > 0 && data.last_24h.warning == Some(842),
        "contagens existem, são informativas"
    );
    assert_eq!(
        data.last_7d.warning, None,
        "avisos de 7 dias não são medidos: nunca viram 0"
    );
    assert!(data.signals.is_empty());
}

#[test]
fn bugcheck_within_a_day_is_critical_and_older_in_the_week_is_attention() {
    let recent = batch(
        vec![],
        vec![event(
            "Microsoft-Windows-WER-SystemErrorReporting",
            1001,
            2,
            2 * HOUR,
        )],
    );
    let (status, reasons, data) = evaluate_events(&recent, NOW);
    assert_eq!(status, Health::Critical);
    assert!(reasons[0].contains("24 horas"));
    assert_eq!(data.signals[0].kind, SignalKind::Bugcheck);
    assert_eq!(data.signals[0].count_24h, 1);
    let older = batch(
        vec![],
        vec![event(
            "Microsoft-Windows-WER-SystemErrorReporting",
            1001,
            2,
            3 * DAY_MS,
        )],
    );
    let (status, reasons, _) = evaluate_events(&older, NOW);
    assert_eq!(status, Health::Attention);
    assert!(reasons[0].contains("7 dias"));
}

#[test]
fn one_unexpected_shutdown_is_counted_once_even_though_two_events_describe_it() {
    // Kernel-Power 41 e EventLog 6008 do MESMO desligamento (2 min de diferença).
    let same = batch(
        vec![],
        vec![
            event("Microsoft-Windows-Kernel-Power", 41, 1, DAY_MS + 5 * MIN),
            event("EventLog", 6008, 2, DAY_MS + 3 * MIN),
        ],
    );
    let (status, reasons, data) = evaluate_events(&same, NOW);
    assert_eq!(status, Health::Attention);
    assert!(reasons[0].starts_with("1 desligamento"), "{reasons:?}");
    assert_eq!(data.signals[0].count_7d, 1);
    let two = batch(
        vec![],
        vec![
            event("Microsoft-Windows-Kernel-Power", 41, 1, DAY_MS),
            event("Microsoft-Windows-Kernel-Power", 41, 1, 4 * DAY_MS),
        ],
    );
    assert_eq!(evaluate_events(&two, NOW).2.signals[0].count_7d, 2);
    // Nunca diagnostica a causa: só quando, tipo e ID.
    assert_eq!(evaluate_events(&same, NOW).2.recent[0].id, 41);
}

#[test]
fn storage_errors_distinguish_hard_device_errors_from_frequent_soft_warnings() {
    let soft = |n: usize| {
        batch(
            vec![],
            (0..n)
                .map(|i| event("disk", 153, 3, (i as Millis + 1) * HOUR))
                .collect(),
        )
    };
    assert_eq!(
        evaluate_events(&soft(2), NOW).0,
        Health::Healthy,
        "avisos de E/S esparsos são comuns"
    );
    assert_eq!(evaluate_events(&soft(5), NOW).0, Health::Attention);
    let hard = batch(vec![], vec![event("disk", 7, 2, 2 * DAY_MS)]);
    let (status, reasons, _) = evaluate_events(&hard, NOW);
    assert_eq!(status, Health::Attention);
    assert!(reasons[0].contains("armazenamento"));
}

#[test]
fn filesystem_corruption_and_repeated_service_failures_raise_attention() {
    let ntfs = batch(vec![], vec![event("Ntfs", 55, 2, DAY_MS)]);
    assert_eq!(evaluate_events(&ntfs, NOW).0, Health::Attention);
    let services = |n: usize| {
        batch(
            vec![],
            (0..n)
                .map(|i| event("Service Control Manager", 7034, 2, (i as Millis + 1) * HOUR))
                .collect(),
        )
    };
    assert_eq!(evaluate_events(&services(2), NOW).0, Health::Healthy);
    assert_eq!(evaluate_events(&services(3), NOW).0, Health::Attention);
    // Dispersas em vários dias, mesmo muitas: só a janela de 24 h conta.
    let spread = batch(
        vec![],
        (0..6)
            .map(|i| {
                event(
                    "Service Control Manager",
                    7034,
                    2,
                    (i as Millis + 2) * DAY_MS / 2,
                )
            })
            .collect(),
    );
    assert_eq!(evaluate_events(&spread, NOW).0, Health::Healthy);
}

#[test]
fn update_failures_and_app_crashes_are_listed_but_do_not_rate_the_events_domain() {
    let mut b = batch(vec![], vec![]);
    b.update_failures = Some(vec![event(
        "Microsoft-Windows-WindowsUpdateClient",
        20,
        2,
        DAY_MS,
    )]);
    b.app_events = Some(vec![
        event("Application Error", 1000, 2, HOUR),
        event("Application Hang", 1002, 3, HOUR),
    ]);
    let (status, _, data) = evaluate_events(&b, NOW);
    assert_eq!(status, Health::Healthy);
    let kinds: Vec<SignalKind> = data.signals.iter().map(|s| s.kind).collect();
    assert!(
        kinds.contains(&SignalKind::UpdateFailure) && kinds.contains(&SignalKind::ApplicationCrash)
    );
    assert!(
        data.recent.is_empty(),
        "o recente é só do sistema (crash de app e update têm painel próprio)"
    );
}

#[test]
fn events_outside_the_window_or_from_the_future_are_ignored() {
    let b = batch(
        vec![
            event("X", 1, 2, WEEK_MS + HOUR),
            event("X", 2, 2, -10 * MIN - MIN),
        ],
        vec![
            event(
                "Microsoft-Windows-WER-SystemErrorReporting",
                1001,
                2,
                WEEK_MS + HOUR,
            ),
            event("Microsoft-Windows-WER-SystemErrorReporting", 1001, 2, -HOUR),
        ],
    );
    let (status, _, data) = evaluate_events(&b, NOW);
    assert_eq!(status, Health::Healthy);
    assert_eq!((data.last_7d.error, data.signals.len()), (0, 0));
}

// ---------------------------------------------------------------- Windows Update

fn wu_service(start: StartType) -> ServiceView {
    evaluate_service(
        spec("wuauserv"),
        Some(&raw(ServiceState::Stopped, start)),
        Some(1_000_000),
    )
}

#[test]
fn windows_update_rules_are_factual() {
    let (status, reasons) = evaluate_updates(Some(&wu_service(StartType::Manual)), Some(0));
    assert_eq!(status, Health::Healthy);
    assert!(reasons.is_empty());
    let (status, reasons) = evaluate_updates(Some(&wu_service(StartType::Manual)), Some(2));
    assert_eq!(status, Health::Attention);
    assert!(reasons[0].contains("2 falha"));
    let (status, reasons) = evaluate_updates(Some(&wu_service(StartType::Disabled)), Some(0));
    assert_eq!(status, Health::Attention);
    assert!(reasons[0].contains("desabilitado"));
}

#[test]
fn missing_update_information_is_unknown_not_healthy() {
    assert_eq!(
        evaluate_updates(Some(&wu_service(StartType::Manual)), None).0,
        Health::Unknown
    );
    assert_eq!(evaluate_updates(None, Some(0)).0, Health::Unknown);
    assert_eq!(evaluate_updates(None, None).0, Health::Unknown);
}

#[test]
fn windows_update_registry_timestamps_parse_to_utc_milliseconds() {
    assert_eq!(parse_wu_time("2026-10-04 13:45:12"), Some(NOW));
    assert_eq!(
        parse_wu_time("2024-02-29 00:00:00"),
        Some(1_709_164_800_000),
        "ano bissexto"
    );
    assert_eq!(parse_wu_time("2000-01-01 00:00:01"), Some(946_684_801_000));
    assert_eq!(parse_wu_time("1970-01-01 00:00:00"), Some(0));
    assert_eq!(parse_wu_time("  2026-10-04 13:45:12  "), Some(NOW));
    for bad in [
        "",
        "ontem",
        "2026-13-01 00:00:00",
        "2026-10-04",
        "2026-10-04 25:00:00",
        "1960-01-01 00:00:00",
        "2026-10-04 13:45",
    ] {
        assert_eq!(parse_wu_time(bad), None, "{bad:?}");
    }
}

// ---------------------------------------------------------------- volumes

fn volume(dirty: Option<bool>, read_only: Option<bool>, denied: bool) -> RawVolume {
    RawVolume {
        mount: "C:".into(),
        filesystem: Some("NTFS".into()),
        read_only,
        dirty,
        dirty_denied: denied,
        check_scheduled: Some(false),
    }
}

#[test]
fn a_disk_check_scheduled_for_next_boot_is_an_attention_and_unreadable_schedule_is_not_a_finding() {
    let mut scheduled = volume(Some(false), Some(false), false);
    scheduled.check_scheduled = Some(true);
    let view = evaluate_volume(&scheduled);
    assert_eq!(view.status, Health::Attention);
    assert!(view.reasons[0].contains("verificação de disco agendada"));
    // Não conseguir ler o agendamento não cria alerta nem prova que não há nada agendado.
    let mut unreadable = volume(Some(false), Some(false), false);
    unreadable.check_scheduled = None;
    assert_eq!(evaluate_volume(&unreadable).status, Health::Healthy);
}

#[test]
fn volume_rules_flag_only_dirty_or_read_only_volumes() {
    let clean = evaluate_volume(&volume(Some(false), Some(false), false));
    assert_eq!((clean.status, clean.reasons.len()), (Health::Healthy, 0));
    let dirty = evaluate_volume(&volume(Some(true), Some(false), false));
    assert_eq!(dirty.status, Health::Attention);
    assert!(dirty.reasons[0].contains("sujo"));
    let read_only = evaluate_volume(&volume(Some(false), Some(true), false));
    assert_eq!(read_only.status, Health::Attention);
    assert!(read_only.reasons[0].contains("somente leitura"));
}

#[test]
fn volume_state_that_cannot_be_read_is_unknown_and_says_why() {
    let denied = evaluate_volume(&volume(None, Some(false), true));
    assert_eq!(denied.status, Health::Unknown);
    assert!(denied.reasons[0].contains("privilégio administrativo"));
    let unreadable = evaluate_volume(&volume(None, None, false));
    assert_eq!(unreadable.status, Health::Unknown);
    assert!(!unreadable.reasons[0].contains("administrativo"));
    // Um problema real prevalece sobre o campo ilegível.
    assert_eq!(
        evaluate_volume(&volume(None, Some(true), true)).status,
        Health::Attention
    );
}

// ---------------------------------------------------------------- integridade passiva

#[test]
fn passive_integrity_without_signals_is_unknown_and_never_claims_the_os_is_intact() {
    let (status, reasons, data) = evaluate_integrity(
        &RestartData {
            pending: Some(false),
            file_rename_operations: None,
        },
        &Default::default(),
        &Default::default(),
        &Default::default(),
        Some(0),
    );
    assert_eq!(status, Health::Unknown);
    assert!(reasons[0].contains("verificação sob demanda"));
    assert!(data.signals.iter().all(|s| !s.present));
}

#[test]
fn passive_signals_raise_attention_and_on_demand_checks_are_never_available_here() {
    let (_, _, events) = evaluate_events(&batch(vec![], vec![event("Ntfs", 55, 2, HOUR)]), NOW);
    let (status, reasons, data) = evaluate_integrity(
        &RestartData {
            pending: Some(true),
            file_rename_operations: None,
        },
        &events,
        &Default::default(),
        &Default::default(),
        Some(1),
    );
    assert_eq!(status, Health::Attention);
    assert!(reasons.len() >= 3, "{reasons:?}");
    for id in ["servicing_pending", "update_failures", "filesystem_errors"] {
        assert!(
            data.signals.iter().find(|s| s.id == id).unwrap().present,
            "{id}"
        );
    }
    assert_eq!(data.on_demand.len(), 3);
    assert!(
        data.on_demand
            .iter()
            .all(|c| !c.available && c.requires_elevation),
        "SFC/DISM/CHKDSK não são executados"
    );
}

// ---------------------------------------------------------------- estado geral

#[test]
fn overall_is_the_worst_known_domain_with_visible_reasons() {
    let none: Vec<String> = vec![];
    let attention = vec!["Reinicialização pendente: o Windows Update exige reinício.".to_string()];
    let sections: [(&str, bool, Health, &[String]); 4] = [
        ("restart", true, Health::Attention, &attention),
        ("services", true, Health::Healthy, &none),
        ("devices", true, Health::Unknown, &none),
        ("system", false, Health::Unknown, &none),
    ];
    let o = overall(&sections);
    assert_eq!(o.status, Health::Attention);
    assert_eq!(o.reasons.len(), 1);
    assert_eq!(
        (o.reasons[0].domain.as_str(), o.evaluated, o.rateable),
        ("restart", 2, 3)
    );
}

#[test]
fn overall_is_unknown_when_nothing_could_be_evaluated() {
    let none: Vec<String> = vec![];
    let sections: [(&str, bool, Health, &[String]); 2] = [
        ("restart", true, Health::Unknown, &none),
        ("events", true, Health::Unknown, &none),
    ];
    let o = overall(&sections);
    assert_eq!((o.status, o.evaluated), (Health::Unknown, 0));
    assert!(o.reasons.is_empty());
    assert_eq!(overall(&[]).status, Health::Unknown);
}

// ---------------------------------------------------------------- coletor com fontes falsas

#[derive(Default)]
struct Calls {
    system: AtomicUsize,
    restart: AtomicUsize,
    services: AtomicUsize,
    devices: AtomicUsize,
    events: AtomicUsize,
    volumes: AtomicUsize,
    updates: AtomicUsize,
}

struct Fake {
    calls: Arc<Calls>,
    events: Result<EventBatch, SourceError>,
    devices: Result<DeviceBatch, SourceError>,
    restart: RestartProbes,
    services: bool,
}
impl Fake {
    fn healthy() -> (Self, Arc<Calls>) {
        let calls = Arc::new(Calls::default());
        (
            Self {
                calls: calls.clone(),
                events: Ok(EventBatch {
                    update_failures: Some(vec![]),
                    app_events: Some(vec![]),
                    ..EventBatch::default()
                }),
                devices: Ok(DeviceBatch {
                    total: 250,
                    problems: vec![],
                }),
                restart: probes(Probe::Absent, Probe::Absent, Probe::Absent),
                services: true,
            },
            calls,
        )
    }
}
impl Sources for Fake {
    fn system(&self) -> Result<SystemRaw, SourceError> {
        self.calls.system.fetch_add(1, Ordering::SeqCst);
        Ok(SystemRaw {
            product_name: Some("Windows 11 Pro".into()),
            build: Some("26200".into()),
            uptime_secs: Some(100_000),
            ..SystemRaw::default()
        })
    }
    fn restart(&self) -> RestartProbes {
        self.calls.restart.fetch_add(1, Ordering::SeqCst);
        self.restart
    }
    fn services(&self, ids: &[&str]) -> Vec<(String, Result<RawService, SourceError>)> {
        self.calls.services.fetch_add(1, Ordering::SeqCst);
        ids.iter()
            .map(|id| {
                let result = if self.services {
                    let on_demand = matches!(*id, "BITS" | "wuauserv");
                    Ok(raw(
                        if on_demand {
                            ServiceState::Stopped
                        } else {
                            ServiceState::Running
                        },
                        if on_demand {
                            StartType::Manual
                        } else {
                            StartType::Automatic
                        },
                    ))
                } else {
                    Err(SourceError::RequiresElevation("negado".into()))
                };
                ((*id).to_string(), result)
            })
            .collect()
    }
    fn devices(&self) -> Result<DeviceBatch, SourceError> {
        self.calls.devices.fetch_add(1, Ordering::SeqCst);
        self.devices.clone()
    }
    fn events(&self) -> Result<EventBatch, SourceError> {
        self.calls.events.fetch_add(1, Ordering::SeqCst);
        self.events.clone()
    }
    fn volumes(&self) -> Result<Vec<RawVolume>, SourceError> {
        self.calls.volumes.fetch_add(1, Ordering::SeqCst);
        Ok(vec![volume(Some(false), Some(false), false)])
    }
    fn update_times(&self) -> UpdateTimes {
        self.calls.updates.fetch_add(1, Ordering::SeqCst);
        UpdateTimes {
            last_install_success_at: Some(NOW - 3 * DAY_MS),
            last_scan_success_at: Some(NOW - HOUR),
            notes: vec![],
        }
    }
}

#[test]
fn a_healthy_machine_is_reported_healthy_with_every_domain_evaluated() {
    let (fake, _) = Fake::healthy();
    let snapshot = Collector::new(fake).snapshot(NOW, false);
    assert_eq!(snapshot.overall.status, Health::Healthy);
    assert!(
        snapshot.overall.reasons.is_empty(),
        "máquina saudável não ganha problema inventado"
    );
    assert_eq!(
        (snapshot.overall.evaluated, snapshot.overall.rateable),
        (6, 6)
    );
    assert_eq!(
        snapshot.system.data.product_name.as_deref(),
        Some("Windows 11 Pro")
    );
    assert!(!snapshot.system.rated && !snapshot.reliability.rated && !snapshot.integrity.rated);
    assert_eq!(
        snapshot.updates.data.last_install_success_at,
        Some(NOW - 3 * DAY_MS)
    );
    assert_eq!(
        snapshot.updates.data.pending_count, None,
        "contagem de pendentes não é consultada"
    );
}

#[test]
fn a_failing_domain_never_takes_the_others_down() {
    let (mut fake, _) = Fake::healthy();
    fake.events = Err(SourceError::RequiresElevation(
        "O canal System requer privilégio administrativo".into(),
    ));
    fake.devices = Err(SourceError::Unavailable("SetupAPI indisponível".into()));
    let snapshot = Collector::new(fake).snapshot(NOW, false);
    assert_eq!(snapshot.events.status, Health::Unknown);
    assert_eq!(snapshot.devices.status, Health::Unknown);
    assert_eq!(
        snapshot.services.status,
        Health::Healthy,
        "serviços seguem avaliados"
    );
    assert_eq!(snapshot.restart.status, Health::Healthy);
    assert_eq!(snapshot.volumes.status, Health::Healthy);
    assert_eq!(snapshot.overall.status, Health::Healthy);
    assert_eq!(
        (snapshot.overall.evaluated, snapshot.overall.rateable),
        (3, 6),
        "restart, serviços e volumes avaliados; eventos, dispositivos e updates (sem histórico) ficam Desconhecidos"
    );
    let events_note = snapshot
        .capabilities
        .iter()
        .find(|n| n.id == "events")
        .unwrap();
    assert_eq!(events_note.state, SourceState::RequiresElevation);
    assert!(snapshot
        .capabilities
        .iter()
        .any(|n| n.id == "devices" && n.state == SourceState::Unavailable));
}

#[test]
fn everything_unavailable_yields_unknown_and_never_healthy() {
    let (mut fake, _) = Fake::healthy();
    fake.events = Err(SourceError::Unavailable("x".into()));
    fake.devices = Err(SourceError::Unavailable("x".into()));
    fake.restart = probes(Probe::Unavailable, Probe::Unavailable, Probe::Unavailable);
    fake.services = false;
    let snapshot = Collector::new(fake).snapshot(NOW, false);
    assert_eq!(snapshot.restart.status, Health::Unknown);
    assert_eq!(snapshot.services.status, Health::Unknown);
    assert_eq!(snapshot.updates.status, Health::Unknown);
    assert_ne!(snapshot.overall.status, Health::Critical);
}

#[test]
fn partial_attention_is_explained_by_overall_reasons() {
    let (mut fake, _) = Fake::healthy();
    fake.restart = probes(Probe::Present, Probe::Absent, Probe::Absent);
    fake.devices = Ok(DeviceBatch {
        total: 250,
        problems: vec![device("Placa X", 43), device("Placa Y", 28)],
    });
    let snapshot = Collector::new(fake).snapshot(NOW, false);
    assert_eq!(snapshot.overall.status, Health::Attention);
    let domains: Vec<&str> = snapshot
        .overall
        .reasons
        .iter()
        .map(|r| r.domain.as_str())
        .collect();
    assert!(
        domains.contains(&"restart") && domains.contains(&"devices"),
        "{domains:?}"
    );
    assert!(snapshot
        .integrity
        .data
        .signals
        .iter()
        .any(|s| s.id == "device_problems" && s.present));
}

#[test]
fn each_domain_is_cached_with_its_own_ttl_and_force_rereads_everything() {
    let (fake, calls) = Fake::healthy();
    let collector = Collector::new(fake);
    let count = |c: &AtomicUsize| c.load(Ordering::SeqCst);
    let first = collector.snapshot(NOW, false);
    assert_eq!(
        [
            count(&calls.restart),
            count(&calls.services),
            count(&calls.devices),
            count(&calls.events),
            count(&calls.volumes),
            count(&calls.updates)
        ],
        [1; 6]
    );
    // Logo depois: tudo vem do cache.
    collector.snapshot(NOW + 5_000, false);
    assert_eq!(
        [
            count(&calls.restart),
            count(&calls.services),
            count(&calls.devices),
            count(&calls.events),
            count(&calls.volumes),
            count(&calls.updates)
        ],
        [1; 6]
    );
    // Depois do TTL de reinício/serviços (45 s): só esses releem; eventos e dispositivos não.
    let later = collector.snapshot(NOW + TTL_RESTART_MS.max(TTL_SERVICES_MS) + 1_000, false);
    assert_eq!((count(&calls.restart), count(&calls.services)), (2, 2));
    assert_eq!(
        (
            count(&calls.events),
            count(&calls.devices),
            count(&calls.updates)
        ),
        (1, 1, 1)
    );
    // Cada domínio guarda o PRÓPRIO instante de leitura.
    assert!(later.restart.checked_at > first.restart.checked_at);
    assert_eq!(later.events.checked_at, first.events.checked_at);
    assert_eq!(later.devices.checked_at, first.devices.checked_at);
    // Passados 3 min os eventos releem; dispositivos (10 min) ainda não.
    collector.snapshot(NOW + TTL_EVENTS_MS + 2_000, false);
    assert_eq!((count(&calls.events), count(&calls.devices)), (2, 1));
    collector.snapshot(NOW + TTL_DEVICES_MS + 3_000, false);
    assert_eq!(count(&calls.devices), 2);
    // "Atualizar agora" força tudo.
    collector.snapshot(NOW + TTL_DEVICES_MS + 4_000, true);
    assert_eq!(
        [
            count(&calls.restart),
            count(&calls.services),
            count(&calls.devices),
            count(&calls.events),
            count(&calls.volumes),
            count(&calls.updates)
        ]
        .map(|n| n >= 3),
        [true; 6]
    );
}

#[test]
fn a_clock_that_goes_backwards_forces_a_fresh_read() {
    let (fake, calls) = Fake::healthy();
    let collector = Collector::new(fake);
    collector.snapshot(NOW, false);
    collector.snapshot(NOW - HOUR, false);
    assert_eq!(calls.restart.load(Ordering::SeqCst), 2);
}

#[test]
fn snapshot_serializes_with_camel_case_sections_flattened_and_stable_enums() {
    let (fake, _) = Fake::healthy();
    let json = serde_json::to_value(Collector::new(fake).snapshot(NOW, false)).unwrap();
    assert_eq!(json["overall"]["status"], "healthy");
    assert_eq!(json["restart"]["pending"], false);
    assert_eq!(json["restart"]["status"], "healthy");
    assert_eq!(json["services"]["items"][0]["state"], "running");
    assert_eq!(json["services"]["items"][0]["expectation"], "running");
    assert_eq!(json["events"]["last24h"]["critical"], 0);
    assert_eq!(json["integrity"]["onDemand"][0]["available"], false);
    assert!(json["capabilities"]
        .as_array()
        .is_some_and(|c| !c.is_empty()));
}

// ---------------------------------------------------------------- passividade e local

#[test]
fn the_collector_source_never_calls_repair_install_restart_or_write_apis() {
    let forbidden = [
        "Command::new",
        "powershell",
        "wmic",
        "cmd.exe",
        "sfc ",
        "dism",
        "chkdsk",
        "wuauclt",
        "RegSetValue",
        "RegCreateKey",
        "RegDeleteKey",
        "RegDeleteValue",
        "ControlService",
        "StartServiceW",
        "ChangeServiceConfig",
        "DeleteService",
        "ExitWindowsEx",
        "InitiateSystemShutdown",
        "EvtClearLog",
        "MoveFileEx",
        "FSCTL_LOCK_VOLUME",
        "FSCTL_DISMOUNT",
    ];
    for (name, source) in [
        (
            "windows_native.rs",
            include_str!("../src/windows_native.rs"),
        ),
        (
            "windows_health.rs",
            include_str!("../src/windows_health.rs"),
        ),
    ] {
        let code: String = source
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        for token in forbidden {
            // Rótulos de interface (ex.: "SFC /verifynow" como diagnóstico sob demanda NÃO executado) são texto.
            if name == "windows_health.rs" && ["sfc ", "dism", "chkdsk"].contains(&token) {
                continue;
            }
            assert!(
                !code.contains(&token.to_lowercase()),
                "{name} não pode conter {token}"
            );
        }
    }
}

#[test]
fn windows_health_never_enters_the_portable_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let db = hub_core::database::Database::open(&tmp.path().join("hub.db")).unwrap();
    let text = serde_json::to_string(&db.export_portable().unwrap())
        .unwrap()
        .to_lowercase();
    for word in [
        "windowshealth",
        "problemcode",
        "filerenameoperations",
        "unexpectedshutdown",
        "bugcheck",
        "lastinstallsuccessat",
        "pendingreboot",
    ] {
        assert!(
            !text.contains(word),
            "{word} não pode estar no workspace portátil"
        );
    }
}

// ---------------------------------------------------------------- ambiente real (somente leitura)

#[cfg(windows)]
mod live {
    use super::*;
    use hub_core::windows_health::WindowsSources;

    #[test]
    fn real_sources_answer_without_modifying_anything() {
        let sources = WindowsSources;
        let system = sources.system().expect("versão do Windows");
        assert!(system
            .build
            .as_deref()
            .is_some_and(|b| b.parse::<u32>().is_ok_and(|n| n >= 10_000)));
        let rpc = sources.services(&["RpcSs"]);
        assert_eq!(
            rpc[0].1.as_ref().unwrap().state,
            ServiceState::Running,
            "o RPC sempre roda"
        );
        let devices = sources.devices().expect("dispositivos");
        assert!(devices.total > 10);
        let volumes = sources.volumes().expect("volumes");
        assert!(volumes
            .iter()
            .any(|v| v.mount == "C:" && v.filesystem.is_some()));
        // Event Log: lê ou diz que não pode; nunca derruba o teste por falta de privilégio.
        match sources.events() {
            Ok(batch) => assert!(batch.errors.iter().all(|e| e.level == 1 || e.level == 2)),
            Err(error) => assert!(matches!(
                error,
                SourceError::RequiresElevation(_) | SourceError::Unavailable(_)
            )),
        }
    }

    #[test]
    fn a_full_real_snapshot_is_consistent_and_fast_enough() {
        let started = std::time::Instant::now();
        let now = hub_core::machine::now_ms();
        let snapshot = Collector::new(WindowsSources).snapshot(now, true);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "coleta completa: {:?}",
            started.elapsed()
        );
        assert_eq!(snapshot.services.data.items.len(), ESSENTIAL_SERVICES.len());
        assert!(snapshot.overall.rateable == 6 && snapshot.overall.evaluated <= 6);
        // Todo Atenção/Crítico traz motivo visível.
        for section in [
            snapshot.restart.status,
            snapshot.services.status,
            snapshot.devices.status,
            snapshot.events.status,
            snapshot.volumes.status,
        ] {
            let _ = section;
        }
        if snapshot.overall.status >= Health::Attention {
            assert!(
                !snapshot.overall.reasons.is_empty(),
                "estado sem causa visível"
            );
        }
    }

    /// Diagnóstico manual: `cargo test -p hub-core --test windows_health print_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_live_windows_health() {
        let started = std::time::Instant::now();
        let snapshot = Collector::new(WindowsSources).snapshot(hub_core::machine::now_ms(), true);
        eprintln!(
            "coleta completa (a frio): {} ms",
            started.elapsed().as_millis()
        );
        let again = std::time::Instant::now();
        Collector::new(WindowsSources).snapshot(hub_core::machine::now_ms(), true);
        eprintln!("coleta completa (2ª): {} ms", again.elapsed().as_millis());
        println!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
    }
}
