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
}
#[derive(Clone)]
struct ProcessSample {
    info: ProcessInfo,
    cwd: Option<std::path::PathBuf>,
}
type ProcessCache = Mutex<Option<(Instant, Vec<ProcessSample>)>>;
fn process_snapshot() -> Vec<ProcessSample> {
    static CACHE: OnceLock<ProcessCache> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some((updated, samples)) = cache.as_ref() {
        if updated.elapsed() < Duration::from_millis(500) {
            return samples.clone();
        }
    }
    let mut system = System::new();
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
            info: ProcessInfo {
                pid: pid.as_u32(),
                name: process.name().to_string_lossy().into(),
                executable: process.exe().map(|path| path.to_string_lossy().into()),
                memory: process.memory(),
                start_time: process.start_time(),
                project_id: None,
                confidence: "unknown".into(),
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
    projects
        .iter()
        .filter(|project| {
            !project.local_path.is_empty()
                && cwd.starts_with(std::path::Path::new(&project.local_path))
        })
        .max_by_key(|project| project.local_path.len())
}
pub fn processes(projects: &[crate::models::Project]) -> Vec<ProcessInfo> {
    let mut result: Vec<_> = process_snapshot()
        .into_iter()
        .map(|mut sample| {
            let project = sample
                .cwd
                .as_deref()
                .and_then(|cwd| owner_of(cwd, projects));
            sample.info.project_id = project.map(|p| p.id.clone());
            sample.info.confidence = if project.is_some() { "cwd" } else { "unknown" }.into();
            sample.info
        })
        .collect();
    result.sort_by_key(|p| std::cmp::Reverse(p.memory));
    result
}
