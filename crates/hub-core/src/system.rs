use crate::commands;
use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{Disks, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
#[derive(Clone, Serialize)]
pub struct Integration {
    pub name: String,
    pub status: String,
    pub detail: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemState {
    pub cpu: Option<f32>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub integrations: Vec<Integration>,
}
pub fn integrations() -> Vec<Integration> {
    static CACHE: OnceLock<Vec<Integration>> = OnceLock::new();
    CACHE.get_or_init(detect_integrations).clone()
}

fn detect_integrations() -> Vec<Integration> {
    let mut values = Vec::new();
    for (name, program) in [
        ("Git", "git"),
        ("GitHub", "gh"),
        ("Claude", "claude"),
        ("Docker", "docker"),
        ("Node.js", "node"),
        ("Rust", "rustc"),
        ("Python", "python"),
        ("MySQL", "mysql"),
    ] {
        let found = commands::executable(program).is_some();
        let (status, detail) = if !found {
            ("unavailable", "Executável não encontrado no PATH")
        } else if program == "gh" {
            if commands::run("gh", &["auth", "status"], None).is_ok() {
                ("connected", "GitHub CLI autenticada")
            } else {
                ("available", "CLI encontrada; autenticação não confirmada")
            }
        } else {
            (
                "available",
                "Executável encontrado; serviço/conta não verificados",
            )
        };
        values.push(Integration {
            name: name.into(),
            status: status.into(),
            detail: detail.into(),
        });
    }
    values.push(Integration {
        name: "Obsidian".into(),
        status: "not_configured".into(),
        detail: "Associação de vault prevista para v0.4".into(),
    });
    values
}
pub fn inspect() -> SystemState {
    static SAMPLER: OnceLock<Mutex<(System, Instant, Option<f32>)>> = OnceLock::new();
    let sampler = SAMPLER.get_or_init(|| {
        let mut system = System::new();
        system.refresh_cpu_all();
        Mutex::new((system, Instant::now(), None))
    });
    let cpu = {
        let mut guard = sampler.lock().unwrap_or_else(|error| error.into_inner());
        if guard.1.elapsed() >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
            guard.0.refresh_cpu_all();
            guard.1 = Instant::now();
            guard.2 = Some(guard.0.global_cpu_usage());
        }
        guard.2
    };
    let mut s = System::new();
    s.refresh_memory();
    let disks = Disks::new_with_refreshed_list();
    SystemState {
        cpu,
        memory_used: s.used_memory(),
        memory_total: s.total_memory(),
        disk_used: disks
            .iter()
            .map(|d| d.total_space() - d.available_space())
            .sum(),
        disk_total: disks.iter().map(|d| d.total_space()).sum(),
        integrations: integrations(),
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub executable: Option<String>,
    pub memory: u64,
    pub start_time: u64,
    pub project_id: Option<String>,
    pub confidence: String,
    /// Iniciado pelo LKR LAB ("Rodar"). Só estes podem ser parados sem confirmação extra.
    pub managed: bool,
}
#[derive(Clone)]
struct ProcessSample {
    info: ProcessInfo,
    cwd: Option<std::path::PathBuf>,
    parent: Option<u32>,
}
type ProcessCache = Mutex<Option<(Instant, Vec<ProcessSample>)>>;
fn snapshot_cache() -> &'static ProcessCache {
    static CACHE: OnceLock<ProcessCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}
/// Descarta a foto de processos (vale 500 ms): usado quando o próprio app inicia ou encerra
/// uma árvore, para a próxima leitura não mostrar processos que acabaram de sumir.
pub fn invalidate_process_cache() {
    *snapshot_cache().lock().unwrap_or_else(|e| e.into_inner()) = None;
}
/// O único inventário de processos do app. O vínculo processo → projeto (cwd/exe) e a
/// telemetria (CPU/memória/E-S) refrescam o mesmo `System`, cada um só com os campos de
/// que precisa; CPU e E-S por processo são deltas entre refreshes da telemetria.
fn process_system() -> &'static Mutex<System> {
    static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
    SYSTEM.get_or_init(|| Mutex::new(System::new()))
}
/// Consumo bruto de um processo desde o refresh de telemetria anterior.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessUsage {
    pub pid: u32,
    pub name: String,
    /// % de UM núcleo (sysinfo); a telemetria normaliza pelo total de threads.
    pub cpu: f32,
    pub memory: u64,
    pub read_bytes: u64,
    pub written_bytes: u64,
}
/// Refresh de telemetria: só nome, CPU, memória e E/S (sem cwd, exe ou linha de comando).
pub fn process_usage() -> Vec<ProcessUsage> {
    let mut system = process_system()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage(),
    );
    system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let io = process.disk_usage();
            ProcessUsage {
                pid: pid.as_u32(),
                name: process.name().to_string_lossy().into(),
                cpu: process.cpu_usage(),
                memory: process.memory(),
                read_bytes: io.read_bytes,
                written_bytes: io.written_bytes,
            }
        })
        .collect()
}
/// Um processo como o sistema operacional o mostra, sem interpretação (Control Plane).
/// Campos que o Windows não deixa ler sem elevação ficam ausentes: nunca são inventados.
#[derive(Debug, Clone)]
pub struct RawProcess {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub executable: Option<String>,
    pub cmd: Vec<String>,
    pub cwd: Option<String>,
    /// Segundos desde a época Unix.
    pub start_time: u64,
    /// % de UM núcleo desde o refresh anterior do mesmo `System` (melhor esforço).
    pub cpu: f32,
    pub memory: u64,
    pub read_bytes: u64,
    pub written_bytes: u64,
}

type InventoryCache = Mutex<Option<(Instant, Vec<RawProcess>)>>;
fn inventory_cache() -> &'static InventoryCache {
    static CACHE: OnceLock<InventoryCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Inventário completo para o Control Plane: UMA leitura (cache de 1 s) com pid, pai, exe, linha de
/// comando, cwd, CPU, memória e E/S. 100% leitura: não encerra, não altera e não abre processos
/// além do que o `sysinfo` já abre para ler.
pub fn inventory() -> Vec<RawProcess> {
    let mut cache = inventory_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some((updated, list)) = cache.as_ref() {
        if updated.elapsed() < Duration::from_secs(1) {
            return list.clone();
        }
    }
    let mut system = process_system()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::Always)
            .with_cmd(UpdateKind::Always),
    );
    let list: Vec<RawProcess> = system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let io = process.disk_usage();
            RawProcess {
                pid: pid.as_u32(),
                parent: process.parent().map(|p| p.as_u32()),
                name: process.name().to_string_lossy().into(),
                executable: process.exe().map(|p| p.to_string_lossy().into()),
                cmd: process
                    .cmd()
                    .iter()
                    .map(|a| a.to_string_lossy().into())
                    .collect(),
                cwd: process.cwd().map(|p| p.to_string_lossy().into()),
                start_time: process.start_time(),
                cpu: process.cpu_usage(),
                memory: process.memory(),
                read_bytes: io.read_bytes,
                written_bytes: io.written_bytes,
            }
        })
        .collect();
    *cache = Some((Instant::now(), list.clone()));
    list
}

fn process_snapshot() -> Vec<ProcessSample> {
    let mut cache = snapshot_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some((updated, samples)) = cache.as_ref() {
        if updated.elapsed() < Duration::from_millis(500) {
            return samples.clone();
        }
    }
    let mut system = process_system()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_memory()
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );
    let mut directories = std::collections::HashMap::new();
    let samples: Vec<_> = system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessSample {
            cwd: process.cwd().and_then(|cwd| {
                directories
                    .entry(cwd.to_path_buf())
                    .or_insert_with(|| cwd.canonicalize().ok())
                    .clone()
            }),
            parent: process.parent().map(|p| p.as_u32()),
            info: ProcessInfo {
                pid: pid.as_u32(),
                name: process.name().to_string_lossy().into(),
                executable: process.exe().map(|path| path.to_string_lossy().into()),
                memory: process.memory(),
                start_time: process.start_time(),
                project_id: None,
                confidence: "unknown".into(),
                managed: false,
            },
        })
        .collect();
    *cache = Some((Instant::now(), samples.clone()));
    samples
}
/// Projeto dono de um cwd: a pasta vinculada mais específica que o contém.
/// Projeto sem vínculo nesta máquina ("" casaria com tudo) nunca é dono de nada.
pub fn owner_of<'a>(
    cwd: &std::path::Path,
    projects: &'a [crate::models::Project],
) -> Option<&'a crate::models::Project> {
    // Compara sem o prefixo verbatim do Windows: a pasta vinculada pode ter sido gravada
    // de qualquer das duas formas e o cwd do processo vem canonicalizado.
    let cwd = crate::runtime::plain_path(cwd.to_path_buf());
    projects
        .iter()
        .filter(|project| {
            !project.local_path.is_empty()
                && cwd.starts_with(crate::runtime::plain_path(std::path::PathBuf::from(
                    &project.local_path,
                )))
        })
        .max_by_key(|project| project.local_path.len())
}
pub fn processes(projects: &[crate::models::Project]) -> Vec<ProcessInfo> {
    processes_managed(projects, &std::collections::HashMap::new())
}
/// Dono de cada processo, só com evidência:
///  1. managed — o PID está numa árvore iniciada pelo LKR LAB (`managed`: PID → projeto);
///  2. cwd — a pasta de trabalho está dentro da pasta vinculada de um projeto;
///  3. descendant — filho de um processo gerenciado que ficou fora do grupo.
///
/// Projeto sem pasta nesta máquina nunca é dono de nada.
pub fn processes_managed(
    projects: &[crate::models::Project],
    managed: &std::collections::HashMap<u32, String>,
) -> Vec<ProcessInfo> {
    processes_managed_with(projects, &|| managed.clone())
}
/// Igual a `processes_managed`, mas pede os PIDs gerenciados DEPOIS de fotografar os
/// processos: tudo que está na foto e ainda vive já está no grupo, então um processo recém-nascido
/// da árvore gerenciada nunca é classificado como externo por uma corrida entre as duas leituras.
pub fn processes_managed_with(
    projects: &[crate::models::Project],
    managed: &dyn Fn() -> std::collections::HashMap<u32, String>,
) -> Vec<ProcessInfo> {
    let samples = process_snapshot();
    let managed = managed();
    let managed = &managed;
    let parents: std::collections::HashMap<u32, Option<u32>> =
        samples.iter().map(|s| (s.info.pid, s.parent)).collect();
    let bound = |id: &String| {
        projects
            .iter()
            .any(|p| &p.id == id && !p.local_path.is_empty())
    };
    let mut result: Vec<_> = samples
        .into_iter()
        .map(|mut sample| {
            let pid = sample.info.pid;
            if let Some(id) = managed.get(&pid).filter(|id| bound(id)) {
                sample.info.project_id = Some(id.clone());
                sample.info.confidence = "managed".into();
                sample.info.managed = true;
                return sample.info;
            }
            if let Some(project) = sample
                .cwd
                .as_deref()
                .and_then(|cwd| owner_of(cwd, projects))
            {
                sample.info.project_id = Some(project.id.clone());
                sample.info.confidence = "cwd".into();
                return sample.info;
            }
            let mut ancestor = sample.parent;
            for _ in 0..8 {
                let Some(parent) = ancestor else { break };
                if let Some(id) = managed.get(&parent).filter(|id| bound(id)) {
                    sample.info.project_id = Some(id.clone());
                    sample.info.confidence = "descendant".into();
                    sample.info.managed = true;
                    break;
                }
                ancestor = parents.get(&parent).copied().flatten();
            }
            sample.info
        })
        .collect();
    result.sort_by_key(|p| std::cmp::Reverse(p.memory));
    result
}
