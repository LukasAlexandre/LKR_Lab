//! Control Plane (SESSION-002): inventário, portas, atribuição com confiança, runtimes gerenciados
//! e descobertos, console e passividade. Os testes de processo usam subprocessos temporários
//! controlados e NUNCA encerram nada que não tenham iniciado.
mod common;
use common::{alive, fixture, have, supervisor, text_of, wait_until};
use hub_core::{
    control_plane::{
        self, attribute, build_snapshot, process_inventory, Confidence, Context, Inputs, Origin,
        PortObservation, ProjectRef, WorktreeRef,
    },
    supervisor::RunState,
    system::{self, RawProcess},
};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
};

// ---------------------------------------------------------------- dados sintéticos

fn raw(pid: u32, parent: Option<u32>, name: &str) -> RawProcess {
    RawProcess {
        pid,
        parent,
        name: name.into(),
        executable: None,
        cmd: vec![],
        cwd: None,
        start_time: 1_000 + pid as u64,
        cpu: 0.0,
        memory: 1_000,
        read_bytes: 0,
        written_bytes: 0,
    }
}
fn with_cwd(mut p: RawProcess, cwd: &str) -> RawProcess {
    p.cwd = Some(cwd.into());
    p
}
fn port(port: u16, pid: Option<u32>) -> PortObservation {
    PortObservation {
        port,
        address: "127.0.0.1".into(),
        protocol: "TCP",
        ip_version: "v4",
        pid,
    }
}
fn project(id: &str, root: &str, ports: &[u16]) -> ProjectRef {
    ProjectRef {
        id: id.into(),
        name: format!("Projeto {id}"),
        root: root.into(),
        ports: ports.to_vec(),
    }
}
fn worktree(id: &str, project: &str, root: &str, session: Option<&str>) -> WorktreeRef {
    WorktreeRef {
        id: id.into(),
        project_id: project.into(),
        name: format!("wt-{id}"),
        root: root.into(),
        session_id: session.map(|s| format!("sid-{s}")),
        session_label: session.map(str::to_string),
        block_id: session.map(|_| "blk".into()),
        block_title: session.map(|_| "Bloco".into()),
    }
}
fn ctx() -> Context {
    Context {
        projects: vec![
            project("lab", "C:\\Dev\\Lab", &[1420]),
            project("other", "C:\\Dev\\Other", &[1420, 3000]),
        ],
        worktrees: vec![worktree(
            "w1",
            "lab",
            "C:\\Dev\\Lab-wt\\feature",
            Some("SESSION-002"),
        )],
    }
}

// ---------------------------------------------------------------- atribuição (pura)

#[test]
fn managed_group_is_exact_and_carries_the_project() {
    let procs = vec![raw(10, None, "npm.exe"), raw(11, Some(10), "node.exe")];
    let managed = HashMap::from([
        (10, ("lab".to_string(), "run-1".to_string())),
        (11, ("lab".to_string(), "run-1".to_string())),
    ]);
    let a = attribute(&procs, &[], &ctx(), &managed);
    assert_eq!(a[&11].confidence, Confidence::Exact);
    assert_eq!(a[&11].project_id.as_deref(), Some("lab"));
    assert_eq!(a[&11].evidence[0].kind, "managed_execution");
    assert!(
        a[&11].worktree_id.is_none(),
        "sem cwd em worktree, sem worktree"
    );
}

#[test]
fn cwd_inside_a_worktree_attributes_worktree_and_session() {
    let procs = vec![with_cwd(
        raw(20, None, "node.exe"),
        "C:\\Dev\\Lab-wt\\feature\\src",
    )];
    let a = attribute(&procs, &[], &ctx(), &HashMap::new());
    let a = &a[&20];
    assert_eq!(a.confidence, Confidence::High);
    assert_eq!(
        (a.project_id.as_deref(), a.worktree_id.as_deref()),
        (Some("lab"), Some("w1"))
    );
    assert_eq!(a.session_label.as_deref(), Some("SESSION-002"));
    assert_eq!(a.block_title.as_deref(), Some("Bloco"));
    assert_eq!(a.evidence[0].kind, "cwd_in_worktree");
}

#[test]
fn worktree_inside_the_project_folder_wins_over_the_project() {
    let c = Context {
        projects: vec![project("lab", "C:\\Dev\\Lab", &[])],
        worktrees: vec![worktree(
            "w1",
            "lab",
            "C:\\Dev\\Lab\\.wt\\feature",
            Some("SESSION-002"),
        )],
    };
    let procs = vec![with_cwd(raw(1, None, "node.exe"), "c:/dev/lab/.wt/feature")];
    assert_eq!(
        attribute(&procs, &[], &c, &HashMap::new())[&1]
            .worktree_id
            .as_deref(),
        Some("w1")
    );
    let procs = vec![with_cwd(raw(2, None, "node.exe"), "C:\\Dev\\Lab\\src")];
    let a = &attribute(&procs, &[], &c, &HashMap::new())[&2];
    assert_eq!(
        (
            a.project_id.as_deref(),
            a.worktree_id.as_deref(),
            a.session_id.as_deref()
        ),
        (Some("lab"), None, None)
    );
}

#[test]
fn cwd_in_project_executable_in_project_and_prefix_lookalikes() {
    let mut exe = raw(3, None, "app.exe");
    exe.executable = Some("C:\\Dev\\Lab\\target\\debug\\app.exe".into());
    let procs = vec![
        with_cwd(raw(1, None, "node.exe"), "C:\\Dev\\Lab\\web"),
        exe,
        with_cwd(raw(4, None, "node.exe"), "C:\\Dev\\Lab-old\\web"),
    ];
    let a = attribute(&procs, &[], &ctx(), &HashMap::new());
    assert_eq!(
        (a[&1].confidence, a[&1].evidence[0].kind),
        (Confidence::High, "cwd_in_project")
    );
    assert_eq!(
        (a[&3].confidence, a[&3].evidence[0].kind),
        (Confidence::High, "executable_in_project")
    );
    assert_eq!(
        a[&4].confidence,
        Confidence::Unknown,
        "C:\\Dev\\Lab-old NÃO está dentro de C:\\Dev\\Lab"
    );
}

#[test]
fn bare_node_exe_stays_unknown_even_with_an_active_project() {
    let procs = vec![raw(30, None, "node.exe")];
    let a = &attribute(&procs, &[], &ctx(), &HashMap::new())[&30];
    assert_eq!(a.confidence, Confidence::Unknown);
    assert!(
        a.project_id.is_none()
            && a.worktree_id.is_none()
            && a.session_id.is_none()
            && a.evidence.is_empty()
    );
}

#[test]
fn command_line_path_and_declared_port_are_only_medium() {
    let mut p = raw(40, None, "node.exe");
    p.cmd = vec!["node".into(), "C:\\Dev\\Other\\server.js".into()];
    let a = attribute(&[p], &[], &ctx(), &HashMap::new());
    assert_eq!(
        (a[&40].confidence, a[&40].project_id.as_deref()),
        (Confidence::Medium, Some("other"))
    );
    // 3000 só é declarada por "other"; 1420 é declarada por DUAS e não prova nada.
    let procs = vec![raw(41, None, "x.exe"), raw(42, None, "y.exe")];
    let a = attribute(
        &procs,
        &[port(3000, Some(41)), port(1420, Some(42))],
        &ctx(),
        &HashMap::new(),
    );
    assert_eq!(
        (a[&41].confidence, a[&41].evidence[0].kind),
        (Confidence::Medium, "declared_port")
    );
    assert_eq!(
        a[&42].confidence,
        Confidence::Unknown,
        "porta declarada por dois Projects não atribui"
    );
}

#[test]
fn descendants_inherit_but_a_reused_pid_parent_does_not() {
    let parent = with_cwd(raw(50, None, "node.exe"), "C:\\Dev\\Lab");
    let child = raw(51, Some(50), "esbuild.exe");
    let a = attribute(
        &[parent.clone(), child.clone()],
        &[],
        &ctx(),
        &HashMap::new(),
    );
    assert_eq!(
        (a[&51].confidence, a[&51].evidence[0].kind),
        (Confidence::High, "ancestor")
    );
    // O "pai" nasceu DEPOIS do filho: o PID foi reutilizado e não é o verdadeiro pai.
    let mut young = parent;
    young.start_time = child.start_time + 100;
    let a = attribute(&[young, child], &[], &ctx(), &HashMap::new());
    assert_eq!(a[&51].confidence, Confidence::Unknown);
}

#[test]
fn project_without_a_local_folder_never_owns_anything() {
    let c = Context {
        projects: vec![project("ghost", "", &[])],
        worktrees: vec![],
    };
    let procs = vec![with_cwd(raw(1, None, "node.exe"), "C:\\qualquer")];
    assert_eq!(
        attribute(&procs, &[], &c, &HashMap::new())[&1].confidence,
        Confidence::Unknown
    );
}

// ---------------------------------------------------------------- redação

#[test]
fn command_lines_are_redacted() {
    let args: Vec<String> = [
        "node",
        "server.js",
        "--token=abc123",
        "--password",
        "hunter2",
        "API_KEY=zzz",
        "https://user:senha@host/x",
        "--port=3000",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let out = control_plane::redact(&args);
    for leaked in ["abc123", "hunter2", "zzz", "senha"] {
        assert!(!out.contains(leaked), "{leaked} vazou em {out}");
    }
    assert!(
        out.contains("--port=3000") && out.contains("server.js") && out.contains("user:***@host")
    );
}

// ---------------------------------------------------------------- snapshot (puro)

fn snapshot(
    procs: Vec<RawProcess>,
    ports: Vec<PortObservation>,
    managed: HashMap<u32, (String, String)>,
    self_pid: u32,
) -> control_plane::ControlPlaneSnapshot {
    let c = ctx();
    build_snapshot(Inputs {
        now_ms: 1,
        processes: procs,
        ports,
        ctx: &c,
        managed,
        runs: vec![],
        self_pid,
    })
}

#[test]
fn discovered_runtime_groups_the_tree_and_has_no_console() {
    // cmd.exe (limite) -> npm -> node (escuta 1420, cwd no projeto) -> esbuild
    let procs = vec![
        raw(101, None, "cmd.exe"),
        raw(102, Some(101), "npm.exe"),
        with_cwd(raw(103, Some(102), "node.exe"), "C:\\Dev\\Lab"),
        raw(104, Some(103), "esbuild.exe"),
    ];
    let s = snapshot(procs, vec![port(1420, Some(103))], HashMap::new(), 999);
    assert_eq!(s.runtimes.len(), 1);
    let r = &s.runtimes[0];
    assert_eq!(
        (r.origin, r.root_pid),
        (Origin::Discovered, Some(102)),
        "sobe até a fronteira (cmd.exe)"
    );
    assert_eq!(r.pids, vec![102, 103, 104]);
    assert_eq!(
        r.tree.iter().map(|n| (n.pid, n.depth)).collect::<Vec<_>>(),
        vec![(102, 0), (103, 1), (104, 2)]
    );
    assert_eq!(r.ports[0].port, 1420);
    assert_eq!(r.association.confidence, Confidence::High);
    assert_eq!(r.association.project_id.as_deref(), Some("lab"));
    assert!(!r.console.available && r.console.run_id.is_none());
    assert_eq!(
        r.console.reason.as_deref(),
        Some(control_plane::NO_CONSOLE_EXTERNAL)
    );
    assert!(r.execution.is_none() && !r.is_self);
}

#[test]
fn unassociated_listener_is_unknown_and_system_is_categorized() {
    let procs = vec![raw(100, None, "node.exe"), raw(4, None, "System")];
    let s = snapshot(
        procs,
        vec![port(8080, Some(100)), port(445, Some(4)), port(9999, None)],
        HashMap::new(),
        999,
    );
    let node = s.runtimes.iter().find(|r| r.ports[0].port == 8080).unwrap();
    assert_eq!(node.association.confidence, Confidence::Unknown);
    assert!(node.association.project_id.is_none() && node.association.session_id.is_none());
    assert_eq!(
        node.category, "dev",
        "Node.js é tecnologia conhecida, mas continua sem Project"
    );
    assert_eq!(
        s.runtimes
            .iter()
            .find(|r| r.ports[0].port == 445)
            .unwrap()
            .category,
        "system"
    );
    let orphan = s.runtimes.iter().find(|r| r.ports[0].port == 9999).unwrap();
    assert_eq!(
        (orphan.root_pid, orphan.console.reason.as_deref()),
        (None, Some(control_plane::NO_CONSOLE_UNIDENTIFIED))
    );
}

#[test]
fn own_process_is_flagged_and_inventory_is_relevance_filtered() {
    let procs = vec![
        raw(1, None, "explorer.exe"),
        raw(2, Some(1), "lk-dev-hub.exe"),
        raw(3, None, "chrome.exe"),
        with_cwd(raw(4, Some(2), "helper.exe"), "C:\\Dev\\Lab"),
    ];
    let s = snapshot(procs.clone(), vec![port(5000, Some(2))], HashMap::new(), 4);
    assert!(s.runtimes[0].is_self);
    let c = ctx();
    let relevant = process_inventory(
        procs.clone(),
        &[port(5000, Some(2))],
        &c,
        &HashMap::new(),
        false,
        4,
    );
    let pids: Vec<u32> = relevant.iter().map(|e| e.process.pid).collect();
    assert_eq!(
        pids,
        vec![1, 2, 4],
        "chrome.exe não é relevante; o pai (explorer) entra para fechar a árvore"
    );
    assert_eq!(
        process_inventory(procs, &[], &c, &HashMap::new(), true, 4).len(),
        4
    );
}

// ---------------------------------------------------------------- observação real (passiva)

#[test]
fn live_inventory_maps_own_pid_parent_and_listening_port() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port_number = listener.local_addr().unwrap().port();
    let own = std::process::id();
    let processes = system::inventory();
    let me = processes
        .iter()
        .find(|p| p.pid == own)
        .expect("o próprio processo está no inventário");
    assert!(me.parent.is_some() && me.start_time > 0 && me.memory > 0 && !me.cmd.is_empty());
    let ports = control_plane::listening_ports().unwrap();
    let mine = ports
        .iter()
        .find(|p| p.port == port_number)
        .expect("porta em escuta");
    assert_eq!((mine.pid, mine.protocol), (Some(own), "TCP"));
    assert_eq!(mine.ip_version, "v4");
    let c = Context::default();
    let snap = build_snapshot(Inputs {
        now_ms: 1,
        processes,
        ports,
        ctx: &c,
        managed: HashMap::new(),
        runs: vec![],
        self_pid: own,
    });
    let runtime = snap
        .runtimes
        .iter()
        .find(|r| r.ports.iter().any(|p| p.port == port_number))
        .unwrap();
    assert!(runtime.is_self, "o teste enxerga a si mesmo");
    assert_eq!(
        runtime.association.confidence,
        Confidence::Unknown,
        "sem Projects no contexto, nada é atribuído"
    );
    drop(listener);
}

#[test]
fn observing_never_mutates_anything() {
    let mut child = spawn_server(&std::env::temp_dir()).0;
    let pid = child.id();
    for _ in 0..3 {
        control_plane::observe(&Context::default(), HashMap::new(), vec![]).unwrap();
        control_plane::observe_processes(&Context::default(), &HashMap::new(), true).unwrap();
    }
    assert!(alive(pid), "observar não encerra processos");
    child.kill().ok();
    child.wait().ok();
}

// ---------------------------------------------------------------- subprocessos reais

const SERVER: &str = "const s=require('http').createServer((q,r)=>r.end('ok')).listen(0,'127.0.0.1',()=>{console.log('listen '+s.address().port);console.error('aviso no stderr')});";

/// Servidor Node EXTERNO (não iniciado pelo supervisor): devolve o processo e a porta.
fn spawn_server(cwd: &std::path::Path) -> (Child, u16) {
    let mut child = Command::new(hub_core::commands::resolve_tool("node").expect("node"))
        .args(["-e", SERVER])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port = line.trim().trim_start_matches("listen ").parse().unwrap();
    (child, port)
}

#[test]
fn managed_runtime_is_exact_has_console_and_stop_ends_only_its_tree() {
    if !have("npm") || !have("node") {
        return;
    }
    let (_t, dir) = fixture(
        "app",
        &[
            (
                "package.json",
                r#"{"name":"app","scripts":{"web":"node server.js"}}"#,
            ),
            ("package-lock.json", "{}"),
            ("server.js", SERVER),
        ],
    );
    let p = common::project("fx", &dir);
    let (sup, _) = supervisor();
    // Sentinela EXTERNO com cwd fora de qualquer Project: tem de sobreviver ao Stop.
    let outside = tempfile::tempdir().unwrap();
    let (mut sentinel, sentinel_port) = spawn_server(outside.path());
    let run = sup.start(&p, "web").unwrap();
    wait_until("servidor gerenciado no ar", || {
        text_of(&sup, &run.id).contains("listen ")
    });
    let text = text_of(&sup, &run.id);
    let managed_port: u16 = text
        .lines()
        .find_map(|l| l.strip_prefix("listen "))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let ctx = Context {
        projects: vec![ProjectRef {
            id: "fx".into(),
            name: "fx".into(),
            root: dir.to_string_lossy().into(),
            ports: vec![],
        }],
        worktrees: vec![],
    };

    let snap = control_plane::observe(&ctx, sup.managed_runs(), sup.all_runs()).unwrap();
    let managed = snap
        .runtimes
        .iter()
        .find(|r| r.id == run.id)
        .expect("runtime gerenciado");
    assert_eq!(
        (managed.origin, managed.state),
        (Origin::Managed, RunState::Running)
    );
    assert_eq!(managed.association.confidence, Confidence::Exact);
    assert_eq!(managed.association.project_id.as_deref(), Some("fx"));
    assert!(
        managed.ports.iter().any(|p| p.port == managed_port),
        "a porta é do runtime gerenciado"
    );
    assert!(managed.pids.len() >= 2, "npm + node (árvore rastreada)");
    assert!(
        managed.console.available && managed.console.run_id.as_deref() == Some(run.id.as_str())
    );
    assert!(managed
        .execution
        .as_ref()
        .is_some_and(|e| e.command == "npm run web"));
    assert!(
        text_of(&sup, &run.id).contains("aviso no stderr"),
        "stderr capturado"
    );
    let logs = sup.logs(&run.id, 0).unwrap();
    assert!(
        logs.lines.iter().any(|l| l.stream == "err")
            && logs.lines.iter().any(|l| l.stream == "out")
    );

    let external = snap
        .runtimes
        .iter()
        .find(|r| r.ports.iter().any(|p| p.port == sentinel_port))
        .expect("runtime descoberto");
    assert_eq!(
        (external.origin, external.association.confidence),
        (Origin::Discovered, Confidence::Unknown)
    );
    assert!(external.association.project_id.is_none());
    assert!(!external.console.available);

    let pids = managed.pids.clone();
    sup.stop_and_wait(&run.id, std::time::Duration::from_secs(15))
        .unwrap();
    wait_until("a árvore da execução sumiu", || {
        pids.iter().all(|pid| !alive(*pid))
    });
    assert!(alive(sentinel.id()), "o processo externo NUNCA é encerrado");
    let after = control_plane::observe(&ctx, sup.managed_runs(), sup.all_runs()).unwrap();
    let stopped = after.runtimes.iter().find(|r| r.id == run.id).unwrap();
    assert_eq!(stopped.state, RunState::Stopped);
    assert!(
        stopped.pids.is_empty() && stopped.console.available,
        "o console continua legível depois de parar"
    );
    assert_eq!(stopped.association.project_id.as_deref(), Some("fx"));
    sentinel.kill().ok();
    sentinel.wait().ok();
}

#[test]
fn external_process_inside_a_project_is_high_but_still_has_no_console() {
    if !have("node") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (mut child, server_port) = spawn_server(&root);
    let ctx = Context {
        projects: vec![ProjectRef {
            id: "fx".into(),
            name: "fx".into(),
            root: root.to_string_lossy().into(),
            ports: vec![],
        }],
        worktrees: vec![],
    };
    // O inventário tem cache de 1 s: espera o processo recém-criado entrar na observação.
    let seen = |snap: &control_plane::ControlPlaneSnapshot| {
        snap.runtimes
            .iter()
            .any(|r| r.ports.iter().any(|p| p.port == server_port) && r.root_pid.is_some())
    };
    wait_until("processo externo no inventário", || {
        seen(&control_plane::observe(&ctx, HashMap::new(), vec![]).unwrap())
    });
    let snap = control_plane::observe(&ctx, HashMap::new(), vec![]).unwrap();
    let r = snap
        .runtimes
        .iter()
        .find(|r| r.ports.iter().any(|p| p.port == server_port))
        .unwrap();
    assert_eq!(
        (r.origin, r.association.confidence),
        (Origin::Discovered, Confidence::High)
    );
    assert_eq!(r.association.project_id.as_deref(), Some("fx"));
    assert!(!r.console.available && r.console.reason.is_some());
    child.kill().ok();
    child.wait().ok();
}

// ---------------------------------------------------------------- sem vazamento portátil

#[test]
fn control_plane_state_never_enters_the_portable_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let db = hub_core::database::Database::open(&tmp.path().join("hub.db")).unwrap();
    let text = serde_json::to_string(&db.export_portable().unwrap())
        .unwrap()
        .to_lowercase();
    for word in [
        "runtime",
        "consolestream",
        "commandline",
        "listening",
        "managedexecution",
        "association",
    ] {
        assert!(
            !text.contains(word),
            "{word} não pode estar no workspace portátil"
        );
    }
}

// ---------------------------------------------------------------- Console Hub (backend)

const BIG: &str =
    "const l='x'.repeat(3900);for(let i=0;i<4500;i++)console.log(i+l);setInterval(()=>{},1000);";
const NOISY: &str = "for(let i=0;i<30000;i++){console.log('out '+i);if(i%3===0)console.error('err '+i);}setInterval(()=>{},1000);";

fn console_project(
    scripts: &str,
    files: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    hub_core::models::Project,
) {
    let mut all = vec![("package.json", scripts), ("package-lock.json", "{}")];
    all.extend_from_slice(files);
    let (tmp, dir) = fixture("con", &all);
    let p = common::project("con", &dir);
    (tmp, dir, p)
}

#[test]
fn output_events_announce_stdout_and_stderr_without_carrying_the_text() {
    if !have("npm") || !have("node") {
        return;
    }
    let (_t, _d, p) = console_project(
        r#"{"scripts":{"web":"node server.js"}}"#,
        &[("server.js", SERVER)],
    );
    let (sup, events) = supervisor();
    let run = sup.start(&p, "web").unwrap();
    wait_until("stdout e stderr capturados", || {
        let text = text_of(&sup, &run.id);
        text.contains("listen ") && text.contains("aviso no stderr")
    });
    wait_until("evento de saída", || {
        events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, hub_core::supervisor::RuntimeEvent::Output { .. }))
    });
    let seqs: Vec<u64> = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            hub_core::supervisor::RuntimeEvent::Output { run_id, seq, .. } if run_id == &run.id => {
                Some(*seq)
            }
            _ => None,
        })
        .collect();
    assert!(
        seqs.windows(2).all(|w| w[0] <= w[1]),
        "avisos de saída em ordem: {seqs:?}"
    );
    let chunk = sup.logs(&run.id, 0).unwrap();
    assert!(
        seqs.iter().all(|s| *s <= chunk.next_seq),
        "o aviso nunca aponta além do buffer"
    );
    assert!(
        chunk.lines.iter().any(|l| l.stream == "out")
            && chunk.lines.iter().any(|l| l.stream == "err")
    );
    sup.stop_and_wait(&run.id, std::time::Duration::from_secs(15))
        .unwrap();
}

#[test]
fn console_buffer_is_bounded_by_bytes_and_keeps_the_newest_lines() {
    if !have("npm") || !have("node") {
        return;
    }
    let (_t, _d, p) = console_project(r#"{"scripts":{"big":"node big.js"}}"#, &[("big.js", BIG)]);
    let (sup, _) = supervisor();
    let run = sup.start(&p, "big").unwrap();
    wait_until("4500 linhas grandes", || {
        sup.logs(&run.id, 0).unwrap().next_seq >= 4500
    });
    let chunk = sup.logs(&run.id, 0).unwrap();
    let bytes: usize = chunk.lines.iter().map(|l| l.text.len()).sum();
    assert!(
        bytes <= hub_core::supervisor::MAX_LOG_BYTES,
        "{bytes} bytes passam do limite"
    );
    assert!(
        chunk.lines.len() < 4500 && chunk.truncated,
        "o limite de bytes descartou as linhas mais antigas"
    );
    assert!(
        chunk.lines.last().unwrap().text.starts_with("4499"),
        "a mais recente fica"
    );
    assert!(chunk.lines.windows(2).all(|w| w[0].seq + 1 == w[1].seq));
    sup.stop_and_wait(&run.id, std::time::Duration::from_secs(15))
        .unwrap();
}

#[test]
fn a_very_noisy_process_neither_blocks_observation_nor_stop_and_capture_needs_no_listener() {
    if !have("npm") || !have("node") {
        return;
    }
    let (_t, _d, p) = console_project(
        r#"{"scripts":{"noisy":"node noisy.js"}}"#,
        &[("noisy.js", NOISY)],
    );
    // Ninguém escuta os eventos (janela fechada): a captura segue igual.
    let sup = hub_core::supervisor::Supervisor::new(std::sync::Arc::new(|_| {}));
    let run = sup.start(&p, "noisy").unwrap();
    let started = std::time::Instant::now();
    for _ in 0..3 {
        control_plane::observe(&Context::default(), sup.managed_runs(), sup.all_runs()).unwrap();
        sup.logs(&run.id, 0).unwrap();
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "observar e ler o buffer não pode travar sob saída intensa"
    );
    wait_until("saída completa capturada", || {
        sup.logs(&run.id, 0).unwrap().next_seq >= 40_000
    });
    sup.stop_and_wait(&run.id, std::time::Duration::from_secs(15))
        .unwrap();
    assert_eq!(
        sup.all_runs()
            .iter()
            .find(|r| r.id == run.id)
            .unwrap()
            .state,
        RunState::Stopped
    );
}

#[test]
fn console_is_local_only_and_commands_are_redacted_before_leaving_the_module() {
    // Estado de console nunca é serializado no workspace portátil (ver teste de passividade) e a
    // linha de comando de um runtime descoberto sai redigida.
    let mut p = raw(900, None, "node.exe");
    p.cmd = vec!["node".into(), "app.js".into(), "--token=segredo-xyz".into()];
    let procs = vec![p];
    let c = Context::default();
    let s = build_snapshot(Inputs {
        now_ms: 1,
        processes: procs,
        ports: vec![port(7000, Some(900))],
        ctx: &c,
        managed: HashMap::new(),
        runs: vec![],
        self_pid: 1,
    });
    let json = serde_json::to_string(&s).unwrap();
    assert!(
        !json.contains("segredo-xyz"),
        "token vazou no contrato do Control Plane"
    );
    assert!(json.contains("--token=***"));
}

#[test]
fn external_service_never_inherits_exact_from_managed_siblings_of_a_shared_host() {
    // cmd (limite) -> host (ex.: o LKR LAB) -> { externo escutando 3500 ; npm gerenciado -> node gerenciado }
    let procs = vec![
        raw(101, None, "cmd.exe"),
        raw(102, Some(101), "host.exe"),
        raw(103, Some(102), "node.exe"),
        raw(104, Some(102), "npm.exe"),
        raw(105, Some(104), "node.exe"),
    ];
    let managed = HashMap::from([
        (104, ("lab".to_string(), "run-1".to_string())),
        (105, ("lab".to_string(), "run-1".to_string())),
    ]);
    let c = ctx();
    let s = build_snapshot(Inputs {
        now_ms: 1,
        processes: procs,
        ports: vec![port(3500, Some(103))],
        ctx: &c,
        managed,
        runs: vec![],
        self_pid: 999,
    });
    let external = s
        .runtimes
        .iter()
        .find(|r| r.ports.iter().any(|p| p.port == 3500))
        .unwrap();
    assert_eq!(external.origin, Origin::Discovered);
    assert_eq!(
        external.root_pid,
        Some(103),
        "não sobe para o hospedeiro dos gerenciados"
    );
    assert_eq!(external.pids, vec![103]);
    assert_eq!(external.association.confidence, Confidence::Unknown);
    assert!(external.association.project_id.is_none());
}
