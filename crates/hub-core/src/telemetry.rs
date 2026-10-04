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
    sensors::{self, Adapter, NetAdapter, Pdh, PowerStatus},
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
/// Temperaturas no modo ativo: a cada 4 rodadas (4 s). Leituras nativas e baratas (D3DKMT, IOCTL
/// de armazenamento, PDH), sem processos externos.
const ACTIVE_TEMPERATURE_TICKS: u64 = 4;
/// Tipo/estado/velocidade das interfaces de rede mudam raramente: releitura a cada 30 s.
const NET_ADAPTERS_TTL_MS: i64 = 30_000;

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
    pub cpu_per_core: Availability,
    pub cpu_base_clock: Availability,
    /// Commit (memória virtual comprometida) e seu limite.
    pub memory_commit: Availability,
    /// Há bateria neste computador.
    pub battery: Availability,
    /// Capacidade de projeto/carga cheia: o Windows não as entrega sem driver ou elevação.
    pub battery_health: Availability,
    /// E/S por disco físico (taxa e operações).
    pub disk_per_device: Availability,
    /// Lista de interfaces com tipo, estado e velocidade de enlace.
    pub network_interfaces: Availability,
}

/// Quão completa é a leitura de um domínio nesta máquina. A interface nunca trata ausência
/// de sensor como falha: `Partial`/`Unavailable` descrevem o que a máquina entrega.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Domain {
    Available,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainAvailability {
    pub cpu: Domain,
    pub memory: Domain,
    pub gpu: Domain,
    pub disk: Domain,
    pub network: Domain,
    pub battery: Domain,
    pub temperatures: Domain,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuTelemetry {
    /// % do total da máquina.
    pub usage: f32,
    /// Clock efetivo (frequência × % de desempenho), quando o contador existe.
    pub clock_mhz: Option<f32>,
    /// Clock base nominal (registro do Windows); `None` se a máquina não informa.
    pub base_mhz: Option<f32>,
    /// % de cada processador lógico, na ordem do sistema.
    pub cores: Vec<f32>,
    /// `false` na primeira amostra: ainda não há intervalo para calcular o uso.
    pub ready: bool,
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
    /// Memória virtual comprometida (RAM + pagefile); separada da RAM física.
    pub commit_used: Option<u64>,
    /// Limite de commit (RAM + pagefile).
    pub commit_limit: Option<u64>,
    /// Pagefile em uso (contador "Paging File % Usage" × tamanho do pagefile). `swap_used` do
    /// sysinfo é "commit além da RAM", não o uso real do arquivo de paginação.
    pub pagefile_used: Option<u64>,
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
    /// Fabricante pelo VendorId PCI.
    pub vendor: Option<String>,
    /// Versão do driver de vídeo informada pelo registro DirectX.
    pub driver_version: Option<String>,
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
    /// Um item por disco FÍSICO (a capacidade dos volumes está em `volumes`).
    pub devices: Vec<DiskDevice>,
    /// `false` na primeira amostra: as taxas ainda não têm intervalo.
    pub ready: bool,
}

/// Atividade de um disco físico. Taxas ausentes (`None`) = o contador ainda não tem intervalo.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskDevice {
    /// Instância PDH (ex.: "0 C:").
    pub instance: String,
    pub number: u32,
    pub model: Option<String>,
    pub nvme: bool,
    /// Letras dos volumes que moram neste disco.
    pub volumes: Vec<String>,
    pub read_per_sec: Option<f64>,
    pub write_per_sec: Option<f64>,
    pub read_ops_per_sec: Option<f64>,
    pub write_ops_per_sec: Option<f64>,
    /// % de tempo ativo (100 − % ocioso).
    pub activity: Option<f32>,
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
    /// Todas as interfaces (sem loopback), a ativa primeiro.
    pub interfaces: Vec<NetworkInterfaceTelemetry>,
    /// `false` na primeira amostra: as taxas ainda não têm intervalo.
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterfaceTelemetry {
    pub name: String,
    pub description: Option<String>,
    /// ethernet | wifi | tunnel | other
    pub kind: String,
    /// `None` se o Windows não informou o estado.
    pub up: Option<bool>,
    pub link_speed_bps: Option<u64>,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub received_bytes: u64,
    pub sent_bytes: u64,
    pub download_bps: Option<f64>,
    pub upload_bps: Option<f64>,
    /// Interface da rota ativa (a que sai para a internet).
    pub active: bool,
}

/// Bateria. Desktop = `present: false` (nunca "0%").
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryTelemetry {
    pub present: bool,
    pub percent: Option<f32>,
    pub charging: Option<bool>,
    pub ac_online: Option<bool>,
    /// Só fora da tomada e se o Windows estimar.
    pub remaining_secs: Option<u64>,
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
    pub battery: BatteryTelemetry,
    pub capabilities: Capabilities,
    pub availability: DomainAvailability,
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

/// Instância PDH de `PhysicalDisk` ("0 C:", "1", "0 C: D:") → (número do disco, letras dos volumes).
pub fn parse_disk_instance(instance: &str) -> Option<(u32, Vec<String>)> {
    let mut parts = instance.split_whitespace();
    let number = parts.next()?.parse::<u32>().ok()?;
    let volumes = parts
        .filter(|p| p.ends_with(':'))
        .map(str::to_string)
        .collect();
    Some((number, volumes))
}

/// Contadores PDH por instância de disco físico, como chegam do sampler.
pub struct DiskCounters<'a> {
    pub read_bytes: &'a [(String, f64)],
    pub write_bytes: &'a [(String, f64)],
    pub read_ops: &'a [(String, f64)],
    pub write_ops: &'a [(String, f64)],
    pub idle: &'a [(String, f64)],
}

/// Um item por disco físico. Contador ausente vira `None` (nunca 0 inventado); o modelo vem do
/// inventário de armazenamento (`models`: número do disco → (modelo, é NVMe)).
pub fn build_disk_devices(
    counters: &DiskCounters,
    models: &HashMap<u32, (String, bool)>,
) -> Vec<DiskDevice> {
    let value = |list: &[(String, f64)], instance: &str| {
        list.iter()
            .find(|(name, _)| name == instance)
            .map(|(_, v)| *v)
            .filter(|v| v.is_finite())
    };
    let mut instances: Vec<&String> = counters
        .read_bytes
        .iter()
        .chain(counters.write_bytes)
        .chain(counters.read_ops)
        .chain(counters.write_ops)
        .chain(counters.idle)
        .map(|(name, _)| name)
        .filter(|name| !name.eq_ignore_ascii_case("_total"))
        .collect();
    instances.sort();
    instances.dedup();
    let mut devices: Vec<DiskDevice> = instances
        .into_iter()
        .filter_map(|instance| {
            let (number, volumes) = parse_disk_instance(instance)?;
            let model = models.get(&number);
            Some(DiskDevice {
                instance: instance.clone(),
                number,
                model: model.map(|(name, _)| name.clone()),
                nvme: model.is_some_and(|(_, nvme)| *nvme),
                volumes,
                read_per_sec: value(counters.read_bytes, instance).map(|v| v.max(0.0)),
                write_per_sec: value(counters.write_bytes, instance).map(|v| v.max(0.0)),
                read_ops_per_sec: value(counters.read_ops, instance).map(|v| v.max(0.0)),
                write_ops_per_sec: value(counters.write_ops, instance).map(|v| v.max(0.0)),
                activity: value(counters.idle, instance)
                    .map(|idle| (100.0 - idle).clamp(0.0, 100.0) as f32),
            })
        })
        .collect();
    devices.sort_by_key(|d| d.number);
    devices
}

/// Contadores de uma interface (sysinfo): bytes no último intervalo e totais desde o boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCounters {
    pub name: String,
    pub received: u64,
    pub transmitted: u64,
    pub total_received: u64,
    pub total_transmitted: u64,
    pub ipv4: Vec<String>,
}

/// Junta o que o Windows diz de cada interface (tipo, estado, velocidade, endereços) com os
/// contadores de tráfego. A ligação é por nome (ou descrição) e, na falta, por IPv4 em comum:
/// o que não casa continua listado, só sem os dados que a máquina não deu. Loopback fica de fora.
pub fn build_interfaces(
    adapters: &[NetAdapter],
    counters: &[InterfaceCounters],
    elapsed_ms: i64,
    active: Option<&str>,
) -> Vec<NetworkInterfaceTelemetry> {
    let rates = |counter: Option<&InterfaceCounters>| match counter {
        Some(c) if elapsed_ms > 0 => (
            Some(rate(c.received, elapsed_ms) * 8.0),
            Some(rate(c.transmitted, elapsed_ms) * 8.0),
        ),
        _ => (None, None),
    };
    let mut used = vec![false; counters.len()];
    let mut out: Vec<NetworkInterfaceTelemetry> = Vec::new();
    for adapter in adapters
        .iter()
        .filter(|a| a.kind != sensors::AdapterKind::Loopback)
    {
        let found = counters.iter().position(|c| {
            c.name == adapter.name
                || c.name == adapter.description
                || (!c.ipv4.is_empty() && c.ipv4.iter().any(|ip| adapter.ipv4.contains(ip)))
        });
        if let Some(i) = found {
            used[i] = true;
        }
        let counter = found.map(|i| &counters[i]);
        let (download_bps, upload_bps) = rates(counter);
        out.push(NetworkInterfaceTelemetry {
            name: adapter.name.clone(),
            description: Some(adapter.description.clone()).filter(|d| !d.is_empty()),
            kind: adapter.kind.as_str().into(),
            up: Some(adapter.up),
            link_speed_bps: adapter.link_speed_bps,
            ipv4: adapter.ipv4.clone(),
            ipv6: adapter.ipv6.clone(),
            received_bytes: counter.map_or(0, |c| c.total_received),
            sent_bytes: counter.map_or(0, |c| c.total_transmitted),
            download_bps,
            upload_bps,
            active: active.is_some_and(|a| a == adapter.name)
                || counter.is_some_and(|c| active.is_some_and(|a| a == c.name)),
        });
    }
    for counter in counters
        .iter()
        .enumerate()
        .filter(|(i, _)| !used[*i])
        .map(|(_, counter)| counter)
    {
        if counter.name.to_lowercase().contains("loopback") {
            continue;
        }
        let (download_bps, upload_bps) = rates(Some(counter));
        out.push(NetworkInterfaceTelemetry {
            name: counter.name.clone(),
            description: None,
            kind: "other".into(),
            up: None,
            link_speed_bps: None,
            ipv4: counter.ipv4.clone(),
            ipv6: Vec::new(),
            received_bytes: counter.total_received,
            sent_bytes: counter.total_transmitted,
            download_bps,
            upload_bps,
            active: active.is_some_and(|a| a == counter.name),
        });
    }
    out.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.up.unwrap_or(false).cmp(&a.up.unwrap_or(false)))
            .then(a.name.cmp(&b.name))
    });
    out
}

/// Bateria a partir do estado de energia. Sem leitura ou sem bateria: `present: false`.
pub fn battery_from(status: Option<&PowerStatus>) -> BatteryTelemetry {
    match status {
        Some(s) if s.battery_present => BatteryTelemetry {
            present: true,
            percent: s.percent.map(f32::from),
            charging: s.charging,
            ac_online: s.ac_online,
            remaining_secs: s.remaining_secs.map(u64::from),
        },
        Some(s) => BatteryTelemetry {
            present: false,
            percent: None,
            charging: None,
            ac_online: s.ac_online,
            remaining_secs: None,
        },
        None => BatteryTelemetry {
            present: false,
            percent: None,
            charging: None,
            ac_online: None,
            remaining_secs: None,
        },
    }
}

/// Resume, por domínio, o que a máquina entrega: tudo, parte ou nada.
pub fn domain_availability(c: &Capabilities, gpu_count: usize) -> DomainAvailability {
    let ok = |a: Availability| a == Availability::Available;
    let grade = |all: bool, any: bool| {
        if all {
            Domain::Available
        } else if any {
            Domain::Partial
        } else {
            Domain::Unavailable
        }
    };
    let temps = [
        ok(c.cpu_package_temperature),
        ok(c.gpu_temperature),
        ok(c.storage_temperature),
    ];
    DomainAvailability {
        cpu: grade(
            ok(c.cpu_usage) && ok(c.cpu_per_core) && ok(c.cpu_clock),
            ok(c.cpu_usage),
        ),
        memory: grade(
            ok(c.memory_usage) && ok(c.memory_commit),
            ok(c.memory_usage),
        ),
        gpu: if gpu_count == 0 {
            Domain::Unavailable
        } else {
            // Com GPU presente o nome já é conhecido: sem nenhuma métrica ainda é parcial.
            grade(
                ok(c.gpu_usage) && ok(c.gpu_memory) && ok(c.gpu_temperature),
                true,
            )
        },
        disk: grade(
            ok(c.disk_io) && ok(c.disk_per_device) && ok(c.storage_temperature),
            ok(c.disk_io) || ok(c.disk_per_device),
        ),
        network: grade(
            ok(c.network_rate) && ok(c.network_interfaces),
            ok(c.network_rate) || ok(c.network_interfaces),
        ),
        battery: grade(ok(c.battery), false),
        // CPU, GPU e disco: só "tudo" se os três têm sensor; a zona ACPI sozinha é parcial.
        temperatures: grade(
            temps.iter().all(|t| *t),
            temps.iter().any(|t| *t) || ok(c.thermal_zone_temperature),
        ),
    }
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
                temperatures: tick.is_multiple_of(ACTIVE_TEMPERATURE_TICKS),
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
const PDH_DISK_READ_BYTES: usize = 7;
const PDH_DISK_WRITE_BYTES: usize = 8;
const PDH_DISK_READS: usize = 9;
const PDH_DISK_WRITES: usize = 10;
const PDH_PAGEFILE_USAGE: usize = 11;
const PDH_PATHS: [&str; 12] = [
    r"\GPU Engine(*)\Utilization Percentage",
    r"\GPU Adapter Memory(*)\Dedicated Usage",
    r"\GPU Adapter Memory(*)\Shared Usage",
    r"\Processor Information(_Total)\Processor Frequency",
    r"\Processor Information(_Total)\% Processor Performance",
    r"\PhysicalDisk(*)\% Idle Time",
    r"\Thermal Zone Information(*)\High Precision Temperature",
    r"\PhysicalDisk(*)\Disk Read Bytes/sec",
    r"\PhysicalDisk(*)\Disk Write Bytes/sec",
    r"\PhysicalDisk(*)\Disk Reads/sec",
    r"\PhysicalDisk(*)\Disk Writes/sec",
    r"\Paging File(_Total)\% Usage",
];

pub struct Sampler {
    system: sysinfo::System,
    networks: sysinfo::Networks,
    disks: sysinfo::Disks,
    pdh: Option<Pdh>,
    threads: usize,
    adapters: Vec<Adapter>,
    adapters_at: Option<i64>,
    net_adapters: Vec<NetAdapter>,
    net_adapters_at: Option<i64>,
    storage_models: HashMap<u32, (String, bool)>,
    storage_at: Option<i64>,
    power: Option<PowerStatus>,
    battery: BatteryTelemetry,
    /// % de uso do pagefile (PDH); atualizado na rodada de E/S.
    pagefile_percent: Option<f32>,
    /// Amostras de carga / de E/S já feitas: a primeira não tem intervalo para taxas.
    load_samples: u32,
    io_samples: u32,
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
            net_adapters: Vec::new(),
            net_adapters_at: None,
            storage_models: HashMap::new(),
            storage_at: None,
            power: None,
            battery: battery_from(None),
            pagefile_percent: None,
            load_samples: 0,
            io_samples: 0,
            last_io: None,
            last_processes: None,
            cpu: CpuTelemetry {
                usage: 0.0,
                clock_mhz: None,
                // Clock base: estático, lido uma vez (não muda durante a sessão).
                base_mhz: sensors::cpu_base_mhz(),
                cores: Vec::new(),
                ready: false,
            },
            memory: MemoryTelemetry {
                total: 0,
                used: 0,
                available: 0,
                percent: 0.0,
                swap_total: 0,
                swap_used: 0,
                commit_used: None,
                commit_limit: None,
                pagefile_used: None,
            },
            gpus: Vec::new(),
            gpu_by_process: HashMap::new(),
            disk_io: DiskIo {
                read_per_sec: 0.0,
                write_per_sec: 0.0,
                activity: None,
                busiest_disk: None,
                devices: Vec::new(),
                ready: false,
            },
            volumes: Vec::new(),
            network: NetworkTelemetry {
                interface: None,
                ipv4: None,
                download_bps: 0.0,
                upload_bps: 0.0,
                interfaces: Vec::new(),
                ready: false,
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
        self.cpu.cores = self
            .system
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage().clamp(0.0, 100.0))
            .collect();
        // O primeiro refresh depois de criar o `System` não tem intervalo: ainda não é uso real.
        self.load_samples = self.load_samples.saturating_add(1);
        self.cpu.ready = self.load_samples >= 2;
        let commit = sensors::commit_charge();
        self.memory = MemoryTelemetry {
            total,
            used: total.saturating_sub(available),
            available,
            percent: memory_percent(total, available),
            swap_total: self.system.total_swap(),
            swap_used: self.system.used_swap(),
            commit_used: commit.map(|c| c.used),
            commit_limit: commit.map(|c| c.limit).filter(|l| *l > 0),
            pagefile_used: self
                .pagefile_percent
                .map(|p| (self.system.total_swap() as f64 * p as f64 / 100.0) as u64),
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
        self.io_samples = self.io_samples.saturating_add(1);
        if self.storage_at.is_none_or(|at| now - at >= ADAPTERS_TTL_MS) {
            self.storage_models = sensors::storage_devices()
                .into_iter()
                .map(|d| (d.disk, (d.model, d.nvme)))
                .collect();
            self.storage_at = Some(now);
        }
        let idle = self.pdh_values(PDH_DISK_IDLE);
        let busiest = busiest_disk(&idle);
        let devices = build_disk_devices(
            &DiskCounters {
                read_bytes: &self.pdh_values(PDH_DISK_READ_BYTES),
                write_bytes: &self.pdh_values(PDH_DISK_WRITE_BYTES),
                read_ops: &self.pdh_values(PDH_DISK_READS),
                write_ops: &self.pdh_values(PDH_DISK_WRITES),
                idle: &idle,
            },
            &self.storage_models,
        );
        self.disk_io = DiskIo {
            read_per_sec: rate(read, elapsed),
            write_per_sec: rate(written, elapsed),
            activity: busiest.as_ref().map(|(_, active)| *active),
            busiest_disk: busiest.map(|(instance, _)| instance),
            devices,
            ready: elapsed > 0,
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
        if self
            .net_adapters_at
            .is_none_or(|at| now - at >= NET_ADAPTERS_TTL_MS)
        {
            self.net_adapters = sensors::network_adapters();
            self.net_adapters_at = Some(now);
        }
        let counters: Vec<InterfaceCounters> = self
            .networks
            .iter()
            .map(|(name, data)| InterfaceCounters {
                name: name.clone(),
                received: data.received(),
                transmitted: data.transmitted(),
                total_received: data.total_received(),
                total_transmitted: data.total_transmitted(),
                ipv4: data
                    .ip_networks()
                    .iter()
                    .filter_map(|net| match net.addr {
                        std::net::IpAddr::V4(ip) if !ip.is_loopback() => Some(ip.to_string()),
                        _ => None,
                    })
                    .collect(),
            })
            .collect();
        self.network = NetworkTelemetry {
            interfaces: build_interfaces(
                &self.net_adapters,
                &counters,
                elapsed,
                interface.as_deref(),
            ),
            interface,
            ipv4,
            download_bps: rate(rx, elapsed) * 8.0,
            upload_bps: rate(tx, elapsed) * 8.0,
            ready: elapsed > 0,
        };
        self.power = sensors::power_status();
        self.battery = battery_from(self.power.as_ref());
        self.pagefile_percent = self
            .pdh_total(PDH_PAGEFILE_USAGE)
            .filter(|v| v.is_finite())
            .map(|v| v.clamp(0.0, 100.0) as f32);
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
                vendor: a.vendor.clone(),
                driver_version: a.driver_version.clone(),
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
            cpu_per_core: (!self.cpu.cores.is_empty()).into(),
            cpu_base_clock: self.cpu.base_mhz.is_some().into(),
            memory_commit: self.memory.commit_limit.is_some().into(),
            battery: self.battery.present.into(),
            battery_health: Availability::Unavailable,
            disk_per_device: (!self.disk_io.devices.is_empty()).into(),
            network_interfaces: (!self.network.interfaces.is_empty()).into(),
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
        let capabilities = self.capabilities();
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
            battery: self.battery.clone(),
            availability: domain_availability(&capabilities, self.gpus.len()),
            capabilities,
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
