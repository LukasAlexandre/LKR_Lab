//! Machine Telemetry: estado DINÂMICO da workstation (Concept 02).
//!
//! Separado do inventário (`machine`, snapshot de até 6h): aqui ficam uso de CPU,
//! memória, GPU, E/S de disco, rede, temperaturas, processos e espaço dos volumes.
//! Nada é persistido; o histórico é um buffer curto em memória para sparklines e para
//! as regras sustentadas de `health`.
//!
//! Um único `Service` por app roda um único thread de amostragem:
//!
//! | Modo | CPU/memória | Disco/rede/GPU | Ranking de processos | Temperaturas |
//! |---|---|---|---|---|
//! | ativo (Dashboard aberto e janela visível) | 1 s | 2 s | 2 s | 10 s |
//! | ocioso (outra tela, minimizado ou oculto) | 5 s | 10 s | — | 60 s |
//!
//! O Dashboard renova um "interesse" (lease de 15 s) enquanto está na tela; ao voltar a
//! ele, a próxima amostra é completa e imediata.
//!
//! Fontes: sysinfo (CPU, memória, volumes, E/S, rede, processos — o mesmo `System` de
//! processos de `system`) e `sensors` (PDH, D3DKMT, NVMe). Métrica sem fonte confiável é
//! `None` e a capability correspondente fica `unavailable`.
use crate::{
    health::{self, HealthStatus, Level, LoadSample, MachineHealth, SensorSeries, SpaceSample},
    sensors::{self, Adapter, Pdh},
};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};

pub const EVENT: &str = "machine://telemetry";
/// Pontos das sparklines (2 min no modo ativo).
pub const HISTORY_LEN: usize = 120;
/// Amostras de carga para as regras sustentadas (cobre os 180 s da regra crítica de CPU).
pub const LOAD_LEN: usize = 240;
pub const TOP_PROCESSES: usize = 8;
pub const WATCH_LEASE_MS: i64 = 15_000;
/// Lista de adaptadores relida a cada 5 min (troca de GPU externa, driver reiniciado).
const ADAPTERS_TTL_MS: i64 = 5 * 60_000;

/// Buffer circular limitado.
#[derive(Debug, Clone)]
pub struct Ring<T> {
    capacity: usize,
    items: VecDeque<T>,
}
impl<T: Clone> Ring<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            items: VecDeque::with_capacity(capacity),
        }
    }
    pub fn push(&mut self, item: T) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(item);
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn to_vec(&self) -> Vec<T> {
        self.items.iter().cloned().collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    Available,
    Unavailable,
}
impl From<bool> for Availability {
    fn from(available: bool) -> Self {
        if available {
            Self::Available
        } else {
            Self::Unavailable
        }
    }
}

/// O que esta máquina consegue medir de verdade. A interface nunca preenche o que falta.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub cpu_usage: Availability,
    pub cpu_clock: Availability,
    pub memory_usage: Availability,
    pub gpu_usage: Availability,
    pub gpu_memory: Availability,
    pub gpu_process_usage: Availability,
    /// Temperatura do pacote da CPU: o Windows não expõe sem driver de terceiros.
    pub cpu_package_temperature: Availability,
    /// Alguma GPU informa temperatura (o detalhe por GPU está em `GpuTelemetry`).
    pub gpu_temperature: Availability,
    pub storage_temperature: Availability,
    /// Zona térmica ACPI: leitura do firmware, sem semântica conhecida (não é a CPU).
    pub thermal_zone_temperature: Availability,
    pub motherboard_temperature: Availability,
    pub disk_io: Availability,
    pub disk_activity: Availability,
    pub network_rate: Availability,
    pub process_disk_io: Availability,
    /// SMART/saúde física do disco: sem fonte confiável, não é avaliada.
    pub storage_physical_health: Availability,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuTelemetry {
    /// % do total da máquina.
    pub usage: f32,
    /// Clock efetivo (frequência × % de desempenho), quando o contador existe.
    pub clock_mhz: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryTelemetry {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub percent: f32,
    pub swap_total: u64,
    pub swap_used: u64,
}

/// O que ESTA GPU informa (independe das outras GPUs da máquina).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuCapabilities {
    pub usage: Availability,
    pub dedicated_memory: Availability,
    pub shared_memory: Availability,
    pub temperature: Availability,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTelemetry {
    /// Identidade da GPU neste boot (endereço PCI). Distingue placas de mesmo modelo.
    pub id: String,
    pub name: String,
    /// % do motor mais ocupado (mesma regra do Gerenciador de Tarefas).
    pub usage: Option<f32>,
    pub dedicated_used: Option<u64>,
    /// Segmento dedicado informado pelo driver. Em integradas é pequeno e NÃO é a memória
    /// gráfica total: elas usam a memória compartilhada.
    pub dedicated_total: Option<u64>,
    /// Memória do sistema em uso pela GPU. Nunca somada à dedicada.
    pub shared_used: Option<u64>,
    /// Limite de memória compartilhada segundo o driver.
    pub shared_total: Option<u64>,
    /// Só temperatura real reportada pelo driver; 0 ou ausência = `None`.
    pub temperature: Option<f32>,
    pub capabilities: GpuCapabilities,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskIo {
    pub read_per_sec: f64,
    pub write_per_sec: f64,
    /// % de tempo ativo (100 − % ocioso) do disco físico MAIS ativo, como o "tempo ativo"
    /// por disco do Gerenciador de Tarefas. A média `_Total` não é usada: com um disco
    /// saturado e outro parado ela mostraria 50%.
    pub activity: Option<f32>,
    /// Instância PDH desse disco (ex.: "0 C:").
    pub busiest_disk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeUsage {
    pub mount: String,
    pub kind: String,
    pub total: u64,
    pub available: u64,
    pub removable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkTelemetry {
    pub interface: Option<String>,
    pub ipv4: Option<String>,
    /// bits por segundo na interface ativa.
    pub download_bps: f64,
    pub upload_bps: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SensorSource {
    Cpu,
    Gpu,
    Storage,
    ThermalZone,
    Motherboard,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemperatureReading {
    pub id: String,
    pub label: String,
    pub source: SensorSource,
    pub celsius: Option<f32>,
    pub warning: Option<f32>,
    pub critical: Option<f32>,
    pub level: Level,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    /// % do total da máquina.
    pub cpu: f32,
    pub memory: u64,
    /// % do motor de GPU mais ocupado pelo processo; `None` sem contador.
    pub gpu: Option<f32>,
    /// E/S do processo em bytes/s (inclui toda E/S de arquivo, como no Gerenciador de Tarefas).
    pub disk_read: f64,
    pub disk_write: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Metric {
    Cpu,
    Memory,
    Gpu,
    Disk,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopProcesses {
    pub cpu: Vec<ProcessEntry>,
    pub memory: Vec<ProcessEntry>,
    /// Vazio quando `gpu_process_usage` é unavailable.
    pub gpu: Vec<ProcessEntry>,
    pub disk: Vec<ProcessEntry>,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Telemetry {
    pub timestamp: i64,
    /// Modo ativo (intervalos curtos) ou ocioso.
    pub active: bool,
    pub cpu: CpuTelemetry,
    pub memory: MemoryTelemetry,
    pub gpus: Vec<GpuTelemetry>,
    pub disk_io: DiskIo,
    pub volumes: Vec<VolumeUsage>,
    pub network: NetworkTelemetry,
    pub temperatures: Vec<TemperatureReading>,
    /// Só no modo ativo.
    pub processes: Option<TopProcesses>,
    pub uptime: u64,
    pub boot_time: u64,
    pub capabilities: Capabilities,
    pub health: MachineHealth,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    pub at: i64,
    pub cpu: f32,
    pub memory: f32,
    pub disk: Option<f32>,
    pub gpu: Option<f32>,
    pub download_bps: f64,
    pub upload_bps: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryState {
    pub latest: Option<Telemetry>,
    pub history: Vec<HistoryPoint>,
}

// ---- regras puras ----

/// Bytes num intervalo → bytes/s. Intervalo inválido não gera taxa.
pub fn rate(bytes: u64, elapsed_ms: i64) -> f64 {
    if elapsed_ms <= 0 {
        return 0.0;
    }
    bytes as f64 * 1000.0 / elapsed_ms as f64
}

/// sysinfo dá % de UM núcleo; o Dashboard mostra % da máquina (como o Gerenciador de Tarefas).
pub fn normalize_cpu(raw: f32, threads: usize) -> f32 {
    if threads == 0 || !raw.is_finite() {
        return 0.0;
    }
    (raw / threads as f32).clamp(0.0, 100.0)
}

pub fn memory_percent(total: u64, available: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    (total.saturating_sub(available) as f64 * 100.0 / total as f64) as f32
}

/// Uso de GPU a partir das instâncias PDH de "GPU Engine": por adaptador e por processo,
/// soma por tipo de motor e fica com o motor mais ocupado.
pub fn gpu_usage(engines: &[(String, f64)]) -> (HashMap<u64, f32>, HashMap<u32, f32>) {
    gpu_usage_grouped(engines, &HashMap::new())
}

/// LUID → LUID principal da GPU a que ele pertence (ver `gpu_luid_map`).
pub type LuidMap = HashMap<u64, u64>;

/// Mapa de todos os LUIDs de cada GPU para o LUID principal dela. LUIDs que não estão aqui
/// (display indireto, software) não pertencem a nenhuma GPU e não viram atividade de GPU.
pub fn gpu_luid_map(adapters: &[Adapter]) -> LuidMap {
    adapters
        .iter()
        .flat_map(|a| a.luids.iter().map(move |l| (*l, a.luid)))
        .collect()
}

/// Como `gpu_usage`, com os LUIDs de uma mesma GPU somados por tipo de motor antes de
/// escolher o mais ocupado. Mapa vazio = cada LUID conta por si (comportamento anterior).
/// O uso POR PROCESSO continua somando todos os adaptadores: é o que o processo gasta no total.
pub fn gpu_usage_grouped(
    engines: &[(String, f64)],
    luids: &LuidMap,
) -> (HashMap<u64, f32>, HashMap<u32, f32>) {
    let mut by_adapter: HashMap<(u64, String), f64> = HashMap::new();
    let mut by_process: HashMap<(u32, String), f64> = HashMap::new();
    for (instance, value) in engines {
        let Some((pid, luid, engine)) = sensors::parse_engine(instance) else {
            continue;
        };
        let key = if luids.is_empty() {
            Some(luid)
        } else {
            luids.get(&luid).copied()
        };
        if let Some(key) = key {
            *by_adapter.entry((key, engine.clone())).or_default() += value;
        }
        *by_process.entry((pid, engine)).or_default() += value;
    }
    let mut adapters: HashMap<u64, f32> = HashMap::new();
    for ((luid, _), value) in by_adapter {
        let entry = adapters.entry(luid).or_default();
        *entry = entry.max(value.clamp(0.0, 100.0) as f32);
    }
    let mut processes: HashMap<u32, f32> = HashMap::new();
    for ((pid, _), value) in by_process {
        let entry = processes.entry(pid).or_default();
        *entry = entry.max(value.clamp(0.0, 100.0) as f32);
    }
    (adapters, processes)
}

/// Contador por adaptador ("luid_..._phys_0") somado por LUID.
/// Disco físico mais ativo a partir de "% Idle Time" por instância (sem `_Total`).
pub fn busiest_disk(idle: &[(String, f64)]) -> Option<(String, f32)> {
    idle.iter()
        .filter(|(instance, _)| !instance.eq_ignore_ascii_case("_total"))
        .map(|(instance, idle)| (instance.clone(), (100.0 - idle).clamp(0.0, 100.0) as f32))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

pub fn by_luid(values: &[(String, f64)]) -> HashMap<u64, u64> {
    by_gpu(values, &HashMap::new())
}

/// Como `by_luid`, somando os LUIDs de uma mesma GPU (memória de cada fonte é distinta).
pub fn by_gpu(values: &[(String, f64)], luids: &LuidMap) -> HashMap<u64, u64> {
    let mut out: HashMap<u64, u64> = HashMap::new();
    for (instance, value) in values {
        let Some(luid) = sensors::parse_luid(instance) else {
            continue;
        };
        let key = if luids.is_empty() {
            Some(luid)
        } else {
            luids.get(&luid).copied()
        };
        if let Some(key) = key {
            *out.entry(key).or_default() += value.max(0.0) as u64;
        }
    }
    out
}

/// Os `n` maiores pela métrica (empate: menor PID). GPU sem dado fica fora do ranking.
pub fn rank(entries: &[ProcessEntry], metric: Metric, n: usize) -> Vec<ProcessEntry> {
    let key = |e: &ProcessEntry| -> Option<f64> {
        match metric {
            Metric::Cpu => Some(e.cpu as f64),
            Metric::Memory => Some(e.memory as f64),
            Metric::Gpu => e.gpu.map(f64::from),
            Metric::Disk => Some(e.disk_read + e.disk_write),
        }
    };
    let mut ranked: Vec<(f64, &ProcessEntry)> = entries
        .iter()
        .filter_map(|e| key(e).map(|k| (k, e)))
        .collect();
    ranked.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.pid.cmp(&b.1.pid))
    });
    ranked.into_iter().take(n).map(|(_, e)| e.clone()).collect()
}

/// Limites (aviso, crítico) e grupo de saúde. Só sensores de semântica conhecida:
/// SSD com os limites que o próprio dispositivo declara; GPU com os padrões de `health`.
/// Zona térmica ACPI, CPU e placa-mãe nunca são avaliadas.
pub fn thresholds(reading: &TemperatureReading) -> Option<(health::ThermalGroup, f32, f32)> {
    match reading.source {
        SensorSource::Gpu => Some((
            health::ThermalGroup::Gpu,
            health::GPU_TEMPERATURE.0,
            health::GPU_TEMPERATURE.1,
        )),
        SensorSource::Storage => Some((
            health::ThermalGroup::Storage,
            reading.warning?,
            reading.critical?,
        )),
        SensorSource::ThermalZone | SensorSource::Cpu | SensorSource::Motherboard => None,
    }
}

/// Nível exibido: classificado só com limite conhecido; leitura sem limite é `Unrated`.
pub fn reading_level(reading: &TemperatureReading) -> Level {
    match (reading.celsius, thresholds(reading)) {
        (None, _) => Level::Unavailable,
        (Some(_), None) => Level::Unrated,
        (celsius, Some((_, warning, critical))) => {
            health::temperature_level(celsius, warning, critical)
        }
    }
}

// ---- amostragem ----

/// O que coletar nesta rodada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Plan {
    pub load: bool,
    pub io: bool,
    pub processes: bool,
    pub temperatures: bool,
}
impl Plan {
    pub const FULL: Plan = Plan {
        load: true,
        io: true,
        processes: true,
        temperatures: true,
    };
    /// Rodada `tick` (1 por segundo) no modo ativo ou ocioso.
    pub fn for_tick(tick: u64, active: bool) -> Plan {
        if active {
            Plan {
                load: true,
                io: tick.is_multiple_of(2),
                processes: tick.is_multiple_of(2),
                temperatures: tick.is_multiple_of(10),
            }
        } else {
            Plan {
                load: tick.is_multiple_of(5),
                io: tick.is_multiple_of(10),
                processes: false,
                temperatures: tick.is_multiple_of(60),
            }
        }
    }
    pub fn any(&self) -> bool {
        self.load || self.io || self.processes || self.temperatures
    }
}

// Índices dos contadores PDH.
const PDH_GPU_ENGINE: usize = 0;
const PDH_GPU_DEDICATED: usize = 1;
const PDH_GPU_SHARED: usize = 2;
const PDH_CPU_FREQUENCY: usize = 3;
const PDH_CPU_PERFORMANCE: usize = 4;
const PDH_DISK_IDLE: usize = 5;
const PDH_THERMAL_ZONE: usize = 6;
const PDH_PATHS: [&str; 7] = [
    r"\GPU Engine(*)\Utilization Percentage",
    r"\GPU Adapter Memory(*)\Dedicated Usage",
    r"\GPU Adapter Memory(*)\Shared Usage",
    r"\Processor Information(_Total)\Processor Frequency",
    r"\Processor Information(_Total)\% Processor Performance",
    r"\PhysicalDisk(*)\% Idle Time",
    r"\Thermal Zone Information(*)\High Precision Temperature",
];

pub struct Sampler {
    system: sysinfo::System,
    networks: sysinfo::Networks,
    disks: sysinfo::Disks,
    pdh: Option<Pdh>,
    threads: usize,
    adapters: Vec<Adapter>,
    adapters_at: Option<i64>,
    last_io: Option<i64>,
    last_processes: Option<i64>,
    cpu: CpuTelemetry,
    memory: MemoryTelemetry,
    gpus: Vec<GpuTelemetry>,
    gpu_by_process: HashMap<u32, f32>,
    disk_io: DiskIo,
    volumes: Vec<VolumeUsage>,
    network: NetworkTelemetry,
    temperatures: Vec<TemperatureReading>,
    sensor_history: HashMap<String, VecDeque<(i64, f32)>>,
    processes: Option<TopProcesses>,
    load: Ring<LoadSample>,
    history: Ring<HistoryPoint>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
        let system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::everything()),
        );
        let threads = system.cpus().len().max(1);
        Self {
            system,
            networks: sysinfo::Networks::new_with_refreshed_list(),
            disks: sysinfo::Disks::new_with_refreshed_list(),
            pdh: Pdh::new(&PDH_PATHS),
            threads,
            adapters: Vec::new(),
            adapters_at: None,
            last_io: None,
            last_processes: None,
            cpu: CpuTelemetry {
                usage: 0.0,
                clock_mhz: None,
            },
            memory: MemoryTelemetry {
                total: 0,
                used: 0,
                available: 0,
                percent: 0.0,
                swap_total: 0,
                swap_used: 0,
            },
            gpus: Vec::new(),
            gpu_by_process: HashMap::new(),
            disk_io: DiskIo {
                read_per_sec: 0.0,
                write_per_sec: 0.0,
                activity: None,
                busiest_disk: None,
            },
            volumes: Vec::new(),
            network: NetworkTelemetry {
                interface: None,
                ipv4: None,
                download_bps: 0.0,
                upload_bps: 0.0,
            },
            temperatures: Vec::new(),
            sensor_history: HashMap::new(),
            processes: None,
            load: Ring::new(LOAD_LEN),
            history: Ring::new(HISTORY_LEN),
        }
    }

    fn pdh_available(&self, counter: usize) -> bool {
        self.pdh.as_ref().is_some_and(|p| p.available(counter))
    }
    fn pdh_values(&self, counter: usize) -> Vec<(String, f64)> {
        self.pdh
            .as_ref()
            .map(|p| p.values(counter))
            .unwrap_or_default()
    }
    fn pdh_total(&self, counter: usize) -> Option<f64> {
        self.pdh_values(counter).first().map(|(_, v)| *v)
    }

    fn sample_load(&mut self, now: i64) {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        let total = self.system.total_memory();
        let available = self.system.available_memory();
        self.cpu.usage = self.system.global_cpu_usage().clamp(0.0, 100.0);
        self.memory = MemoryTelemetry {
            total,
            used: total.saturating_sub(available),
            available,
            percent: memory_percent(total, available),
            swap_total: self.system.total_swap(),
            swap_used: self.system.used_swap(),
        };
        self.load.push(LoadSample {
            at: now,
            cpu: self.cpu.usage,
            memory: self.memory.percent,
        });
    }

    fn sample_io(&mut self, now: i64) {
        let elapsed = self.last_io.map(|t| now - t).unwrap_or(0);
        self.last_io = Some(now);
        if let Some(pdh) = self.pdh.as_mut() {
            pdh.collect();
        }
        // Clock efetivo.
        self.cpu.clock_mhz = match (
            self.pdh_total(PDH_CPU_FREQUENCY),
            self.pdh_total(PDH_CPU_PERFORMANCE),
        ) {
            (Some(base), Some(performance)) if base > 0.0 && performance > 0.0 => {
                Some((base * performance / 100.0) as f32)
            }
            _ => None,
        };
        // Volumes e E/S de disco.
        self.disks.refresh(true);
        let (mut read, mut written) = (0u64, 0u64);
        let mut volumes = Vec::new();
        for disk in self.disks.iter().filter(|d| d.total_space() > 0) {
            let usage = disk.usage();
            read += usage.read_bytes;
            written += usage.written_bytes;
            volumes.push(VolumeUsage {
                mount: disk.mount_point().to_string_lossy().into(),
                kind: crate::machine::disk_kind(disk).into(),
                total: disk.total_space(),
                available: disk.available_space(),
                removable: disk.is_removable(),
            });
        }
        volumes.sort_by(|a, b| a.mount.cmp(&b.mount));
        self.volumes = volumes;
        let busiest = busiest_disk(&self.pdh_values(PDH_DISK_IDLE));
        self.disk_io = DiskIo {
            read_per_sec: rate(read, elapsed),
            write_per_sec: rate(written, elapsed),
            activity: busiest.as_ref().map(|(_, active)| *active),
            busiest_disk: busiest.map(|(instance, _)| instance),
        };
        // Rede: interface da rota ativa.
        self.networks.refresh(true);
        let interfaces = crate::machine::network_interfaces(&self.networks);
        let (ipv4, interface) = crate::machine::active_route(&interfaces);
        let (rx, tx) = interface
            .as_ref()
            .and_then(|name| self.networks.iter().find(|(n, _)| *n == name))
            .map(|(_, data)| (data.received(), data.transmitted()))
            .unwrap_or((0, 0));
        self.network = NetworkTelemetry {
            interface,
            ipv4,
            download_bps: rate(rx, elapsed) * 8.0,
            upload_bps: rate(tx, elapsed) * 8.0,
        };
        // GPU.
        if self
            .adapters_at
            .is_none_or(|at| now - at >= ADAPTERS_TTL_MS)
        {
            self.adapters = sensors::gpu_adapters();
            self.adapters_at = Some(now);
        }
        let luids = gpu_luid_map(&self.adapters);
        let (by_adapter, by_process) = gpu_usage_grouped(&self.pdh_values(PDH_GPU_ENGINE), &luids);
        let dedicated = by_gpu(&self.pdh_values(PDH_GPU_DEDICATED), &luids);
        let shared = by_gpu(&self.pdh_values(PDH_GPU_SHARED), &luids);
        let engines = self.pdh_available(PDH_GPU_ENGINE);
        let memory = self.pdh_available(PDH_GPU_DEDICATED);
        let previous = std::mem::take(&mut self.gpus);
        self.gpus = self
            .adapters
            .iter()
            .map(|a| GpuTelemetry {
                id: a.id.clone(),
                name: a.name.clone(),
                usage: engines.then(|| by_adapter.get(&a.luid).copied().unwrap_or(0.0)),
                dedicated_used: memory.then(|| dedicated.get(&a.luid).copied().unwrap_or(0)),
                dedicated_total: a.dedicated,
                shared_used: memory.then(|| shared.get(&a.luid).copied().unwrap_or(0)),
                shared_total: a.shared,
                temperature: previous
                    .iter()
                    .find(|g| g.id == a.id)
                    .and_then(|g| g.temperature),
                capabilities: GpuCapabilities {
                    usage: engines.into(),
                    dedicated_memory: (memory && a.dedicated.is_some()).into(),
                    shared_memory: memory.into(),
                    temperature: Availability::Unavailable,
                },
            })
            .collect();
        for gpu in &mut self.gpus {
            gpu.capabilities.temperature = gpu.temperature.is_some().into();
        }
        self.gpu_by_process = by_process;
    }

    fn sample_processes(&mut self, now: i64) {
        let elapsed = self.last_processes.map(|t| now - t).unwrap_or(0);
        self.last_processes = Some(now);
        let engines = self.pdh_available(PDH_GPU_ENGINE);
        let entries: Vec<ProcessEntry> = crate::system::process_usage()
            .into_iter()
            // PID 0 é o tempo ocioso do sistema, não um consumidor.
            .filter(|p| p.pid != 0)
            .map(|p| ProcessEntry {
                cpu: normalize_cpu(p.cpu, self.threads),
                memory: p.memory,
                gpu: engines.then(|| self.gpu_by_process.get(&p.pid).copied().unwrap_or(0.0)),
                disk_read: rate(p.read_bytes, elapsed),
                disk_write: rate(p.written_bytes, elapsed),
                pid: p.pid,
                name: p.name,
            })
            .collect();
        self.processes = Some(TopProcesses {
            cpu: rank(&entries, Metric::Cpu, TOP_PROCESSES),
            memory: rank(&entries, Metric::Memory, TOP_PROCESSES),
            gpu: if engines {
                rank(&entries, Metric::Gpu, TOP_PROCESSES)
            } else {
                Vec::new()
            },
            disk: rank(&entries, Metric::Disk, TOP_PROCESSES),
            total: entries.len(),
        });
    }

    fn sample_temperatures(&mut self, now: i64) {
        let mut readings = vec![TemperatureReading {
            id: "cpu".into(),
            label: "CPU".into(),
            source: SensorSource::Cpu,
            celsius: None,
            warning: None,
            critical: None,
            level: Level::Unavailable,
        }];
        for adapter in &self.adapters {
            let celsius = sensors::gpu_temperature(adapter.luid);
            if let Some(gpu) = self.gpus.iter_mut().find(|g| g.id == adapter.id) {
                gpu.temperature = celsius;
                gpu.capabilities.temperature = celsius.is_some().into();
            }
            readings.push(TemperatureReading {
                id: format!("gpu:{}", adapter.id),
                label: format!("GPU · {}", adapter.name),
                source: SensorSource::Gpu,
                celsius,
                warning: None,
                critical: None,
                level: Level::Unavailable,
            });
        }
        for disk in sensors::storage_temperatures() {
            readings.push(TemperatureReading {
                id: format!("disk:{}", disk.disk),
                label: format!(
                    "{} · {}",
                    if disk.nvme { "NVMe" } else { "Disco" },
                    disk.model
                ),
                source: SensorSource::Storage,
                celsius: Some(disk.celsius),
                warning: disk.warning,
                critical: disk.critical,
                level: Level::Unavailable,
            });
        }
        for (instance, decikelvin) in self.pdh_values(PDH_THERMAL_ZONE) {
            let celsius = (decikelvin / 10.0 - 273.15) as f32;
            let zone = instance.rsplit('.').next().unwrap_or(&instance).to_string();
            readings.push(TemperatureReading {
                id: format!("zone:{instance}"),
                // Leitura do firmware: não comprovadamente a CPU, por isso nunca é rotulada assim.
                label: format!("Sensor térmico do sistema (ACPI {zone})"),
                source: SensorSource::ThermalZone,
                celsius: (decikelvin > 0.0 && (-40.0..150.0).contains(&celsius)).then_some(celsius),
                warning: None,
                critical: None,
                level: Level::Unavailable,
            });
        }
        readings.push(TemperatureReading {
            id: "motherboard".into(),
            label: "Placa-mãe".into(),
            source: SensorSource::Motherboard,
            celsius: None,
            warning: None,
            critical: None,
            level: Level::Unavailable,
        });
        for reading in &mut readings {
            reading.level = reading_level(reading);
            if thresholds(reading).is_some() {
                if let Some(celsius) = reading.celsius {
                    let series = self.sensor_history.entry(reading.id.clone()).or_default();
                    series.push_back((now, celsius));
                    while series.len() > 6 {
                        series.pop_front();
                    }
                }
            }
        }
        self.sensor_history
            .retain(|id, _| readings.iter().any(|r| &r.id == id && r.celsius.is_some()));
        self.temperatures = readings;
    }

    fn capabilities(&self) -> Capabilities {
        let any = |source: SensorSource| {
            self.temperatures
                .iter()
                .any(|t| t.source == source && t.celsius.is_some())
        };
        Capabilities {
            cpu_usage: Availability::Available,
            cpu_clock: self.cpu.clock_mhz.is_some().into(),
            memory_usage: (self.memory.total > 0).into(),
            gpu_usage: (self.pdh_available(PDH_GPU_ENGINE) && !self.adapters.is_empty()).into(),
            gpu_memory: (self.pdh_available(PDH_GPU_DEDICATED) && !self.adapters.is_empty()).into(),
            gpu_process_usage: self.pdh_available(PDH_GPU_ENGINE).into(),
            cpu_package_temperature: any(SensorSource::Cpu).into(),
            gpu_temperature: any(SensorSource::Gpu).into(),
            storage_temperature: any(SensorSource::Storage).into(),
            thermal_zone_temperature: any(SensorSource::ThermalZone).into(),
            motherboard_temperature: any(SensorSource::Motherboard).into(),
            disk_io: Availability::Available,
            disk_activity: self.disk_io.activity.is_some().into(),
            network_rate: Availability::Available,
            process_disk_io: Availability::Available,
            storage_physical_health: Availability::Unavailable,
        }
    }

    fn health(&self, now: i64) -> MachineHealth {
        let volumes: Vec<SpaceSample> = self
            .volumes
            .iter()
            .filter(|v| !v.removable)
            .map(|v| SpaceSample {
                mount: v.mount.clone(),
                total: v.total,
                available: v.available,
            })
            .collect();
        let sensors: Vec<SensorSeries> = self
            .temperatures
            .iter()
            .filter_map(|reading| {
                let (group, warning, critical) = thresholds(reading)?;
                let readings = self
                    .sensor_history
                    .get(&reading.id)?
                    .iter()
                    .copied()
                    .collect();
                Some(SensorSeries {
                    group,
                    label: reading.label.clone(),
                    readings,
                    warning,
                    critical,
                })
            })
            .collect();
        health::evaluate(&self.load.to_vec(), &volumes, &sensors, now)
    }

    /// Uma rodada de amostragem; devolve o estado completo atual.
    pub fn sample(&mut self, now: i64, plan: Plan, active: bool) -> Telemetry {
        if plan.load {
            self.sample_load(now);
        }
        if plan.io {
            self.sample_io(now);
        }
        if plan.processes {
            self.sample_processes(now);
        }
        if !active {
            // Ranking só existe com o Dashboard na tela: nada de dado velho apresentado como atual.
            self.processes = None;
            self.last_processes = None;
        }
        if plan.temperatures {
            self.sample_temperatures(now);
        }
        if plan.load {
            let gpu = self
                .gpus
                .iter()
                .filter_map(|g| g.usage)
                .fold(None, |max: Option<f32>, u| {
                    Some(max.map_or(u, |m| m.max(u)))
                });
            self.history.push(HistoryPoint {
                at: now,
                cpu: self.cpu.usage,
                memory: self.memory.percent,
                disk: self.disk_io.activity,
                gpu,
                download_bps: self.network.download_bps,
                upload_bps: self.network.upload_bps,
            });
        }
        Telemetry {
            timestamp: now,
            active,
            cpu: self.cpu.clone(),
            memory: self.memory.clone(),
            gpus: self.gpus.clone(),
            disk_io: self.disk_io.clone(),
            volumes: self.volumes.clone(),
            network: self.network.clone(),
            temperatures: self.temperatures.clone(),
            processes: self.processes.clone(),
            uptime: sysinfo::System::uptime(),
            boot_time: sysinfo::System::boot_time(),
            capabilities: self.capabilities(),
            health: self.health(now),
        }
    }

    pub fn history(&self) -> Vec<HistoryPoint> {
        self.history.to_vec()
    }
}

// ---- serviço (um thread por app) ----

type Emit = Arc<dyn Fn(&Telemetry) + Send + Sync>;
type Visible = Arc<dyn Fn() -> bool + Send + Sync>;

struct Shared {
    state: Mutex<TelemetryState>,
    watch_until: AtomicI64,
    wake: Mutex<bool>,
    signal: Condvar,
    stop: AtomicBool,
}

/// Dono do thread de amostragem. Criado uma vez no setup do app; `stop` encerra o thread.
pub struct Service {
    shared: Arc<Shared>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Service {
    /// `visible`: janela principal visível e não minimizada.
    pub fn start(emit: Emit, visible: Visible) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(TelemetryState {
                latest: None,
                history: Vec::new(),
            }),
            watch_until: AtomicI64::new(0),
            wake: Mutex::new(true),
            signal: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("lkr-telemetry".into())
            .spawn(move || run(worker, emit, visible))
            .ok();
        Self {
            shared,
            thread: Mutex::new(thread),
        }
    }

    /// Renova o interesse do Dashboard (lease). Ao ganhar interesse, amostra na hora.
    pub fn watch(&self, now: i64) {
        let previous = self
            .shared
            .watch_until
            .swap(now + WATCH_LEASE_MS, Ordering::SeqCst);
        if previous <= now {
            self.poke();
        }
    }

    /// Amostra completa imediata ("Atualizar agora").
    pub fn poke(&self) {
        *self.shared.wake.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.shared.signal.notify_all();
    }

    pub fn snapshot(&self) -> TelemetryState {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.poke();
        if let Some(thread) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: Arc<Shared>, emit: Emit, visible: Visible) {
    let mut sampler = Sampler::new();
    let mut tick: u64 = 0;
    let mut was_active = false;
    while !shared.stop.load(Ordering::SeqCst) {
        let now = crate::machine::now_ms();
        let active = shared.watch_until.load(Ordering::SeqCst) > now && visible();
        let woken = std::mem::take(&mut *shared.wake.lock().unwrap_or_else(|e| e.into_inner()));
        let plan = if woken || (active && !was_active) {
            Plan::FULL
        } else {
            Plan::for_tick(tick, active)
        };
        was_active = active;
        if plan.any() {
            let telemetry = sampler.sample(now, plan, active);
            {
                let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                state.history = sampler.history();
                state.latest = Some(telemetry.clone());
            }
            emit(&telemetry);
        }
        tick = tick.wrapping_add(1);
        let guard = shared.wake.lock().unwrap_or_else(|e| e.into_inner());
        if !*guard && !shared.stop.load(Ordering::SeqCst) {
            let _ = shared.signal.wait_timeout(guard, Duration::from_secs(1));
        }
    }
}

/// Situação geral para a barra superior e o cabeçalho.
pub fn status_of(state: &TelemetryState) -> Option<HealthStatus> {
    state.latest.as_ref().map(|t| t.health.status)
}
