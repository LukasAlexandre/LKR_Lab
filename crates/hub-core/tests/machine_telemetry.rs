//! Machine Telemetry (SESSION-002, Block 06): CPU por núcleo, memória/commit, GPU (vendor e
//! driver), disco físico, rede por interface, bateria, aquecimento (warm-up), disponibilidade por
//! domínio, isolamento de falhas e passividade. Dado ausente é `None`/`Unavailable`: nunca 0.
use hub_core::{
    sensors::{
        adapter_type, classify_if_type, format_driver_version, interpret_power, physical_gpus,
        vendor_name, AdapterKind, NetAdapter, Pci, PowerStatus, RawAdapter,
    },
    telemetry::{
        battery_from, build_disk_devices, build_interfaces, domain_availability,
        parse_disk_instance, Availability, Capabilities, DiskCounters, Domain, InterfaceCounters,
        Plan, Sampler,
    },
};
use std::collections::HashMap;

fn pairs(items: &[(&str, f64)]) -> Vec<(String, f64)> {
    items.iter().map(|(n, v)| (n.to_string(), *v)).collect()
}

// ---------------------------------------------------------------- disco físico

#[test]
fn disk_instances_parse_number_and_volume_letters() {
    assert_eq!(
        parse_disk_instance("0 C:"),
        Some((0, vec!["C:".to_string()]))
    );
    assert_eq!(
        parse_disk_instance("1 D: E:"),
        Some((1, vec!["D:".to_string(), "E:".to_string()]))
    );
    assert_eq!(parse_disk_instance("2"), Some((2, vec![])));
    assert_eq!(parse_disk_instance("_Total"), None);
    assert_eq!(parse_disk_instance(""), None);
}

#[test]
fn disk_devices_separate_physical_disks_from_volumes_and_never_invent_values() {
    let read = pairs(&[
        ("0 C:", 5_000_000.0),
        ("1 D:", 0.0),
        ("_Total", 5_000_000.0),
    ]);
    let write = pairs(&[("0 C:", 1_000.0), ("_Total", 1_000.0)]);
    let reads = pairs(&[("0 C:", 120.0)]);
    let idle = pairs(&[("0 C:", 40.0), ("1 D:", 100.0), ("_Total", 70.0)]);
    let models = HashMap::from([
        (0, ("SAMSUNG MZALQ512HBLU".to_string(), true)),
        (1, ("KINGSTON SNV2S1000G".to_string(), true)),
    ]);
    let devices = build_disk_devices(
        &DiskCounters {
            read_bytes: &read,
            write_bytes: &write,
            read_ops: &reads,
            write_ops: &[],
            idle: &idle,
        },
        &models,
    );
    assert_eq!(devices.len(), 2, "_Total não é um disco");
    let (c, d) = (&devices[0], &devices[1]);
    assert_eq!((c.number, c.volumes.clone()), (0, vec!["C:".to_string()]));
    assert_eq!(c.model.as_deref(), Some("SAMSUNG MZALQ512HBLU"));
    assert!(c.nvme);
    assert_eq!(c.read_per_sec, Some(5_000_000.0));
    assert_eq!(c.read_ops_per_sec, Some(120.0));
    assert_eq!(
        c.write_ops_per_sec, None,
        "contador ausente: nada de 0 inventado"
    );
    assert_eq!(c.activity, Some(60.0), "atividade = 100 - % ocioso");
    assert_eq!(d.write_per_sec, None);
    assert_eq!(d.activity, Some(0.0));
}

#[test]
fn disk_devices_without_counters_are_empty_during_warmup_and_unknown_model_stays_none() {
    let none: Vec<(String, f64)> = Vec::new();
    let empty = build_disk_devices(
        &DiskCounters {
            read_bytes: &none,
            write_bytes: &none,
            read_ops: &none,
            write_ops: &none,
            idle: &none,
        },
        &HashMap::new(),
    );
    assert!(empty.is_empty());
    let idle = pairs(&[("3", 90.0)]);
    let unknown = build_disk_devices(
        &DiskCounters {
            read_bytes: &none,
            write_bytes: &none,
            read_ops: &none,
            write_ops: &none,
            idle: &idle,
        },
        &HashMap::new(),
    );
    assert_eq!(unknown[0].model, None);
    assert!(!unknown[0].nvme);
}

// ---------------------------------------------------------------- rede

fn adapter(name: &str, kind: AdapterKind, up: bool, speed: Option<u64>, ip: &str) -> NetAdapter {
    NetAdapter {
        name: name.into(),
        description: format!("{name} adapter"),
        kind,
        up,
        link_speed_bps: speed,
        ipv4: if ip.is_empty() {
            vec![]
        } else {
            vec![ip.into()]
        },
        ipv6: vec![],
        if_index: 0,
        mac: None,
        dhcp_v4: false,
        ipv4_metric: None,
        ipv4_prefix: None,
        gateways: vec![],
        dns: vec![],
        network_guid: None,
    }
}
fn counters(name: &str, rx: u64, tx: u64, ip: &str) -> InterfaceCounters {
    InterfaceCounters {
        name: name.into(),
        received: rx,
        transmitted: tx,
        total_received: rx * 10,
        total_transmitted: tx * 10,
        ipv4: if ip.is_empty() {
            vec![]
        } else {
            vec![ip.into()]
        },
    }
}

#[test]
fn network_lists_every_interface_active_first_without_loopback() {
    let adapters = vec![
        adapter("Ethernet", AdapterKind::Ethernet, false, None, ""),
        adapter(
            "Loopback Pseudo-Interface 1",
            AdapterKind::Loopback,
            true,
            None,
            "127.0.0.1",
        ),
        adapter(
            "Wi-Fi",
            AdapterKind::Wifi,
            true,
            Some(574_000_000),
            "192.168.1.42",
        ),
        adapter(
            "vEthernet (WSL)",
            AdapterKind::Ethernet,
            true,
            Some(10_000_000_000),
            "172.20.0.1",
        ),
    ];
    let c = vec![
        counters("Wi-Fi", 1_000_000, 250_000, "192.168.1.42"),
        counters("vEthernet (WSL)", 0, 0, "172.20.0.1"),
    ];
    let list = build_interfaces(&adapters, &c, 2_000, Some("Wi-Fi"));
    let names: Vec<&str> = list.iter().map(|i| i.name.as_str()).collect();
    assert!(!names.iter().any(|n| n.contains("Loopback")));
    assert_eq!(names[0], "Wi-Fi", "a interface ativa vem primeiro");
    assert!(list[0].active && list.iter().filter(|i| i.active).count() == 1);
    let wifi = &list[0];
    assert_eq!(wifi.kind, "wifi");
    assert_eq!(wifi.link_speed_bps, Some(574_000_000));
    // 1 000 000 bytes em 2 s = 500 000 B/s = 4 Mbit/s.
    assert_eq!(wifi.download_bps, Some(4_000_000.0));
    assert_eq!(wifi.upload_bps, Some(1_000_000.0));
    let eth = list.iter().find(|i| i.name == "Ethernet").unwrap();
    assert_eq!(eth.up, Some(false));
    assert_eq!(
        eth.link_speed_bps, None,
        "desconectada: sem velocidade inventada"
    );
    assert_eq!(eth.download_bps, None, "sem contador: sem taxa");
}

#[test]
fn network_first_sample_has_no_rates_and_unmatched_counters_stay_listed() {
    let adapters = vec![adapter(
        "Wi-Fi",
        AdapterKind::Wifi,
        true,
        Some(1),
        "10.0.0.2",
    )];
    let c = vec![
        counters("Wi-Fi", 999, 999, "10.0.0.2"),
        counters("Interface Misteriosa", 5, 5, ""),
    ];
    let first = build_interfaces(&adapters, &c, 0, None);
    assert!(first
        .iter()
        .all(|i| i.download_bps.is_none() && i.upload_bps.is_none()));
    let mystery = first
        .iter()
        .find(|i| i.name == "Interface Misteriosa")
        .unwrap();
    assert_eq!((mystery.kind.as_str(), mystery.up), ("other", None));
    assert_eq!(first.len(), 2, "nada é descartado nem duplicado");
}

#[test]
fn network_matches_counters_by_description_or_shared_ip() {
    let adapters = vec![adapter(
        "Wi-Fi",
        AdapterKind::Wifi,
        true,
        Some(1),
        "10.0.0.2",
    )];
    let by_description = vec![counters("Wi-Fi adapter", 100, 100, "")];
    assert_eq!(
        build_interfaces(&adapters, &by_description, 1_000, None).len(),
        1
    );
    let by_ip = vec![counters("nome diferente", 100, 100, "10.0.0.2")];
    let list = build_interfaces(&adapters, &by_ip, 1_000, None);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].download_bps, Some(800.0));
}

#[test]
fn interface_types_are_classified_conservatively() {
    assert_eq!(classify_if_type(6), AdapterKind::Ethernet);
    assert_eq!(classify_if_type(71), AdapterKind::Wifi);
    assert_eq!(classify_if_type(24), AdapterKind::Loopback);
    assert_eq!(classify_if_type(131), AdapterKind::Tunnel);
    assert_eq!(classify_if_type(9999), AdapterKind::Other);
}

// ---------------------------------------------------------------- bateria

#[test]
fn desktop_without_battery_is_not_applicable_never_zero_percent() {
    // BatteryFlag 128 = "sem bateria do sistema"; percent 255 = desconhecido.
    let desktop = interpret_power(1, 128, 255, u32::MAX);
    assert!(!desktop.battery_present);
    assert_eq!(desktop.percent, None);
    assert_eq!(desktop.charging, None);
    let battery = battery_from(Some(&desktop));
    assert!(!battery.present && battery.percent.is_none() && battery.remaining_secs.is_none());
    assert_eq!(battery.ac_online, Some(true));
    assert!(!battery_from(None).present);
    let unknown = interpret_power(255, 255, 255, u32::MAX);
    assert!(!unknown.battery_present && unknown.ac_online.is_none());
}

#[test]
fn notebook_battery_reports_charge_ac_charging_and_remaining_time() {
    let on_ac: PowerStatus = interpret_power(1, 9, 87, u32::MAX); // alta + carregando
    assert!(on_ac.battery_present);
    assert_eq!(
        (on_ac.percent, on_ac.ac_online, on_ac.charging),
        (Some(87), Some(true), Some(true))
    );
    assert_eq!(on_ac.remaining_secs, None, "na tomada não há autonomia");
    let on_battery = interpret_power(0, 1, 63, 7_200);
    assert_eq!(on_battery.charging, Some(false));
    assert_eq!(on_battery.remaining_secs, Some(7_200));
    let full = interpret_power(1, 1, 100, u32::MAX);
    assert_eq!((full.percent, full.charging), (Some(100), Some(false)));
    let t = battery_from(Some(&on_battery));
    assert_eq!(
        (t.present, t.percent, t.remaining_secs),
        (true, Some(63.0), Some(7_200))
    );
    assert_eq!(
        interpret_power(0, 1, 255, 100).percent,
        None,
        "255% não existe"
    );
}

// ---------------------------------------------------------------- GPU

fn pci(bus: u32, device: u32) -> Option<Pci> {
    Some(Pci {
        bus,
        device,
        function: 0,
    })
}
fn gpu(luid: u64, name: &str, vendor: u32, driver: u64, p: Option<Pci>) -> RawAdapter {
    RawAdapter {
        luid,
        name: name.into(),
        dedicated: Some(4 << 30),
        shared: Some(8 << 30),
        flags: Some(adapter_type::RENDER | adapter_type::DISPLAY), // GPU física
        pci: p,
        vendor_id: Some(vendor),
        driver_version: Some(driver),
    }
}

#[test]
fn vendor_and_driver_come_from_real_ids_only() {
    assert_eq!(vendor_name(0x10DE), Some("NVIDIA"));
    assert_eq!(vendor_name(0x8086), Some("Intel"));
    assert_eq!(vendor_name(0x1002), Some("AMD"));
    assert_eq!(
        vendor_name(0x1234),
        None,
        "id desconhecido: sem fabricante inventado"
    );
    assert_eq!(
        format_driver_version(0x0001_0002_0003_0004).as_deref(),
        Some("1.2.3.4")
    );
    assert_eq!(format_driver_version(0), None);
}

#[test]
fn integrated_plus_discrete_gpu_each_keep_their_own_vendor_and_driver() {
    let adapters = physical_gpus(vec![
        gpu(
            0x10,
            "Intel(R) Iris(R) Xe Graphics",
            0x8086,
            0x0001_0000_0064_0001,
            pci(0, 2),
        ),
        gpu(
            0x20,
            "NVIDIA GeForce GTX 1650",
            0x10DE,
            0x0001_000E_0000_2F6B,
            pci(1, 0),
        ),
    ]);
    assert_eq!(adapters.len(), 2, "iGPU e dGPU são duas GPUs");
    assert_eq!(adapters[0].vendor.as_deref(), Some("Intel"));
    assert_eq!(adapters[1].vendor.as_deref(), Some("NVIDIA"));
    assert_ne!(adapters[0].driver_version, adapters[1].driver_version);
    assert!(adapters.iter().all(|a| a.driver_version.is_some()));
}

#[test]
fn gpu_without_vendor_or_driver_info_stays_none() {
    let mut raw = gpu(0x30, "GPU Qualquer", 0x1234, 0, pci(3, 0));
    raw.vendor_id = None;
    raw.driver_version = None;
    let adapters = physical_gpus(vec![raw]);
    assert_eq!(
        (
            adapters[0].vendor.clone(),
            adapters[0].driver_version.clone()
        ),
        (None, None)
    );
}

// ---------------------------------------------------------------- disponibilidade por domínio

fn caps(all: Availability) -> Capabilities {
    Capabilities {
        cpu_usage: all,
        cpu_clock: all,
        memory_usage: all,
        gpu_usage: all,
        gpu_memory: all,
        gpu_process_usage: all,
        cpu_package_temperature: all,
        gpu_temperature: all,
        storage_temperature: all,
        thermal_zone_temperature: all,
        motherboard_temperature: all,
        disk_io: all,
        disk_activity: all,
        network_rate: all,
        process_disk_io: all,
        storage_physical_health: all,
        cpu_per_core: all,
        cpu_base_clock: all,
        memory_commit: all,
        battery: all,
        battery_health: all,
        disk_per_device: all,
        network_interfaces: all,
    }
}

#[test]
fn domains_are_available_partial_or_unavailable_by_what_the_machine_delivers() {
    let full = domain_availability(&caps(Availability::Available), 2);
    assert_eq!(full.cpu, Domain::Available);
    assert_eq!(full.temperatures, Domain::Available);
    assert_eq!(full.battery, Domain::Available);

    let none = domain_availability(&caps(Availability::Unavailable), 0);
    assert_eq!(none.gpu, Domain::Unavailable);
    assert_eq!(none.battery, Domain::Unavailable);
    assert_eq!(none.temperatures, Domain::Unavailable);
    assert_eq!(none.network, Domain::Unavailable);
}

#[test]
fn partial_telemetry_does_not_turn_missing_sensors_into_failure() {
    // Esta máquina: tem GPU, SSD com temperatura e tudo de CPU/rede, mas sem temperatura de CPU.
    let mut c = caps(Availability::Available);
    c.cpu_package_temperature = Availability::Unavailable;
    c.battery_health = Availability::Unavailable;
    let d = domain_availability(&c, 2);
    assert_eq!(d.temperatures, Domain::Partial);
    assert_eq!(
        d.cpu,
        Domain::Available,
        "faltar temperatura não rebaixa a CPU"
    );
    assert_eq!(
        d.battery,
        Domain::Available,
        "saúde da bateria é outro campo"
    );
    // GPU presente sem métricas: identidade conhecida, medição ausente.
    let mut g = caps(Availability::Available);
    (g.gpu_usage, g.gpu_memory, g.gpu_temperature) = (
        Availability::Unavailable,
        Availability::Unavailable,
        Availability::Unavailable,
    );
    assert_eq!(domain_availability(&g, 1).gpu, Domain::Partial);
}

#[test]
fn one_failing_domain_never_degrades_the_others() {
    let mut c = caps(Availability::Available);
    c.gpu_usage = Availability::Unavailable;
    c.gpu_memory = Availability::Unavailable;
    c.gpu_temperature = Availability::Unavailable;
    let d = domain_availability(&c, 1);
    assert_eq!(d.gpu, Domain::Partial);
    assert_eq!(
        (d.cpu, d.memory, d.disk, d.network),
        (
            Domain::Available,
            Domain::Available,
            Domain::Available,
            Domain::Available
        )
    );
}

// ---------------------------------------------------------------- amostragem real

#[test]
fn live_sampler_warms_up_then_reports_cores_commit_and_rates() {
    let mut sampler = Sampler::new();
    let first = sampler.sample(1_000, Plan::FULL, true);
    assert!(
        !first.cpu.ready,
        "a primeira amostra de CPU ainda não tem intervalo"
    );
    assert!(
        !first.network.ready && !first.disk_io.ready,
        "taxas só a partir da segunda amostra"
    );
    assert!(first
        .network
        .interfaces
        .iter()
        .all(|i| i.download_bps.is_none()));
    std::thread::sleep(std::time::Duration::from_millis(400));
    let second = sampler.sample(3_000, Plan::FULL, true);
    assert!(second.cpu.ready && second.network.ready && second.disk_io.ready);
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    assert_eq!(
        second.cpu.cores.len(),
        threads,
        "um valor por processador lógico"
    );
    assert!(second.cpu.cores.iter().all(|c| (0.0..=100.0).contains(c)));
    assert!((0.0..=100.0).contains(&second.cpu.usage));
    assert!(second.memory.total > 0 && second.memory.available <= second.memory.total);
    assert!(second.uptime > 0 && second.boot_time > 0);
    // Rede: nunca loopback e nenhum valor absurdo (taxa negativa ou infinita).
    assert!(second
        .network
        .interfaces
        .iter()
        .all(|i| i.kind != "loopback"));
    assert!(second
        .network
        .interfaces
        .iter()
        .all(|i| i.download_bps.is_none_or(|v| v.is_finite() && v >= 0.0)));
    // Bateria: ausente nunca vira porcentagem.
    if !second.battery.present {
        assert!(second.battery.percent.is_none() && second.battery.remaining_secs.is_none());
    } else {
        assert!(second
            .battery
            .percent
            .is_none_or(|p| (0.0..=100.0).contains(&p)));
    }
    assert_eq!(
        second.capabilities.battery == Availability::Available,
        second.battery.present
    );
    assert_eq!(
        second.capabilities.battery_health,
        Availability::Unavailable
    );
    if cfg!(windows) {
        assert!(
            second.cpu.base_mhz.is_some_and(|m| m > 0.0),
            "clock base do registro"
        );
        assert!(second
            .memory
            .commit_limit
            .is_some_and(|l| l >= second.memory.total / 2));
        assert!(second.memory.commit_used.is_some());
        // Pagefile real (PDH): nunca maior que o pagefile e nunca o "swap" do sysinfo.
        assert!(second
            .memory
            .pagefile_used
            .is_none_or(|used| used <= second.memory.swap_total));
        assert!(!second.network.interfaces.is_empty());
    }
    // Sem sensor de CPU confiável: nunca um valor inventado.
    assert!(second
        .temperatures
        .iter()
        .filter(|t| t.id == "cpu")
        .all(|t| t.celsius.is_none()));
}

#[test]
fn partial_plans_degrade_domains_independently() {
    let mut sampler = Sampler::new();
    // Só carga: CPU e memória medidas; disco/rede/GPU ainda sem leitura própria.
    let load_only = sampler.sample(
        1_000,
        Plan {
            load: true,
            io: false,
            processes: false,
            temperatures: false,
        },
        false,
    );
    assert!(load_only.memory.total > 0);
    assert!(load_only.disk_io.devices.is_empty() && load_only.network.interfaces.is_empty());
    assert!(!load_only.disk_io.ready && !load_only.network.ready);
    assert_eq!(
        load_only.availability.cpu,
        Domain::Partial,
        "sem núcleos ainda: parcial, não falha"
    );
    assert_eq!(load_only.availability.network, Domain::Partial);
}

// ---------------------------------------------------------------- passividade e local

#[test]
fn collector_never_spawns_processes_or_calls_powershell() {
    for (name, source) in [
        ("telemetry.rs", include_str!("../src/telemetry.rs")),
        ("sensors.rs", include_str!("../src/sensors.rs")),
    ] {
        let code: String = source
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "Command::new",
            "powershell",
            "wmic",
            "Get-CimInstance",
            "tokio::process",
        ] {
            assert!(
                !code.to_lowercase().contains(&forbidden.to_lowercase()),
                "{name} não pode usar {forbidden}: telemetria é só API nativa em processo"
            );
        }
    }
}

#[test]
fn live_telemetry_never_enters_the_portable_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let db = hub_core::database::Database::open(&tmp.path().join("hub.db")).unwrap();
    let text = serde_json::to_string(&db.export_portable().unwrap())
        .unwrap()
        .to_lowercase();
    for word in [
        "downloadbps",
        "uploadbps",
        "commitused",
        "commitlimit",
        "remainingsecs",
        "readpersec",
        "domainavailability",
        "batterytelemetry",
        "driverversion",
    ] {
        assert!(
            !text.contains(word),
            "{word} não pode estar no workspace portátil"
        );
    }
}

/// Diagnóstico manual: `cargo test -p hub-core --test machine_telemetry print_live -- --ignored --nocapture`.
#[test]
#[ignore]
fn print_live_telemetry() {
    let mut sampler = Sampler::new();
    let created = std::time::Instant::now();
    sampler.sample(1_000, Plan::FULL, true);
    eprintln!(
        "primeira amostra completa: {} ms",
        created.elapsed().as_millis()
    );
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    let t = sampler.sample(3_000, Plan::FULL, true);
    // Custo por rodada: carga (1 s), E/S + rede + GPU (2 s), temperaturas (4 s) e completa.
    for (name, plan) in [
        (
            "carga",
            Plan {
                load: true,
                io: false,
                processes: false,
                temperatures: false,
            },
        ),
        (
            "E/S",
            Plan {
                load: false,
                io: true,
                processes: false,
                temperatures: false,
            },
        ),
        (
            "temperaturas",
            Plan {
                load: false,
                io: false,
                processes: false,
                temperatures: true,
            },
        ),
        ("completa", Plan::FULL),
    ] {
        let mut total = std::time::Duration::ZERO;
        for i in 0..5 {
            let started = std::time::Instant::now();
            sampler.sample(5_000 + i * 2_000, plan, true);
            total += started.elapsed();
        }
        eprintln!(
            "rodada {name}: média {} ms (5 amostras)",
            total.as_millis() / 5
        );
    }
    println!("{}", serde_json::to_string_pretty(&t).unwrap());
}
