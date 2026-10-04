//! Fontes nativas do Windows Health (Block 07): SOMENTE LEITURA, em processo.
//!
//! * registro (HKLM, `KEY_READ`): versão, reinício pendente, histórico do Windows Update;
//! * Service Control Manager: estado e modo de início (`SC_MANAGER_CONNECT`, `QUERY_*`), sem
//!   iniciar, parar nem configurar nada;
//! * Configuration Manager/SetupAPI: dispositivos presentes e o código de problema de cada um;
//! * Event Log API (`EvtQuery`): consultas FILTRADAS por provedor/ID e janela de tempo, com teto,
//!   guardando só provedor, ID, nível e instante (a mensagem do evento nunca é lida);
//! * volumes: sistema de arquivos, somente-leitura e bit de "sujo".
//!
//! Nenhuma chamada daqui escreve no registro, executa SFC/DISM/CHKDSK, inicia/para serviço,
//! instala atualização ou reinicia. Fora do Windows tudo é "indisponível".
use crate::windows_health::{
    DeviceBatch, EventBatch, Probe, RawService, RawVolume, RestartProbes, SourceError, SystemRaw,
    UpdateTimes,
};

#[cfg(windows)]
pub use win::*;

#[cfg(not(windows))]
mod fallback {
    use super::*;
    fn unavailable<T>() -> Result<T, SourceError> {
        Err(SourceError::Unavailable(
            "Disponível somente no Windows".into(),
        ))
    }
    pub fn system() -> Result<SystemRaw, SourceError> {
        unavailable()
    }
    pub fn restart_probes() -> RestartProbes {
        RestartProbes {
            cbs_reboot_pending: Probe::Unavailable,
            wu_reboot_required: Probe::Unavailable,
            pending_file_renames: Probe::Unavailable,
        }
    }
    pub fn service(_name: &str) -> Result<RawService, SourceError> {
        unavailable()
    }
    pub fn devices() -> Result<DeviceBatch, SourceError> {
        unavailable()
    }
    pub fn events() -> Result<EventBatch, SourceError> {
        unavailable()
    }
    pub fn volumes() -> Result<Vec<RawVolume>, SourceError> {
        unavailable()
    }
    pub fn update_times() -> UpdateTimes {
        UpdateTimes::default()
    }
}
#[cfg(not(windows))]
pub use fallback::*;

#[cfg(windows)]
mod win {
    use super::*;
    use crate::{
        sensors::registry::{probe_key, Key},
        windows_health::{
            parse_wu_time, RawDevice, RawEvent, ServiceState, SourceNote, SourceState, StartType,
        },
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER,
            ERROR_NO_MORE_ITEMS, ERROR_SERVICE_DOES_NOT_EXIST, HANDLE, INVALID_HANDLE_VALUE,
        },
        Storage::FileSystem::{
            CreateFileW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::IO::DeviceIoControl,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    fn from_wide(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end]).trim().to_string()
    }
    fn text(value: Option<String>) -> Option<String> {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    // ------------------------------------------------------------------ sistema

    pub fn system() -> Result<SystemRaw, SourceError> {
        let key = Key::local_machine(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").ok_or_else(
            || SourceError::Unavailable("Registro da versão do Windows ilegível".into()),
        )?;
        let architecture =
            Key::local_machine(r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment")
                .and_then(|k| k.string("PROCESSOR_ARCHITECTURE"))
                .map(|arch| match arch.to_ascii_uppercase().as_str() {
                    "AMD64" => "x64".to_string(),
                    "ARM64" => "ARM64".to_string(),
                    "X86" => "x86".to_string(),
                    _ => arch,
                });
        Ok(SystemRaw {
            product_name: text(key.string("ProductName")),
            edition: text(key.string("EditionID")),
            version: text(key.string("DisplayVersion")).or_else(|| text(key.string("ReleaseId"))),
            build: text(key.string("CurrentBuild")),
            ubr: key.number("UBR"),
            architecture,
            installed_at: key.number("InstallDate").map(|secs| secs as i64 * 1000),
            boot_time: Some(sysinfo::System::boot_time()),
            uptime_secs: Some(sysinfo::System::uptime()),
        })
    }

    // ------------------------------------------------------------------ reinício pendente

    pub fn restart_probes() -> RestartProbes {
        let cbs = probe_key(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending",
        );
        let wu = probe_key(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired",
        );
        let renames = match Key::local_machine(r"SYSTEM\CurrentControlSet\Control\Session Manager")
        {
            // Multi-string vazia ocupa só 2–4 bytes: só conta se há alguma operação.
            Some(key) => key.probe_value("PendingFileRenameOperations", 4),
            None => Probe::Unavailable,
        };
        RestartProbes {
            cbs_reboot_pending: cbs,
            wu_reboot_required: wu,
            pending_file_renames: renames,
        }
    }

    // ------------------------------------------------------------------ Windows Update (registro)

    pub fn update_times() -> UpdateTimes {
        let read = |sub: &str, id: &str, label: &str, notes: &mut Vec<SourceNote>| -> Option<i64> {
            let path = format!(
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\Results\{sub}"
            );
            match probe_key(&path) {
                Probe::Present => {
                    let at = Key::local_machine(&path)
                        .and_then(|k| k.string("LastSuccessTime"))
                        .and_then(|t| parse_wu_time(&t));
                    if at.is_none() {
                        notes.push(SourceNote::new(
                            id,
                            label,
                            SourceState::Unavailable,
                            Some("O Windows não registrou a data neste valor"),
                        ));
                    } else {
                        notes.push(SourceNote::ok(id, label));
                    }
                    at
                }
                Probe::Absent => {
                    notes.push(SourceNote::new(
                        id,
                        label,
                        SourceState::Unavailable,
                        Some("O Windows não mantém mais este registro nesta versão"),
                    ));
                    None
                }
                Probe::Denied => {
                    notes.push(SourceNote::new(
                        id,
                        label,
                        SourceState::RequiresElevation,
                        Some("Requer privilégio administrativo"),
                    ));
                    None
                }
                Probe::Unavailable => {
                    notes.push(SourceNote::new(
                        id,
                        label,
                        SourceState::Unavailable,
                        Some("Registro ilegível"),
                    ));
                    None
                }
            }
        };
        let mut notes = Vec::new();
        let install = read(
            "Install",
            "update_install_time",
            "Última instalação bem-sucedida",
            &mut notes,
        );
        let detect = read(
            "Detect",
            "update_scan_time",
            "Última verificação bem-sucedida",
            &mut notes,
        );
        UpdateTimes {
            last_install_success_at: install,
            last_scan_success_at: detect,
            notes,
        }
    }

    // ------------------------------------------------------------------ serviços (SCM, só consulta)

    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceConfig2W,
        QueryServiceConfigW, QueryServiceStatusEx, QUERY_SERVICE_CONFIGW, SC_HANDLE,
        SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
        SERVICE_DELAYED_AUTO_START_INFO, SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS,
        SERVICE_STATUS_PROCESS,
    };

    struct Sc(SC_HANDLE);
    impl Drop for Sc {
        fn drop(&mut self) {
            // SAFETY: handle aberto por OpenSCManagerW/OpenServiceW, fechado uma vez.
            unsafe { CloseServiceHandle(self.0) };
        }
    }

    pub fn service(name: &str) -> Result<RawService, SourceError> {
        // SAFETY: parâmetros nulos = máquina local e banco de serviços ativo; só conexão.
        let manager =
            unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
        if manager.is_null() {
            return Err(denied_or(
                unsafe { GetLastError() },
                "Não foi possível conectar ao gerenciador de serviços",
            ));
        }
        let manager = Sc(manager);
        let wide_name = wide(name);
        // SAFETY: nome terminado em NUL; direitos só de consulta.
        let handle = unsafe {
            OpenServiceW(
                manager.0,
                wide_name.as_ptr(),
                SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG,
            )
        };
        if handle.is_null() {
            let code = unsafe { GetLastError() };
            return Err(if code == ERROR_SERVICE_DOES_NOT_EXIST {
                SourceError::Unavailable("O serviço não existe nesta instalação".into())
            } else {
                denied_or(code, "Não foi possível consultar o serviço")
            });
        }
        let service = Sc(handle);

        // SAFETY: estrutura de saída com o tamanho informado.
        let mut status: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
        let mut needed = 0u32;
        let ok = unsafe {
            QueryServiceStatusEx(
                service.0,
                SC_STATUS_PROCESS_INFO,
                (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
                std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut needed,
            )
        };
        if ok == 0 {
            return Err(denied_or(
                unsafe { GetLastError() },
                "Não foi possível ler o estado do serviço",
            ));
        }
        let state = match status.dwCurrentState {
            1 => ServiceState::Stopped,
            2 => ServiceState::StartPending,
            3 => ServiceState::StopPending,
            4 | 5 => ServiceState::Running,
            6 | 7 => ServiceState::Paused,
            _ => ServiceState::Unknown,
        };

        // Modo de início: 1ª chamada só descobre o tamanho do buffer.
        let mut size = 0u32;
        // SAFETY: sem buffer, só consulta o tamanho (falha esperada com ERROR_INSUFFICIENT_BUFFER).
        unsafe { QueryServiceConfigW(service.0, std::ptr::null_mut(), 0, &mut size) };
        let start = if unsafe { GetLastError() } == ERROR_INSUFFICIENT_BUFFER && size > 0 {
            // u64: alinhamento de 8 bytes exigido pela estrutura.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            // SAFETY: buffer com pelo menos \`size\` bytes.
            let ok = unsafe {
                QueryServiceConfigW(service.0, buffer.as_mut_ptr().cast(), size, &mut size)
            };
            if ok == 0 {
                StartType::Unknown
            } else {
                // SAFETY: preenchido pelo Windows dentro de \`buffer\`.
                let config = unsafe { &*(buffer.as_ptr().cast::<QUERY_SERVICE_CONFIGW>()) };
                match config.dwStartType {
                    0..=2 => {
                        let mut delayed = SERVICE_DELAYED_AUTO_START_INFO {
                            fDelayedAutostart: 0,
                        };
                        let mut needed = 0u32;
                        // SAFETY: estrutura de 4 bytes com o tamanho informado.
                        let ok = unsafe {
                            QueryServiceConfig2W(
                                service.0,
                                SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                                (&mut delayed as *mut SERVICE_DELAYED_AUTO_START_INFO).cast(),
                                std::mem::size_of::<SERVICE_DELAYED_AUTO_START_INFO>() as u32,
                                &mut needed,
                            )
                        };
                        if ok != 0 && delayed.fDelayedAutostart != 0 {
                            StartType::AutomaticDelayed
                        } else {
                            StartType::Automatic
                        }
                    }
                    3 => StartType::Manual,
                    4 => StartType::Disabled,
                    _ => StartType::Unknown,
                }
            }
        } else {
            StartType::Unknown
        };
        Ok(RawService { state, start })
    }

    fn denied_or(code: u32, message: &str) -> SourceError {
        if code == ERROR_ACCESS_DENIED {
            SourceError::RequiresElevation(format!("{message}: requer privilégio administrativo"))
        } else {
            SourceError::Unavailable(format!("{message} (erro {code})"))
        }
    }

    // ------------------------------------------------------------------ dispositivos (SetupAPI + CM)

    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Get_DevNode_Status, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
        SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT,
        SPDRP_CLASS, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME, SPDRP_MFG, SP_DEVINFO_DATA,
    };

    fn device_property(set: isize, data: &SP_DEVINFO_DATA, property: u32) -> Option<String> {
        let mut buffer = [0u16; 512];
        let mut kind = 0u32;
        let mut required = 0u32;
        // SAFETY: buffer de 1024 bytes; \`data\` válido enquanto o conjunto de dispositivos existe.
        let ok = unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                set,
                data,
                property,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 2) as u32,
                &mut required,
            )
        };
        (ok != 0)
            .then(|| from_wide(&buffer))
            .filter(|text| !text.is_empty())
    }

    /// Dispositivos PRESENTES e o código de problema de cada um (0 = sem problema).
    pub fn devices() -> Result<DeviceBatch, SourceError> {
        // SAFETY: classe/enumerador/janela nulos + DIGCF_*: todos os dispositivos presentes.
        let set = unsafe {
            SetupDiGetClassDevsW(
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                DIGCF_PRESENT | DIGCF_ALLCLASSES,
            )
        };
        if set == -1 || set == 0 {
            return Err(denied_or(
                unsafe { GetLastError() },
                "Não foi possível enumerar os dispositivos",
            ));
        }
        struct Set(isize);
        impl Drop for Set {
            fn drop(&mut self) {
                // SAFETY: conjunto criado por SetupDiGetClassDevsW, destruído uma vez.
                unsafe { SetupDiDestroyDeviceInfoList(self.0) };
            }
        }
        let guard = Set(set);
        let (mut total, mut problems) = (0u32, Vec::new());
        for index in 0..8192u32 {
            // SAFETY: estrutura inicializada com o tamanho correto em \`cbSize\`.
            let mut data: SP_DEVINFO_DATA = unsafe { std::mem::zeroed() };
            data.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if unsafe { SetupDiEnumDeviceInfo(guard.0, index, &mut data) } == 0 {
                break; // ERROR_NO_MORE_ITEMS
            }
            total += 1;
            let (mut status, mut problem) = (0u32, 0u32);
            // SAFETY: saídas válidas; devnode vindo da própria enumeração.
            let result =
                unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, data.DevInst, 0) };
            if result != 0 || problem == 0 {
                continue;
            }
            problems.push(RawDevice {
                name: device_property(guard.0, &data, SPDRP_FRIENDLYNAME)
                    .or_else(|| device_property(guard.0, &data, SPDRP_DEVICEDESC))
                    .unwrap_or_else(|| "Dispositivo desconhecido".into()),
                class: device_property(guard.0, &data, SPDRP_CLASS),
                manufacturer: device_property(guard.0, &data, SPDRP_MFG),
                problem_code: problem,
            });
        }
        let _ = ERROR_NO_MORE_ITEMS;
        Ok(DeviceBatch { total, problems })
    }

    // ------------------------------------------------------------------ Event Log (consultas filtradas)

    use windows_sys::Win32::System::EventLog::{
        EvtClose, EvtCreateRenderContext, EvtNext, EvtQuery, EvtRender, EVT_VARIANT,
    };

    const EVT_QUERY_CHANNEL_PATH: u32 = 0x1;
    const EVT_QUERY_REVERSE: u32 = 0x200;
    const EVT_RENDER_CONTEXT_SYSTEM: u32 = 1;
    const EVT_RENDER_EVENT_VALUES: u32 = 0;
    // EVT_VARIANT_TYPE
    const VAR_STRING: u32 = 1;
    const VAR_BYTE: u32 = 4;
    const VAR_UINT16: u32 = 6;
    const VAR_FILETIME: u32 = 17;
    // EVT_SYSTEM_PROPERTY_ID
    const SYS_PROVIDER: usize = 0;
    const SYS_EVENT_ID: usize = 2;
    const SYS_LEVEL: usize = 4;
    const SYS_TIME: usize = 8;
    /// Janela de 7 dias / 24 h em milissegundos (XPath \`timediff\`).
    const WEEK_MS: u64 = 604_800_000;
    const DAY_MS: u64 = 86_400_000;
    /// Teto de eventos lidos por consulta: nunca percorre o log inteiro.
    const CAP_ERRORS: usize = 2000;
    const CAP_SIGNALS: usize = 1000;
    const CAP_WARNINGS: usize = 1000;

    struct Evt(isize);
    impl Drop for Evt {
        fn drop(&mut self) {
            if self.0 != 0 {
                // SAFETY: handle devolvido pela Event Log API, fechado uma vez.
                unsafe { EvtClose(self.0) };
            }
        }
    }

    /// FILETIME (100 ns desde 1601) → ms desde a época Unix.
    fn filetime_ms(ticks: u64) -> i64 {
        (ticks / 10_000) as i64 - 11_644_473_600_000
    }

    fn render(context: isize, event: isize) -> Option<RawEvent> {
        // SAFETY: buffer alinhado de 8 KB; o Windows informa em \`used\` o que precisar.
        let mut buffer = vec![0u64; 1024];
        let (mut used, mut count) = (0u32, 0u32);
        let ok = unsafe {
            EvtRender(
                context,
                event,
                EVT_RENDER_EVENT_VALUES,
                (buffer.len() * 8) as u32,
                buffer.as_mut_ptr().cast(),
                &mut used,
                &mut count,
            )
        };
        if ok == 0 || count < 9 {
            return None;
        }
        let values = buffer.as_ptr().cast::<EVT_VARIANT>();
        // SAFETY: \`count\` ≥ 9 variantes escritas pelo Windows em \`buffer\`.
        unsafe {
            let at = |index: usize| &*values.add(index);
            let provider = at(SYS_PROVIDER);
            let id = at(SYS_EVENT_ID);
            let level = at(SYS_LEVEL);
            let time = at(SYS_TIME);
            if provider.Type & 0x7f != VAR_STRING || provider.Anonymous.StringVal.is_null() {
                return None;
            }
            let mut name = Vec::new();
            let mut cursor = provider.Anonymous.StringVal;
            while *cursor != 0 && name.len() < 256 {
                name.push(*cursor);
                cursor = cursor.add(1);
            }
            Some(RawEvent {
                provider: String::from_utf16_lossy(&name),
                id: if id.Type & 0x7f == VAR_UINT16 {
                    id.Anonymous.UInt16Val as u32
                } else {
                    return None;
                },
                level: if level.Type & 0x7f == VAR_BYTE {
                    level.Anonymous.ByteVal
                } else {
                    0
                },
                at: if time.Type & 0x7f == VAR_FILETIME {
                    filetime_ms(time.Anonymous.FileTimeVal)
                } else {
                    return None;
                },
            })
        }
    }

    /// Lê até \`cap\` eventos (mais recentes primeiro). Com \`render_events: false\` só conta.
    fn query(
        channel: &str,
        xpath: &str,
        cap: usize,
        render_events: bool,
    ) -> Result<(Vec<RawEvent>, usize, bool), SourceError> {
        let channel_w = wide(channel);
        let xpath_w = wide(xpath);
        // SAFETY: strings terminadas em NUL; sessão nula = máquina local.
        let handle = unsafe {
            EvtQuery(
                0,
                channel_w.as_ptr(),
                xpath_w.as_ptr(),
                EVT_QUERY_CHANNEL_PATH | EVT_QUERY_REVERSE,
            )
        };
        if handle == 0 {
            let code = unsafe { GetLastError() };
            return Err(if code == ERROR_ACCESS_DENIED {
                SourceError::RequiresElevation(format!(
                    "O canal {channel} requer privilégio administrativo"
                ))
            } else {
                SourceError::Unavailable(format!("Canal {channel} indisponível (erro {code})"))
            });
        }
        let query = Evt(handle);
        // SAFETY: contexto de renderização das propriedades do sistema; fechado pelo guard.
        let context =
            Evt(unsafe { EvtCreateRenderContext(0, std::ptr::null(), EVT_RENDER_CONTEXT_SYSTEM) });
        let (mut events, mut seen, mut truncated) = (Vec::new(), 0usize, false);
        'read: loop {
            let mut batch = [0isize; 64];
            let mut returned = 0u32;
            // SAFETY: \`batch\` comporta 64 handles; timeout de 2 s por lote.
            let ok = unsafe {
                EvtNext(
                    query.0,
                    batch.len() as u32,
                    batch.as_mut_ptr(),
                    2000,
                    0,
                    &mut returned,
                )
            };
            if ok == 0 {
                let code = unsafe { GetLastError() };
                if code == ERROR_NO_MORE_ITEMS {
                    break;
                }
                return Err(SourceError::Unavailable(format!(
                    "Leitura do canal {channel} falhou (erro {code})"
                )));
            }
            for handle in batch.iter().take(returned as usize) {
                let event = Evt(*handle);
                if seen >= cap {
                    truncated = true;
                    continue; // fecha o restante do lote pelo guard
                }
                seen += 1;
                if render_events {
                    if let Some(parsed) = render(context.0, event.0) {
                        events.push(parsed);
                    }
                }
            }
            if truncated {
                break 'read;
            }
        }
        Ok((events, seen, truncated))
    }

    fn timed(window_ms: u64, condition: &str) -> String {
        format!("*[System[{condition} and TimeCreated[timediff(@SystemTime) <= {window_ms}]]]")
    }

    pub fn events() -> Result<EventBatch, SourceError> {
        // Críticos e erros do System (contagem por nível): sozinhos nunca mudam o estado.
        let (errors, _, errors_cut) = query(
            "System",
            &timed(WEEK_MS, "(Level=1 or Level=2)"),
            CAP_ERRORS,
            true,
        )?;
        // Sinais específicos por provedor/ID (qualquer nível): é o que as regras avaliam.
        let signals_xpath = timed(
            WEEK_MS,
            "((Provider[@Name='Microsoft-Windows-Kernel-Power'] and EventID=41) \
             or (Provider[@Name='EventLog'] and EventID=6008) \
             or (Provider[@Name='Microsoft-Windows-WER-SystemErrorReporting'] and EventID=1001) \
             or (Provider[@Name='disk'] and (EventID=7 or EventID=11 or EventID=15 or EventID=51 or EventID=52 or EventID=153)) \
             or ((Provider[@Name='Ntfs'] or Provider[@Name='Microsoft-Windows-Ntfs']) and (EventID=55 or EventID=137 or EventID=140)) \
             or (Provider[@Name='Service Control Manager'] and (EventID=7000 or EventID=7001 or EventID=7009 or EventID=7011 or EventID=7031 or EventID=7034)))",
        );
        let (signals, _, signals_cut) = query("System", &signals_xpath, CAP_SIGNALS, true)?;
        // Avisos: só a contagem das últimas 24 h, com teto (ruído; nunca avaliado).
        let (_, warnings, warnings_cut) =
            query("System", &timed(DAY_MS, "Level=3"), CAP_WARNINGS, false)?;

        let mut notes = vec![SourceNote::ok(
            "events",
            "Registro de Eventos do Windows (System)",
        )];
        let update_failures = match query(
            "Microsoft-Windows-WindowsUpdateClient/Operational",
            &timed(WEEK_MS, "(EventID=20 or EventID=31)"),
            200,
            true,
        ) {
            Ok((events, _, _)) => {
                notes.push(SourceNote::ok(
                    "events:update",
                    "Falhas do Windows Update (Event Log)",
                ));
                Some(events)
            }
            Err(error) => {
                let (state, reason) = match &error {
                    SourceError::RequiresElevation(reason) => {
                        (SourceState::RequiresElevation, reason.clone())
                    }
                    SourceError::Unavailable(reason) => (SourceState::Unavailable, reason.clone()),
                };
                notes.push(SourceNote::new(
                    "events:update",
                    "Falhas do Windows Update (Event Log)",
                    state,
                    Some(&reason),
                ));
                None
            }
        };
        // Última instalação bem-sucedida: o evento 19 mais recente (sem janela de tempo).
        let last_update_success = match query(
            "Microsoft-Windows-WindowsUpdateClient/Operational",
            "*[System[EventID=19]]",
            1,
            true,
        ) {
            Ok((events, _, _)) => events.first().map(|e| e.at),
            Err(_) => None,
        };
        let app_events = match query(
            "Application",
            &timed(
                WEEK_MS,
                "((Provider[@Name='Application Error'] and EventID=1000) or (Provider[@Name='Application Hang'] and EventID=1002))",
            ),
            CAP_SIGNALS,
            true,
        ) {
            Ok((events, _, _)) => {
                notes.push(SourceNote::ok("events:application", "Falhas de aplicativos (Event Log)"));
                Some(events)
            }
            Err(error) => {
                let (state, reason) = match &error {
                    SourceError::RequiresElevation(reason) => (SourceState::RequiresElevation, reason.clone()),
                    SourceError::Unavailable(reason) => (SourceState::Unavailable, reason.clone()),
                };
                notes.push(SourceNote::new("events:application", "Falhas de aplicativos (Event Log)", state, Some(&reason)));
                None
            }
        };
        Ok(EventBatch {
            errors,
            signals,
            warnings_24h: warnings as u32,
            warnings_capped: warnings_cut,
            update_failures,
            app_events,
            last_update_success,
            truncated: errors_cut || signals_cut,
            notes,
        })
    }

    // ------------------------------------------------------------------ volumes

    const FSCTL_IS_VOLUME_DIRTY: u32 = 0x0009_0078;
    const VOLUME_IS_DIRTY: u32 = 0x1;
    const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;
    const DRIVE_FIXED: u32 = 3;

    /// Bit de "sujo" por `FSCTL_IS_VOLUME_DIRTY` num handle de VOLUME aberto só para leitura (o mesmo
    /// que o `fsutil dirty query` usa; nada é escrito). Sem privilégio administrativo o Windows nega
    /// a abertura: `Err(true)` = requer elevação (nunca é lido como "volume limpo").
    fn dirty_flag(letter: char) -> Result<bool, bool> {
        const GENERIC_READ: u32 = 0x8000_0000;
        let path = wide(&format!(r"\\.\{letter}:"));
        // SAFETY: caminho terminado em NUL; abertura somente leitura do volume.
        let handle: HANDLE = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(unsafe { GetLastError() } == ERROR_ACCESS_DENIED);
        }
        let mut flags = 0u32;
        let mut returned = 0u32;
        // SAFETY: saída de 4 bytes; handle válido até o CloseHandle abaixo.
        let ok = unsafe {
            DeviceIoControl(
                handle,
                FSCTL_IS_VOLUME_DIRTY,
                std::ptr::null(),
                0,
                (&mut flags as *mut u32).cast(),
                4,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        // SAFETY: handle aberto por CreateFileW, fechado uma vez.
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return Err(false);
        }
        Ok(flags & VOLUME_IS_DIRTY != 0)
    }

    /// Letras de unidade com verificação de disco (autochk) AGENDADA para a próxima inicialização:
    /// entradas `autocheck autochk ... \??\X:` em `BootExecute`. Legível sem administrador;
    /// `None` se o valor não pôde ser lido (nunca vira "nenhuma agendada").
    fn scheduled_checks() -> Option<Vec<char>> {
        let entries = Key::local_machine(r"SYSTEM\CurrentControlSet\Control\Session Manager")?
            .multi_string("BootExecute")?;
        Some(
            entries
                .iter()
                .filter(|entry| entry.to_ascii_lowercase().starts_with("autocheck"))
                .filter_map(|entry| {
                    let at = entry.find(r"\??\")? + 4;
                    let letter = entry[at..].chars().next()?;
                    (entry[at + 1..].starts_with(':') && letter.is_ascii_alphabetic())
                        .then(|| letter.to_ascii_uppercase())
                })
                .collect(),
        )
    }

    pub fn volumes() -> Result<Vec<RawVolume>, SourceError> {
        // SAFETY: sem parâmetros; devolve a máscara das letras de unidade.
        let mask = unsafe { GetLogicalDrives() };
        if mask == 0 {
            return Err(SourceError::Unavailable(
                "Não foi possível enumerar as unidades".into(),
            ));
        }
        let scheduled = scheduled_checks();
        let mut out = Vec::new();
        for index in 0..26u32 {
            if mask & (1 << index) == 0 {
                continue;
            }
            let letter = (b'A' + index as u8) as char;
            let root = wide(&format!(r"{letter}:\"));
            // SAFETY: raiz terminada em NUL.
            if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
                continue;
            }
            let (mut flags, mut name) = (0u32, [0u16; 64]);
            // SAFETY: buffer de 64 posições; demais saídas opcionais nulas.
            let ok = unsafe {
                GetVolumeInformationW(
                    root.as_ptr(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut flags,
                    name.as_mut_ptr(),
                    name.len() as u32,
                )
            };
            let (dirty, dirty_denied) = match dirty_flag(letter) {
                Ok(dirty) => (Some(dirty), false),
                Err(denied) => (None, denied),
            };
            out.push(RawVolume {
                mount: format!("{letter}:"),
                filesystem: (ok != 0)
                    .then(|| from_wide(&name))
                    .filter(|n| !n.is_empty()),
                read_only: (ok != 0).then_some(flags & FILE_READ_ONLY_VOLUME != 0),
                dirty,
                dirty_denied,
                check_scheduled: scheduled.as_ref().map(|letters| letters.contains(&letter)),
            });
        }
        Ok(out)
    }

    #[allow(dead_code)]
    fn _types() {
        let _: Option<RawDevice> = None;
    }
}
