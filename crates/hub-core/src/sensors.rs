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

/// Adaptador de vídeo físico presente agora.
#[derive(Debug, Clone, PartialEq)]
pub struct Adapter {
    /// LUID do boot atual (liga o adaptador aos contadores PDH). Nunca é persistido.
    pub luid: u64,
    pub name: String,
    /// Segmento de memória DEDICADO informado pelo driver. Em GPU integrada é pequeno e não
    /// representa a memória gráfica total: ela usa a memória do sistema (`shared`).
    pub dedicated: Option<u64>,
    /// Limite de memória do sistema que a GPU pode usar (compartilhada), segundo o driver.
    pub shared: Option<u64>,
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
    use super::{Adapter, StorageTemperature};
    pub fn gpu_adapters() -> Vec<Adapter> {
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
    use super::{Adapter, StorageTemperature};

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

    /// Adaptadores físicos presentes: entradas do registro DirectX (descrição, LUID e
    /// memória dedicada) cujo LUID abre agora. Renderizador de software fica de fora.
    pub fn gpu_adapters() -> Vec<Adapter> {
        let Some(root) = Key::local_machine(r"SOFTWARE\Microsoft\DirectX") else {
            return Vec::new();
        };
        let mut found: Vec<Adapter> = Vec::new();
        for sub in root.subkeys() {
            let Some(key) = root.subkey(&sub) else {
                continue;
            };
            let (Some(name), Some(luid)) = (key.string("Description"), key.number("AdapterLuid"))
            else {
                continue;
            };
            if name.is_empty()
                || name.starts_with("Microsoft Basic")
                || found.iter().any(|a| a.luid == luid)
                || KmtAdapter::open(luid).is_none()
            {
                continue;
            }
            let dedicated = key.number("DedicatedVideoMemory").filter(|m| *m > 0);
            let shared = key.number("SharedSystemMemory").filter(|m| *m > 0);
            found.push(Adapter {
                luid,
                name,
                dedicated,
                shared,
            });
        }
        found
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
            let units: Vec<u16> = data
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
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
