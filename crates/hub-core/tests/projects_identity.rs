//! Identidade de projeto (UUID + locator), inspeção passiva e vínculo local.
mod common;
use hub_core::{
    database::{Database, RegisterRequest},
    inspect::{inspect_folder, RegistrationStatus},
    locator::{choose_remote, normalize_remote, Remote, RepositoryLocator},
    models::Location,
    portable,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
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
fn repo(root: &Path, name: &str, remote: Option<&str>) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    if let Some(url) = remote {
        git(&dir, &["remote", "add", "origin", url]);
    }
    dir
}
fn sub(repo: &Path, rel: &str) -> PathBuf {
    let dir = repo.join(rel);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn db(root: &Path) -> Database {
    Database::open(&root.join("t.db")).unwrap()
}
fn register(db: &mut Database, dir: &Path, name: &str) -> hub_core::database::RegisterResult {
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
}
fn locator(remote: &str, path: &str) -> RepositoryLocator {
    RepositoryLocator {
        remote: remote.into(),
        path: path.into(),
    }
}
fn canon(url: &str) -> Option<String> {
    normalize_remote(url).map(|n| n.canonical)
}

// ---------- normalização de remote ----------
#[test]
fn ssh_https_and_scp_like_are_the_same_repository() {
    let a = canon("git@github.com:Org/Repo.git");
    assert_eq!(a, canon("https://github.com/org/repo"));
    assert_eq!(a, canon("ssh://git@github.com/org/repo.git"));
    assert_eq!(a, canon("https://www.github.com/ORG/repo/"));
    assert_eq!(a.as_deref(), Some("github.com/org/repo"));
}
#[test]
fn unknown_hosts_keep_case_and_ports_are_identity() {
    assert_ne!(
        canon("https://git.example.com/Org/Repo"),
        canon("https://git.example.com/org/repo")
    );
    assert_ne!(
        canon("ssh://git@host.example:2222/o/r"),
        canon("ssh://git@host.example/o/r")
    );
}
#[test]
fn credentials_and_garbage_are_not_identity() {
    let n = canon("https://user:secret@github.com/org/repo").unwrap();
    assert!(!n.contains("secret") && !n.contains('@'));
    assert!(
        canon("").is_none()
            && canon("C:\\Users\\x\\repo").is_none()
            && canon("not a url").is_none()
    );
}
#[test]
fn main_remote_rule_prefers_origin_and_is_deterministic() {
    let r = |n: &str, u: &str| Remote {
        name: n.into(),
        url: u.into(),
    };
    let remotes = [
        r("fork", "https://github.com/me/x"),
        r("origin", "https://github.com/org/x"),
    ];
    assert_eq!(
        choose_remote(&remotes, None).remote.unwrap().canonical,
        "github.com/org/x"
    );
    assert_eq!(
        choose_remote(
            &[
                r("a", "https://github.com/o/x"),
                r("b", "https://github.com/p/x")
            ],
            None
        )
        .remote,
        None
    );
    assert_eq!(
        choose_remote(
            &[
                r("a", "https://github.com/o/x"),
                r("b", "https://github.com/p/x")
            ],
            Some("b")
        )
        .remote
        .unwrap()
        .canonical,
        "github.com/p/x"
    );
    assert!(choose_remote(&[], None).remote.is_none());
}

// ---------- locator a partir de Git real ----------
#[test]
fn monorepo_subpaths_are_distinct_projects_of_the_same_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path(), "mono", Some("git@github.com:org/mono.git"));
    let (web, api) = (sub(&r, "apps/web"), sub(&r, "apps/api"));
    let mut db = db(tmp.path());
    let a = register(&mut db, &web, "Web");
    let b = register(&mut db, &api, "Api");
    assert!(a.registered && b.registered);
    assert_ne!(
        a.project.as_ref().unwrap().project.id,
        b.project.as_ref().unwrap().project.id
    );
    let la = a.project.unwrap().project.locator.unwrap();
    assert_eq!(
        (la.remote.as_str(), la.path.as_str()),
        ("github.com/org/mono", "apps/web")
    );
}
#[test]
fn repo_root_has_empty_subpath_and_no_remote_means_no_locator() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path(), "r", Some("https://github.com/org/r.git"));
    let i = inspect_folder(r.to_str().unwrap(), &[]);
    assert_eq!(i.locator, Some(locator("github.com/org/r", "")));
    let local = repo(tmp.path(), "local", None);
    let i = inspect_folder(local.to_str().unwrap(), &[]);
    assert!(i.valid && i.locator.is_none() && i.git.is_some());
    let plain = tmp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let i = inspect_folder(plain.to_str().unwrap(), &[]);
    assert!(i.valid && i.locator.is_none() && i.git.is_none());
}
#[test]
fn project_without_locator_is_never_deduplicated_by_name_or_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (repo(tmp.path(), "a", None), repo(tmp.path(), "b", None));
    let mut db = db(tmp.path());
    let (x, y) = (
        register(&mut db, &a, "Mesmo nome"),
        register(&mut db, &b, "Mesmo nome"),
    );
    assert!(x.registered && y.registered);
}

// ---------- detecção de projeto existente ----------
#[test]
fn same_locator_is_known_and_creates_no_new_uuid() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", Some("https://github.com/org/x"));
    let b = repo(tmp.path(), "b", Some("git@github.com:org/x.git"));
    let mut db = db(tmp.path());
    let first = register(&mut db, &a, "X");
    assert!(first.registered);
    let second = register(&mut db, &b, "X de novo");
    assert!(!second.registered && second.project.is_none());
    assert_eq!(second.registration.status, RegistrationStatus::Known);
    assert_eq!(db.projects().unwrap().len(), 1);
    // Vinculado e disponível em outra pasta: nunca sobrescreve em silêncio.
    assert!(!second.registration.matches[0].can_locate);
    let id = first.project.unwrap().project.id;
    assert!(db.bind(&id, b.to_str().unwrap(), true).is_err());
}
#[test]
fn same_folder_is_already_registered_here() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", Some("https://github.com/org/x"));
    let mut db = db(tmp.path());
    assert!(register(&mut db, &a, "X").registered);
    let again = register(&mut db, &a, "X");
    assert!(!again.registered);
    assert_eq!(again.registration.status, RegistrationStatus::AlreadyHere);
    assert_eq!(db.projects().unwrap().len(), 1);
}
#[test]
fn different_subpath_of_same_remote_is_new() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path(), "m", Some("https://github.com/org/m"));
    let mut db = db(tmp.path());
    assert!(register(&mut db, &sub(&r, "a"), "A").registered);
    let b = inspect_folder(
        sub(&r, "b").to_str().unwrap(),
        &db.projects_for_matching().unwrap(),
    );
    assert_eq!(b.registration.status, RegistrationStatus::New);
}
#[test]
fn imported_unbound_project_can_be_located_and_missing_binding_replaced() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", Some("https://github.com/org/x"));
    let mut db = db(tmp.path());
    let id = register(&mut db, &a, "X").project.unwrap().project.id;
    // Vínculo some (pasta apagada): Localizar pode trocar.
    let moved = repo(tmp.path(), "moved", Some("https://github.com/org/x"));
    std::fs::remove_dir_all(&a).unwrap();
    let i = inspect_folder(
        moved.to_str().unwrap(),
        &db.projects_for_matching().unwrap(),
    );
    assert_eq!(i.registration.status, RegistrationStatus::Known);
    assert!(i.registration.matches[0].can_locate);
    assert!(db.bind(&id, moved.to_str().unwrap(), true).unwrap().bound);
    assert_eq!(db.projects().unwrap().len(), 1);
}
#[test]
fn bind_rejects_folder_of_another_repository_or_subpath() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path(), "m", Some("https://github.com/org/m"));
    let other = repo(tmp.path(), "o", Some("https://github.com/org/other"));
    let mut db = db(tmp.path());
    let id = register(&mut db, &sub(&r, "a"), "A")
        .project
        .unwrap()
        .project
        .id;
    std::fs::remove_dir_all(r.join("a")).unwrap();
    assert!(db.bind(&id, other.to_str().unwrap(), true).is_err());
    assert!(db.bind(&id, sub(&r, "b").to_str().unwrap(), true).is_err());
}
#[test]
fn register_revalidates_in_backend_so_ui_cannot_duplicate() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", Some("https://github.com/org/x"));
    let mut db = db(tmp.path());
    // Duas inspeções "novas" feitas antes de qualquer cadastro (corrida): só uma cria.
    let stale = inspect_folder(a.to_str().unwrap(), &[]);
    let req = |n: &str| RegisterRequest {
        folder: PathBuf::from(&stale.folder),
        locator: stale.locator.clone(),
        repository: stale.repository.clone(),
        stack: vec![],
        name: n.into(),
        description: String::new(),
    };
    assert!(db.register(req("um")).unwrap().registered);
    assert!(!db.register(req("dois")).unwrap().registered);
    assert_eq!(db.projects().unwrap().len(), 1);
}
#[test]
fn registration_field_limits_are_enforced_by_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", None);
    let mut db = db(tmp.path());
    let i = inspect_folder(a.to_str().unwrap(), &[]);
    let mk = |name: String, description: String| RegisterRequest {
        folder: PathBuf::from(&i.folder),
        locator: None,
        repository: String::new(),
        stack: vec![],
        name,
        description,
    };
    assert!(db.register(mk("x".repeat(51), String::new())).is_err());
    assert!(db.register(mk("  ".into(), String::new())).is_err());
    assert!(db.register(mk("ok".into(), "d".repeat(201))).is_err());
    assert!(
        db.register(mk("x".repeat(50), "d".repeat(200)))
            .unwrap()
            .registered
    );
}
#[test]
fn project_id_is_uuid_v4_not_derived_from_path_or_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", Some("https://github.com/org/x"));
    let mut db = db(tmp.path());
    let p = register(&mut db, &a, "X").project.unwrap().project;
    let id = uuid::Uuid::parse_str(&p.id).unwrap();
    assert_eq!(id.get_version_num(), 4);
    assert!(!p.id.contains("org") && !p.slug.is_empty());
}

// ---------- inspeção ----------
#[test]
fn invalid_folders_are_reported_not_panicked() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("f.txt");
    std::fs::write(&file, "x").unwrap();
    for p in [
        file.to_str().unwrap(),
        tmp.path().join("nao-existe").to_str().unwrap(),
        "relativo/x",
        "",
    ] {
        let i = inspect_folder(p, &[]);
        assert!(!i.valid && i.error.is_some(), "{p}");
        assert_eq!(i.registration.status, RegistrationStatus::Invalid);
    }
}
#[test]
fn valid_folder_without_stack_is_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let i = inspect_folder(tmp.path().to_str().unwrap(), &[]);
    assert!(i.valid && i.stack.is_empty() && i.registration.status == RegistrationStatus::New);
}
#[test]
fn inspection_detects_stack_manager_scripts_and_files() {
    let (_t, dir) = common::fixture(
        "node",
        &[
            (
                "package.json",
                r#"{"name":"demo","scripts":{"dev":"vite","build":"vite build","test":"vitest"},"dependencies":{"react":"19.0.0","typescript":"5.0.0"}}"#,
            ),
            ("pnpm-lock.yaml", ""),
            ("README.md", "# x"),
            ("src/main.tsx", ""),
        ],
    );
    let i = inspect_folder(dir.to_str().unwrap(), &[]);
    let labels: Vec<_> = i.stack.iter().map(|s| s.label.to_string()).collect();
    assert!(labels.iter().any(|l| l.contains("React")), "{labels:?}");
    assert!(
        labels.iter().any(|l| l.contains("TypeScript")),
        "{labels:?}"
    );
    assert!(i.package_manager.is_some());
    let names: Vec<_> = i.scripts.iter().map(|s| s.name.as_str()).collect();
    assert!(["dev", "build", "test"].iter().all(|n| names.contains(n)));
    assert!(i.important_files.iter().any(|f| f == "package.json"));
    assert!(!i.structure.is_empty());
}
#[test]
fn inspection_never_executes_scripts_or_build_code() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("evil");
    std::fs::create_dir_all(&dir).unwrap();
    let marker = tmp.path().join("EXECUTED");
    let m = marker.to_string_lossy().replace('\\', "/");
    let node = format!("require('fs').writeFileSync('{m}','x')");
    std::fs::write(
        dir.join("package.json"),
        format!(r#"{{"scripts":{{"preinstall":"node -e \"{node}\"","dev":"node -e \"{node}\"","postinstall":"node -e \"{node}\""}}}}"#),
    )
    .unwrap();
    std::fs::write(
        dir.join("build.rs"),
        format!(
            "fn main(){{std::fs::write(r\"{}\",\"x\").unwrap();}}",
            marker.display()
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname=\"evil\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("docker-compose.yml"),
        "services:\n  a:\n    image: x\n",
    )
    .unwrap();
    git(&dir, &["init", "-q"]);
    // Config de repositório hostil: fsmonitor executaria um programa em `git status`.
    git(
        &dir,
        &["config", "core.fsmonitor", &format!("node -e \"{node}\"")],
    );
    let i = inspect_folder(dir.to_str().unwrap(), &[]);
    assert!(i.valid);
    assert!(!marker.exists(), "a inspeção executou código do projeto");
}
#[test]
fn inspection_does_not_change_git_state() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path(), "r", Some("https://github.com/org/r"));
    std::fs::write(r.join("a.txt"), "x").unwrap();
    let snap = |d: &Path| {
        let out = |a: &[&str]| {
            Command::new("git")
                .args(a)
                .current_dir(d)
                .output()
                .unwrap()
                .stdout
        };
        (
            out(&["status", "--porcelain=v1"]),
            out(&["rev-parse", "--abbrev-ref", "HEAD"]),
            out(&["config", "--list", "--local"]),
        )
    };
    let before = snap(&r);
    let i = inspect_folder(r.to_str().unwrap(), &[]);
    assert!(i.git.is_some());
    assert_eq!(before, snap(&r));
}
#[test]
fn inspection_output_has_no_secrets_from_remote_url() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(
        tmp.path(),
        "r",
        Some("https://user:tok3n@github.com/org/r.git"),
    );
    let i = inspect_folder(r.to_str().unwrap(), &[]);
    let json = serde_json::to_string(&i).unwrap();
    assert!(!json.contains("tok3n"), "{json}");
}

// ---------- portátil ----------
#[test]
fn portable_has_locator_but_never_a_local_path() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path(), "r", Some("https://github.com/org/r"));
    let mut db = db(tmp.path());
    register(&mut db, &sub(&r, "pkg"), "R");
    let ws = db.export_portable().unwrap();
    let json = serde_json::to_string(&ws).unwrap();
    assert!(json.contains("github.com/org/r") && json.contains("\"pkg\""));
    assert!(
        !json.contains("localPath") && !json.contains(&tmp.path().to_string_lossy().to_string())
    );
    assert_eq!(
        portable::content_hash(&ws),
        portable::content_hash(&db.export_portable().unwrap())
    );
}
#[test]
fn old_workspace_without_locator_still_loads_and_roundtrips() {
    let old = r#"{"version":1,"projects":[{"id":"p-1","slug":"a","name":"A","description":"","repository":"","stack":[],"tags":[],"ports":[],"commands":[],"createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z"}],"prompts":[],"knowledge":[]}"#;
    let ws: portable::PortableWorkspace = serde_json::from_str(old).unwrap();
    assert!(ws.projects[0].locator.is_none());
    portable::validate(&ws).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mut db = db(tmp.path());
    db.apply_portable(&ws).unwrap();
    let back = db.export_portable().unwrap();
    assert!(back.projects[0].locator.is_none());
    assert_eq!(db.projects().unwrap()[0].locator, None);
    assert_eq!(
        hub_core::projects::location(&db.projects().unwrap()[0]),
        Location::Unbound
    );
}
#[test]
fn invalid_locators_are_rejected_by_portable_validation() {
    let ok = locator("github.com/org/r", "apps/web");
    assert!(portable::valid_locator(&ok));
    for bad in [
        locator("https://github.com/o/r", ""),
        locator("git@github.com:o/r", ""),
        locator("host", ""),
        locator("C:\\Users\\x", ""),
        locator("github.com/o/r", "C:/x"),
        locator("github.com/o/r", "../x"),
        locator("github.com/o/r", "/abs"),
        locator("github.com/o/r", "a\\b"),
    ] {
        assert!(!portable::valid_locator(&bad), "{bad:?}");
    }
}
#[test]
fn locator_changes_move_the_sync_hash_and_hash_is_deterministic() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", Some("https://github.com/org/x"));
    let mut db = db(tmp.path());
    register(&mut db, &a, "X");
    let mut ws = db.export_portable().unwrap();
    let h = portable::content_hash(&ws);
    assert_eq!(h, portable::content_hash(&ws.clone()));
    ws.projects[0].locator.as_mut().unwrap().path = "sub".into();
    assert_ne!(h, portable::content_hash(&ws));
}
#[test]
fn bind_alone_does_not_change_portable_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let a = repo(tmp.path(), "a", Some("https://github.com/org/x"));
    let mut db = db(tmp.path());
    let id = register(&mut db, &a, "X").project.unwrap().project.id;
    let h = portable::content_hash(&db.export_portable().unwrap());
    db.bind(&id, a.to_str().unwrap(), true).unwrap();
    assert_eq!(h, portable::content_hash(&db.export_portable().unwrap()));
}

/// Validação real: o próprio repositório do LKR LAB (checkout de verdade, com remote) é
/// inspecionado sem alterar o Git e, depois de cadastrado, nunca gera um segundo UUID.
#[test]
fn this_very_repository_is_inspected_passively_and_never_duplicated() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = |d: &Path| {
        Command::new("git")
            .args(["status", "--porcelain=v1"])
            .current_dir(d)
            .output()
            .unwrap()
            .stdout
    };
    let before = status(&root);
    let inspected = inspect_folder(root.to_str().unwrap(), &[]);
    assert!(inspected.valid && inspected.git.is_some());
    assert!(inspected.stack.iter().any(|s| s.label.contains("Rust")));
    assert_eq!(before, status(&root), "a inspeção alterou o working tree");
    if inspected.locator.is_none() {
        return; // checkout sem remote (CI): nada a deduplicar por locator
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut db = db(tmp.path());
    assert!(register(&mut db, &root, "LKR LAB").registered);
    let again = register(&mut db, &root, "LKR LAB");
    assert!(!again.registered);
    assert_eq!(db.projects().unwrap().len(), 1);
    // Um worktree/clone do mesmo remote é "conhecido" (Localizar), não um projeto novo.
    let clone = repo(tmp.path(), "clone", None);
    let remote = inspected.locator.as_ref().unwrap().remote.clone();
    git(
        &clone,
        &["remote", "add", "origin", &format!("https://{remote}")],
    );
    let i = inspect_folder(
        clone.to_str().unwrap(),
        &db.projects_for_matching().unwrap(),
    );
    assert_eq!(i.registration.status, RegistrationStatus::Known);
}
