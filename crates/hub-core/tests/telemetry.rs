//! Machine Telemetry e Machine Health (Concept 02): regras puras e o sampler real.
use hub_core::{
    health::{
        evaluate, free_percent, sustained, temperature_level, HealthStatus, Level, LoadSample,
        SensorSeries, SpaceSample, ThermalGroup,
    },
    sensors::{parse_engine, parse_luid},
    telemetry::{
        busiest_disk, by_luid, gpu_usage, memory_percent, normalize_cpu, rank, rate, reading_level,
        thresholds, Availability, Metric, Plan, ProcessEntry, Ring, Sampler, SensorSource,
        TemperatureReading, HISTORY_LEN,
    },
};

const T0: i64 = 1_790_000_000_000;
const GB: u64 = 1024 * 1024 * 1024;

fn load(seconds: i64, cpu: f32, memory: f32) -> Vec<LoadSample> {
    (0..=seconds)
        .map(|s| LoadSample {
            at: T0 + s * 1000,
            cpu,
            memory,
        })
        .collect()
}
fn now(seconds: i64) -> i64 {
    T0 + seconds * 1000
}
fn volume(mount: &str, total: u64, available: u64) -> SpaceSample {
    SpaceSample {
        mount: mount.into(),
        total,
        available,
    }
}
fn process(
    pid: u32,
    cpu: f32,
    memory: u64,
    gpu: Option<f32>,
    read: f64,
    write: f64,
) -> ProcessEntry {
    ProcessEntry {
        pid,
        name: format!("p{pid}.exe"),
        cpu,
        memory,
        gpu,
        disk_read: read,
        disk_write: write,
    }
}

// ---- cálculos ----

#[test]
fn cpu_is_normalized_to_the_whole_machine() {
    assert_eq!(normalize_cpu(400.0, 8), 50.0);
    assert_eq!(normalize_cpu(800.0, 8), 100.0);
    assert_eq!(normalize_cpu(900.0, 8), 100.0, "nunca passa de 100%");
    assert_eq!(normalize_cpu(12.0, 0), 0.0);
    assert_eq!(normalize_cpu(f32::NAN, 8), 0.0);
}

#[test]
fn memory_percent_uses_available_memory() {
    assert_eq!(memory_percent(32 * GB, 16 * GB), 50.0);
    assert_eq!(memory_percent(32 * GB, 32 * GB), 0.0);
    assert_eq!(memory_percent(0, 0), 0.0);
    assert!((memory_percent(24 * GB, 6 * GB) - 75.0).abs() < 0.01);
}

#[test]
fn disk_activity_is_the_busiest_physical_disk_not_the_average() {
    let idle = vec![
        ("0 C:".to_string(), 0.4),
        ("1 D:".to_string(), 100.0),
        ("_Total".to_string(), 50.2),
    ];
    let (disk, active) = busiest_disk(&idle).unwrap();
    assert_eq!(disk, "0 C:");
    assert!((active - 99.6).abs() < 0.01, "não 50% da média");
    assert_eq!(busiest_disk(&[("_Total".to_string(), 10.0)]), None);
    assert_eq!(busiest_disk(&[]), None);
    // Contador fora da faixa é limitado a 0–100%.
    assert_eq!(busiest_disk(&[("0 C:".to_string(), 140.0)]).unwrap().1, 0.0);
}

#[test]
fn rates_come_from_deltas_between_samples() {
    // Disco: 4 MB em 2 s = 2 MB/s. Rede: 1 MB em 500 ms.
    assert_eq!(rate(4 * 1024 * 1024, 2000), 2.0 * 1024.0 * 1024.0);
    assert_eq!(rate(1_000_000, 500), 2_000_000.0);
    // Primeira amostra (sem intervalo) não inventa taxa.
    assert_eq!(rate(123, 0), 0.0);
    assert_eq!(rate(123, -5), 0.0);
}

#[test]
fn gpu_usage_takes_the_busiest_engine_per_adapter_and_process() {
    let nvidia = "luid_0x00000000_0x00010088";
    let intel = "luid_0x00000000_0x0000fc4e";
    let engines = vec![
        (format!("pid_10_{nvidia}_phys_0_eng_0_engtype_3D"), 30.0),
        (format!("pid_11_{nvidia}_phys_0_eng_0_engtype_3D"), 25.0),
        (
            format!("pid_10_{nvidia}_phys_0_eng_5_engtype_VideoDecode"),
            40.0,
        ),
        (format!("pid_12_{intel}_phys_0_eng_0_engtype_3D"), 5.0),
        ("lixo".to_string(), 99.0),
    ];
    let (adapters, processes) = gpu_usage(&engines);
    assert_eq!(adapters[&0x10088], 55.0, "3D: 30 + 25 > decode 40");
    assert_eq!(adapters[&0xfc4e], 5.0);
    assert_eq!(
        processes[&10], 40.0,
        "decode é o motor mais ocupado do pid 10"
    );
    assert_eq!(processes[&11], 25.0);
    assert_eq!(parse_luid(nvidia), Some(0x10088));
    assert_eq!(
        parse_engine(&engines[0].0),
        Some((10, 0x10088, "3d".into()))
    );
    let memory = by_luid(&[(format!("{nvidia}_phys_0"), 144.0 * 1048576.0)]);
    assert_eq!(memory[&0x10088], 144 * 1048576);
}

// ---- ranking ----

#[test]
fn processes_are_ranked_by_the_selected_metric() {
    let list = vec![
        process(1, 5.0, 100, Some(1.0), 0.0, 0.0),
        process(2, 50.0, 10, Some(0.0), 10.0, 0.0),
        process(3, 1.0, 900, None, 0.0, 500.0),
        process(4, 50.0, 300, Some(60.0), 100.0, 100.0),
    ];
    let pids = |m| rank(&list, m, 3).iter().map(|p| p.pid).collect::<Vec<_>>();
    assert_eq!(
        pids(Metric::Cpu),
        vec![2, 4, 1],
        "empate de CPU: menor PID primeiro"
    );
    assert_eq!(pids(Metric::Memory), vec![3, 4, 1]);
    assert_eq!(pids(Metric::Disk), vec![3, 4, 2], "leitura + escrita");
    assert_eq!(
        pids(Metric::Gpu),
        vec![4, 1, 2],
        "sem dado de GPU fica fora"
    );
    assert!(rank(&[], Metric::Memory, 8).is_empty());
}

#[test]
fn ring_buffer_is_bounded() {
    let mut ring = Ring::new(3);
    for i in 0..10 {
        ring.push(i);
    }
    assert_eq!(ring.len(), 3);
    assert_eq!(ring.to_vec(), vec![7, 8, 9]);
    assert_eq!(HISTORY_LEN, 120);
}

#[test]
fn sampling_plan_depends_on_activity() {
    let active: Vec<Plan> = (0..10).map(|t| Plan::for_tick(t, true)).collect();
    assert!(active.iter().all(|p| p.load), "CPU/memória a cada 1 s");
    assert_eq!(
        active.iter().filter(|p| p.processes).count(),
        5,
        "processos a cada 2 s"
    );
    assert_eq!(
        active.iter().filter(|p| p.temperatures).count(),
        3,
        "temperaturas a cada 4 s (ticks 0, 4 e 8 de 10)"
    );
    let idle: Vec<Plan> = (0..60).map(|t| Plan::for_tick(t, false)).collect();
    assert!(
        idle.iter().all(|p| !p.processes),
        "sem ranking em segundo plano"
    );
    assert_eq!(idle.iter().filter(|p| p.load).count(), 12, "5 s");
    assert_eq!(idle.iter().filter(|p| p.io).count(), 6, "10 s");
}

// ---- saúde ----

#[test]
fn quiet_machine_is_healthy_without_alerts() {
    let health = evaluate(
        &load(200, 20.0, 50.0),
        &[volume("C:\\", 100 * GB, 50 * GB)],
        &[],
        now(200),
    );
    assert_eq!(health.status, HealthStatus::Healthy);
    assert!(health.alerts.is_empty());
    let alerts = health.checks.iter().find(|c| c.id == "alerts").unwrap();
    assert_eq!(alerts.label, "Nenhum alerta ativo");
}

#[test]
fn short_cpu_spike_is_not_an_alert() {
    let mut samples = load(200, 10.0, 40.0);
    for s in samples.iter_mut().rev().take(30) {
        s.cpu = 100.0;
    }
    let health = evaluate(&samples, &[], &[], now(200));
    assert_eq!(
        health.status,
        HealthStatus::Healthy,
        "30 s a 100% não basta"
    );
}

#[test]
fn sustained_cpu_becomes_attention_then_critical() {
    let attention = evaluate(&load(90, 92.0, 40.0), &[], &[], now(90));
    assert_eq!(attention.status, HealthStatus::Attention);
    assert_eq!(attention.alerts[0].source, "cpu");
    let critical = evaluate(&load(200, 97.0, 40.0), &[], &[], now(200));
    assert_eq!(critical.status, HealthStatus::Critical);
    // Mesmo a 100%, sem cobrir 180 s não é crítico.
    let early = evaluate(&load(120, 100.0, 40.0), &[], &[], now(120));
    assert_eq!(early.status, HealthStatus::Attention);
}

#[test]
fn sustained_memory_pressure_is_reported() {
    let attention = evaluate(&load(130, 10.0, 91.0), &[], &[], now(130));
    assert_eq!(attention.status, HealthStatus::Attention);
    let memory = attention.checks.iter().find(|c| c.id == "memory").unwrap();
    assert!(!memory.ok);
    let critical = evaluate(&load(190, 10.0, 96.0), &[], &[], now(190));
    assert_eq!(critical.status, HealthStatus::Critical);
    // Só 20 s acima de 95% depois de RAM normal: ainda não.
    let mut samples = load(190, 10.0, 50.0);
    for s in samples.iter_mut().rev().take(20) {
        s.memory = 99.0;
    }
    assert_eq!(
        evaluate(&samples, &[], &[], now(190)).status,
        HealthStatus::Healthy
    );
    // 60 s a 91% (a janela antiga) já não basta: é só uma oscilação.
    assert_eq!(
        evaluate(&load(60, 10.0, 91.0), &[], &[], now(60)).status,
        HealthStatus::Healthy
    );
}

#[test]
fn low_free_space_raises_storage_alerts() {
    assert!((free_percent(100, 7) - 7.0).abs() < f64::EPSILON);
    let health = evaluate(
        &load(10, 10.0, 40.0),
        &[
            volume("C:\\", 100 * GB, 9 * GB),
            volume("D:\\", 100 * GB, 3 * GB),
        ],
        &[],
        now(10),
    );
    assert_eq!(health.status, HealthStatus::Critical);
    assert_eq!(health.alerts.len(), 2);
    assert_eq!(
        health.alerts[0].severity,
        HealthStatus::Critical,
        "crítico primeiro"
    );
    assert!(health.alerts[0].title.contains("D:"));
    let storage = health.checks.iter().find(|c| c.id == "storage").unwrap();
    assert!(!storage.ok);
}

#[test]
fn missing_sensors_never_reduce_health_nor_claim_normal_temperatures() {
    let health = evaluate(&load(10, 10.0, 40.0), &[], &[], now(10));
    assert_eq!(health.status, HealthStatus::Healthy);
    // Sem leitura térmica, nenhum item de temperatura no checklist.
    let ids: Vec<&str> = health.checks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["memory", "storage", "alerts"]);
    assert!(health
        .checks
        .iter()
        .all(|c| !c.label.contains("Temperatura")));
    // Sensor sem leitura também não conta.
    let empty = SensorSeries {
        group: ThermalGroup::Gpu,
        label: "GPU".into(),
        readings: vec![],
        warning: 85.0,
        critical: 95.0,
    };
    let health = evaluate(&[], &[], &[empty], now(0));
    assert_eq!(health.status, HealthStatus::Healthy);
    assert!(health.checks.iter().all(|c| c.id != "gpu-temperature"));
}

#[test]
fn checklist_only_lists_what_was_observed() {
    let reading = |group, value| SensorSeries {
        group,
        label: "x".into(),
        readings: vec![(T0, value)],
        warning: 80.0,
        critical: 90.0,
    };
    let health = evaluate(
        &load(10, 10.0, 40.0),
        &[],
        &[
            reading(ThermalGroup::Gpu, 60.0),
            reading(ThermalGroup::Storage, 38.0),
        ],
        now(10),
    );
    let labels: Vec<&str> = health.checks.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "Memória dentro da faixa",
            "Armazenamento com espaço disponível",
            "GPUs monitoradas dentro dos limites observáveis",
            "SSDs dentro dos limites térmicos do dispositivo",
            "Nenhum alerta ativo",
        ]
    );
    assert!(health.checks.iter().all(|c| c.ok));
}

#[test]
fn hot_sensor_needs_sustained_readings() {
    let series = |values: &[f32]| SensorSeries {
        group: ThermalGroup::Storage,
        label: "SSD".into(),
        readings: values
            .iter()
            .enumerate()
            .map(|(i, v)| (T0 + i as i64 * 10_000, *v))
            .collect(),
        warning: 81.0,
        critical: 87.0,
    };
    let at = |n: usize| T0 + (n as i64 - 1) * 10_000;
    // Uma leitura alta isolada: normal.
    assert_eq!(
        evaluate(&[], &[], &[series(&[40.0, 40.0, 40.0, 90.0])], at(4)).status,
        HealthStatus::Healthy
    );
    // 30 s acima do aviso: atenção; acima do crítico: crítico.
    assert_eq!(
        evaluate(&[], &[], &[series(&[40.0, 82.0, 83.0, 84.0, 85.0])], at(5)).status,
        HealthStatus::Attention
    );
    assert_eq!(
        evaluate(&[], &[], &[series(&[88.0, 89.0, 90.0, 91.0])], at(4)).status,
        HealthStatus::Critical
    );
    let ok = evaluate(&[], &[], &[series(&[40.0, 41.0])], at(2));
    let thermal = ok
        .checks
        .iter()
        .find(|c| c.id == "storage-temperature")
        .unwrap();
    assert!(thermal.ok);
}

#[test]
fn sensor_levels_and_thresholds() {
    assert_eq!(temperature_level(None, 80.0, 90.0), Level::Unavailable);
    assert_eq!(temperature_level(Some(50.0), 80.0, 90.0), Level::Normal);
    assert_eq!(temperature_level(Some(80.0), 80.0, 90.0), Level::Attention);
    assert_eq!(temperature_level(Some(95.0), 80.0, 90.0), Level::Critical);
    let reading = |source, celsius: Option<f32>, limits: Option<(f32, f32)>| TemperatureReading {
        id: "x".into(),
        label: "x".into(),
        source,
        celsius,
        warning: limits.map(|l| l.0),
        critical: limits.map(|l| l.1),
        level: Level::Unavailable,
    };
    // SSD: só com os limites declarados pelo próprio dispositivo.
    assert_eq!(
        thresholds(&reading(
            SensorSource::Storage,
            Some(38.0),
            Some((81.0, 87.0))
        )),
        Some((ThermalGroup::Storage, 81.0, 87.0))
    );
    let no_limits = reading(SensorSource::Storage, Some(38.0), None);
    assert_eq!(thresholds(&no_limits), None);
    assert_eq!(reading_level(&no_limits), Level::Unrated);
    assert_eq!(
        thresholds(&reading(SensorSource::Gpu, Some(60.0), None)),
        Some((ThermalGroup::Gpu, 85.0, 95.0))
    );
    // Zona ACPI: exibida, nunca avaliada, mesmo muito quente.
    let zone = reading(SensorSource::ThermalZone, Some(93.0), None);
    assert_eq!(thresholds(&zone), None);
    assert_eq!(reading_level(&zone), Level::Unrated);
    assert_eq!(thresholds(&reading(SensorSource::Cpu, None, None)), None);
    assert_eq!(
        reading_level(&reading(SensorSource::Cpu, None, None)),
        Level::Unavailable
    );
    assert_eq!(
        thresholds(&reading(SensorSource::Motherboard, None, None)),
        None
    );
    assert_eq!(
        reading_level(&reading(SensorSource::Gpu, Some(96.0), None)),
        Level::Critical
    );
}

#[test]
fn sustained_requires_window_coverage() {
    let samples = load(10, 99.0, 0.0);
    assert!(sustained(
        &samples,
        |s| s.at,
        now(10),
        10_000,
        |s| s.cpu > 90.0
    ));
    assert!(!sustained(
        &samples,
        |s| s.at,
        now(10),
        11_000,
        |s| s.cpu > 90.0
    ));
    assert!(!sustained(
        &[] as &[LoadSample],
        |s| s.at,
        now(0),
        1,
        |_| true
    ));
}

// ---- sampler real ----

#[test]
fn real_sampler_reports_only_what_this_machine_can_measure() {
    let mut sampler = Sampler::new();
    let first = sampler.sample(T0, Plan::FULL, true);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let t = sampler.sample(T0 + 1100, Plan::FULL, true);
    assert!(first.cpu.usage >= 0.0 && t.cpu.usage <= 100.0);
    assert!(t.memory.total > 0 && t.memory.used <= t.memory.total);
    assert_eq!(t.capabilities.cpu_usage, Availability::Available);
    assert_eq!(
        t.capabilities.motherboard_temperature,
        Availability::Unavailable
    );
    assert_eq!(
        t.capabilities.storage_physical_health,
        Availability::Unavailable
    );
    // Sem fonte, sem valor.
    let cpu = t.temperatures.iter().find(|r| r.id == "cpu").unwrap();
    assert_eq!((cpu.celsius, cpu.level), (None, Level::Unavailable));
    for reading in &t.temperatures {
        assert_eq!(
            reading.celsius.is_none(),
            reading.level == Level::Unavailable,
            "{reading:?}"
        );
    }
    assert_eq!(
        t.capabilities.cpu_package_temperature,
        Availability::Unavailable
    );
    // Zona ACPI: nunca rotulada como CPU, nunca classificada.
    for zone in t
        .temperatures
        .iter()
        .filter(|r| r.source == SensorSource::ThermalZone)
    {
        assert!(zone.label.starts_with("Sensor térmico do sistema (ACPI"));
        assert_eq!(
            zone.level,
            if zone.celsius.is_some() {
                Level::Unrated
            } else {
                Level::Unavailable
            }
        );
    }
    for gpu in &t.gpus {
        if t.capabilities.gpu_usage == Availability::Unavailable {
            assert!(gpu.usage.is_none());
        }
        // Capability de temperatura por GPU coerente com o valor; 0 °C nunca vira leitura.
        assert_eq!(
            gpu.capabilities.temperature == Availability::Available,
            gpu.temperature.is_some()
        );
        assert!(gpu.temperature.is_none_or(|c| c > 0.0));
        // Dedicada e compartilhada separadas.
        if let (Some(used), Some(total)) = (gpu.dedicated_used, gpu.dedicated_total) {
            assert!(used <= total + 64 * 1024 * 1024, "{gpu:?}");
        }
    }
    let top = t.processes.as_ref().expect("ranking no modo ativo");
    assert!(!top.memory.is_empty());
    assert!(top.memory.windows(2).all(|w| w[0].memory >= w[1].memory));
    assert!(top.cpu.iter().all(|p| p.pid != 0 && p.cpu <= 100.0));
    if t.capabilities.gpu_process_usage == Availability::Unavailable {
        assert!(top.gpu.is_empty());
    }
    // Modo ocioso: nada de ranking velho.
    let idle = sampler.sample(T0 + 2200, Plan::for_tick(1, false), false);
    assert!(idle.processes.is_none());
    println!("{}", serde_json::to_string_pretty(&t.capabilities).unwrap());
    println!(
        "cpu {:.1}% clock {:?} | mem {:.1}% | disk {:?}% | gpus {:?} | temps {:?} | net {:?}",
        t.cpu.usage,
        t.cpu.clock_mhz,
        t.memory.percent,
        t.disk_io.activity,
        t.gpus
            .iter()
            .map(|g| (
                &g.name,
                g.usage,
                g.dedicated_used,
                g.dedicated_total,
                g.shared_used,
                g.temperature
            ))
            .collect::<Vec<_>>(),
        t.temperatures
            .iter()
            .map(|r| (&r.label, r.celsius, r.level))
            .collect::<Vec<_>>(),
        (t.network.download_bps, t.network.upload_bps),
    );
    println!(
        "top mem: {:?}",
        top.memory
            .iter()
            .take(3)
            .map(|p| (&p.name, p.memory))
            .collect::<Vec<_>>()
    );
    println!("health: {:?}", t.health.status);
}
