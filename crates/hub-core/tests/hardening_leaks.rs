//! Block 10 — vazamentos: ciclos repetidos de iniciar/parar execuções gerenciadas e de observar o
//! Control Plane não podem deixar processos órfãos nem fazer crescer os handles do processo.
mod common;
use common::{fixture, project, supervisor, wait_until};
use hub_core::{control_plane, system};
use std::{collections::HashMap, time::Duration};

const SERVICE: &str = "setInterval(() => process.stdout.write('tick\\n'), 400);\nprocess.stderr.write('pronto\\n');\n";

fn tools() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn handles() -> u32 {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let mut count = 0u32;
    // SAFETY: consulta só de leitura do próprio processo.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
    count
}
#[cfg(not(windows))]
fn handles() -> u32 {
    0
}

fn threads() -> usize {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system
        .process(sysinfo::Pid::from_u32(std::process::id()))
        .and_then(|p| p.tasks().map(|t| t.len()))
        .unwrap_or(0)
}

#[test]
fn repeated_start_stop_and_observation_leave_no_orphans_and_no_handle_growth() {
    if !tools() {
        eprintln!("node indisponível: cenário ignorado");
        return;
    }
    let (_tmp, dir) = fixture(
        "leaks",
        &[
            ("package.json", r#"{"scripts":{"svc":"node svc.js"}}"#),
            ("package-lock.json", "{}"),
            ("svc.js", SERVICE),
        ],
    );
    let p = project("lk", &dir);
    let ctx = hub_core::control_plane::Context {
        projects: vec![control_plane::ProjectRef {
            id: "lk".into(),
            name: "Leaks".into(),
            root: dir.to_string_lossy().into(),
            ports: vec![],
        }],
        worktrees: vec![],
    };
    let (sup, _events) = supervisor();
    // aquece (primeiras alocações de threads/handles não contam como crescimento)
    for _ in 0..2 {
        let run = sup.start(&p, "svc").unwrap();
        wait_until("rodando", || {
            sup.runs_for("lk")
                .iter()
                .any(|r| r.id == run.id && r.state.is_active())
        });
        sup.stop_and_wait(&run.id, Duration::from_secs(15)).unwrap();
    }
    // O inventário do sysinfo guarda um handle por processo vivo da máquina (platô do tamanho da lista
    // de processos, não por ciclo): aquece antes do baseline para medir só o crescimento por ciclo.
    let _ = control_plane::observe(&ctx, sup.managed_runs(), sup.all_runs()).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let before = (handles(), threads());

    let mut pids: Vec<u32> = vec![];
    for _ in 0..12 {
        let run = sup.start(&p, "svc").unwrap();
        wait_until("rodando", || {
            sup.runs_for("lk")
                .iter()
                .any(|r| r.id == run.id && r.state.is_active())
        });
        pids.extend(sup.managed_pids().keys().copied());
        // observar durante a execução (Control Plane + atribuição) a cada ciclo
        let snapshot = control_plane::observe(&ctx, sup.managed_runs(), sup.all_runs()).unwrap();
        assert!(snapshot.runtimes.iter().any(|r| r.execution.is_some()));
        let _ = control_plane::observe_processes(&ctx, &sup.managed_runs(), false).unwrap();
        sup.stop_and_wait(&run.id, Duration::from_secs(15)).unwrap();
        assert!(sup.managed_pids().is_empty());
    }
    pids.sort_unstable();
    pids.dedup();
    assert!(!pids.is_empty());
    wait_until("nenhum processo gerenciado restou", || {
        let alive: HashMap<u32, ()> = system::inventory()
            .into_iter()
            .map(|p| (p.pid, ()))
            .collect();
        pids.iter().all(|pid| !alive.contains_key(pid))
    });
    // nenhum processo com cwd no fixture ficou vivo
    let leftovers: Vec<_> = system::inventory()
        .into_iter()
        .filter(|p| {
            p.cwd
                .as_deref()
                .is_some_and(|c| c.to_lowercase().contains("leaks"))
        })
        .map(|p| (p.pid, p.name))
        .collect();
    assert!(leftovers.is_empty(), "órfãos: {leftovers:?}");

    std::thread::sleep(Duration::from_millis(500));
    let after = (handles(), threads());
    // Cada execução finalizada retida (até KEEP_FINISHED = 10) segura o handle do Job Object:
    // crescimento limitado, nunca por ciclo.
    assert!(
        after.0 <= before.0 + 40,
        "handles {} → {}",
        before.0,
        after.0
    );
    assert!(
        after.1 <= before.1 + 12,
        "threads {} → {}",
        before.1,
        after.1
    );
}
