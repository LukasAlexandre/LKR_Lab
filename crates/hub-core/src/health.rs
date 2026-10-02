//! Machine Health: interpretação determinística da telemetria (Concept 02).
//!
//! Três estados (Saudável, Atenção, Crítico), sem nota numérica. Cada regra é uma função
//! pura sobre amostras com instante, para que um pico curto nunca vire alerta:
//!
//! | Sinal | Atenção | Crítico |
//! |---|---|---|
//! | CPU total | ≥ 90% em TODAS as amostras dos últimos 60 s | ≥ 95% em todas as dos últimos 180 s |
//! | Memória em uso | ≥ 90% nos últimos 30 s | ≥ 95% nos últimos 60 s |
//! | Espaço livre de um volume fixo | < 10% | < 5% |
//! | Temperatura de GPU/SSD com limite conhecido | ≥ aviso nas leituras dos últimos 30 s | ≥ crítico nos últimos 30 s |
//!
//! "Sustentado" exige cobertura: a janela inteira precisa estar coberta por amostras (a
//! mais antiga considerada é anterior ao início da janela) e todas atenderem à condição.
//! Limites de temperatura: SSD só com os limites declarados pelo próprio dispositivo;
//! GPU 85/95 °C. Só entram sensores de semântica conhecida: a zona térmica ACPI é exibida,
//! mas não avaliada (não se sabe o que ela mede), e a temperatura do pacote da CPU não tem
//! fonte. Sensor indisponível ou não avaliado nunca gera alerta nem item de checklist.
//! Sem SMART, a saúde física dos discos não é avaliada: só a capacidade.
use serde::Serialize;

pub const CPU_ATTENTION: f32 = 90.0;
pub const CPU_CRITICAL: f32 = 95.0;
pub const CPU_ATTENTION_WINDOW_MS: i64 = 60_000;
pub const CPU_CRITICAL_WINDOW_MS: i64 = 180_000;
pub const MEMORY_ATTENTION: f32 = 90.0;
pub const MEMORY_CRITICAL: f32 = 95.0;
pub const MEMORY_ATTENTION_WINDOW_MS: i64 = 30_000;
pub const MEMORY_CRITICAL_WINDOW_MS: i64 = 60_000;
pub const FREE_SPACE_ATTENTION: f64 = 10.0;
pub const FREE_SPACE_CRITICAL: f64 = 5.0;
pub const TEMPERATURE_WINDOW_MS: i64 = 30_000;
pub const GPU_TEMPERATURE: (f32, f32) = (85.0, 95.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HealthStatus {
    Healthy,
    Attention,
    Critical,
}

/// Nível de uma leitura isolada (ex.: um sensor), incluindo a ausência dela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Normal,
    Attention,
    Critical,
    /// Leitura válida de um sensor sem limite conhecido (ex.: zona térmica ACPI): exibida,
    /// nunca classificada.
    Unrated,
    Unavailable,
}

/// Grupo de sensores com semântica conhecida, avaliados pela saúde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalGroup {
    Gpu,
    Storage,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAlert {
    pub severity: HealthStatus,
    /// "cpu" | "memory" | "storage" | "temperature"
    pub source: String,
    pub title: String,
    pub detail: String,
}

/// Item do checklist "Saúde e alertas". Só existe para o que foi observado de fato.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    pub id: String,
    pub label: String,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineHealth {
    pub status: HealthStatus,
    pub alerts: Vec<MachineAlert>,
    pub checks: Vec<HealthCheck>,
}

/// Amostra mínima para as regras de CPU e memória.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadSample {
    pub at: i64,
    pub cpu: f32,
    pub memory: f32,
}

/// Volume fixo com capacidade.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceSample {
    pub mount: String,
    pub total: u64,
    pub available: u64,
}

/// Leituras recentes de um sensor (mais antiga primeiro), com seus limites.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorSeries {
    pub group: ThermalGroup,
    pub label: String,
    pub readings: Vec<(i64, f32)>,
    pub warning: f32,
    pub critical: f32,
}

/// Todas as amostras dos últimos `window` ms atendem `ok`, e a janela está coberta.
pub fn sustained<T>(
    samples: &[T],
    at: impl Fn(&T) -> i64,
    now: i64,
    window: i64,
    ok: impl Fn(&T) -> bool,
) -> bool {
    let start = now - window;
    for sample in samples.iter().rev() {
        if !ok(sample) {
            return false;
        }
        if at(sample) <= start {
            return true;
        }
    }
    false
}

pub fn free_percent(total: u64, available: u64) -> f64 {
    if total == 0 {
        return 100.0;
    }
    available as f64 * 100.0 / total as f64
}

/// Nível instantâneo de uma leitura (para colorir a interface).
pub fn temperature_level(celsius: Option<f32>, warning: f32, critical: f32) -> Level {
    match celsius {
        None => Level::Unavailable,
        Some(t) if t >= critical => Level::Critical,
        Some(t) if t >= warning => Level::Attention,
        Some(_) => Level::Normal,
    }
}

fn alert(severity: HealthStatus, source: &str, title: String, detail: String) -> MachineAlert {
    MachineAlert {
        severity,
        source: source.into(),
        title,
        detail,
    }
}

pub fn evaluate(
    load: &[LoadSample],
    volumes: &[SpaceSample],
    sensors: &[SensorSeries],
    now: i64,
) -> MachineHealth {
    let mut alerts = Vec::new();

    let cpu_over = |limit: f32| move |s: &LoadSample| s.cpu >= limit;
    if sustained(
        load,
        |s| s.at,
        now,
        CPU_CRITICAL_WINDOW_MS,
        cpu_over(CPU_CRITICAL),
    ) {
        alerts.push(alert(
            HealthStatus::Critical,
            "cpu",
            "CPU saturada".into(),
            format!("Uso de CPU acima de {CPU_CRITICAL:.0}% há mais de 3 minutos."),
        ));
    } else if sustained(
        load,
        |s| s.at,
        now,
        CPU_ATTENTION_WINDOW_MS,
        cpu_over(CPU_ATTENTION),
    ) {
        alerts.push(alert(
            HealthStatus::Attention,
            "cpu",
            "CPU muito ocupada".into(),
            format!("Uso de CPU acima de {CPU_ATTENTION:.0}% há mais de 1 minuto."),
        ));
    }

    let memory_over = |limit: f32| move |s: &LoadSample| s.memory >= limit;
    let memory_critical = sustained(
        load,
        |s| s.at,
        now,
        MEMORY_CRITICAL_WINDOW_MS,
        memory_over(MEMORY_CRITICAL),
    );
    let memory_attention = sustained(
        load,
        |s| s.at,
        now,
        MEMORY_ATTENTION_WINDOW_MS,
        memory_over(MEMORY_ATTENTION),
    );
    if memory_critical {
        alerts.push(alert(
            HealthStatus::Critical,
            "memory",
            "Memória quase esgotada".into(),
            format!("Mais de {MEMORY_CRITICAL:.0}% da RAM em uso há mais de 1 minuto."),
        ));
    } else if memory_attention {
        alerts.push(alert(
            HealthStatus::Attention,
            "memory",
            "Memória alta".into(),
            format!("Mais de {MEMORY_ATTENTION:.0}% da RAM em uso há mais de 30 segundos."),
        ));
    }

    let mut storage_ok = true;
    for volume in volumes {
        let free = free_percent(volume.total, volume.available);
        let severity = if free < FREE_SPACE_CRITICAL {
            HealthStatus::Critical
        } else if free < FREE_SPACE_ATTENTION {
            HealthStatus::Attention
        } else {
            continue;
        };
        storage_ok = false;
        alerts.push(alert(
            severity,
            "storage",
            format!("Pouco espaço em {}", volume.mount),
            format!("Só {free:.1}% livre."),
        ));
    }

    // Por grupo observado: (há leitura, todos dentro do limite).
    let mut gpu_thermal: Option<bool> = None;
    let mut storage_thermal: Option<bool> = None;
    for sensor in sensors {
        if sensor.readings.is_empty() {
            continue;
        }
        let group = match sensor.group {
            ThermalGroup::Gpu => &mut gpu_thermal,
            ThermalGroup::Storage => &mut storage_thermal,
        };
        group.get_or_insert(true);
        let over = |limit: f32| move |r: &(i64, f32)| r.1 >= limit;
        let severity = if sustained(
            &sensor.readings,
            |r| r.0,
            now,
            TEMPERATURE_WINDOW_MS,
            over(sensor.critical),
        ) {
            HealthStatus::Critical
        } else if sustained(
            &sensor.readings,
            |r| r.0,
            now,
            TEMPERATURE_WINDOW_MS,
            over(sensor.warning),
        ) {
            HealthStatus::Attention
        } else {
            continue;
        };
        *group = Some(false);
        let current = sensor.readings.last().map(|r| r.1).unwrap_or_default();
        alerts.push(alert(
            severity,
            "temperature",
            format!("{} quente", sensor.label),
            format!(
                "{current:.0} °C (limite de aviso {:.0} °C).",
                sensor.warning
            ),
        ));
    }

    alerts.sort_by_key(|a| std::cmp::Reverse(a.severity));
    let status = alerts
        .iter()
        .map(|a| a.severity)
        .max()
        .unwrap_or(HealthStatus::Healthy);
    let check = |id: &str, ok: bool, good: &str, bad: &str| HealthCheck {
        id: id.into(),
        label: if ok { good } else { bad }.into(),
        ok,
    };
    let memory_ok = !(memory_attention || memory_critical);
    let mut checks = vec![
        check(
            "memory",
            memory_ok,
            "Memória dentro da faixa",
            "Memória sob pressão",
        ),
        check(
            "storage",
            storage_ok,
            "Armazenamento com espaço disponível",
            "Armazenamento com pouco espaço",
        ),
    ];
    // Temperatura só aparece no checklist para o que foi medido de verdade.
    if let Some(ok) = gpu_thermal {
        checks.push(check(
            "gpu-temperature",
            ok,
            "GPUs monitoradas dentro dos limites observáveis",
            "GPU acima do limite térmico",
        ));
    }
    if let Some(ok) = storage_thermal {
        checks.push(check(
            "storage-temperature",
            ok,
            "SSDs dentro dos limites térmicos do dispositivo",
            "SSD acima do limite térmico do dispositivo",
        ));
    }
    checks.push(HealthCheck {
        id: "alerts".into(),
        label: if alerts.is_empty() {
            "Nenhum alerta ativo".into()
        } else {
            format!(
                "{} {} ativo{}",
                alerts.len(),
                if alerts.len() == 1 {
                    "alerta"
                } else {
                    "alertas"
                },
                if alerts.len() == 1 { "" } else { "s" }
            )
        },
        ok: alerts.is_empty(),
    });
    MachineHealth {
        status,
        alerts,
        checks,
    }
}
