//! Fontes nativas do Network & Security (Block 08): SOMENTE LEITURA, em processo.
//!
//! * registro (HKLM, `KEY_READ`): política do Firewall por perfil, estado e assinaturas do Microsoft
//!   Defender, Secure Boot e a categoria (Público/Privado/Domínio) da rede de cada interface;
//! * Windows Security Center (`WscGetSecurityProviderHealth`): a SAÚDE agregada de antivírus e
//!   firewall (o nome de cada produto de terceiros não é exposto sem WMI e não é inventado);
//! * TPM Base Services (`Tbsi_GetDeviceInfo`): presença e versão do TPM, sem comandos ao chip;
//! * `GetFirmwareType`: UEFI ou BIOS legado.
//!
//! Nada aqui escreve no registro, altera firewall, inicia varredura do Defender, mexe em BitLocker,
//! lê chaves de recuperação, senhas, tokens ou segredos de Wi-Fi, ou abre/fecha portas. Fora do
//! Windows tudo é "indisponível".
use crate::network_security::{
    BitlockerVolume, DefenderRaw, FirewallProfileRaw, ProfileKind, RawConnection, SecureBootRaw,
    TpmRaw, WscHealth, WscProvider,
};
use crate::windows_health::SourceError;
use std::collections::HashMap;

/// Conexões TCP estabelecidas (IPv4 e IPv6) com o PID dono. Só leitura; sem reverse DNS.
pub fn connections() -> Result<Vec<RawConnection>, SourceError> {
    use netstat2::{
        get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState,
    };
    let sockets = get_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP,
    )
    .map_err(|e| SourceError::Unavailable(format!("Não foi possível enumerar conexões: {e}")))?;
    let mut out = Vec::new();
    for socket in sockets {
        let ProtocolSocketInfo::Tcp(t) = socket.protocol_socket_info else {
            continue;
        };
        if t.state != TcpState::Established {
            continue;
        }
        out.push(RawConnection {
            local: t.local_addr,
            local_port: t.local_port,
            remote: t.remote_addr,
            remote_port: t.remote_port,
            pid: socket.associated_pids.first().copied(),
        });
    }
    Ok(out)
}

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
    pub fn firewall_policy() -> Result<Vec<FirewallProfileRaw>, SourceError> {
        unavailable()
    }
    pub fn network_profiles(
        _guids: &[String],
    ) -> Result<HashMap<String, ProfileKind>, SourceError> {
        unavailable()
    }
    pub fn wsc_health(_provider: WscProvider) -> Option<WscHealth> {
        None
    }
    pub fn defender() -> Result<DefenderRaw, SourceError> {
        unavailable()
    }
    pub fn secure_boot() -> Result<SecureBootRaw, SourceError> {
        unavailable()
    }
    pub fn tpm() -> Result<TpmRaw, SourceError> {
        unavailable()
    }
    pub fn bitlocker() -> Result<Vec<BitlockerVolume>, SourceError> {
        unavailable()
    }
}
#[cfg(not(windows))]
pub use fallback::*;

#[cfg(windows)]
mod win {
    use super::*;
    use crate::{
        network_security::{FirewallAction, SecureBootState, DEFENDER_PROVIDER_GUID},
        sensors::registry::{probe_key, Key},
        windows_health::{Millis, Probe, RawService},
    };

    /// FILETIME (8 bytes, 100 ns desde 1601) → ms desde a época Unix; zero = nunca.
    fn filetime_bytes_ms(bytes: &[u8]) -> Option<Millis> {
        let ticks = u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?);
        (ticks > 0).then(|| (ticks / 10_000) as Millis - 11_644_473_600_000)
    }

    // ------------------------------------------------------------------ firewall (política por perfil)

    const FIREWALL_BASE: &str =
        r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy";

    /// Política configurada do Firewall do Windows por perfil. É a configuração gravada, não o estado
    /// de execução; ação padrão ausente no registro significa "padrão do Windows" e fica `None`.
    pub fn firewall_policy() -> Result<Vec<FirewallProfileRaw>, SourceError> {
        let mut profiles = Vec::new();
        for (key_name, kind) in [
            ("DomainProfile", ProfileKind::Domain),
            ("StandardProfile", ProfileKind::Private),
            ("PublicProfile", ProfileKind::Public),
        ] {
            let path = format!(r"{FIREWALL_BASE}\{key_name}");
            match probe_key(&path) {
                Probe::Present => {
                    let key = Key::local_machine(&path);
                    let action = |name: &str| {
                        key.as_ref().and_then(|k| k.number(name)).map(|v| {
                            if v == 0 {
                                FirewallAction::Allow
                            } else {
                                FirewallAction::Block
                            }
                        })
                    };
                    profiles.push(FirewallProfileRaw {
                        kind,
                        enabled: key
                            .as_ref()
                            .and_then(|k| k.number("EnableFirewall"))
                            .map(|v| v != 0),
                        default_inbound: action("DefaultInboundAction"),
                        default_outbound: action("DefaultOutboundAction"),
                    });
                }
                Probe::Denied => {
                    return Err(SourceError::RequiresElevation(
                        "A política do firewall requer privilégio administrativo".into(),
                    ))
                }
                _ => profiles.push(FirewallProfileRaw {
                    kind,
                    enabled: None,
                    default_inbound: None,
                    default_outbound: None,
                }),
            }
        }
        Ok(profiles)
    }

    /// Categoria (Público/Privado/Domínio) de cada rede, pelo GUID que o Windows dá à interface.
    /// Rede sem perfil gravado não entra no mapa (a categoria é desconhecida, não "pública").
    ///
    /// A chave `NetworkList\Profiles` só abre com privilégio administrativo; sem ele a categoria vira
    /// `requires_elevation` (nunca é adivinhada).
    pub fn network_profiles(guids: &[String]) -> Result<HashMap<String, ProfileKind>, SourceError> {
        match probe_key(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Profiles") {
            Probe::Present => {}
            Probe::Denied => {
                return Err(SourceError::RequiresElevation(
                    "A categoria da rede (Público/Privado/Domínio) fica em uma chave do registro que exige administrador".into(),
                ))
            }
            _ => {
                return Err(SourceError::Unavailable(
                    "A lista de perfis de rede não está disponível".into(),
                ))
            }
        }
        let mut map = HashMap::new();
        for guid in guids {
            let path = format!(
                r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Profiles\{guid}"
            );
            let Some(key) = Key::local_machine(&path) else {
                continue;
            };
            let kind = match key.number("Category") {
                Some(0) => ProfileKind::Public,
                Some(1) => ProfileKind::Private,
                Some(2) => ProfileKind::Domain,
                _ => continue,
            };
            map.insert(guid.clone(), kind);
        }
        Ok(map)
    }

    // ------------------------------------------------------------------ Security Center (saúde)

    use windows_sys::Win32::System::SecurityCenter::{
        WscGetSecurityProviderHealth, WSC_SECURITY_PROVIDER_ANTIVIRUS,
        WSC_SECURITY_PROVIDER_FIREWALL,
    };

    /// Saúde agregada que o Windows Security Center atribui à categoria. `None` se o serviço não responde.
    pub fn wsc_health(provider: WscProvider) -> Option<WscHealth> {
        let provider = match provider {
            WscProvider::Antivirus => WSC_SECURITY_PROVIDER_ANTIVIRUS,
            WscProvider::Firewall => WSC_SECURITY_PROVIDER_FIREWALL,
        };
        let mut health = 0i32;
        // SAFETY: saída de 4 bytes válida durante a chamada; só consulta o serviço de segurança.
        let result = unsafe { WscGetSecurityProviderHealth(provider as u32, &mut health) };
        (result == 0).then_some(match health {
            0 => WscHealth::Good,
            1 => WscHealth::NotMonitored,
            2 => WscHealth::Poor,
            3 => WscHealth::Snooze,
            _ => return None,
        })
    }

    // ------------------------------------------------------------------ Microsoft Defender

    const DEFENDER: &str = r"SOFTWARE\Microsoft\Windows Defender";

    pub fn defender() -> Result<DefenderRaw, SourceError> {
        let Some(root) = Key::local_machine(DEFENDER) else {
            return Err(SourceError::Unavailable(
                "O Microsoft Defender não está instalado ou o registro é ilegível".into(),
            ));
        };
        let service: Option<RawService> = crate::windows_native::service("WinDefend").ok();
        let signatures = Key::local_machine(&format!(r"{DEFENDER}\Signature Updates"));
        let realtime = Key::local_machine(&format!(r"{DEFENDER}\Real-Time Protection"));
        let policy = Key::local_machine(r"SOFTWARE\Policies\Microsoft\Windows Defender");
        let atp = Key::local_machine(r"SOFTWARE\Microsoft\Windows Advanced Threat Protection");
        Ok(DefenderRaw {
            service,
            service_flag: root.number("IsServiceRunning").map(|v| v != 0),
            disable_antivirus: root.number("DisableAntiVirus").map(|v| v != 0),
            disable_antispyware: root.number("DisableAntiSpyware").map(|v| v != 0),
            policy_disabled: policy
                .as_ref()
                .and_then(|k| k.number("DisableAntiSpyware"))
                .map(|v| v != 0),
            realtime_disabled: realtime
                .as_ref()
                .and_then(|k| k.number("DisableRealtimeMonitoring"))
                .map(|v| v != 0),
            forced_passive: atp
                .as_ref()
                .and_then(|k| k.number("ForceDefenderPassiveMode"))
                .map(|v| v != 0),
            signature_version: signatures
                .as_ref()
                .and_then(|k| k.string("AVSignatureVersion")),
            engine_version: signatures.as_ref().and_then(|k| k.string("EngineVersion")),
            signatures_updated_at: signatures
                .as_ref()
                .and_then(|k| k.binary("SignaturesLastUpdated"))
                .and_then(|b| filetime_bytes_ms(&b)),
            third_party_av: Key::local_machine(r"SOFTWARE\Microsoft\Security Center\Provider\Av")
                .map(|k| {
                    k.subkeys()
                        .iter()
                        .filter(|n| !n.eq_ignore_ascii_case(DEFENDER_PROVIDER_GUID))
                        .count() as u32
                }),
            // Ameaças ativas só vêm da API do Defender (não usada: a leitura é passiva).
            active_threats: None,
        })
    }

    // ------------------------------------------------------------------ Secure Boot e TPM

    use windows_sys::Win32::System::{
        SystemInformation::GetFirmwareType, TpmBaseServices::Tbsi_GetDeviceInfo,
    };

    pub fn secure_boot() -> Result<SecureBootRaw, SourceError> {
        let mut firmware = 0i32;
        // SAFETY: saída de 4 bytes válida durante a chamada.
        let known = unsafe { GetFirmwareType(&mut firmware) } != 0;
        let uefi = known && firmware == 2;
        let bios = known && firmware == 1;
        let state = match Key::local_machine(r"SYSTEM\CurrentControlSet\Control\SecureBoot\State") {
            Some(key) => match key.number("UEFISecureBootEnabled") {
                Some(0) => SecureBootState::Disabled,
                Some(_) => SecureBootState::Enabled,
                None => SecureBootState::Unavailable,
            },
            // Sem a chave: firmware legado não suporta Secure Boot; em UEFI a ausência é indisponível.
            None if bios => SecureBootState::Unsupported,
            None => SecureBootState::Unavailable,
        };
        Ok(SecureBootRaw {
            state,
            uefi: known.then_some(uefi),
        })
    }

    /// Presença e versão do TPM pelos TPM Base Services (nenhum comando é enviado ao chip).
    pub fn tpm() -> Result<TpmRaw, SourceError> {
        const TBS_E_TPM_NOT_FOUND: u32 = 0x8028_400F;
        // SAFETY: estrutura de 16 bytes (TPM_DEVICE_INFO) com o tamanho informado.
        let mut info = [0u32; 4];
        let result = unsafe {
            Tbsi_GetDeviceInfo(
                std::mem::size_of_val(&info) as u32,
                info.as_mut_ptr().cast(),
            )
        };
        if result == TBS_E_TPM_NOT_FOUND {
            return Ok(TpmRaw {
                present: false,
                version: None,
            });
        }
        if result != 0 {
            return Err(SourceError::Unavailable(format!(
                "TPM Base Services indisponível (erro 0x{result:08X})"
            )));
        }
        let version = match info[1] {
            1 => Some("1.2".to_string()),
            2 => Some("2.0".to_string()),
            _ => None,
        };
        Ok(TpmRaw {
            present: true,
            version,
        })
    }

    // ------------------------------------------------------------------ BitLocker

    /// O estado do BitLocker por volume só é exposto por WMI/FVE com privilégio administrativo; nenhuma
    /// chave de recuperação é lida em hipótese alguma. Sem elevação o domínio é "requer elevação".
    pub fn bitlocker() -> Result<Vec<BitlockerVolume>, SourceError> {
        Err(SourceError::RequiresElevation(
            "O estado do BitLocker por volume requer privilégio administrativo".into(),
        ))
    }
}
