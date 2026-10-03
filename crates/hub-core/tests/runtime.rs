//! Project Runtime Manager: detecção, Git do projeto, ownership e processos gerenciados.
//! Os testes de processo usam npm/node reais em pastas temporárias e NUNCA encerram
//! nada que não tenham iniciado (um Node "sentinela" externo prova isso).
use hub_core::{
    git,
    models::{Project, ProjectCommand},
    projects, runtime,
    runtime::{RuntimeStatus, ScriptKind},
    supervisor::{RunState, RuntimeEvent, Supervisor},
    system,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

fn project(id: &str, dir: &Path) -> Project {
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
fn unbound(id: &str) -> Project {
    project(id, Path::new(""))
}
fn write(dir: &Path, name: &str, text: &str) {
    if let Some(parent) = dir.join(name).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(dir.join(name), text).unwrap();
}
fn node_project(scripts: &str, lock: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("app");
    std::fs::create_dir(&dir).unwrap();
    write(
        &dir,
        "package.json",
        &format!(r#"{{"name":"app","scripts":{scripts}}}"#),
    );
    if !lock.is_empty() {
        write(&dir, lock, "{}");
    }
    (tmp, dir)
}
fn ids(detection: &runtime::Detection) -> Vec<&'static str> {
    detection.stack.iter().map(|s| s.id).collect()
}

// ------------------------------------------------------------------ detecção

#[test]
fn node_npm_project_scripts_and_package_manager() {
    let (_t, dir) = node_project(
        r#"{"build":"vite build","test":"vitest","dev":"vite","lint":"eslint .","start":"node x.js","predev":"echo","tauri":"tauri"}"#,
        "package-lock.json",
    );
    let d = runtime::detect(&dir);
    assert_eq!(ids(&d), vec!["node"]);
    assert_eq!(d.package_manager.as_ref().unwrap().name, "npm");
    assert_eq!(
        d.package_manager.as_ref().unwrap().evidence,
        "package-lock.json"
    );
    let names: Vec<_> = d.scripts.iter().map(|s| s.name.as_str()).collect();
    // dev/start primeiro; ganchos pre* não são ações; só scripts reais.
    assert_eq!(
        names,
        vec!["dev", "start", "tauri", "build", "lint", "test"]
    );
    let kind = |n: &str| d.scripts.iter().find(|s| s.name == n).unwrap().kind;
    assert_eq!(kind("dev"), ScriptKind::Service);
    assert_eq!(kind("build"), ScriptKind::Task);
    assert_eq!(kind("tauri"), ScriptKind::Other);
}
#[test]
fn package_manager_by_lockfile_and_never_invented() {
    for (lock, name) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
    ] {
        let (_t, dir) = node_project(r#"{"dev":"x"}"#, lock);
        assert_eq!(
            runtime::detect(&dir).package_manager.unwrap().name,
            name,
            "{lock}"
        );
    }
    // Sem lockfile: não inventa npm.
    let (_t, dir) = node_project(r#"{"dev":"x"}"#, "");
    let d = runtime::detect(&dir);
    assert!(d.package_manager.is_none());
    assert!(d.package_manager_note.unwrap().contains("não identificado"));
    // Campo packageManager do próprio package.json serve de evidência.
    write(
        &dir,
        "package.json",
        r#"{"packageManager":"pnpm@9.1.0","scripts":{"dev":"x"}}"#,
    );
    assert_eq!(runtime::detect(&dir).package_manager.unwrap().name, "pnpm");
    // Dois lockfiles de gerenciadores diferentes: ambíguo.
    let (_t, dir) = node_project(r#"{"dev":"x"}"#, "package-lock.json");
    write(&dir, "yarn.lock", "");
    let d = runtime::detect(&dir);
    assert!(d.package_manager.is_none());
    assert!(d.package_manager_note.unwrap().contains("ambíguo"));
}
#[test]
fn stack_fixtures_rust_tauri_astro_python_docker_unknown_and_combinations() {
    let tmp = tempfile::tempdir().unwrap();
    let make = |name: &str, files: &[(&str, &str)]| {
        let dir = tmp.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        for (file, text) in files {
            write(&dir, file, text);
        }
        dir
    };
    assert_eq!(
        ids(&runtime::detect(&make(
            "rust",
            &[("Cargo.toml", "[package]")]
        ))),
        vec!["rust"]
    );
    assert_eq!(
        ids(&runtime::detect(&make(
            "tauri",
            &[("Cargo.toml", ""), ("src-tauri/tauri.conf.json", "{}")]
        ))),
        vec!["rust", "tauri"]
    );
    let astro = make(
        "astro",
        &[
            ("package.json", r#"{"devDependencies":{"vite":"5"}}"#),
            ("astro.config.mjs", ""),
        ],
    );
    assert_eq!(ids(&runtime::detect(&astro)), vec!["node", "vite", "astro"]);
    assert_eq!(
        ids(&runtime::detect(&make("py1", &[("pyproject.toml", "")]))),
        vec!["python"]
    );
    assert_eq!(
        ids(&runtime::detect(&make("py2", &[("requirements.txt", "")]))),
        vec!["python"]
    );
    assert_eq!(
        ids(&runtime::detect(&make("py3", &[("Pipfile", "")]))),
        vec!["python"]
    );
    assert_eq!(
        ids(&runtime::detect(&make(
            "docker",
            &[("Dockerfile", "FROM scratch")]
        ))),
        vec!["docker"]
    );
    assert_eq!(
        ids(&runtime::detect(&make("compose", &[("compose.yml", "")]))),
        vec!["docker"]
    );
    let next = make(
        "next",
        &[(
            "package.json",
            r#"{"dependencies":{"next":"15","react":"19"}}"#,
        )],
    );
    assert_eq!(ids(&runtime::detect(&next)), vec!["node", "next", "react"]);
    let unknown = make("unknown", &[("notes.txt", "x")]);
    let d = runtime::detect(&unknown);
    assert!(d.stack.is_empty() && d.scripts.is_empty() && d.package_manager.is_none());
    let combo = make(
        "combo",
        &[
            (
                "package.json",
                r#"{"dependencies":{"react":"19"},"devDependencies":{"@tauri-apps/cli":"2"},"scripts":{"dev":"vite"}}"#,
            ),
            ("vite.config.ts", ""),
            ("Cargo.toml", ""),
            ("Dockerfile", ""),
            ("pnpm-lock.yaml", ""),
        ],
    );
    let d = runtime::detect(&combo);
    assert_eq!(
        ids(&d),
        vec!["node", "rust", "tauri", "vite", "react", "docker"]
    );
    assert_eq!(d.package_manager.unwrap().name, "pnpm");
    // Evidência real em cada item.
    assert!(runtime::detect(&combo)
        .stack
        .iter()
        .all(|s| !s.evidence.is_empty()));
}
#[test]
fn scripts_are_real_valid_and_cache_follows_file_changes() {
    let (_t, dir) = node_project(
        r#"{"ok":"x","a b":"x","x&y":"x","dev;rm":"x","bad":5,"-flag":"x"}"#,
        "package-lock.json",
    );
    let names: Vec<_> = runtime::detect(&dir)
        .scripts
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, vec!["ok"]);
    // package.json mudou: a detecção em cache é invalidada.
    std::thread::sleep(Duration::from_millis(20));
    write(
        &dir,
        "package.json",
        r#"{"scripts":{"dev":"vite","build":"vite build"}}"#,
    );
    let names: Vec<_> = runtime::detect(&dir)
        .scripts
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, vec!["dev", "build"]);
    // package.json inválido ou gigante não derruba a detecção.
    write(&dir, "package.json", "{quebrado");
    assert!(runtime::detect(&dir).scripts.is_empty());
}
#[test]
fn only_local_package_json_scripts_can_run_never_portable_commands() {
    let (_t, dir) = node_project(r#"{"dev":"vite"}"#, "package-lock.json");
    let mut p = project("p", &dir);
    // Comando vindo do workspace portátil: dado, nunca ação.
    p.commands = vec![ProjectCommand {
        name: "evil".into(),
        program: "powershell".into(),
        args: vec!["-c".into(), "calc".into()],
    }];
    for bad in [
        "evil",
        "powershell",
        "dev; calc",
        "dev && calc",
        "../dev",
        "",
        "dev ",
        "$(calc)",
        "a\nb",
    ] {
        assert!(runtime::launch_spec(&p, bad).is_err(), "{bad:?}");
    }
    if hub_core::commands::resolve_tool("npm").is_some() {
        let spec = runtime::launch_spec(&p, "dev").unwrap();
        assert_eq!(spec.args, vec!["run".to_string(), "dev".to_string()]);
        assert_eq!(spec.display, "npm run dev");
        assert!(spec.program.is_absolute());
        assert!(!spec.cwd.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(spec.kind, ScriptKind::Service);
    }
    // Sem gerenciador identificado: não executa.
    let (_t2, dir2) = node_project(r#"{"dev":"vite"}"#, "");
    assert!(runtime::launch_spec(&project("q", &dir2), "dev").is_err());
}
#[test]
fn unbound_and_missing_projects_never_execute() {
    assert!(runtime::launch_spec(&unbound("u"), "dev").is_err());
    let (t, dir) = node_project(r#"{"dev":"vite"}"#, "package-lock.json");
    let p = project("m", &dir);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(runtime::launch_spec(&p, "dev")
        .unwrap_err()
        .contains("Localizar"));
    drop(t);
    let rt = runtime::inspect(&unbound("u"), &[unbound("u")], vec![], &HashMap::new());
    assert_eq!(rt.status, RuntimeStatus::Unbound);
    assert!(!rt.can_run && rt.detection.scripts.is_empty() && rt.processes.is_empty());
    let rt = runtime::inspect(&p, std::slice::from_ref(&p), vec![], &HashMap::new());
    assert_eq!(rt.status, RuntimeStatus::Missing);
    assert!(!rt.can_run);
}
#[test]
fn runtime_snapshot_for_a_ready_project_has_identity_git_and_no_secrets() {
    let (_t, dir) = node_project(
        r#"{"dev":"vite --token=abc123secret"}"#,
        "package-lock.json",
    );
    let p = project("ready", &dir);
    let rt = runtime::inspect(&p, std::slice::from_ref(&p), vec![], &HashMap::new());
    assert_eq!(rt.status, RuntimeStatus::Ready);
    assert!(rt.can_run);
    assert_eq!(rt.project_id, "ready");
    assert!(!rt.git.unwrap().is_repo);
    let text = hub_core::snapshot::generate(
        &p,
        &runtime::inspect(&p, std::slice::from_ref(&p), vec![], &HashMap::new()),
    )
    .unwrap();
    assert!(text.contains("## Runtime"));
    assert!(text.contains("Node.js") && text.contains("npm") && text.contains("dev (serviço)"));
    // O texto do script (pode conter segredo) não vai para o contexto de IA.
    assert!(!text.contains("abc123secret"));
    let json = serde_json::to_string(&runtime::inspect(
        &p,
        std::slice::from_ref(&p),
        vec![],
        &HashMap::new(),
    ))
    .unwrap();
    assert!(json.contains("\"status\":\"ready\"") && json.contains("\"packageManager\""));
}

// ------------------------------------------------------------------ Git do projeto

fn git_cmd(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}
fn repo(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git_cmd(&dir, &["init", "-q", "-b", "main"]);
    write(&dir, "a.txt", "um\n");
    git_cmd(&dir, &["add", "."]);
    git_cmd(&dir, &["commit", "-q", "-m", "init"]);
    dir
}
#[test]
fn git_summary_clean_modified_staged_untracked_detached_and_not_a_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "r");
    let clean = git::summary(&dir);
    assert!(clean.is_repo && clean.clean && clean.branch == "main" && !clean.detached);
    assert_eq!(
        (
            clean.staged,
            clean.unstaged,
            clean.untracked,
            clean.conflicts
        ),
        (0, 0, 0, 0)
    );

    write(&dir, "a.txt", "dois\n");
    let modified = git::summary(&dir);
    assert!(!modified.clean && modified.unstaged == 1 && modified.staged == 0);
    git_cmd(&dir, &["add", "a.txt"]);
    let staged = git::summary(&dir);
    assert_eq!((staged.staged, staged.unstaged), (1, 0));
    write(&dir, "novo.txt", "x\n");
    let untracked = git::summary(&dir);
    assert_eq!(untracked.untracked, 1);
    assert_eq!(untracked.changes, 2);

    git_cmd(&dir, &["commit", "-q", "-am", "dois"]);
    git_cmd(&dir, &["checkout", "-q", "--detach"]);
    let detached = git::summary(&dir);
    assert!(detached.detached && detached.branch == "(detached)");

    let plain = tmp.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let none = git::summary(&plain);
    assert!(!none.is_repo && none.error.is_some());
}
#[test]
fn git_summary_conflict_ahead_and_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "r");
    git_cmd(&dir, &["checkout", "-q", "-b", "other"]);
    write(&dir, "a.txt", "lado other\n");
    git_cmd(&dir, &["commit", "-q", "-am", "other"]);
    git_cmd(&dir, &["checkout", "-q", "main"]);
    write(&dir, "a.txt", "lado main\n");
    git_cmd(&dir, &["commit", "-q", "-am", "main"]);
    // Merge com conflito (falha de propósito: o status é o que importa).
    let _ = Command::new("git")
        .args(["merge", "other"])
        .current_dir(&dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output();
    let conflicted = git::summary(&dir);
    assert_eq!(conflicted.conflicts, 1, "{conflicted:?}");
    assert!(!conflicted.clean);
    git_cmd(&dir, &["merge", "--abort"]);

    // ahead/behind contra um remote local.
    let remote = tmp.path().join("remote.git");
    git_cmd(
        tmp.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git_cmd(&dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git_cmd(&dir, &["push", "-q", "-u", "origin", "main"]);
    write(&dir, "b.txt", "local\n");
    git_cmd(&dir, &["add", "."]);
    git_cmd(&dir, &["commit", "-q", "-m", "local"]);
    let ahead = git::summary(&dir);
    assert_eq!((ahead.ahead, ahead.behind), (Some(1), Some(0)));
    assert!(ahead.upstream.is_some());

    let other = tmp.path().join("other-clone");
    git_cmd(
        tmp.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    write(&other, "c.txt", "remoto\n");
    git_cmd(&other, &["add", "."]);
    git_cmd(&other, &["commit", "-q", "-m", "remoto"]);
    git_cmd(&other, &["push", "-q", "origin", "main"]);
    git_cmd(&dir, &["fetch", "-q"]);
    let diverged = git::summary(&dir);
    assert_eq!((diverged.ahead, diverged.behind), (Some(1), Some(1)));
}

// ------------------------------------------------------------------ ownership

#[test]
fn ownership_managed_pid_external_and_unbound() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("app");
    std::fs::create_dir(&dir).unwrap();
    let me = std::process::id();
    let owned = project("owned", &dir);
    let managed: HashMap<u32, String> = [(me, "owned".to_string())].into();

    let list = system::processes_managed(std::slice::from_ref(&owned), &managed);
    let mine = list.iter().find(|p| p.pid == me).unwrap();
    assert_eq!(mine.project_id.as_deref(), Some("owned"));
    assert!(mine.managed && mine.confidence == "managed");

    // Sem PID gerenciado e sem cwd dentro da pasta: externo, sem dono.
    let list = system::processes_managed(std::slice::from_ref(&owned), &HashMap::new());
    let mine = list.iter().find(|p| p.pid == me).unwrap();
    assert!(mine.project_id.is_none() && !mine.managed);

    // Projeto sem pasta nunca é dono, nem de PID "gerenciado" (o bug do "" casando com tudo).
    let ghost = unbound("ghost");
    let managed: HashMap<u32, String> = [(me, "ghost".to_string())].into();
    let list = system::processes_managed(std::slice::from_ref(&ghost), &managed);
    assert!(list.iter().all(|p| p.project_id.is_none()));
}
#[test]
fn ownership_root_subdirectory_similar_prefix_and_multiple_projects() {
    let (root, nested, deep, sibling) = if cfg!(windows) {
        (
            r"C:\Dev\app",
            r"C:\Dev\app\packages\ui",
            r"C:\Dev\app\packages\ui\src",
            r"C:\Dev\app-two\src",
        )
    } else {
        (
            "/dev/app",
            "/dev/app/packages/ui",
            "/dev/app/packages/ui/src",
            "/dev/app-two/src",
        )
    };
    let all = [
        project("root", Path::new(root)),
        project("ui", Path::new(nested)),
        unbound("ghost"),
    ];
    assert_eq!(system::owner_of(Path::new(root), &all).unwrap().id, "root");
    assert_eq!(system::owner_of(Path::new(deep), &all).unwrap().id, "ui");
    assert!(system::owner_of(Path::new(sibling), &all).is_none());
}
#[test]
fn projects_with_unbound_ghost_never_get_runtime_processes() {
    let ghost = unbound("ghost");
    let rt = runtime::inspect(
        &ghost,
        std::slice::from_ref(&ghost),
        vec![],
        &HashMap::new(),
    );
    assert!(rt.processes.is_empty() && rt.ports.is_empty() && rt.services.is_empty());
}

// ------------------------------------------------------------------ processos gerenciados

fn tools_available() -> bool {
    hub_core::commands::resolve_tool("npm").is_some()
        && hub_core::commands::resolve_tool("node").is_some()
}
const SERVICE: &str =
    "console.log('servico no ar'); console.error('aviso no stderr'); setInterval(() => {}, 1000);";
const TASK: &str = "console.log('tarefa concluida');";
const BOOM: &str = "console.error('falhou feio'); process.exit(3);";
const TREE: &str = "const { spawn } = require('child_process');\nconst c = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });\nconsole.log('neto ' + c.pid);\nsetInterval(() => {}, 1000);";
const FLOOD: &str =
    "for (let i = 0; i < 5000; i++) console.log('linha ' + i); setInterval(() => {}, 1000);";

fn fixture() -> (tempfile::TempDir, PathBuf, Project) {
    let (tmp, dir) = node_project(
        r#"{"svc":"node svc.js","task":"node task.js","boom":"node boom.js","tree":"node tree.js","flood":"node flood.js"}"#,
        "package-lock.json",
    );
    for (name, body) in [
        ("svc.js", SERVICE),
        ("task.js", TASK),
        ("boom.js", BOOM),
        ("tree.js", TREE),
        ("flood.js", FLOOD),
    ] {
        write(&dir, name, body);
    }
    let p = project("fx", &dir);
    (tmp, dir, p)
}
type Events = Arc<Mutex<Vec<RuntimeEvent>>>;
fn supervisor() -> (Supervisor, Events) {
    let events: Events = Arc::default();
    let sink = events.clone();
    (
        Supervisor::new(Arc::new(move |e| sink.lock().unwrap().push(e))),
        events,
    )
}
fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(25);
    while Instant::now() < deadline {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("tempo esgotado esperando: {what}");
}
/// Estados entregues pelo sink, em ordem. O evento sai logo DEPOIS de o estado mudar, então
/// quem confere a lista precisa esperar o evento final chegar.
fn states(events: &Events) -> Vec<RunState> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            RuntimeEvent::State { state, .. } => Some(*state),
            _ => None,
        })
        .collect()
}
fn state_of(s: &Supervisor, id: &str) -> RunState {
    s.runs_for("fx")
        .into_iter()
        .find(|r| r.id == id)
        .unwrap()
        .state
}
fn all_text(s: &Supervisor, id: &str) -> String {
    s.logs(id, 0)
        .unwrap()
        .lines
        .iter()
        .map(|l| format!("{}:{}", l.stream, l.text))
        .collect::<Vec<_>>()
        .join("\n")
}
fn alive(pid: u32) -> bool {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system.process(sysinfo::Pid::from_u32(pid)).is_some()
}

#[test]
fn task_runs_captures_stdout_and_completes() {
    if !tools_available() {
        return;
    }
    let (_t, _d, p) = fixture();
    let (sup, events) = supervisor();
    let run = sup.start(&p, "task").unwrap();
    assert_eq!(run.command, "npm run task");
    assert_eq!(run.kind, ScriptKind::Other);
    wait_until("tarefa concluída", || {
        state_of(&sup, &run.id) == RunState::Completed
    });
    assert!(all_text(&sup, &run.id).contains("out:tarefa concluida"));
    assert_eq!(sup.runs_for("fx")[0].exit_code, Some(0));
    wait_until("evento final", || {
        states(&events).last() == Some(&RunState::Completed)
    });
    let kinds = states(&events);
    assert_eq!(kinds.first(), Some(&RunState::Starting));
    assert_eq!(kinds.last(), Some(&RunState::Completed));
}
#[test]
fn failing_script_reports_failed_with_exit_code_and_stderr() {
    if !tools_available() {
        return;
    }
    let (_t, _d, p) = fixture();
    let (sup, _) = supervisor();
    let run = sup.start(&p, "boom").unwrap();
    wait_until("falha", || state_of(&sup, &run.id) == RunState::Failed);
    assert_eq!(sup.runs_for("fx")[0].exit_code, Some(3));
    assert!(all_text(&sup, &run.id).contains("err:falhou feio"));
}
#[test]
fn service_stays_alive_streams_both_outputs_and_stops_cleanly() {
    if !tools_available() {
        return;
    }
    let (_t, _d, p) = fixture();
    let (sup, events) = supervisor();
    let run = sup.start(&p, "svc").unwrap();
    wait_until("serviço rodando", || {
        state_of(&sup, &run.id) == RunState::Running
    });
    wait_until("saídas", || {
        let text = all_text(&sup, &run.id);
        text.contains("out:servico no ar") && text.contains("err:aviso no stderr")
    });
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(
        state_of(&sup, &run.id),
        RunState::Running,
        "serviço não pode terminar sozinho"
    );
    // Segunda instância do mesmo script é recusada.
    assert!(sup
        .start(&p, "svc")
        .unwrap_err()
        .contains("já está em execução"));
    let pids: Vec<u32> = sup.managed_pids().keys().copied().collect();
    assert!(!pids.is_empty());

    sup.stop_and_wait(&run.id, Duration::from_secs(15)).unwrap();
    assert_eq!(state_of(&sup, &run.id), RunState::Stopped);
    assert!(sup.managed_pids().is_empty());
    wait_until("processos encerrados", || pids.iter().all(|p| !alive(*p)));
    wait_until("evento final", || {
        states(&events).last() == Some(&RunState::Stopped)
    });
    let seen = states(&events);
    assert!(seen.contains(&RunState::Stopping) && seen.last() == Some(&RunState::Stopped));
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, RuntimeEvent::Output { .. })));
}
#[test]
fn stop_kills_the_whole_tree_and_spares_unrelated_node_processes() {
    if !tools_available() {
        return;
    }
    // Node externo (não é do LKR LAB): precisa sobreviver a tudo.
    let mut sentinel = Command::new("node")
        .args(["-e", "setInterval(() => {}, 1000)"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let (_t, _d, p) = fixture();
    let (sup, _) = supervisor();
    let run = sup.start(&p, "tree").unwrap();
    wait_until("neto criado", || {
        all_text(&sup, &run.id).contains("out:neto ")
    });
    let grandchild: u32 = all_text(&sup, &run.id)
        .split("neto ")
        .nth(1)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let managed = sup.managed_pids();
    assert!(
        managed.contains_key(&grandchild),
        "o neto precisa estar na árvore gerenciada: {managed:?}"
    );
    assert!(managed.len() >= 3, "npm → node → neto: {managed:?}");
    assert!(!managed.contains_key(&sentinel.id()));
    let tree: Vec<u32> = managed.keys().copied().collect();

    sup.stop_and_wait(&run.id, Duration::from_secs(15)).unwrap();
    wait_until("árvore encerrada", || tree.iter().all(|pid| !alive(*pid)));
    assert!(alive(sentinel.id()), "o Node externo não pode ser morto");
    assert!(sentinel.try_wait().unwrap().is_none());
    let _ = sentinel.kill();
    let _ = sentinel.wait();
}
#[test]
fn restart_waits_for_shutdown_then_starts_a_fresh_instance() {
    if !tools_available() {
        return;
    }
    let (_t, _d, p) = fixture();
    let (sup, _) = supervisor();
    let first = sup.start(&p, "svc").unwrap();
    wait_until("rodando", || state_of(&sup, &first.id) == RunState::Running);
    let old: Vec<u32> = sup.managed_pids().keys().copied().collect();
    let second = sup.restart(&p, &first.id).unwrap();
    assert_ne!(second.id, first.id);
    assert_eq!(state_of(&sup, &first.id), RunState::Stopped);
    assert!(
        old.iter().all(|pid| !alive(*pid)),
        "a instância anterior precisa ter sumido antes da nova"
    );
    wait_until("nova rodando", || {
        state_of(&sup, &second.id) == RunState::Running
    });
    assert!(sup.managed_pids().keys().all(|pid| !old.contains(pid)));
    sup.stop_and_wait(&second.id, Duration::from_secs(15))
        .unwrap();
}
#[test]
fn log_buffer_is_bounded_and_ordered() {
    if !tools_available() {
        return;
    }
    let (_t, _d, p) = fixture();
    let (sup, _) = supervisor();
    let run = sup.start(&p, "flood").unwrap();
    wait_until("5000 linhas", || {
        sup.logs(&run.id, 0).unwrap().next_seq >= 5000
    });
    let chunk = sup.logs(&run.id, 0).unwrap();
    assert!(chunk.lines.len() <= 2000);
    assert!(chunk.truncated);
    assert!(chunk.lines.windows(2).all(|w| w[0].seq + 1 == w[1].seq));
    assert!(chunk.lines.last().unwrap().text.starts_with("linha "));
    let tail = sup.logs(&run.id, chunk.next_seq - 5).unwrap();
    assert_eq!(tail.lines.len(), 5);
    assert!(!tail.truncated);
    sup.stop_and_wait(&run.id, Duration::from_secs(15)).unwrap();
}
#[test]
fn supervisor_refuses_unbound_missing_unknown_and_foreign_requests() {
    if !tools_available() {
        return;
    }
    let (_t, dir, p) = fixture();
    let (sup, events) = supervisor();
    assert!(sup.start(&unbound("fx"), "svc").is_err());
    assert!(sup.start(&p, "nao-existe").is_err());
    assert!(sup.start(&p, "svc && calc").is_err());
    assert!(sup.logs("inexistente", 0).is_err());
    assert!(sup.stop("inexistente").is_err());
    let missing = project("fx", &dir.join("sumiu"));
    assert!(sup.start(&missing, "svc").is_err());
    assert!(
        events.lock().unwrap().is_empty(),
        "nada pode ter sido iniciado"
    );
    assert!(sup.managed_pids().is_empty());
    // Restart de uma execução de outro projeto é recusado.
    let run = sup.start(&p, "svc").unwrap();
    let other = project("other", &dir);
    assert!(sup.restart(&other, &run.id).is_err());
    sup.stop_and_wait(&run.id, Duration::from_secs(15)).unwrap();
}
#[test]
fn runtime_snapshot_marks_managed_run_processes_and_status() {
    if !tools_available() {
        return;
    }
    let (_t, _d, p) = fixture();
    let (sup, _) = supervisor();
    let run = sup.start(&p, "svc").unwrap();
    wait_until("rodando", || state_of(&sup, &run.id) == RunState::Running);
    let rt = runtime::inspect_live(&p, std::slice::from_ref(&p), sup.runs_for("fx"), &|| {
        sup.managed_pids()
    });
    assert_eq!(rt.status, RuntimeStatus::Running);
    assert!(!rt.processes.is_empty() && rt.processes.iter().all(|x| x.managed));
    assert!(!rt.external_running);
    assert_eq!(rt.runs[0].id, run.id);
    let snapshot = hub_core::snapshot::generate(&p, &rt).unwrap();
    assert!(snapshot.contains("npm run svc") && snapshot.contains("Running"));
    sup.stop_and_wait(&run.id, Duration::from_secs(15)).unwrap();
    let after = runtime::inspect(
        &p,
        std::slice::from_ref(&p),
        sup.runs_for("fx"),
        &sup.managed_pids(),
    );
    assert_ne!(after.status, RuntimeStatus::Running);
}
#[test]
fn dropping_the_supervisor_closes_the_group_and_leaves_no_orphans() {
    if !tools_available() {
        return;
    }
    let (_t, _d, p) = fixture();
    let (tx, rx) = mpsc::channel();
    {
        let (sup, _) = supervisor();
        let run = sup.start(&p, "tree").unwrap();
        wait_until("neto criado", || {
            all_text(&sup, &run.id).contains("out:neto ")
        });
        tx.send(sup.managed_pids().keys().copied().collect::<Vec<u32>>())
            .unwrap();
        sup.stop_all();
    }
    let pids = rx.recv().unwrap();
    wait_until("sem órfãos", || pids.iter().all(|pid| !alive(*pid)));
    let _ = projects::location(&p);
}

/// Ferramenta de desenvolvimento: imprime o runtime REAL de uma pasta
/// (`LKR_PROJECT_DIR=… cargo test -p hub-core --test runtime print_runtime_json -- --ignored --nocapture`).
#[test]
#[ignore = "utilitário manual: lê LKR_PROJECT_DIR"]
fn print_runtime_json() {
    let dir = std::env::var("LKR_PROJECT_DIR").expect("defina LKR_PROJECT_DIR");
    let p = project("real", Path::new(&dir));
    let rt = runtime::inspect(&p, std::slice::from_ref(&p), vec![], &HashMap::new());
    println!("RUNTIME_JSON={}", serde_json::to_string(&rt).unwrap());
}
