use crate::{models::Project, system, HubResult};
use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use serde::Serialize;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    pub port: u16,
    pub address: String,
    pub protocol: String,
    pub pid: Option<u32>,
    pub process: String,
    pub executable: Option<String>,
    pub start_time: Option<u64>,
    pub project_id: Option<String>,
    pub expected_by: Vec<String>,
    pub confidence: String,
    pub conflict: bool,
}
pub fn inspect(projects: &[Project]) -> HubResult<Vec<PortInfo>> {
    let processes = system::processes(projects);
    inspect_with_processes(projects, &processes)
}

pub fn inspect_with_processes(
    projects: &[Project],
    processes: &[system::ProcessInfo],
) -> HubResult<Vec<PortInfo>> {
    let sockets = get_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP | ProtocolFlags::UDP,
    )
    .map_err(|e| format!("Não foi possível enumerar portas: {e}"))?;
    let mut result = Vec::new();
    let by_pid: std::collections::HashMap<_, _> = processes
        .iter()
        .map(|process| (process.pid, process))
        .collect();
    for socket in sockets {
        let (port, address, protocol) = match socket.protocol_socket_info {
            ProtocolSocketInfo::Tcp(t) if t.state == TcpState::Listen => {
                (t.local_port, t.local_addr.to_string(), "TCP")
            }
            ProtocolSocketInfo::Udp(u) => (u.local_port, u.local_addr.to_string(), "UDP"),
            _ => continue,
        };
        // Multiple owners are represented separately; missing ownership stays unknown.
        let pids: Vec<Option<u32>> = if socket.associated_pids.is_empty() {
            vec![None]
        } else {
            socket.associated_pids.into_iter().map(Some).collect()
        };
        for pid in pids {
            let process = pid.and_then(|id| by_pid.get(&id).copied());
            let expected: Vec<String> = projects
                .iter()
                .filter(|p| p.ports.iter().any(|p| p.port == port))
                .map(|p| p.id.clone())
                .collect();
            let owner = process.and_then(|p| p.project_id.clone());
            let conflict = expected.len() > 1
                || owner
                    .as_ref()
                    .is_some_and(|id| !expected.is_empty() && !expected.contains(id));
            result.push(PortInfo {
                port,
                address: address.clone(),
                protocol: protocol.into(),
                pid,
                process: process
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "Desconhecido".into()),
                executable: process.and_then(|p| p.executable.clone()),
                start_time: process.map(|p| p.start_time),
                project_id: owner,
                expected_by: expected,
                confidence: process
                    .map(|p| p.confidence.clone())
                    .unwrap_or_else(|| "unknown".into()),
                conflict,
            });
        }
    }
    result.sort_by_key(|p| p.port);
    Ok(result)
}
pub fn validate_kill(
    pid: u32,
    expected_start: u64,
    actual_start: u64,
    confirmed: bool,
) -> HubResult<()> {
    if !confirmed {
        return Err("Confirmação explícita necessária.".into());
    }
    if pid <= 4 || pid == std::process::id() {
        return Err("Processo protegido.".into());
    }
    if expected_start == 0 || expected_start != actual_start {
        return Err("O processo mudou desde a consulta. Atualize a lista.".into());
    }
    Ok(())
}
pub fn kill(pid: u32, start_time: u64, confirmed: bool) -> HubResult<()> {
    let system = sysinfo::System::new_all();
    let p = system
        .process(sysinfo::Pid::from_u32(pid))
        .ok_or("Processo não existe mais.")?;
    validate_kill(pid, start_time, p.start_time(), confirmed)?;
    let name = p.name().to_string_lossy().to_lowercase();
    if [
        "system",
        "registry",
        "smss.exe",
        "csrss.exe",
        "wininit.exe",
        "services.exe",
        "lsass.exe",
        "winlogon.exe",
        "svchost.exe",
    ]
    .contains(&name.as_str())
    {
        return Err("Processo crítico protegido.".into());
    }
    if !p.kill() {
        return Err("Sistema operacional recusou o encerramento.".into());
    }
    Ok(())
}
