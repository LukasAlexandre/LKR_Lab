//! Machine Registry: "qual computador está executando esta instância?"
//! (docs/concepts/machine-registry/CONCEPT-01.md).
//!
//! A identidade é o `machine_id`, um UUID gerado pelo LKR LAB no cadastro e
//! persistido no SQLite local. Hostname, IP, interfaces e hardware são atributos
//! detectados: mudam a cada snapshot e nunca criam outra máquina.
//!
//! Tudo aqui é estado desta máquina (classe C de docs/STATE.md): nada vai para o
//! workspace portátil.
//!
//! A detecção é passiva: lê o que o sistema já expõe (sysinfo, registro do Windows,
//! tabela de rotas). Não executa ferramentas, não lê variáveis de ambiente nem arquivos
//! do usuário, e não guarda MAC, número de série ou chave de produto.
use crate::{database::Database, HubResult};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// Validade do snapshot: com 6 horas ou mais, a próxima consulta detecta de novo.
pub const SNAPSHOT_TTL_MS: i64 = 6 * 60 * 60 * 1000;
pub const NAME_MAX: usize = 60;
pub const DESCRIPTION_MAX: usize = 120;
/// Uso / Local. A interface mostra Casa, Trabalho e Outro.
pub const USAGES: [&str; 3] = ["home", "work", "other"];

/// Únicos comandos aceitos antes do cadastro: detectar, revisar e cadastrar.
pub const SETUP_COMMANDS: [&str; 3] = ["machine_status", "machine_refresh", "machine_register"];
/// Resposta de qualquer outro comando enquanto o computador não está cadastrado.
pub const NOT_REGISTERED: &str =
    "MACHINE_NOT_REGISTERED: Cadastre este computador para liberar o LKR LAB.";

/// O gate global. Vale para toda chamada da interface ao backend, não só para a navegação.
pub fn command_allowed(command: &str, registered: bool) -> bool {
    registered || SETUP_COMMANDS.contains(&command)
}

/// Snapshot expirado: nunca detectado, com 6h ou mais, ou relógio que voltou no tempo.
pub fn is_stale(last_detected_at: Option<i64>, now: i64) -> bool {
    match last_detected_at {
        None => true,
        Some(at) => now < at || now - at >= SNAPSHOT_TTL_MS,
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GpuInfo {
    pub name: String,
    /// Memória de vídeo DEDICADA em bytes, quando o driver informa (nunca a compartilhada).
    pub memory: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StorageInfo {
    pub mount: String,
    /// "ssd", "hdd" ou "unknown".
    pub kind: String,
    pub total: u64,
    pub removable: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkInterface {
    pub name: String,
    pub ipv4: Vec<String>,
}

/// Atributos detectados. Todo campo é opcional: o que o sistema não informa fica
/// ausente ("indisponível"), e isso nunca impede o cadastro.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MachineSnapshot {
    pub hostname: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub os_build: Option<String>,
    pub cpu_model: Option<String>,
    pub cpu_cores: Option<u32>,
    pub cpu_threads: Option<u32>,
    /// RAM instalada, em bytes.
    pub memory_total: Option<u64>,
    pub gpus: Vec<GpuInfo>,
    pub storage: Vec<StorageInfo>,
    pub network_interfaces: Vec<NetworkInterface>,
    pub active_interface: Option<String>,
    pub local_ipv4: Option<String>,
    /// Segundos desde o boot.
    pub uptime: Option<u64>,
    /// ms desde a época Unix.
    pub detected_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub machine_id: String,
    pub name: String,
    pub usage: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_detected_at: Option<i64>,
}

/// O que o usuário controla no cadastro. Todo o resto é detectado.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MachineInput {
    pub name: String,
    pub usage: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineStatus {
    pub registered: bool,
    pub machine: Option<Machine>,
    /// Antes do cadastro: a última detecção em memória (ainda não persistida).
    pub snapshot: Option<MachineSnapshot>,
    pub stale: bool,
    pub ttl_ms: i64,
}

pub fn validate(input: MachineInput) -> HubResult<MachineInput> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("Informe um nome para este computador.".into());
    }
    if name.chars().count() > NAME_MAX || name.chars().any(char::is_control) {
        return Err(format!(
            "O nome do computador aceita até {NAME_MAX} caracteres, sem quebras de linha."
        ));
    }
    if !USAGES.contains(&input.usage.as_str()) {
        return Err("Escolha o uso / local deste computador.".into());
    }
    let description = input.description.trim().to_string();
    if description.chars().count() > DESCRIPTION_MAX {
        return Err(format!(
            "A descrição aceita até {DESCRIPTION_MAX} caracteres."
        ));
    }
    Ok(MachineInput {
        name,
        usage: input.usage,
        description,
    })
}

/// Fonte da detecção. O backend usa `SystemDetector`; os testes usam fontes falsas.
pub trait Detector {
    fn detect(&self, now: i64) -> MachineSnapshot;
}

pub struct SystemDetector;

impl Detector for SystemDetector {
    fn detect(&self, now: i64) -> MachineSnapshot {
        detect_system(now)
    }
}

/// Máquina persistida e seu último snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct MachineRecord {
    pub machine: Machine,
    pub snapshot: Option<MachineSnapshot>,
}

impl Database {
    pub fn machine(&self) -> HubResult<Option<MachineRecord>> {
        self.conn
            .query_row(
                "SELECT machine_id,name,usage,description,created_at,updated_at,last_detected_at,snapshot FROM machine WHERE id=1",
                [],
                |r| {
                    let snapshot: Option<String> = r.get(7)?;
                    Ok(MachineRecord {
                        machine: Machine {
                            machine_id: r.get(0)?,
                            name: r.get(1)?,
                            usage: r.get(2)?,
                            description: r.get(3)?,
                            created_at: r.get(4)?,
                            updated_at: r.get(5)?,
                            last_detected_at: r.get(6)?,
                        },
                        // Snapshot ilegível vale como ausente: a próxima detecção o substitui.
                        snapshot: snapshot.and_then(|text| serde_json::from_str(&text).ok()),
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    /// Cria a identidade desta máquina. Só existe uma; um segundo cadastro é recusado.
    pub fn register_machine(
        &self,
        input: &MachineInput,
        snapshot: &MachineSnapshot,
    ) -> HubResult<Machine> {
        if self.machine()?.is_some() {
            return Err("Este computador já está cadastrado.".into());
        }
        let machine_id = uuid::Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO machine(id,machine_id,name,usage,description,snapshot,last_detected_at) VALUES(1,?1,?2,?3,?4,?5,?6)",
                params![
                    machine_id,
                    input.name,
                    input.usage,
                    input.description,
                    serde_json::to_string(snapshot).map_err(|e| e.to_string())?,
                    snapshot.detected_at
                ],
            )
            .map_err(|_| "Este computador já está cadastrado.".to_string())?;
        self.conn
            .execute(
                "INSERT INTO activities(action) VALUES('Computador cadastrado no LKR LAB')",
                [],
            )
            .map_err(|e| e.to_string())?;
        self.machine()?
            .map(|record| record.machine)
            .ok_or_else(|| "Cadastro do computador não encontrado.".into())
    }

    /// Substitui o snapshot. A identidade (machine_id) e os dados do usuário não mudam.
    pub fn store_machine_snapshot(&self, snapshot: &MachineSnapshot) -> HubResult<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE machine SET snapshot=?1,last_detected_at=?2 WHERE id=1",
                params![
                    serde_json::to_string(snapshot).map_err(|e| e.to_string())?,
                    snapshot.detected_at
                ],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("Computador ainda não cadastrado.".into());
        }
        Ok(())
    }

    /// Edita só a metadata do usuário. machine_id, created_at, snapshot e
    /// last_detected_at ficam como estão; `input` já deve estar validado.
    pub fn update_machine_metadata(&self, input: &MachineInput) -> HubResult<Machine> {
        let changed = self
            .conn
            .execute(
                "UPDATE machine SET name=?1,usage=?2,description=?3,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=1",
                params![input.name, input.usage, input.description],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("Computador ainda não cadastrado.".into());
        }
        self.conn
            .execute(
                "INSERT INTO activities(action) VALUES('Cadastro do computador atualizado')",
                [],
            )
            .map_err(|e| e.to_string())?;
        self.machine()?
            .map(|record| record.machine)
            .ok_or_else(|| "Cadastro do computador não encontrado.".into())
    }
}

/// Estado do registro em execução: o gate (cadastrado ou não) e, antes do cadastro,
/// a detecção pendente, que só é persistida quando o usuário confirma.
pub struct Registry {
    registered: AtomicBool,
    pending: Mutex<Option<MachineSnapshot>>,
}

fn locked(db: &Mutex<Database>) -> HubResult<std::sync::MutexGuard<'_, Database>> {
    db.lock()
        .map_err(|_| "Banco temporariamente indisponível".into())
}

impl Registry {
    /// Reconhece a máquina pelo machine_id persistido: se existe, o ambiente já nasce liberado.
    pub fn load(db: &Database) -> HubResult<Self> {
        Ok(Self {
            registered: AtomicBool::new(db.machine()?.is_some()),
            pending: Mutex::new(None),
        })
    }

    pub fn is_registered(&self) -> bool {
        self.registered.load(Ordering::SeqCst)
    }

    pub fn allows(&self, command: &str) -> bool {
        command_allowed(command, self.is_registered())
    }

    fn pending(&self) -> Option<MachineSnapshot> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Leitura sem detecção.
    pub fn status(&self, db: &Mutex<Database>, now: i64) -> HubResult<MachineStatus> {
        let stored = locked(db)?.machine()?;
        Ok(match stored {
            Some(record) => MachineStatus {
                registered: true,
                stale: is_stale(record.machine.last_detected_at, now),
                machine: Some(record.machine),
                snapshot: record.snapshot,
                ttl_ms: SNAPSHOT_TTL_MS,
            },
            None => {
                let pending = self.pending();
                MachineStatus {
                    registered: false,
                    stale: is_stale(pending.as_ref().map(|s| s.detected_at), now),
                    machine: None,
                    snapshot: pending,
                    ttl_ms: SNAPSHOT_TTL_MS,
                }
            }
        })
    }

    /// Detecta de novo só se o snapshot expirou ou se `force` ("Atualizar detecção").
    /// A detecção roda fora do lock do banco.
    pub fn refresh(
        &self,
        db: &Mutex<Database>,
        detector: &dyn Detector,
        now: i64,
        force: bool,
    ) -> HubResult<MachineStatus> {
        let current = self.status(db, now)?;
        if !force && !current.stale {
            return Ok(current);
        }
        let snapshot = detector.detect(now);
        if current.registered {
            locked(db)?.store_machine_snapshot(&snapshot)?;
        } else {
            *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot);
        }
        self.status(db, now)
    }

    /// "Cadastrar computador": gera o machine_id, persiste o cadastro com o snapshot
    /// revisado (ou um novo, se o pendente expirou) e libera o ambiente.
    pub fn register(
        &self,
        db: &Mutex<Database>,
        detector: &dyn Detector,
        input: MachineInput,
        now: i64,
    ) -> HubResult<MachineStatus> {
        if self.is_registered() {
            return Err("Este computador já está cadastrado.".into());
        }
        let input = validate(input)?;
        let snapshot = self
            .pending()
            .filter(|s| !is_stale(Some(s.detected_at), now))
            .unwrap_or_else(|| detector.detect(now));
        locked(db)?.register_machine(&input, &snapshot)?;
        self.registered.store(true, Ordering::SeqCst);
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.status(db, now)
    }

    /// "Salvar alterações" em Este computador: mesmas regras do cadastro, sem detectar.
    pub fn update(
        &self,
        db: &Mutex<Database>,
        input: MachineInput,
        now: i64,
    ) -> HubResult<MachineStatus> {
        if !self.is_registered() {
            return Err("Computador ainda não cadastrado.".into());
        }
        let input = validate(input)?;
        locked(db)?.update_machine_metadata(&input)?;
        self.status(db, now)
    }
}

// ---- detecção ----

fn text(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Interfaces com IPv4 (sem loopback), em ordem de nome. Usado pelo inventário e pela telemetria.
pub(crate) fn network_interfaces(networks: &sysinfo::Networks) -> Vec<NetworkInterface> {
    let mut interfaces: Vec<NetworkInterface> = networks
        .iter()
        .filter_map(|(name, data)| {
            let ipv4: Vec<String> = data
                .ip_networks()
                .iter()
                .filter_map(|net| match net.addr {
                    IpAddr::V4(ip) if !ip.is_loopback() => Some(ip.to_string()),
                    _ => None,
                })
                .collect();
            (!ipv4.is_empty()).then(|| NetworkInterface {
                name: name.clone(),
                ipv4,
            })
        })
        .collect();
    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    interfaces
}

/// (IPv4 de saída, interface que o possui), pela tabela de rotas.
pub(crate) fn active_route(interfaces: &[NetworkInterface]) -> (Option<String>, Option<String>) {
    let ip = route_ipv4().map(|ip| ip.to_string());
    let name = ip.as_ref().and_then(|ip| {
        interfaces
            .iter()
            .find(|i| i.ipv4.contains(ip))
            .map(|i| i.name.clone())
    });
    (ip, name)
}

pub(crate) fn disk_kind(disk: &sysinfo::Disk) -> &'static str {
    match disk.kind() {
        sysinfo::DiskKind::SSD => "ssd",
        sysinfo::DiskKind::HDD => "hdd",
        _ => "unknown",
    }
}

/// Coleta passiva. Cada atributo é independente: um que falha fica ausente.
pub fn detect_system(now: i64) -> MachineSnapshot {
    use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};
    let system = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    let cpus = system.cpus();
    let os = os_details();
    let network_interfaces = network_interfaces(&Networks::new_with_refreshed_list());
    let (local_ipv4, active_interface) = active_route(&network_interfaces);
    let mut storage: Vec<StorageInfo> = Disks::new_with_refreshed_list()
        .iter()
        .filter(|d| d.total_space() > 0)
        .map(|d| StorageInfo {
            mount: d.mount_point().to_string_lossy().into(),
            kind: disk_kind(d).into(),
            total: d.total_space(),
            removable: d.is_removable(),
        })
        .collect();
    storage.sort_by(|a, b| a.mount.cmp(&b.mount));
    MachineSnapshot {
        hostname: text(System::host_name()),
        os_name: text(System::long_os_version()).or_else(|| text(System::name())),
        os_version: os.0,
        os_build: os.1,
        cpu_model: text(cpus.first().map(|c| c.brand().to_string())),
        cpu_cores: System::physical_core_count().map(|n| n as u32),
        cpu_threads: (!cpus.is_empty()).then_some(cpus.len() as u32),
        memory_total: Some(system.total_memory()).filter(|m| *m > 0),
        // Adaptadores presentes agora, com a memória DEDICADA (a compartilhada não entra).
        gpus: crate::sensors::gpu_adapters()
            .into_iter()
            .map(|a| GpuInfo {
                name: a.name,
                memory: a.dedicated,
            })
            .collect(),
        storage,
        network_interfaces,
        active_interface,
        local_ipv4,
        uptime: Some(System::uptime()).filter(|u| *u > 0),
        detected_at: now,
    }
}

/// IPv4 que o sistema usaria para sair da rede local. `connect` em UDP só consulta a
/// tabela de rotas: nenhum pacote é enviado. 192.0.2.1 é endereço de documentação
/// (RFC 5737), nunca um servidor real.
fn route_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// (versão, build). No Windows: DisplayVersion (ex.: 24H2) e CurrentBuild.UBR.
#[cfg(windows)]
fn os_details() -> (Option<String>, Option<String>) {
    let Some(key) = crate::sensors::registry::Key::local_machine(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
    ) else {
        return (
            text(sysinfo::System::os_version()),
            text(sysinfo::System::kernel_version()),
        );
    };
    let version =
        text(key.string("DisplayVersion")).or_else(|| text(sysinfo::System::os_version()));
    let build = text(key.string("CurrentBuild")).map(|build| match key.number("UBR") {
        Some(ubr) => format!("{build}.{ubr}"),
        None => build,
    });
    (
        version,
        build.or_else(|| text(sysinfo::System::kernel_version())),
    )
}

#[cfg(not(windows))]
fn os_details() -> (Option<String>, Option<String>) {
    (
        text(sysinfo::System::os_version()),
        text(sysinfo::System::kernel_version()),
    )
}
