//! Auxiliares dos testes de stacks (Rust, Tauri, Docker): fixtures em pastas temporárias.
#![allow(dead_code)]
use hub_core::{
    models::Project,
    supervisor::{RunState, RuntimeEvent, Supervisor},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub fn project(id: &str, dir: &Path) -> Project {
    Project {
        id: id.into(),
        name: id.into(),
        slug: id.into(),
        description: String::new(),
        local_path: dir.to_string_lossy().into(),
        locator: None,
        repository: String::new(),
        stack: vec![],
        tags: vec![],
        ports: vec![],
        commands: vec![],
        created_at: String::new(),
        updated_at: String::new(),
    }
}

pub fn write(dir: &Path, name: &str, text: &str) {
    if let Some(parent) = dir.join(name).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(dir.join(name), text).unwrap();
}

/// Pasta temporária com o nome pedido (o nome do projeto Compose deriva dela).
pub fn fixture(name: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, text) in files {
        write(&dir, file, text);
    }
    (tmp, dir)
}

pub type Events = Arc<Mutex<Vec<RuntimeEvent>>>;
pub fn supervisor() -> (Supervisor, Events) {
    let events: Events = Arc::default();
    let sink = events.clone();
    (
        Supervisor::new(Arc::new(move |e| sink.lock().unwrap().push(e))),
        events,
    )
}

pub fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    wait_for(what, Duration::from_secs(60), &mut ok)
}
pub fn wait_for(what: &str, limit: Duration, ok: &mut dyn FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("tempo esgotado esperando: {what}");
}

pub fn state_of(s: &Supervisor, project: &str, id: &str) -> RunState {
    s.runs_for(project)
        .into_iter()
        .find(|r| r.id == id)
        .unwrap()
        .state
}
pub fn text_of(s: &Supervisor, id: &str) -> String {
    s.logs(id, 0)
        .unwrap()
        .lines
        .iter()
        .map(|l| l.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn alive(pid: u32) -> bool {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system.process(sysinfo::Pid::from_u32(pid)).is_some()
}
pub fn have(tool: &str) -> bool {
    hub_core::commands::resolve_tool(tool).is_some()
}
