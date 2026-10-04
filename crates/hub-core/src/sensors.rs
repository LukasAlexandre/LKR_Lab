//! Fontes nativas de hardware usadas pelo inventário (`machine`) e pela telemetria
//! (`telemetry`). Só APIs do próprio Windows, sem driver nem ferramenta de terceiros:
//!
//! * registro (HKLM, somente leitura): versão do sistema e adaptadores DirectX;
//! * D3DKMT (gdi32): adaptador presente agora e temperatura da GPU (a mesma fonte do
//!   Gerenciador de Tarefas);
//! * PDH (contadores de desempenho, nomes em inglês para funcionar em qualquer idioma):
//!   uso de GPU e por processo, memória de GPU, clock da CPU, atividade de disco e zonas
//!   térmicas ACPI;
//! * `IOCTL_STORAGE_QUERY_PROPERTY`: temperatura dos SSDs NVMe, aberta sem permissão de
//!   leitura do disco (acesso 0), com os limites de aviso/crítico do próprio dispositivo.
//!
//! O que não existe na máquina volta vazio ou `None`; nunca há valor estimado.

/// Bits de `D3DKMT_ADAPTERTYPE` (d3dkmthk.h). É o que o Windows usa para distinguir um
/// adaptador de renderização de um de vídeo virtual: o mesmo valor aparece no registro
/// DirectX (`AdapterType`) e em `D3DKMTQueryAdapterInfo(KMTQAITYPE_ADAPTERTYPE)`.
pub mod adapter_type {
    pub const RENDER: u32 = 1 << 0;
    pub const DISPLAY: u32 = 1 << 1;
    pub const SOFTWARE: u32 = 1 << 2;
    pub const INDIRECT_DISPLAY: u32 = 1 << 6;
    pub const PARAVIRTUALIZED: u32 = 1 << 7;
    pub const COMPUTE_ONLY: u32 = 1 << 11;
}

/// O que um adaptador DirectX/WDDM é de fato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterClass {
    /// Renderiza ou computa de verdade: uma GPU (física, ou a paravirtualizada de uma VM).
    Physical,
    /// Display indireto (Microsoft IDD: acesso remoto, monitor virtual). Só exibe; não é GPU.
    Indirect,
    /// Renderizador de software (Microsoft Basic Render Driver, WARP).
    Software,
    /// Só tem saída de vídeo, sem render nem compute.
    DisplayOnly,
}

pub fn classify_adapter(flags: u32) -> AdapterClass {
    use adapter_type::*;
    if flags & SOFTWARE != 0 {
        AdapterClass::Software
    } else if flags & INDIRECT_DISPLAY != 0 {
        AdapterClass::Indirect
    } else if flags & (RENDER | COMPUTE_ONLY) != 0 {
        AdapterClass::Physical
    } else {
        AdapterClass::DisplayOnly
    }
}

/// Último recurso, só quando o Windows não informa o tipo: nomes de adaptadores que nunca
/// são uma GPU. A decisão normal é sempre pelos bits de `adapter_type`, nunca pelo nome.
fn looks_virtual(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "microsoft basic",
        "microsoft remote",
        "idd device",
        "indirect display",
        "virtual display",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

/// Endereço PCI (barramento:dispositivo.função) do adaptador, via `KMTQAITYPE_ADAPTERADDRESS`.
/// É a identidade FÍSICA: duas placas iguais têm endereços diferentes; um display virtual não
/// tem endereço PCI de verdade (o Windows devolve um endereço sintético ou inválido).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pci {
    pub bus: u32,
    pub device: u32,
    pub function: u32,
}
impl Pci {
    /// Endereço utilizável: o Windows usa 0xFFFFFFFF para "não se aplica".
    pub fn valid(&self) -> bool {
        self.bus != u32::MAX && self.device < 32 && self.function < 8
    }
}

/// Adaptador DirectX como o sistema o descreve, ainda sem decidir se é uma GPU.
#[derive(Debug, Clone, PartialEq)]
pub struct RawAdapter {
    pub luid: u64,
    pub name: String,
    pub dedicated: Option<u64>,
    pub shared: Option<u64>,
    /// Bits de `adapter_type`; `None` se nem o driver nem o registro informaram.
    pub flags: Option<u32>,
    pub pci: Option<Pci>,
    /// VendorId PCI informado pelo registro DirectX.
    pub vendor_id: Option<u32>,
    /// `DriverVersion` empacotado (4 × 16 bits) do registro DirectX.
    pub driver_version: Option<u64>,
}

/// GPUs de verdade a partir dos adaptadores presentes.
///
/// * só entra quem renderiza/computa e não é software nem display indireto;
/// * LUIDs com o MESMO endereço PCI são a mesma GPU (um adaptador, várias fontes PDH);
/// * nome, VendorId e DeviceId NUNCA fundem nada: duas placas idênticas são duas GPUs;
/// * sem endereço PCI confiável, nada é fundido (na dúvida, não esconde uma GPU).
pub fn physical_gpus(raw: Vec<RawAdapter>) -> Vec<Adapter> {
    let mut gpus: Vec<(Option<Pci>, Adapter)> = Vec::new();
    for adapter in raw {
        if adapter.name.is_empty() {
            continue;
        }
        let is_gpu = match adapter.flags {
            Some(flags) => classify_adapter(flags) == AdapterClass::Physical,
            None => !looks_virtual(&adapter.name),
        };
        if !is_gpu || gpus.iter().any(|(_, g)| g.luids.contains(&adapter.luid)) {
            continue;
        }
        if let Some(pci) = adapter.pci {
            if let Some((_, same)) = gpus.iter_mut().find(|(p, _)| *p == Some(pci)) {
                same.luids.push(adapter.luid);
                same.dedicated = same.dedicated.max(adapter.dedicated);
                same.shared = same.shared.max(adapter.shared);
                if same.vendor.is_none() {
                    same.vendor = adapter.vendor_id.and_then(vendor_name).map(String::from);
                }
                if same.driver_version.is_none() {
                    same.driver_version = adapter.driver_version.and_then(format_driver_version);
                }
                continue;
            }
        }
        let id = match adapter.pci {
            Some(p) => format!("pci:{}:{}.{}", p.bus, p.device, p.function),
            None => format!("luid:{:x}", adapter.luid),
        };
        gpus.push((
            adapter.pci,
            Adapter {
                id,
                luid: adapter.luid,
                luids: vec![adapter.luid],
                name: adapter.name,
                dedicated: adapter.dedicated,
                shared: adapter.shared,
                vendor: adapter.vendor_id.and_then(vendor_name).map(String::from),
                driver_version: adapter.driver_version.and_then(format_driver_version),
            },
        ));
    }
    // Ordem estável: com endereço PCI primeiro (a integrada fica antes das placas), depois por LUID.
    gpus.sort_by_key(|(pci, a)| (pci.is_none(), *pci, a.luid));
    gpus.into_iter().map(|(_, adapter)| adapter).collect()
}

/// GPU (adaptador físico de renderização) presente agora.
#[derive(Debug, Clone, PartialEq)]
pub struct Adapter {
    /// Identidade da GPU neste boot: `pci:0:2.0` (endereço PCI) ou, sem endereço, `luid:…`.
    /// Nunca é persistida nem sai da máquina.
    pub id: String,
    /// LUID principal do boot atual (liga o adaptador aos contadores PDH e ao D3DKMT).
    pub luid: u64,
    /// Todos os LUIDs cujos contadores PDH pertencem a esta GPU (inclui `luid`).
    pub luids: Vec<u64>,
    pub name: String,
    /// Segmento de memória DEDICADO informado pelo driver. Em GPU integrada é pequeno e não
    /// representa a memória gráfica total: ela usa a memória do sistema (`shared`).
    pub dedicated: Option<u64>,
    /// Limite de memória do sistema que a GPU pode usar (compartilhada), segundo o driver.
    pub shared: Option<u64>,
    /// Fabricante pelo VendorId PCI; `None` se o id não é de um fabricante conhecido.
    pub vendor: Option<String>,
    /// Versão do driver de vídeo (`a.b.c.d`) segundo o registro DirectX.
    pub driver_version: Option<String>,
}

/// Fabricante a partir do VendorId PCI. Só ids de fabricantes de GPU conhecidos; o resto é `None`.
pub fn vendor_name(vendor_id: u32) -> Option<&'static str> {
    match vendor_id {
        0x10DE => Some("NVIDIA"),
        0x1002 | 0x1022 => Some("AMD"),
        0x8086 => Some("Intel"),
        0x5143 => Some("Qualcomm"),
        _ => None,
    }
}

/// `DriverVersion` do registro DirectX (4 palavras de 16 bits) → `a.b.c.d`. Zero = desconhecido.
pub fn format_driver_version(packed: u64) -> Option<String> {
    (packed != 0).then(|| {
        format!(
            "{}.{}.{}.{}",
            (packed >> 48) & 0xFFFF,
            (packed >> 32) & 0xFFFF,
            (packed >> 16) & 0xFFFF,
            packed & 0xFFFF
        )
    })
}

/// Estado de energia como o Windows o informa (`GetSystemPowerStatus`). Capacidade de projeto e
/// de carga cheia NÃO fazem parte disto: o Windows não as entrega sem driver/elevação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerStatus {
    pub battery_present: bool,
    pub ac_online: Option<bool>,
    pub percent: Option<u8>,
    pub charging: Option<bool>,
    /// Segundos restantes na bateria; só existe com a máquina fora da tomada.
    pub remaining_secs: Option<u32>,
}

/// Interpreta `SYSTEM_POWER_STATUS`. Valores "desconhecido" (255, `u32::MAX`) viram `None`:
/// nunca são mostrados como 0%.
pub fn interpret_power(ac_line: u8, battery_flag: u8, percent: u8, lifetime: u32) -> PowerStatus {
    let no_battery = battery_flag & 128 != 0;
    let unknown = battery_flag == 255;
    let present = !no_battery && !unknown;
    PowerStatus {
        battery_present: present,
        ac_online: match ac_line {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        },
        percent: (present && percent <= 100).then_some(percent),
        charging: present.then_some(battery_flag & 8 != 0),
        remaining_secs: (present && ac_line == 0 && lifetime != u32::MAX).then_some(lifetime),
    }
}

/// Memória virtual do sistema: o limite de commit (RAM + pagefile) e o que já está comprometido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitCharge {
    pub used: u64,
    pub limit: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    Ethernet,
    Wifi,
    Loopback,
    Tunnel,
    Other,
}
impl AdapterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ethernet => "ethernet",
            Self::Wifi => "wifi",
            Self::Loopback => "loopback",
            Self::Tunnel => "tunnel",
            Self::Other => "other",
        }
    }
}

/// `IfType` da IANA → tipo de interface. O que não é reconhecido com segurança é `Other`.
pub fn classify_if_type(if_type: u32) -> AdapterKind {
    match if_type {
        6 => AdapterKind::Ethernet,
        71 => AdapterKind::Wifi,
        24 => AdapterKind::Loopback,
        131 | 23 | 150 => AdapterKind::Tunnel,
        _ => AdapterKind::Other,
    }
}

/// Interface de rede como o Windows a descreve (`GetAdaptersAddresses`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetAdapter {
    pub name: String,
    pub description: String,
    pub kind: AdapterKind,
    pub up: bool,
    /// bits/s da negociação do enlace; `None` se desconectada ou se o driver não informa.
    pub link_speed_bps: Option<u64>,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
}

/// Disco físico presente (número do `PhysicalDriveN`, modelo e barramento).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDevice {
    pub disk: u32,
    pub model: String,
    pub nvme: bool,
}

/// Temperatura de um SSD/disco, com os limites declarados pelo dispositivo.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageTemperature {
    pub disk: u32,
    pub model: String,
    /// Barramento NVMe (STORAGE_BUS_TYPE = 17).
    pub nvme: bool,
    pub celsius: f32,
    pub warning: Option<f32>,
    pub critical: Option<f32>,
}

/// Instância PDH de GPU: `luid_0x00000000_0x00010088` → 0x10088.
pub fn parse_luid(instance: &str) -> Option<u64> {
    let rest = &instance[instance.find("luid_0x")? + 7..];
    let high = u64::from_str_radix(rest.get(..8)?, 16).ok()?;
    let low = u64::from_str_radix(
        rest.get(11..19)
            .filter(|_| rest.get(8..11) == Some("_0x"))?,
        16,
    )
    .ok()?;
    Some((high << 32) | low)
}

/// Instância PDH de engine: `pid_1234_luid_..._engtype_3D` → (1234, luid, "3D").
pub fn parse_engine(instance: &str) -> Option<(u32, u64, String)> {
    let pid = instance.strip_prefix("pid_")?;
    let pid: u32 = pid[..pid.find('_')?].parse().ok()?;
    let luid = parse_luid(instance)?;
    let engine = instance[instance.find("engtype_")? + 8..].to_ascii_lowercase();
    Some((pid, luid, engine))
}

#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod fallback {
    use super::{
        Adapter, CommitCharge, NetAdapter, PowerStatus, StorageDevice, StorageTemperature,
    };
    pub fn gpu_adapters() -> Vec<Adapter> {
        Vec::new()
    }
    pub fn power_status() -> Option<PowerStatus> {
        None
    }
    pub fn commit_charge() -> Option<CommitCharge> {
        None
    }
    pub fn network_adapters() -> Vec<NetAdapter> {
        Vec::new()
    }
    pub fn cpu_base_mhz() -> Option<f32> {
        None
    }
    pub fn storage_devices() -> Vec<StorageDevice> {
        Vec::new()
    }
    pub fn gpu_temperature(_luid: u64) -> Option<f32> {
        None
    }
    pub fn storage_temperatures() -> Vec<StorageTemperature> {
        Vec::new()
    }
    /// Sem PDH fora do Windows: toda leitura volta vazia.
    pub struct Pdh;
    impl Pdh {
        pub fn new(_paths: &[&str]) -> Option<Self> {
            None
        }
        pub fn collect(&mut self) -> bool {
            false
        }
        pub fn values(&self, _counter: usize) -> Vec<(String, f64)> {
            Vec::new()
        }
    }
}
#[cfg(not(windows))]
pub use fallback::*;

#[cfg(windows)]
mod windows {
    use super::registry::Key;
    use super::{
        Adapter, CommitCharge, NetAdapter, PowerStatus, StorageDevice, StorageTemperature,
    };

    // ---- D3DKMT (gdi32): mesmas estruturas do WDK, declaradas aqui ----
    #[repr(C)]
    struct OpenAdapterFromLuid {
        luid_low: u32,
        luid_high: i32,
        adapter: u32,
    }
    #[repr(C)]
    struct QueryAdapterInfo {
        adapter: u32,
        kind: i32,
        data: *mut core::ffi::c_void,
        size: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct AdapterPerfData {
        physical_adapter_index: u32,
        memory_frequency: u64,
        max_memory_frequency: u64,
        max_memory_frequency_oc: u64,
        memory_bandwidth: u64,
        pcie_bandwidth: u64,
        fan_rpm: u32,
        power: u32,
        /// Décimos de grau Celsius; 0 = o driver não informa.
        temperature: u32,
        power_state_override: u8,
    }
    #[repr(C)]
    struct CloseAdapter {
        adapter: u32,
    }
    const KMTQAITYPE_ADAPTERPERFDATA: i32 = 62;
    #[link(name = "gdi32")]
    extern "system" {
        fn D3DKMTOpenAdapterFromLuid(data: *mut OpenAdapterFromLuid) -> i32;
        fn D3DKMTQueryAdapterInfo(data: *mut QueryAdapterInfo) -> i32;
        fn D3DKMTCloseAdapter(data: *const CloseAdapter) -> i32;
    }

    /// Abre o adaptador pelo LUID; falha se ele não está presente neste boot.
    struct KmtAdapter(u32);
    impl KmtAdapter {
        fn open(luid: u64) -> Option<Self> {
            let mut data = OpenAdapterFromLuid {
                luid_low: luid as u32,
                luid_high: (luid >> 32) as i32,
                adapter: 0,
            };
            // SAFETY: estrutura válida e viva durante a chamada.
            (unsafe { D3DKMTOpenAdapterFromLuid(&mut data) } == 0).then_some(Self(data.adapter))
        }
    }
    impl Drop for KmtAdapter {
        fn drop(&mut self) {
            // SAFETY: o handle veio de D3DKMTOpenAdapterFromLuid e é fechado uma vez.
            unsafe { D3DKMTCloseAdapter(&CloseAdapter { adapter: self.0 }) };
        }
    }

    const KMTQAITYPE_ADAPTERADDRESS: i32 = 6;
    const KMTQAITYPE_ADAPTERTYPE: i32 = 15;
    #[repr(C)]
    #[derive(Default)]
    struct AdapterAddress {
        bus: u32,
        device: u32,
        function: u32,
    }

    impl KmtAdapter {
        fn query<T: Default>(&self, kind: i32) -> Option<T> {
            let mut value = T::default();
            let mut query = QueryAdapterInfo {
                adapter: self.0,
                kind,
                data: (&mut value as *mut T).cast(),
                size: std::mem::size_of::<T>() as u32,
            };
            // SAFETY: `value` tem o tamanho informado e vive durante a chamada.
            (unsafe { D3DKMTQueryAdapterInfo(&mut query) } == 0).then_some(value)
        }
        /// Bits de `adapter_type` informados pelo driver agora.
        fn adapter_type(&self) -> Option<u32> {
            self.query::<u32>(KMTQAITYPE_ADAPTERTYPE)
        }
        /// Endereço PCI real; `None` se o Windows devolve "não se aplica".
        fn pci(&self) -> Option<super::Pci> {
            let address = self.query::<AdapterAddress>(KMTQAITYPE_ADAPTERADDRESS)?;
            let pci = super::Pci {
                bus: address.bus,
                device: address.device,
                function: address.function,
            };
            pci.valid().then_some(pci)
        }
    }

    /// GPUs presentes: entradas do registro DirectX cujo LUID abre agora, classificadas pelo
    /// tipo do adaptador (D3DKMT agora; o `AdapterType` do registro só como reserva). Displays
    /// indiretos (MS IDD) e renderizadores de software não são GPUs; a identidade física é o
    /// endereço PCI. As regras vivem em `physical_gpus` (puro e testado).
    pub fn gpu_adapters() -> Vec<Adapter> {
        let Some(root) = Key::local_machine(r"SOFTWARE\Microsoft\DirectX") else {
            return Vec::new();
        };
        let mut raw: Vec<super::RawAdapter> = Vec::new();
        for sub in root.subkeys() {
            let Some(key) = root.subkey(&sub) else {
                continue;
            };
            let (Some(name), Some(luid)) = (key.string("Description"), key.number("AdapterLuid"))
            else {
                continue;
            };
            let Some(present) = KmtAdapter::open(luid) else {
                continue; // entrada antiga: o adaptador não existe neste boot
            };
            raw.push(super::RawAdapter {
                luid,
                name,
                dedicated: key.number("DedicatedVideoMemory").filter(|m| *m > 0),
                shared: key.number("SharedSystemMemory").filter(|m| *m > 0),
                flags: present
                    .adapter_type()
                    .or_else(|| key.number("AdapterType").map(|t| t as u32)),
                pci: present.pci(),
                vendor_id: key.number("VendorId").map(|v| v as u32),
                driver_version: key.number("DriverVersion"),
            });
        }
        super::physical_gpus(raw)
    }

    /// Temperatura reportada pelo driver (WDDM 2.4+). Integradas costumam não informar.
    pub fn gpu_temperature(luid: u64) -> Option<f32> {
        let adapter = KmtAdapter::open(luid)?;
        let mut perf = AdapterPerfData::default();
        let mut query = QueryAdapterInfo {
            adapter: adapter.0,
            kind: KMTQAITYPE_ADAPTERPERFDATA,
            data: (&mut perf as *mut AdapterPerfData).cast(),
            size: std::mem::size_of::<AdapterPerfData>() as u32,
        };
        // SAFETY: `perf` tem o tamanho informado e vive durante a chamada.
        if unsafe { D3DKMTQueryAdapterInfo(&mut query) } != 0 {
            return None;
        }
        let celsius = perf.temperature as f32 / 10.0;
        (perf.temperature > 0 && celsius < 150.0).then_some(celsius)
    }

    // ---- NVMe / discos ----
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
        System::IO::DeviceIoControl,
    };
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    const STORAGE_DEVICE_PROPERTY: u32 = 0;
    const STORAGE_DEVICE_TEMPERATURE_PROPERTY: u32 = 52;

    struct Disk(HANDLE);
    impl Drop for Disk {
        fn drop(&mut self) {
            // SAFETY: handle aberto por CreateFileW, fechado uma vez.
            unsafe { CloseHandle(self.0) };
        }
    }
    impl Disk {
        /// Acesso 0: só consultas de propriedade, nunca leitura do conteúdo do disco.
        fn open(index: u32) -> Option<Self> {
            let path: Vec<u16> = format!(r"\\.\PhysicalDrive{index}")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: caminho termina em NUL; demais ponteiros nulos são permitidos.
            let handle = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            };
            (handle != INVALID_HANDLE_VALUE && !handle.is_null()).then_some(Self(handle))
        }
        fn property(&self, id: u32) -> Option<Vec<u8>> {
            // STORAGE_PROPERTY_QUERY { PropertyId, QueryType = PropertyStandardQuery, ... }
            let mut query = [0u8; 12];
            query[..4].copy_from_slice(&id.to_le_bytes());
            let mut out = vec![0u8; 1024];
            let mut returned = 0u32;
            // SAFETY: buffers com os tamanhos informados, vivos durante a chamada.
            let ok = unsafe {
                DeviceIoControl(
                    self.0,
                    IOCTL_STORAGE_QUERY_PROPERTY,
                    query.as_ptr().cast(),
                    query.len() as u32,
                    out.as_mut_ptr().cast(),
                    out.len() as u32,
                    &mut returned,
                    std::ptr::null_mut(),
                )
            };
            (ok != 0).then(|| {
                out.truncate(returned as usize);
                out
            })
        }
    }

    fn u16_at(data: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
    }
    fn i16_at(data: &[u8], at: usize) -> Option<i16> {
        Some(i16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
    }
    fn u32_at(data: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
    }

    /// (modelo, é NVMe) do STORAGE_DEVICE_DESCRIPTOR: ProductId (offset 16) e BusType
    /// (byte 28). O número de série (SerialNumberOffset) nunca é lido.
    fn describe(disk: &Disk) -> (Option<String>, bool) {
        let Some(data) = disk.property(STORAGE_DEVICE_PROPERTY) else {
            return (None, false);
        };
        let model = u32_at(&data, 16).and_then(|offset| {
            let bytes = data.get(offset as usize..).filter(|_| offset > 0)?;
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
            let text = String::from_utf8_lossy(&bytes[..end]).trim().to_string();
            (!text.is_empty()).then_some(text)
        });
        (model, data.get(28) == Some(&17))
    }

    /// STORAGE_TEMPERATURE_DATA_DESCRIPTOR: cabeçalho de 24 bytes (limites crítico/aviso
    /// em 8 e 10, quantidade em 12) e STORAGE_TEMPERATURE_INFO de 16 bytes; o primeiro é
    /// a temperatura composta do dispositivo.
    pub fn storage_temperatures() -> Vec<StorageTemperature> {
        let mut found = Vec::new();
        for index in 0..16 {
            let Some(disk) = Disk::open(index) else { break };
            let Some(data) = disk.property(STORAGE_DEVICE_TEMPERATURE_PROPERTY) else {
                continue;
            };
            let (Some(count), Some(celsius)) = (u16_at(&data, 12), i16_at(&data, 26)) else {
                continue;
            };
            if count == 0 || celsius <= 0 || celsius > 150 {
                continue;
            }
            let limit = |at| {
                i16_at(&data, at)
                    .filter(|t| *t > 0 && *t < 150)
                    .map(f32::from)
            };
            let (model, nvme) = describe(&disk);
            found.push(StorageTemperature {
                disk: index,
                model: model.unwrap_or_else(|| format!("Disco {index}")),
                nvme,
                celsius: celsius as f32,
                warning: limit(10),
                critical: limit(8),
            });
        }
        found
    }

    /// Discos físicos presentes (modelo e barramento), sem permissão de leitura do conteúdo.
    pub fn storage_devices() -> Vec<StorageDevice> {
        let mut found = Vec::new();
        for index in 0..16 {
            let Some(disk) = Disk::open(index) else { break };
            let (model, nvme) = describe(&disk);
            found.push(StorageDevice {
                disk: index,
                model: model.unwrap_or_else(|| format!("Disco {index}")),
                nvme,
            });
        }
        found
    }

    /// Clock base nominal da CPU (registro, MHz): o mesmo valor que o Gerenciador de Tarefas
    /// mostra como "Velocidade base".
    pub fn cpu_base_mhz() -> Option<f32> {
        Key::local_machine(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")?
            .number("~MHz")
            .filter(|mhz| *mhz > 0)
            .map(|mhz| mhz as f32)
    }

    /// Estado de energia (bateria, tomada, carga). `None` só se a chamada falhar.
    pub fn power_status() -> Option<PowerStatus> {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut status = SYSTEM_POWER_STATUS {
            ACLineStatus: 255,
            BatteryFlag: 255,
            BatteryLifePercent: 255,
            SystemStatusFlag: 0,
            BatteryLifeTime: u32::MAX,
            BatteryFullLifeTime: u32::MAX,
        };
        // SAFETY: estrutura válida e viva durante a chamada.
        (unsafe { GetSystemPowerStatus(&mut status) } != 0).then(|| {
            super::interpret_power(
                status.ACLineStatus,
                status.BatteryFlag,
                status.BatteryLifePercent,
                status.BatteryLifeTime,
            )
        })
    }

    /// Commit (memória virtual comprometida) e seu limite (RAM + pagefile), em bytes.
    pub fn commit_charge() -> Option<CommitCharge> {
        use windows_sys::Win32::System::ProcessStatus::{
            GetPerformanceInfo, PERFORMANCE_INFORMATION,
        };
        // SAFETY: estrutura de saída inicializada com zeros e o tamanho correto em `cb`.
        let mut info: PERFORMANCE_INFORMATION = unsafe { std::mem::zeroed() };
        info.cb = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
        // SAFETY: `info` válida durante a chamada.
        if unsafe { GetPerformanceInfo(&mut info, info.cb) } == 0 || info.PageSize == 0 {
            return None;
        }
        let page = info.PageSize as u64;
        Some(CommitCharge {
            used: (info.CommitTotal as u64).saturating_mul(page),
            limit: (info.CommitLimit as u64).saturating_mul(page),
        })
    }

    /// Interfaces de rede com tipo, estado, velocidade de enlace e endereços IPv4/IPv6.
    pub fn network_adapters() -> Vec<NetAdapter> {
        use std::net::{Ipv4Addr, Ipv6Addr};
        use windows_sys::Win32::{
            Foundation::ERROR_BUFFER_OVERFLOW,
            NetworkManagement::IpHelper::{
                GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
                GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
            },
            Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6},
        };
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let mut size = 16 * 1024u32;
        let mut buffer: Vec<u64> = Vec::new();
        let mut status = ERROR_BUFFER_OVERFLOW;
        for _ in 0..3 {
            // u64: alinhamento de 8 bytes exigido pela lista encadeada.
            buffer = vec![0u64; (size as usize).div_ceil(8)];
            // SAFETY: buffer com pelo menos `size` bytes, vivo durante a chamada.
            status = unsafe {
                GetAdaptersAddresses(
                    AF_UNSPEC as u32,
                    flags,
                    std::ptr::null(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                )
            };
            if status != ERROR_BUFFER_OVERFLOW {
                break;
            }
        }
        if status != 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut current = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        while !current.is_null() {
            // SAFETY: lista encadeada escrita pelo Windows dentro de `buffer`, vivo neste laço.
            let adapter = unsafe { &*current };
            let (mut ipv4, mut ipv6) = (Vec::new(), Vec::new());
            let mut unicast = adapter.FirstUnicastAddress;
            while !unicast.is_null() {
                // SAFETY: nó da lista de endereços dentro de `buffer`.
                let node = unsafe { &*unicast };
                let sockaddr = node.Address.lpSockaddr;
                if !sockaddr.is_null() {
                    // SAFETY: `lpSockaddr` aponta para a estrutura da família indicada.
                    let family = unsafe { (*sockaddr).sa_family };
                    if family == AF_INET {
                        let sin = unsafe { &*(sockaddr as *const SOCKADDR_IN) };
                        let b = unsafe { sin.sin_addr.S_un.S_addr }.to_ne_bytes();
                        ipv4.push(Ipv4Addr::new(b[0], b[1], b[2], b[3]).to_string());
                    } else if family == AF_INET6 {
                        let sin6 = unsafe { &*(sockaddr as *const SOCKADDR_IN6) };
                        let ip = Ipv6Addr::from(unsafe { sin6.sin6_addr.u.Byte });
                        // Link-local (fe80::/10) e loopback não identificam a máquina na rede.
                        if (ip.segments()[0] & 0xffc0) != 0xfe80 && !ip.is_loopback() {
                            ipv6.push(ip.to_string());
                        }
                    }
                }
                unicast = node.Next;
            }
            let speed = adapter.TransmitLinkSpeed;
            out.push(NetAdapter {
                // SAFETY: nomes são texto UTF-16 terminado em NUL dentro de `buffer`.
                name: unsafe { wide_ptr(adapter.FriendlyName) },
                description: unsafe { wide_ptr(adapter.Description) },
                kind: super::classify_if_type(adapter.IfType),
                up: adapter.OperStatus == 1,
                link_speed_bps: (speed != 0 && speed != u64::MAX).then_some(speed),
                ipv4,
                ipv6,
            });
            current = adapter.Next as *const IP_ADAPTER_ADDRESSES_LH;
        }
        out
    }

    // ---- PDH ----
    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
        PDH_MORE_DATA,
    };
    const PDH_FMT_NOCAP100: u32 = 0x8000;

    /// Consulta PDH com contadores em inglês (independe do idioma do Windows).
    /// Contadores que não existem na máquina simplesmente não são adicionados.
    pub struct Pdh {
        query: PDH_HQUERY,
        counters: Vec<Option<PDH_HCOUNTER>>,
    }
    // SAFETY: os handles PDH são usados só pelo dono (thread do sampler) e o acesso é
    // serializado por `&mut self`.
    unsafe impl Send for Pdh {}

    impl Pdh {
        pub fn new(paths: &[&str]) -> Option<Self> {
            let mut query: PDH_HQUERY = std::ptr::null_mut();
            // SAFETY: fonte de dados nula = tempo real.
            if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) } != 0 {
                return None;
            }
            let counters = paths
                .iter()
                .map(|path| {
                    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
                    let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
                    // SAFETY: caminho termina em NUL; consulta aberta acima.
                    (unsafe { PdhAddEnglishCounterW(query, wide.as_ptr(), 0, &mut counter) } == 0)
                        .then_some(counter)
                })
                .collect();
            Some(Self { query, counters })
        }
        pub fn available(&self, counter: usize) -> bool {
            matches!(self.counters.get(counter), Some(Some(_)))
        }
        /// Contadores de taxa precisam de duas coletas antes do primeiro valor.
        pub fn collect(&mut self) -> bool {
            // SAFETY: consulta válida enquanto `self` existe.
            unsafe { PdhCollectQueryData(self.query) == 0 }
        }
        /// (instância, valor) de cada instância válida do contador.
        pub fn values(&self, counter: usize) -> Vec<(String, f64)> {
            let Some(Some(counter)) = self.counters.get(counter).copied() else {
                return Vec::new();
            };
            let format = PDH_FMT_DOUBLE | PDH_FMT_NOCAP100;
            let (mut size, mut count) = (0u32, 0u32);
            // SAFETY: primeira chamada só informa o tamanho necessário.
            let status = unsafe {
                PdhGetFormattedCounterArrayW(
                    counter,
                    format,
                    &mut size,
                    &mut count,
                    std::ptr::null_mut(),
                )
            };
            if status != PDH_MORE_DATA || size == 0 {
                return Vec::new();
            }
            // Buffer alinhado para os itens; os nomes ficam no mesmo bloco, depois deles.
            let items =
                (size as usize).div_ceil(std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
            let mut buffer: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = vec![Default::default(); items];
            // SAFETY: `buffer` tem pelo menos `size` bytes.
            let status = unsafe {
                PdhGetFormattedCounterArrayW(
                    counter,
                    format,
                    &mut size,
                    &mut count,
                    buffer.as_mut_ptr(),
                )
            };
            if status != 0 {
                return Vec::new();
            }
            buffer
                .iter()
                .take(count as usize)
                .filter(|item| item.FmtValue.CStatus <= 1)
                .map(|item| {
                    // SAFETY: szName aponta para texto terminado em NUL dentro de `buffer`;
                    // o valor foi pedido como double.
                    let name = unsafe { wide_ptr(item.szName) };
                    (name, unsafe { item.FmtValue.Anonymous.doubleValue })
                })
                .collect()
        }
    }
    impl Drop for Pdh {
        fn drop(&mut self) {
            // SAFETY: consulta aberta por PdhOpenQueryW, fechada uma vez.
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    /// SAFETY: `ptr` nulo ou texto UTF-16 terminado em NUL.
    unsafe fn wide_ptr(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}

/// Leitura do registro do Windows, só de valores (HKLM, KEY_READ).
#[cfg(windows)]
pub(crate) mod registry {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE,
            KEY_READ, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_QWORD, REG_SZ,
        },
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub fn wide_to_string(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end]).trim().to_string()
    }

    pub struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: a chave foi aberta por RegOpenKeyExW e é fechada uma única vez.
            unsafe { RegCloseKey(self.0) };
        }
    }

    impl Key {
        fn open(parent: HKEY, path: &str) -> Option<Key> {
            let path = wide(path);
            let mut handle: HKEY = std::ptr::null_mut();
            // SAFETY: `path` termina em NUL e `handle` recebe a chave aberta.
            let status = unsafe { RegOpenKeyExW(parent, path.as_ptr(), 0, KEY_READ, &mut handle) };
            (status == ERROR_SUCCESS).then_some(Key(handle))
        }

        pub fn local_machine(path: &str) -> Option<Key> {
            Self::open(HKEY_LOCAL_MACHINE, path)
        }

        pub fn subkey(&self, name: &str) -> Option<Key> {
            Self::open(self.0, name)
        }

        pub fn subkeys(&self) -> Vec<String> {
            let mut names = Vec::new();
            for index in 0..1024 {
                let mut buffer = [0u16; 256];
                let mut len = buffer.len() as u32;
                // SAFETY: `buffer` tem `len` posições; demais saídas opcionais são nulas.
                let status = unsafe {
                    RegEnumKeyExW(
                        self.0,
                        index,
                        buffer.as_mut_ptr(),
                        &mut len,
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if status != ERROR_SUCCESS {
                    break;
                }
                names.push(String::from_utf16_lossy(&buffer[..len as usize]));
            }
            names
        }

        fn raw(&self, name: &str) -> Option<(u32, Vec<u8>)> {
            let name = wide(name);
            let mut kind = 0u32;
            let mut size = 0u32;
            // SAFETY: primeira chamada só consulta tipo e tamanho (sem buffer).
            let status = unsafe {
                RegQueryValueExW(
                    self.0,
                    name.as_ptr(),
                    std::ptr::null(),
                    &mut kind,
                    std::ptr::null_mut(),
                    &mut size,
                )
            };
            if status != ERROR_SUCCESS || size == 0 || size > 64 * 1024 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            // SAFETY: `data` tem exatamente `size` bytes.
            let status = unsafe {
                RegQueryValueExW(
                    self.0,
                    name.as_ptr(),
                    std::ptr::null(),
                    &mut kind,
                    data.as_mut_ptr(),
                    &mut size,
                )
            };
            if status != ERROR_SUCCESS {
                return None;
            }
            data.truncate(size as usize);
            Some((kind, data))
        }

        pub fn string(&self, name: &str) -> Option<String> {
            let (kind, data) = self.raw(name)?;
            if kind != REG_SZ && kind != REG_EXPAND_SZ {
                return None;
            }
            let (pairs, _) = data.as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().map(|c| u16::from_le_bytes(*c)).collect();
            Some(wide_to_string(&units))
        }

        pub fn number(&self, name: &str) -> Option<u64> {
            let (kind, data) = self.raw(name)?;
            match (kind, data.len()) {
                (REG_DWORD | REG_BINARY, 4) => {
                    Some(u32::from_le_bytes(data[..4].try_into().ok()?) as u64)
                }
                (REG_QWORD | REG_BINARY, 8) => Some(u64::from_le_bytes(data[..8].try_into().ok()?)),
                _ => None,
            }
        }
    }
}
