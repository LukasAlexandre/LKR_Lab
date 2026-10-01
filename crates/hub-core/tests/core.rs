use hub_core::{
    database::Database,
    git,
    models::{Location, ProjectInput, ProjectPort, Prompt},
    ports::validate_kill,
    projects,
};
use std::{path::Path, process::Command};
#[test]
#[ignore = "Read-only host timing probe; run explicitly"]
fn local_collection_profile() {
    for sample in 0..3 {
        let start = std::time::Instant::now();
        let processes = hub_core::system::processes(&[]);
        let process_ms = start.elapsed().as_millis();
        let start = std::time::Instant::now();
        let ports = hub_core::ports::inspect(&[]).unwrap();
        println!(
            "sample={sample} processes={} process_ms={process_ms} ports={} port_ms={}",
            processes.len(),
            ports.len(),
            start.elapsed().as_millis()
        );
    }
}
fn input(path: &Path) -> ProjectInput {
    ProjectInput {
        name: "LK Test".into(),
        description: "Fixture".into(),
        local_path: path.to_string_lossy().into(),
        repository: "https://github.com/example/repo".into(),
        stack: vec!["Rust".into()],
        tags: vec![],
        ports: vec![ProjectPort {
            name: "frontend".into(),
            port: 3000,
        }],
        commands: vec![],
    }
}
#[test]
fn migrations_crud_and_persistence() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.db");
    let mut db = Database::open(&path).unwrap();
    assert_eq!(db.prompts().unwrap().len(), 6);
    let p = db.save(None, input(tmp.path())).unwrap();
    assert_eq!(db.projects().unwrap().len(), 1);
    assert!(db.save(None, input(tmp.path())).is_err());
    let mut update = input(tmp.path());
    update.name = "Edited".into();
    db.save(Some(&p.id), update).unwrap();
    drop(db);
    let mut db = Database::open(&path).unwrap();
    assert_eq!(db.project(&p.id).unwrap().name, "Edited");
    assert_eq!(db.prompts().unwrap().len(), 6);
    assert!(db.delete(&p.id, false).is_err());
    db.save_prompt(Prompt {
        id: "local".into(),
        title: "Test".into(),
        category: "Development".into(),
        project_id: Some(p.id.clone()),
        body: "Hello".into(),
    })
    .unwrap();
    db.delete(&p.id, true).unwrap();
    assert_eq!(db.prompts().unwrap().len(), 6);
    assert!(db.projects().unwrap().is_empty());
    assert!(tmp.path().is_dir());
    assert_eq!(db.activities().unwrap().len(), 3);
}
#[test]
fn missing_local_path_is_observed_not_persisted() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("projeto");
    std::fs::create_dir(&folder).unwrap();
    let mut db = Database::open(&tmp.path().join("test.db")).unwrap();
    let saved = db.save(None, input(&folder)).unwrap();
    assert_eq!(projects::entry(saved.clone()).location, Location::Available);

    // Mesma situação de um cadastro vindo de outra máquina: o caminho não existe aqui.
    std::fs::remove_dir(&folder).unwrap();
    let listed = db.projects().unwrap().into_iter().next().unwrap();
    let entry = projects::entry(listed);
    assert_eq!(entry.location, Location::Missing);
    assert_eq!(entry.project.local_path, saved.local_path);
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["location"], "missing");
    assert_eq!(json["id"], saved.id.as_str());

    // A observação nunca vai para o banco.
    let stored: String = db
        .conn
        .query_row("SELECT data FROM projects", [], |r| r.get(0))
        .unwrap();
    assert!(!stored.contains("location"));
    assert!(!stored.contains("localPath"));
    assert!(!projects::path_available("relativo/projeto"));
}
#[test]
fn invalid_input_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = input(tmp.path());
    p.name = " ".into();
    assert!(projects::validate(p).is_err());
    let mut p = input(tmp.path());
    p.ports.push(p.ports[0].clone());
    assert!(projects::validate(p).is_err());
    let mut p = input(tmp.path());
    p.local_path = "relative/path".into();
    assert!(projects::validate(p).is_err());
}

#[test]
fn knowledge_migration_preserves_existing_data_and_project_deletion_keeps_notes() {
    use hub_core::models::KnowledgeEntry;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("v1.db");
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch(include_str!("../migrations/001_initial.sql"))
        .unwrap();
    old.execute(
        "UPDATE prompt_templates SET body='User content' WHERE id='audit'",
        [],
    )
    .unwrap();
    drop(old);
    let mut db = Database::open(&path).unwrap();
    assert_eq!(
        db.prompts()
            .unwrap()
            .iter()
            .find(|p| p.id == "audit")
            .unwrap()
            .body,
        "User content"
    );
    let project = db.save(None, input(tmp.path())).unwrap();
    let mut note = KnowledgeEntry {
        id: String::new(),
        project_id: Some(project.id.clone()),
        title: "Decision".into(),
        kind: "decision".into(),
        body: "Local content".into(),
        tags: "architecture".into(),
        updated_at: String::new(),
    };
    note.id = db.save_knowledge(note.clone()).unwrap();
    note.body = "Updated content".into();
    db.save_knowledge(note.clone()).unwrap();
    assert_eq!(db.knowledge().unwrap().len(), 1);
    let mut invalid = note.clone();
    invalid.body.clear();
    assert!(db.save_knowledge(invalid).is_err());
    db.delete(&project.id, true).unwrap();
    drop(db);
    let reopened = Database::open(&path).unwrap();
    let notes = reopened.knowledge().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body, "Updated content");
    assert_eq!(notes[0].project_id, None);
    assert!(!notes[0].updated_at.is_empty());
}
#[test]
fn repository_rejects_credentials_and_schemes() {
    assert!(projects::valid_repository(
        "https://github.com/lukas/project.git"
    ));
    for u in [
        "https://token@github.com/repo",
        "javascript:alert(1)",
        "https://github.com/repo?token=abc",
        "https://github.com/repo\n",
        "file:///etc/passwd",
    ] {
        assert!(!projects::valid_repository(u), "{u}");
    }
}
#[test]
fn git_parser_handles_changes_and_no_upstream() {
    let s=git::parse_status("# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -3\n1 M. N... 100644 100644 100644 abc abc file\n1 .M N... 100644 100644 100644 abc abc file\n? new\n");
    assert_eq!((s.ahead, s.behind), (Some(2), Some(3)));
    assert_eq!((s.staged, s.unstaged, s.untracked), (1, 1, 1));
    assert!(!s.clean);
    let s = git::parse_status("# branch.oid (initial)\n# branch.head main\n");
    assert!(s.clean);
    assert!(s.ahead.is_none());
}
fn command(path: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(args)
        .current_dir(path)
        .status()
        .unwrap()
        .success());
}
#[test]
fn git_fixture_real_status_and_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    command(tmp.path(), &["init", "-b", "main"]);
    command(tmp.path(), &["config", "user.name", "Fixture"]);
    command(
        tmp.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(tmp.path().join("README.md"), "fixture").unwrap();
    command(tmp.path(), &["add", "README.md"]);
    command(tmp.path(), &["commit", "-m", "test fixture"]);
    let state = git::inspect(tmp.path()).unwrap();
    assert!(state.clean);
    assert_eq!(state.branch, "main");
    assert_eq!(state.commits.len(), 1);
    std::fs::write(tmp.path().join("README.md"), "changed").unwrap();
    assert_eq!(git::inspect(tmp.path()).unwrap().unstaged, 1);
    assert_eq!(git::worktrees(tmp.path()).unwrap().len(), 1);
}
#[test]
fn git_null_status_preserves_paths_and_rename_source() {
    let records = [
        "# branch.head main",
        "# branch.oid abc",
        "? new folder/file name.txt",
        "1 .M N... 100644 100644 100644 abc abc spaced name.txt",
        "2 R. N... 100644 100644 100644 abc abc R100 renamed file.txt",
        "old file.txt",
        "",
    ];
    let state = git::parse_status(&records.join("\0"));
    assert_eq!(state.files.len(), 3);
    assert_eq!(state.files[0].path, "new folder/file name.txt");
    assert_eq!(state.files[1].path, "spaced name.txt");
    assert_eq!(state.files[2].original.as_deref(), Some("old file.txt"));
    assert_eq!(state.staged, 1);
    assert_eq!(state.unstaged, 1);
    assert!(!state.clean);
}
#[test]
fn kill_guards_never_kill_real_processes() {
    assert!(validate_kill(10000, 20, 20, false).is_err());
    assert!(validate_kill(10000, 20, 21, true).is_err());
    assert!(validate_kill(1, 20, 20, true).is_err());
    assert!(validate_kill(std::process::id(), 20, 20, true).is_err());
    assert!(validate_kill(10000, 0, 0, true).is_err());
    assert!(validate_kill(10000, 20, 20, true).is_ok());
}
#[test]
fn worktree_lifecycle_refuses_unconfirmed_dirty_and_foreign_paths() {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    command(&repo, &["init", "-b", "main"]);
    command(&repo, &["config", "user.name", "Fixture"]);
    command(&repo, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(repo.join("README.md"), "fixture").unwrap();
    command(&repo, &["add", "."]);
    command(&repo, &["commit", "-m", "fixture"]);
    let target = fixture.path().join("work tree");
    git::create_worktree(&repo, target.to_str().unwrap(), "feature/worktree").unwrap();
    assert_eq!(git::worktrees(&repo).unwrap().len(), 2);
    assert!(git::remove_worktree(&repo, target.to_str().unwrap(), false).is_err());
    assert!(git::remove_worktree(&repo, repo.to_str().unwrap(), true).is_err());
    assert!(git::remove_worktree(&repo, fixture.path().to_str().unwrap(), true).is_err());
    std::fs::write(target.join("unsaved.txt"), "keep me").unwrap();
    assert!(git::remove_worktree(&repo, target.to_str().unwrap(), true).is_err());
    assert!(target.join("unsaved.txt").is_file());
    std::fs::remove_file(target.join("unsaved.txt")).unwrap();
    git::remove_worktree(&repo, target.to_str().unwrap(), true).unwrap();
    assert!(!target.exists());
    assert_eq!(git::worktrees(&repo).unwrap().len(), 1);
}
#[test]
fn discovery_never_executes_project_scripts() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("package.json"),r#"{"dependencies":{"react":"1"},"devDependencies":{"typescript":"1"},"scripts":{"prepare":"exit 1"}}"#).unwrap();
    let d = projects::discover(tmp.path().to_str().unwrap()).unwrap();
    assert!(d.stack.contains(&"React".into()));
    assert!(d.stack.contains(&"TypeScript".into()));
    assert!(!d.is_git);
}
#[test]
fn snapshot_does_not_read_env_contents() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".env"), "JWT_SECRET=sentinel-never-include").unwrap();
    let mut db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let p = db.save(None, input(tmp.path())).unwrap();
    let snapshot = hub_core::snapshot::generate(&p).unwrap();
    assert!(!snapshot.contains("sentinel-never-include"));
    assert!(snapshot.contains(".env: true"));
    assert!(snapshot.contains("NOT VERIFIED"));
}
#[test]
#[ignore = "Requires OS socket enumeration; run explicitly on Windows/host"]
fn live_port_detection_uses_read_only_socket() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let entries = hub_core::ports::inspect(&[]).unwrap();
    assert!(entries
        .iter()
        .any(|p| p.port == port && p.protocol == "TCP"));
}
