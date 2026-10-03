//! Visão agregada da página Projetos (Concept 03): disponibilidade, isolamento de falhas,
//! totais e "Localizar" sobre a fundação de identidade/vínculo.
use hub_core::{
    database::{Database, RegisterRequest},
    git::GitSummary,
    inspect::inspect_folder,
    models::{Location, Project},
    overview::{
        build, facts_from, is_dirty, totals, Dimension, DimensionStatus, LiveFacts, RuntimeSummary,
        Sources, StackSource,
    },
    ports::PortInfo,
    runtime::ScriptKind,
    supervisor::{RunInfo, RunState},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "LKR Test")
        .env("GIT_AUTHOR_EMAIL", "lkr@example.invalid")
        .env("GIT_COMMITTER_NAME", "LKR Test")
        .env("GIT_COMMITTER_EMAIL", "lkr@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}
fn repo(root: &Path, name: &str, remote: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["remote", "add", "origin", remote]);
    dir
}
fn db(root: &Path) -> Database {
    Database::open(&root.join("t.db")).unwrap()
}
fn register(db: &mut Database, dir: &Path, name: &str) -> Project {
    let known = db.projects_for_matching().unwrap();
    let i = inspect_folder(dir.to_str().unwrap(), &known);
    assert!(i.valid, "{:?}", i.error);
    db.register(RegisterRequest {
        folder: PathBuf::from(&i.folder),
        locator: i.locator,
        repository: i.repository,
        stack: i.stack.iter().map(|s| s.label.to_string()).collect(),
        name: name.into(),
        description: String::new(),
    })
    .unwrap()
    .project
    .unwrap()
    .project
}
fn clean() -> GitSummary {
    GitSummary {
        is_repo: true,
        branch: "main".into(),
        clean: true,
        ..Default::default()
    }
}
fn dirty() -> GitSummary {
    GitSummary {
        is_repo: true,
        branch: "main".into(),
        changes: 2,
        unstaged: 2,
        ..Default::default()
    }
}
fn project_at(id: &str, dir: &Path) -> Project {
    Project {
        id: id.into(),
        name: id.into(),
        slug: id.into(),
        description: String::new(),
        local_path: dir.to_string_lossy().into(),
        locator: None,
        repository: String::new(),
        stack: vec!["Registrada".into()],
        tags: vec![],
        ports: vec![],
        commands: vec![],
        created_at: String::new(),
        updated_at: String::new(),
    }
}
fn unbound(id: &str) -> Project {
    let mut p = project_at(id, Path::new("."));
    p.local_path = String::new();
    p
}

fn stack_ok(_: &Path) -> Vec<String> {
    vec!["Node.js".into()]
}
fn sources<'a>(
    git: &'a (dyn Fn(&Path) -> Dimension<GitSummary> + Sync + 'a),
    stack: &'a (dyn Fn(&Path) -> Vec<String> + Sync + 'a),
    live: Result<LiveFacts, String>,
) -> Sources<'a> {
    Sources {
        git,
        stack,
        live,
        last_activity: HashMap::new(),
    }
}
fn live(entries: &[(&str, bool)]) -> Result<LiveFacts, String> {
    Ok(LiveFacts {
        by_project: entries
            .iter()
            .map(|(id, running)| {
                (
                    id.to_string(),
                    RuntimeSummary {
                        running: *running,
                        ..Default::default()
                    },
                )
            })
            .collect(),
    })
}
fn port(project: &str, port: u16, protocol: &str) -> PortInfo {
    PortInfo {
        port,
        address: "127.0.0.1".into(),
        protocol: protocol.into(),
        pid: Some(1),
        process: "node".into(),
        executable: None,
        start_time: None,
        project_id: Some(project.into()),
        expected_by: vec![],
        confidence: "cwd".into(),
        conflict: false,
    }
}
fn run(project: &str, kind: ScriptKind, state: RunState, observer: bool) -> RunInfo {
    RunInfo {
        id: "r".into(),
        project_id: project.into(),
        script: "dev".into(),
        command_id: "node:dev".into(),
        label: "dev".into(),
        source: "node",
        observer,
        selection: None,
        command: "npm run dev".into(),
        kind,
        state,
        pid: Some(1),
        exit_code: None,
        started_at: 0,
        last_seq: 0,
    }
}

#[test]
fn available_project_gets_git_runtime_and_detected_stack() {
    let tmp = tempfile::tempdir().unwrap();
    let a = project_at("a", tmp.path());
    let g = |_: &Path| Dimension::available(clean());
    let o = build(vec![a], &sources(&g, &stack_ok, live(&[("a", true)])), 2);
    let p = &o.projects[0];
    assert_eq!(p.entry.location, Location::Available);
    assert_eq!(p.git.status, DimensionStatus::Available);
    assert!(p.runtime.data.as_ref().unwrap().running);
    assert_eq!(p.stack, vec!["Node.js".to_string()]);
    assert_eq!(p.stack_source, StackSource::Detected);
}

#[test]
fn missing_and_unbound_never_query_git_stack_or_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let mut missing = project_at("m", &tmp.path().join("apagada"));
    missing.stack = vec!["Cadastrada".into()];
    let calls = AtomicUsize::new(0);
    let g = |_: &Path| {
        calls.fetch_add(1, Ordering::SeqCst);
        Dimension::available(clean())
    };
    let s = |_: &Path| {
        calls.fetch_add(1, Ordering::SeqCst);
        vec!["Detectada".to_string()]
    };
    let o = build(
        vec![missing, unbound("u")],
        &sources(&g, &s, live(&[("m", true), ("u", true)])),
        4,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0, "sem pasta não há consulta");
    assert_eq!(o.projects[0].entry.location, Location::Missing);
    assert_eq!(o.projects[1].entry.location, Location::Unbound);
    for p in &o.projects {
        assert_eq!(p.git.status, DimensionStatus::NotApplicable);
        assert_eq!(p.runtime.status, DimensionStatus::NotApplicable);
        assert!(p.git.data.is_none() && p.runtime.data.is_none());
        assert_eq!(p.stack_source, StackSource::Registered);
    }
    // A stack mostrada é a do cadastro portátil, nunca uma detecção inventada.
    assert_eq!(o.projects[0].stack, vec!["Cadastrada".to_string()]);
    assert_eq!(
        o.totals.running, 0,
        "projeto sem pasta nunca está em execução"
    );
    assert_eq!(o.totals.dirty, 0);
}

#[test]
fn git_clean_and_dirty_feed_the_dirty_total() {
    let tmp = tempfile::tempdir().unwrap();
    let a = project_at("limpo", &tmp.path().join("a"));
    let b = project_at("sujo", &tmp.path().join("b"));
    std::fs::create_dir_all(&a.local_path).unwrap();
    std::fs::create_dir_all(&b.local_path).unwrap();
    let g = |path: &Path| {
        Dimension::available(if path.ends_with("b") {
            dirty()
        } else {
            clean()
        })
    };
    let o = build(vec![a, b], &sources(&g, &stack_ok, live(&[])), 2);
    assert!(!is_dirty(o.projects[0].git.data.as_ref().unwrap()));
    assert!(is_dirty(o.projects[1].git.data.as_ref().unwrap()));
    assert_eq!(o.totals.dirty, 1);
}

#[test]
fn ahead_or_behind_alone_is_not_local_changes() {
    let behind = GitSummary {
        is_repo: true,
        ahead: Some(2),
        behind: Some(3),
        clean: true,
        ..Default::default()
    };
    assert!(!is_dirty(&behind));
}

#[test]
fn real_git_repo_is_clean_then_dirty() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path(), "r", "https://github.com/org/r");
    let p = project_at("r", &dir);
    let g = hub_core::overview::real_git;
    let o = build(vec![p.clone()], &sources(&g, &stack_ok, live(&[])), 1);
    assert!(o.projects[0].git.data.as_ref().unwrap().clean);
    std::fs::write(dir.join("novo.txt"), "x").unwrap();
    let o = build(vec![p], &sources(&g, &stack_ok, live(&[])), 1);
    assert!(is_dirty(o.projects[0].git.data.as_ref().unwrap()));
}

#[test]
fn folder_without_git_is_not_applicable_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let p = project_at("plain", tmp.path());
    let g = hub_core::overview::real_git;
    let o = build(vec![p], &sources(&g, &stack_ok, live(&[])), 1);
    assert_eq!(o.projects[0].git.status, DimensionStatus::NotApplicable);
}

#[test]
fn runtime_running_needs_active_service_run_or_listening_tcp_port() {
    let a = project_at("a", Path::new("."));
    let b = project_at("b", Path::new("."));
    let c = project_at("c", Path::new("."));
    let all = vec![a, b, c];
    let ports = vec![
        port("a", 3000, "TCP"),
        port("a", 3000, "TCP"),
        port("c", 53, "UDP"),
    ];
    let mut runs = HashMap::new();
    runs.insert(
        "b".to_string(),
        vec![
            run("b", ScriptKind::Task, RunState::Running, false),
            run("b", ScriptKind::Service, RunState::Running, true),
        ],
    );
    let f = facts_from(&all, &runs, &ports);
    let a = &f.by_project["a"];
    assert!(a.running);
    assert_eq!(
        a.listening_ports,
        vec![3000],
        "porta repetida conta uma vez"
    );
    // Tarefa em andamento e observador de logs não são "em execução".
    assert!(!f.by_project["b"].running);
    // UDP não é serviço escutando.
    assert!(!f.by_project["c"].running);
    runs.insert(
        "b".to_string(),
        vec![run("b", ScriptKind::Service, RunState::Running, false)],
    );
    let f = facts_from(&all, &runs, &ports);
    assert!(f.by_project["b"].running);
    assert_eq!(f.by_project["b"].managed_runs, 1);
    // Execução já parada não conta.
    runs.insert(
        "b".to_string(),
        vec![run("b", ScriptKind::Service, RunState::Stopped, false)],
    );
    assert!(!facts_from(&all, &runs, &ports).by_project["b"].running);
}

#[test]
fn git_error_is_isolated_to_that_project() {
    let tmp = tempfile::tempdir().unwrap();
    let a = project_at("ok", &tmp.path().join("a"));
    let b = project_at("ruim", &tmp.path().join("b"));
    std::fs::create_dir_all(&a.local_path).unwrap();
    std::fs::create_dir_all(&b.local_path).unwrap();
    let g = |path: &Path| {
        if path.ends_with("b") {
            Dimension::error("git: tempo limite excedido")
        } else {
            Dimension::available(clean())
        }
    };
    let o = build(vec![a, b], &sources(&g, &stack_ok, live(&[])), 2);
    assert_eq!(o.projects.len(), 2);
    assert_eq!(o.projects[0].git.status, DimensionStatus::Available);
    assert_eq!(o.projects[1].git.status, DimensionStatus::Error);
    assert!(o.projects[1]
        .git
        .message
        .as_deref()
        .unwrap()
        .contains("tempo"));
    // As demais dimensões do projeto com erro de Git continuam disponíveis.
    assert_eq!(o.projects[1].runtime.status, DimensionStatus::Available);
    assert_eq!(o.projects[1].stack, vec!["Node.js".to_string()]);
}

#[test]
fn git_panic_does_not_take_down_the_list() {
    let tmp = tempfile::tempdir().unwrap();
    let a = project_at("a", &tmp.path().join("a"));
    let b = project_at("b", &tmp.path().join("b"));
    std::fs::create_dir_all(&a.local_path).unwrap();
    std::fs::create_dir_all(&b.local_path).unwrap();
    let g = |path: &Path| -> Dimension<GitSummary> {
        if path.ends_with("a") {
            panic!("falha simulada");
        }
        Dimension::available(clean())
    };
    let o = build(vec![a, b], &sources(&g, &stack_ok, live(&[])), 2);
    assert_eq!(o.projects[0].git.status, DimensionStatus::Error);
    assert_eq!(o.projects[1].git.status, DimensionStatus::Available);
}

#[test]
fn runtime_error_is_isolated_and_missing_stays_not_applicable() {
    let tmp = tempfile::tempdir().unwrap();
    let a = project_at("a", tmp.path());
    let g = |_: &Path| Dimension::available(clean());
    let o = build(
        vec![a, unbound("u")],
        &sources(&g, &stack_ok, Err("Portas: sem acesso".into())),
        2,
    );
    assert_eq!(o.projects[0].runtime.status, DimensionStatus::Error);
    assert_eq!(o.projects[0].git.status, DimensionStatus::Available);
    assert_eq!(o.projects[1].runtime.status, DimensionStatus::NotApplicable);
    assert_eq!(o.totals.running, 0);
}

#[test]
fn unknown_stack_does_not_break_the_project() {
    let tmp = tempfile::tempdir().unwrap();
    let a = project_at("a", tmp.path());
    let g = |_: &Path| Dimension::available(clean());
    let none = |_: &Path| Vec::<String>::new();
    let o = build(vec![a], &sources(&g, &none, live(&[])), 1);
    assert!(o.projects[0].stack.is_empty());
    assert_eq!(o.projects[0].git.status, DimensionStatus::Available);
}

#[test]
fn totals_close_with_the_cards_and_order_is_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let mut list = vec![];
    for name in ["a", "b", "c"] {
        let dir = tmp.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        list.push(project_at(name, &dir));
    }
    list.push(project_at("gone", &tmp.path().join("gone")));
    list.push(unbound("sem-pasta"));
    let g = |path: &Path| {
        Dimension::available(if path.ends_with("c") {
            dirty()
        } else {
            clean()
        })
    };
    let o = build(
        list,
        &sources(&g, &stack_ok, live(&[("a", true), ("c", true)])),
        4,
    );
    let ids: Vec<_> = o
        .projects
        .iter()
        .map(|p| p.entry.project.id.as_str())
        .collect();
    assert_eq!(ids, ["a", "b", "c", "gone", "sem-pasta"]);
    let t = &o.totals;
    assert_eq!(
        (
            t.total,
            t.available,
            t.missing,
            t.unbound,
            t.running,
            t.dirty
        ),
        (5, 3, 1, 1, 2, 1)
    );
    assert_eq!(t.available + t.missing + t.unbound, t.total);
    assert_eq!(*t, totals(&o.projects));
}

#[test]
fn many_projects_respect_the_worker_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut list = vec![];
    for i in 0..24 {
        let dir = tmp.path().join(format!("p{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        list.push(project_at(&format!("p{i}"), &dir));
    }
    let active = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let g = |_: &Path| {
        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
        peak.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(15));
        active.fetch_sub(1, Ordering::SeqCst);
        Dimension::available(clean())
    };
    let o = build(list, &sources(&g, &stack_ok, live(&[])), 64);
    assert_eq!(o.projects.len(), 24);
    assert!(peak.load(Ordering::SeqCst) <= hub_core::overview::MAX_WORKERS);
}

#[test]
fn last_activity_comes_from_the_activities_table_only() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", "https://github.com/org/a");
    let b = repo(tmp.path(), "b", "https://github.com/org/b");
    let mut db = db(tmp.path());
    let pa = register(&mut db, &a, "A");
    register(&mut db, &b, "B");
    db.activity(&pa.id, "Contexto gerado").unwrap();
    let last = db.last_activity_by_project().unwrap();
    assert_eq!(
        last.len(),
        2,
        "o cadastro já registra uma atividade por projeto"
    );
    assert!(last[&pa.id].starts_with("20"));
}

// ---- Localizar (reaproveita a fundação do Concept 04: `Database::bind`) ----

fn imported_unbound(tmp: &Path, remote: &str) -> (Database, String, PathBuf) {
    let origin = repo(tmp, "origem", remote);
    let mut first = Database::open(&tmp.join("a.db")).unwrap();
    let id = register(&mut first, &origin, "X").id;
    let ws = first.export_portable().unwrap();
    let mut second = Database::open(&tmp.join("b.db")).unwrap();
    second.apply_portable(&ws).unwrap();
    (second, id, origin)
}

#[test]
fn locate_binds_existing_project_keeping_uuid() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut db, id, _) = imported_unbound(tmp.path(), "https://github.com/org/x");
    assert_eq!(
        hub_core::projects::location(&db.projects().unwrap()[0]),
        Location::Unbound
    );
    let clone = repo(tmp.path(), "clone", "https://github.com/org/x");
    let result = db.bind(&id, clone.to_str().unwrap(), false).unwrap();
    assert!(result.bound);
    let all = db.projects().unwrap();
    assert_eq!(all.len(), 1, "Localizar nunca cria Project novo");
    assert_eq!(all[0].id, id, "UUID preservado");
    assert_eq!(hub_core::projects::location(&all[0]), Location::Available);
    // O binding ficou na pasta escolhida e o overview passa a vê-lo como disponível.
    let g = |_: &Path| Dimension::available(clean());
    let o = build(all, &sources(&g, &stack_ok, live(&[])), 1);
    assert_eq!(o.totals.available, 1);
    assert_eq!(o.totals.unbound, 0);
}

#[test]
fn locate_refuses_folder_of_another_project() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut db, id, _) = imported_unbound(tmp.path(), "https://github.com/org/x");
    // Outro projeto já vinculado à pasta "y" nesta máquina.
    let y = repo(tmp.path(), "y", "https://github.com/org/y");
    let other = register(&mut db, &y, "Y");
    let err = db.bind(&id, y.to_str().unwrap(), true).unwrap_err();
    assert!(err.contains("já está vinculada"), "{err}");
    // Pasta de outro repositório, ainda não cadastrada: também recusada.
    let z = repo(tmp.path(), "z", "https://github.com/org/z");
    assert!(db.bind(&id, z.to_str().unwrap(), true).is_err());
    let all = db.projects().unwrap();
    assert_eq!(all.len(), 2);
    let target = all.iter().find(|p| p.id == id).unwrap();
    assert!(target.local_path.is_empty(), "nada foi vinculado");
    let untouched = all.iter().find(|p| p.id == other.id).unwrap();
    assert!(!untouched.local_path.is_empty());
}

#[test]
fn locate_replaces_a_missing_binding_with_the_correct_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", "https://github.com/org/x");
    let mut db = db(tmp.path());
    let id = register(&mut db, &a, "X").id;
    std::fs::remove_dir_all(&a).unwrap();
    assert_eq!(
        hub_core::projects::location(&db.projects().unwrap()[0]),
        Location::Missing
    );
    let moved = repo(tmp.path(), "movida", "https://github.com/org/x");
    assert!(db.bind(&id, moved.to_str().unwrap(), true).unwrap().bound);
    let p = &db.projects().unwrap()[0];
    assert_eq!(p.id, id);
    assert!(p.local_path.ends_with("movida"));
}

#[test]
fn project_activity_is_per_project_and_knows_if_context_was_generated() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", "https://github.com/org/a");
    let b = repo(tmp.path(), "b", "https://github.com/org/b");
    let mut db = db(tmp.path());
    let pa = register(&mut db, &a, "A");
    let pb = register(&mut db, &b, "B");
    let before = db.project_activity(&pa.id, 8).unwrap();
    assert_eq!(before.items.len(), 1, "só o cadastro");
    assert!(!before.context_generated);
    db.activity(&pa.id, hub_core::database::CONTEXT_ACTIVITY)
        .unwrap();
    db.activity(&pa.id, "Outra ação").unwrap();
    let after = db.project_activity(&pa.id, 8).unwrap();
    assert!(after.context_generated);
    assert_eq!(after.items[0].action, "Outra ação", "mais nova primeiro");
    assert!(after
        .items
        .iter()
        .all(|i| i.project_id.as_deref() == Some(pa.id.as_str())));
    assert!(!db.project_activity(&pb.id, 8).unwrap().context_generated);
    assert_eq!(db.project_activity(&pa.id, 2).unwrap().items.len(), 2);
    assert!(db
        .project_activity("inexistente", 8)
        .unwrap()
        .items
        .is_empty());
}
