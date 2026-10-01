//! Sync do workspace: lógica (bridge falso) e ponta a ponta (bridge Node real +
//! repositórios Git temporários com remote "bare" local; nenhuma rede).
use hub_core::{
    bridge::{Bridge, BridgeError, HttpBridge, PushOutcome, RemoteFile, RemoteInfo},
    database::Database,
    models::{Location, ProjectInput, Prompt},
    portable::{self, PortablePreferences, PortableWorkspace},
    projects,
    sync::{self, Resolution, SyncState},
};
use std::{
    cell::RefCell,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{atomic::AtomicBool, Mutex},
};

fn input(name: &str, path: &Path, repository: &str) -> ProjectInput {
    ProjectInput {
        name: name.into(),
        description: "d".into(),
        local_path: path.to_string_lossy().into(),
        repository: repository.into(),
        stack: vec!["Rust".into()],
        tags: vec![],
        ports: vec![],
        commands: vec![],
    }
}
fn folder(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn machine(root: &Path, name: &str) -> Mutex<Database> {
    Mutex::new(Database::open(&root.join(format!("{name}.db"))).unwrap())
}
fn add_project(db: &Mutex<Database>, name: &str, dir: &Path) -> String {
    db.lock()
        .unwrap()
        .save(None, input(name, dir, "https://github.com/org/lkr"))
        .unwrap()
        .id
}
fn export(db: &Mutex<Database>) -> PortableWorkspace {
    db.lock().unwrap().export_portable().unwrap()
}
fn hash(db: &Mutex<Database>) -> String {
    portable::content_hash(&export(db))
}
fn remote_ok(ws: &PortableWorkspace) -> RemoteInfo {
    RemoteInfo {
        has_remote: true,
        fetched: true,
        fetch_error: None,
        file: RemoteFile::Ok(Box::new(ws.clone())),
    }
}
fn remote_with(file: RemoteFile) -> RemoteInfo {
    RemoteInfo {
        has_remote: true,
        fetched: true,
        fetch_error: None,
        file,
    }
}
fn idle() -> AtomicBool {
    AtomicBool::new(false)
}

// ---------------------------------------------------------------- bridge falso

struct Fake {
    remote: RefCell<Result<RemoteInfo, BridgeError>>,
    push: RefCell<Vec<Result<PushOutcome, BridgeError>>>,
    update: RefCell<Result<(), BridgeError>>,
    pushed: RefCell<Vec<PortableWorkspace>>,
    updates: RefCell<u32>,
    on_remote: RefCell<Option<Box<dyn Fn()>>>,
}
impl Fake {
    fn new(remote: Result<RemoteInfo, BridgeError>) -> Self {
        Self {
            remote: RefCell::new(remote),
            push: RefCell::new(vec![]),
            update: RefCell::new(Ok(())),
            pushed: RefCell::new(vec![]),
            updates: RefCell::new(0),
            on_remote: RefCell::new(None),
        }
    }
    fn push_ok(self) -> Self {
        self.push.borrow_mut().push(Ok(PushOutcome {
            pushed: true,
            commit: Some("abc1234".into()),
        }));
        self
    }
}
impl Bridge for Fake {
    fn remote(&self) -> Result<RemoteInfo, BridgeError> {
        if let Some(hook) = self.on_remote.borrow().as_ref() {
            hook();
        }
        self.remote.borrow().clone()
    }
    fn push(&self, ws: &PortableWorkspace) -> Result<PushOutcome, BridgeError> {
        self.pushed.borrow_mut().push(ws.clone());
        let mut results = self.push.borrow_mut();
        if results.is_empty() {
            Err(BridgeError::Unavailable("sem resultado".into()))
        } else {
            results.remove(0)
        }
    }
    fn update(&self) -> Result<(), BridgeError> {
        *self.updates.borrow_mut() += 1;
        self.update.borrow().clone()
    }
}
fn rejected(code: &str) -> BridgeError {
    BridgeError::Rejected {
        code: code.into(),
        message: "mensagem do bridge".into(),
    }
}

// ---------------------------------------------------------------- hash

#[test]
fn hash_is_deterministic_and_ignores_order_and_timestamps() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    let a = add_project(&db, "A", &folder(tmp.path(), "a"));
    add_project(&db, "B", &folder(tmp.path(), "b"));
    let ws = export(&db);
    let h = portable::content_hash(&ws);
    assert_eq!(h.len(), 64);
    assert_eq!(h, portable::content_hash(&ws));

    let mut shuffled = ws.clone();
    shuffled.projects.reverse();
    shuffled.prompts.reverse();
    assert_eq!(portable::content_hash(&shuffled), h);

    let mut stamped = ws.clone();
    stamped.projects[0].updated_at = "2030-01-01T00:00:00Z".into();
    stamped.projects[0].created_at = "2020-01-01T00:00:00Z".into();
    assert_eq!(portable::content_hash(&stamped), h);

    // Uma edição real muda o hash; só salvar de novo não muda.
    let before = hash(&db);
    let project = db.lock().unwrap().project(&a).unwrap();
    let same = ProjectInput {
        name: project.name.clone(),
        description: project.description.clone(),
        local_path: String::new(),
        repository: project.repository.clone(),
        stack: project.stack.clone(),
        tags: project.tags.clone(),
        ports: project.ports.clone(),
        commands: project.commands.clone(),
    };
    db.lock().unwrap().save(Some(&a), same).unwrap();
    assert_eq!(hash(&db), before);
    let mut edited = ws.clone();
    edited.projects[0].description = "outra".into();
    assert_ne!(portable::content_hash(&edited), h);
    let mut tagged = ws.clone();
    tagged.preferences.density = "compact".into();
    assert_ne!(portable::content_hash(&tagged), h);
}
#[test]
fn local_paths_never_reach_the_hash_or_the_export() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    let marker = folder(tmp.path(), "marcador_unico_xyz");
    add_project(&db, "A", &marker);
    let json = serde_json::to_string(&export(&db)).unwrap();
    assert!(!json.contains("marcador_unico_xyz") && !json.contains("localPath"));
}

// ---------------------------------------------------------------- decisão

#[test]
fn local_status_tracks_the_base_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    assert_eq!(sync::local_status(&db).unwrap().state, SyncState::Clean); // máquina nova
    add_project(&db, "A", &folder(tmp.path(), "a"));
    assert_eq!(
        sync::local_status(&db).unwrap().state,
        SyncState::LocalDirty
    );
    let current = hash(&db);
    db.lock().unwrap().mark_synced(&current).unwrap();
    assert_eq!(sync::local_status(&db).unwrap().state, SyncState::Clean);
    add_project(&db, "B", &folder(tmp.path(), "b"));
    assert_eq!(
        sync::local_status(&db).unwrap().state,
        SyncState::LocalDirty
    );
}
#[test]
fn clean_when_local_and_remote_agree_and_base_is_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    add_project(&db, "A", &folder(tmp.path(), "a"));
    let bridge = Fake::new(Ok(remote_ok(&export(&db))));
    let status = sync::sync(&db, &bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Clean);
    assert!(!status.pushed && !status.applied);
    assert!(bridge.pushed.borrow().is_empty());
    let current = hash(&db);
    assert_eq!(
        db.lock().unwrap().sync_meta().unwrap().base_hash,
        Some(current)
    );
}
#[test]
fn status_reports_remote_changed_local_dirty_and_diverged_without_acting() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    add_project(&db, "A", &folder(tmp.path(), "a"));
    let base = export(&db);
    db.lock()
        .unwrap()
        .mark_synced(&portable::content_hash(&base))
        .unwrap();

    let mut remote = base.clone();
    remote.projects[0].description = "mudou no remoto".into();
    let b = Fake::new(Ok(remote_ok(&remote)));
    assert_eq!(
        sync::status(&db, &b).unwrap().state,
        SyncState::RemoteChanged
    );

    add_project(&db, "B", &folder(tmp.path(), "b"));
    let b = Fake::new(Ok(remote_ok(&base)));
    assert_eq!(sync::status(&db, &b).unwrap().state, SyncState::LocalDirty);
    let b = Fake::new(Ok(remote_ok(&remote)));
    let status = sync::status(&db, &b).unwrap();
    assert_eq!(status.state, SyncState::Diverged);
    assert!(status.message.contains("Nada foi sobrescrito"));
    assert!(b.pushed.borrow().is_empty());
}

// ---------------------------------------------------------------- sync up / down

#[test]
fn sync_up_publishes_only_after_the_bridge_confirms_and_then_records_base() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    add_project(&db, "A", &folder(tmp.path(), "a"));
    // Remoto sem o arquivo: publicar.
    let failing = Fake::new(Ok(remote_with(RemoteFile::Missing)));
    failing.push.borrow_mut().push(Err(rejected("AUTH")));
    let status = sync::sync(&db, &failing, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Error);
    assert_eq!(status.code.as_deref(), Some("AUTH"));
    // Falha do Git: NÃO conta como sincronizado.
    assert_eq!(db.lock().unwrap().sync_meta().unwrap().base_hash, None);
    assert_eq!(
        sync::local_status(&db).unwrap().state,
        SyncState::LocalDirty
    );

    let ok = Fake::new(Ok(remote_with(RemoteFile::Missing))).push_ok();
    let status = sync::sync(&db, &ok, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Clean);
    assert!(status.pushed);
    assert_eq!(ok.pushed.borrow()[0], export(&db));
    let meta = db.lock().unwrap().sync_meta().unwrap();
    assert_eq!(meta.base_hash, Some(hash(&db)));
    assert!(meta.last_synced_at.is_some());
    assert_eq!(meta.last_applied_hash, None); // nada foi aplicado
}
#[test]
fn sync_down_applies_remote_keeps_bindings_and_records_applied_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let a = machine(tmp.path(), "a");
    let pid = add_project(&a, "Projeto", &folder(tmp.path(), "a-dir"));
    let b = machine(tmp.path(), "b");
    let b_dir = repo_dir(tmp.path(), "b-dir");
    // B já conhece o projeto e o tem vinculado a OUTRA pasta.
    b.lock().unwrap().apply_portable(&export(&a)).unwrap();
    b.lock()
        .unwrap()
        .bind(&pid, b_dir.to_str().unwrap(), true)
        .unwrap();
    let current = hash(&b);
    b.lock().unwrap().mark_synced(&current).unwrap();

    // A muda e publica.
    let mut remote = export(&a);
    remote.projects[0].name = "Projeto renomeado".into();
    remote.preferences.sidebar_compact = true;
    remote.preferences.prompt_favorites = vec!["audit".into()];
    let bridge = Fake::new(Ok(remote_ok(&remote)));
    let status = sync::sync(&b, &bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Clean);
    assert!(status.applied && !status.pushed);
    assert!(bridge.pushed.borrow().is_empty());
    assert_eq!(
        status.preferences.unwrap().prompt_favorites,
        vec!["audit".to_string()]
    );

    let project = b.lock().unwrap().project(&pid).unwrap();
    assert_eq!(project.name, "Projeto renomeado");
    assert!(!project.local_path.is_empty() && project.local_path.contains("b-dir"));
    assert_eq!(projects::location(&project), Location::Available);
    let meta = b.lock().unwrap().sync_meta().unwrap();
    assert_eq!(
        meta.last_applied_hash,
        Some(portable::content_hash(&remote))
    );
    assert_eq!(meta.base_hash, meta.last_applied_hash);
    assert_eq!(hash(&b), portable::content_hash(&remote)); // ponto fixo: sem divergência fantasma
    assert!(b.lock().unwrap().preferences().unwrap().sidebar_compact);
}
#[test]
fn new_machine_adopts_remote_and_projects_arrive_unbound() {
    let tmp = tempfile::tempdir().unwrap();
    let a = machine(tmp.path(), "a");
    add_project(&a, "P", &folder(tmp.path(), "a-dir"));
    let b = machine(tmp.path(), "b");
    let bridge = Fake::new(Ok(remote_ok(&export(&a))));
    let status = sync::sync(&b, &bridge, None, &idle()).unwrap();
    assert!(status.applied);
    let project = b.lock().unwrap().projects().unwrap().remove(0);
    assert_eq!(projects::location(&project), Location::Unbound);
    assert_eq!(hash(&b), hash(&a));
}
#[test]
fn divergence_never_overwrites_and_needs_an_explicit_choice() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    add_project(&db, "A", &folder(tmp.path(), "a"));
    let base = export(&db);
    db.lock()
        .unwrap()
        .mark_synced(&portable::content_hash(&base))
        .unwrap();
    add_project(&db, "Local novo", &folder(tmp.path(), "b"));
    let mut remote = base.clone();
    remote.projects[0].description = "remoto".into();

    let bridge = Fake::new(Ok(remote_ok(&remote))).push_ok();
    let before = export(&db);
    let status = sync::sync(&db, &bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Diverged);
    assert!(bridge.pushed.borrow().is_empty());
    assert_eq!(export(&db), before);

    // "Usar o remoto" aplica; "manter o local" publica — sempre por escolha explícita.
    let took = sync::sync(&db, &bridge, Some(Resolution::TakeRemote), &idle()).unwrap();
    assert!(took.applied);
    assert_eq!(hash(&db), portable::content_hash(&remote));

    let db2 = machine(tmp.path(), "c");
    add_project(&db2, "X", &folder(tmp.path(), "x"));
    db2.lock().unwrap().mark_synced("hash-antigo").unwrap();
    let bridge = Fake::new(Ok(remote_ok(&remote))).push_ok();
    let kept = sync::sync(&db2, &bridge, Some(Resolution::KeepLocal), &idle()).unwrap();
    assert!(kept.pushed);
    assert_eq!(bridge.pushed.borrow()[0], export(&db2));
}
#[test]
fn edit_during_sync_is_not_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let a = machine(tmp.path(), "a");
    add_project(&a, "P", &folder(tmp.path(), "a-dir"));
    let b = machine(tmp.path(), "b");
    let bridge = Fake::new(Ok(remote_ok(&export(&a))));
    let b_ref: &'static Mutex<Database> = Box::leak(Box::new(b));
    let dir = folder(tmp.path(), "novo");
    *bridge.on_remote.borrow_mut() = Some(Box::new(move || {
        b_ref
            .lock()
            .unwrap()
            .save(None, input("Digitado agora", &dir, ""))
            .unwrap();
    }));
    // O hook edita DEPOIS de o sync ler o local e antes de decidir/aplicar.
    let status = sync::sync(b_ref, &bridge, None, &idle()).unwrap();
    // Máquina nova + edição concorrente: nunca deve perder o projeto digitado.
    let names: Vec<_> = b_ref
        .lock()
        .unwrap()
        .projects()
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(names.contains(&"Digitado agora".to_string()), "{status:?}");
}

// ---------------------------------------------------------------- erros

#[test]
fn bridge_down_and_git_offline_keep_the_app_working() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    add_project(&db, "A", &folder(tmp.path(), "a"));
    let down = Fake::new(Err(BridgeError::Unavailable("recusado".into())));
    let status = sync::sync(&db, &down, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Offline);
    assert_eq!(status.code.as_deref(), Some("BRIDGE_DOWN"));
    assert!(status.message.contains("npm run lab"));

    let mut offline = remote_with(RemoteFile::Missing);
    offline.fetched = false;
    offline.fetch_error = Some(("NETWORK".into(), "x".into()));
    let status = sync::sync(&db, &Fake::new(Ok(offline)), None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Offline);
    assert_eq!(status.code.as_deref(), Some("NETWORK"));
    assert_eq!(db.lock().unwrap().sync_meta().unwrap().base_hash, None);
    assert_eq!(db.lock().unwrap().projects().unwrap().len(), 1);
    // Sem bridge de verdade: a porta fechada também é "indisponível", sem pânico.
    let closed = HttpBridge::new(1);
    assert!(matches!(closed.remote(), Err(BridgeError::Unavailable(_))));
}
#[test]
fn invalid_or_future_remote_is_never_applied_or_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    add_project(&db, "A", &folder(tmp.path(), "a"));
    let before = export(&db);
    for file in [
        RemoteFile::Invalid("quebrado".into()),
        RemoteFile::Newer("versão futura".into()),
    ] {
        let bridge = Fake::new(Ok(remote_with(file))).push_ok();
        let status = sync::sync(&db, &bridge, None, &idle()).unwrap();
        assert_eq!(status.state, SyncState::Error);
        assert!(bridge.pushed.borrow().is_empty());
        assert_eq!(export(&db), before);
    }
    // Schema válido na forma, mas inseguro/inconsistente: validação Rust recusa.
    let mut hostile = before.clone();
    hostile.projects[0]
        .commands
        .push(hub_core::models::ProjectCommand {
            name: "x".into(),
            program: r"C:\Windows\System32\cmd.exe".into(),
            args: vec![],
        });
    let bridge = Fake::new(Ok(remote_ok(&hostile)));
    let status = sync::sync(&db, &bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Error);
    assert!(!status.applied);
    assert_eq!(export(&db), before);
    let mut future = before.clone();
    future.version = 99;
    let status = sync::sync(&db, &Fake::new(Ok(remote_ok(&future))), None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Error);
}
#[test]
fn applied_hash_only_advances_when_apply_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    add_project(&db, "A", &folder(tmp.path(), "a"));
    let mut bad = export(&db);
    bad.prompts[0].project_id = Some("fantasma".into());
    let before = db.lock().unwrap().sync_meta().unwrap();
    assert!(db.lock().unwrap().apply_synced(&bad, "novo-hash").is_err());
    assert_eq!(db.lock().unwrap().sync_meta().unwrap(), before);
}
#[test]
fn push_retries_after_a_strict_update_only_when_the_remote_file_did_not_move() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    add_project(&db, "A", &folder(tmp.path(), "a"));
    let base = export(&db);
    db.lock()
        .unwrap()
        .mark_synced(&portable::content_hash(&base))
        .unwrap();
    add_project(&db, "B", &folder(tmp.path(), "b"));

    let bridge = Fake::new(Ok(remote_ok(&base)));
    bridge.push.borrow_mut().push(Err(rejected("REMOTE_AHEAD")));
    bridge.push.borrow_mut().push(Ok(PushOutcome {
        pushed: true,
        commit: None,
    }));
    let status = sync::sync(&db, &bridge, None, &idle()).unwrap();
    assert!(status.pushed);
    assert_eq!(*bridge.updates.borrow(), 1);

    // Se o update recusar (código em desenvolvimento), nada é publicado nem marcado.
    let db = machine(tmp.path(), "c");
    add_project(&db, "A", &folder(tmp.path(), "c1"));
    let base = export(&db);
    db.lock()
        .unwrap()
        .mark_synced(&portable::content_hash(&base))
        .unwrap();
    add_project(&db, "B", &folder(tmp.path(), "c2"));
    let bridge = Fake::new(Ok(remote_ok(&base)));
    bridge.push.borrow_mut().push(Err(rejected("REMOTE_AHEAD")));
    *bridge.update.borrow_mut() = Err(rejected("LOCAL_CHANGES"));
    let status = sync::sync(&db, &bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Error);
    assert_eq!(status.code.as_deref(), Some("LOCAL_CHANGES"));
    assert_eq!(bridge.pushed.borrow().len(), 1);
    assert_eq!(
        sync::local_status(&db).unwrap().state,
        SyncState::LocalDirty
    );
}
#[test]
fn concurrent_syncs_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    let running = AtomicBool::new(true);
    let bridge = Fake::new(Ok(remote_with(RemoteFile::Missing)));
    assert!(sync::sync(&db, &bridge, None, &running).is_err());
}
#[test]
fn preferences_round_trip_and_are_normalized() {
    let tmp = tempfile::tempdir().unwrap();
    let db = machine(tmp.path(), "a");
    let saved = db
        .lock()
        .unwrap()
        .save_preferences(PortablePreferences {
            sidebar_compact: true,
            density: "inventada".into(),
            prompt_favorites: vec!["b".into(), "a".into(), "a".into(), "../x".into()],
        })
        .unwrap();
    assert_eq!(saved.density, "comfortable");
    assert_eq!(
        saved.prompt_favorites,
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(export(&db).preferences, saved);
    let before = sync::local_status(&db).unwrap();
    assert_eq!(before.state, SyncState::LocalDirty); // preferência é dado portátil
}

// ---------------------------------------------------------------- ponta a ponta (bridge real)

fn git_env() -> Vec<(&'static str, String)> {
    vec![
        ("GIT_AUTHOR_NAME", "LKR Test".into()),
        ("GIT_AUTHOR_EMAIL", "lkr@example.invalid".into()),
        ("GIT_COMMITTER_NAME", "LKR Test".into()),
        ("GIT_COMMITTER_EMAIL", "lkr@example.invalid".into()),
        ("GIT_CONFIG_NOSYSTEM", "1".into()),
        (
            "GIT_CONFIG_GLOBAL",
            std::env::temp_dir()
                .join("lkr-empty.gitconfig")
                .to_string_lossy()
                .into(),
        ),
        ("GIT_TERMINAL_PROMPT", "0".into()),
    ]
}
fn git(dir: &Path, args: &[&str]) -> String {
    std::fs::write(std::env::temp_dir().join("lkr-empty.gitconfig"), "").unwrap();
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .envs(git_env())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
/// Pasta de repositório Git com o remote origin que os projetos de teste esperam (SSH ≡ HTTPS).
fn repo_dir(root: &Path, name: &str) -> PathBuf {
    let dir = folder(root, name);
    git(&dir, &["init", "--quiet"]);
    git(
        &dir,
        &["remote", "add", "origin", "git@github.com:org/lkr.git"],
    );
    dir
}
fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}
struct Lab {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    remote: PathBuf,
}
impl Lab {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        // Sem o prefixo verbatim do Windows: o Git e o bridge comparam caminhos comuns.
        let canonical = std::fs::canonicalize(tmp.path()).unwrap();
        let root = PathBuf::from(
            canonical
                .to_string_lossy()
                .strip_prefix(r"\\?\")
                .unwrap_or(&canonical.to_string_lossy())
                .to_string(),
        );
        let remote = root.join("remote.git");
        git(
            &root,
            &[
                "init",
                "--quiet",
                "--bare",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ],
        );
        let seed = root.join("seed");
        git(
            &root,
            &["init", "--quiet", "-b", "main", seed.to_str().unwrap()],
        );
        std::fs::write(seed.join("README.md"), "# projeto\n").unwrap();
        git(&seed, &["add", "README.md"]);
        git(&seed, &["commit", "--quiet", "-m", "init"]);
        git(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&seed, &["push", "--quiet", "-u", "origin", "main"]);
        Self {
            _tmp: tmp,
            root,
            remote,
        }
    }
    fn clone_as(&self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        git(
            &self.root,
            &[
                "clone",
                "--quiet",
                self.remote.to_str().unwrap(),
                dir.to_str().unwrap(),
            ],
        );
        dir
    }
}
struct Running {
    child: Child,
    bridge: HttpBridge,
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn start_bridge(repo: &Path) -> Running {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lkr-lab/bridge/e2e-server.mjs");
    let mut child = Command::new("node")
        .arg(script)
        .arg(repo)
        .envs(git_env())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port: u16 = line.trim().strip_prefix("PORT ").unwrap().parse().unwrap();
    Running {
        child,
        bridge: HttpBridge::new(port),
    }
}

#[test]
fn e2e_a_publishes_b_receives_ids_equal_paths_stay_local() {
    if !node_available() {
        return;
    }
    let lab = Lab::new();
    let (a_repo, b_repo) = (lab.clone_as("a"), lab.clone_as("b"));
    let (bridge_a, bridge_b) = (start_bridge(&a_repo), start_bridge(&b_repo));

    let a = machine(&lab.root, "a");
    let a_dir = folder(&lab.root, "marcador_pc_a");
    let id = add_project(&a, "LKR_Lab", &a_dir);
    let status = sync::sync(&a, &bridge_a.bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Clean, "{status:?}");
    assert!(status.pushed);

    // Só data/workspace.json foi publicado, sem caminho nenhum da máquina A.
    let files = git(
        &lab.remote,
        &["log", "-1", "--name-only", "--format=", "main"],
    );
    assert_eq!(files, "data/workspace.json");
    let published = git(&lab.remote, &["show", "main:data/workspace.json"]);
    assert!(!published.contains("marcador_pc_a") && !published.contains("localPath"));

    // B: máquina nova.
    let b = machine(&lab.root, "b");
    let status = sync::sync(&b, &bridge_b.bridge, None, &idle()).unwrap();
    assert!(status.applied, "{status:?}");
    let project = b.lock().unwrap().project(&id).unwrap();
    assert_eq!(projects::location(&project), Location::Unbound);
    assert_eq!(hash(&a), hash(&b));
    // B localiza a pasta dela: binding local, nada muda no hash.
    let b_dir = repo_dir(&lab.root, "pc_b_dir");
    b.lock()
        .unwrap()
        .bind(&id, b_dir.to_str().unwrap(), true)
        .unwrap();
    assert_eq!(hash(&a), hash(&b));
    assert_eq!(
        sync::status(&b, &bridge_b.bridge).unwrap().state,
        SyncState::Clean
    );

    // A edita e publica; B recebe e continua vinculado.
    let mut edit = input("LKR_Lab 2", &a_dir, "https://github.com/org/lkr");
    edit.local_path = String::new();
    a.lock().unwrap().save(Some(&id), edit).unwrap();
    assert!(
        sync::sync(&a, &bridge_a.bridge, None, &idle())
            .unwrap()
            .pushed
    );
    assert_eq!(
        sync::status(&b, &bridge_b.bridge).unwrap().state,
        SyncState::RemoteChanged
    );
    assert!(
        sync::sync(&b, &bridge_b.bridge, None, &idle())
            .unwrap()
            .applied
    );
    let project = b.lock().unwrap().project(&id).unwrap();
    assert_eq!(project.name, "LKR_Lab 2");
    assert_eq!(projects::location(&project), Location::Available);
    assert_eq!(hash(&a), hash(&b));

    // Segundo sync sem mudanças: nada de commit novo.
    let head = git(&lab.remote, &["rev-parse", "main"]);
    assert_eq!(
        sync::sync(&a, &bridge_a.bridge, None, &idle())
            .unwrap()
            .state,
        SyncState::Clean
    );
    assert_eq!(git(&lab.remote, &["rev-parse", "main"]), head);
}
#[test]
fn e2e_both_machines_change_is_a_divergence_and_remote_is_untouched() {
    if !node_available() {
        return;
    }
    let lab = Lab::new();
    let (a_repo, b_repo) = (lab.clone_as("a"), lab.clone_as("b"));
    let (bridge_a, bridge_b) = (start_bridge(&a_repo), start_bridge(&b_repo));
    let a = machine(&lab.root, "a");
    let id = add_project(&a, "Base", &folder(&lab.root, "a-dir"));
    sync::sync(&a, &bridge_a.bridge, None, &idle()).unwrap();
    let b = machine(&lab.root, "b");
    sync::sync(&b, &bridge_b.bridge, None, &idle()).unwrap();

    let edit = |db: &Mutex<Database>, description: &str| {
        let mut p = input("Base", Path::new("."), "https://github.com/org/lkr");
        p.local_path = String::new();
        p.description = description.into();
        db.lock().unwrap().save(Some(&id), p).unwrap();
    };
    edit(&a, "versão de A");
    edit(&b, "versão de B");
    assert!(
        sync::sync(&a, &bridge_a.bridge, None, &idle())
            .unwrap()
            .pushed
    );
    let remote_head = git(&lab.remote, &["rev-parse", "main"]);

    let before = export(&b);
    let status = sync::sync(&b, &bridge_b.bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Diverged, "{status:?}");
    assert_eq!(export(&b), before, "o local de B não pode ser sobrescrito");
    assert_eq!(
        git(&lab.remote, &["rev-parse", "main"]),
        remote_head,
        "o remoto não pode ser sobrescrito"
    );
    assert!(git(&lab.remote, &["show", "main:data/workspace.json"]).contains("versão de A"));
}
#[test]
fn e2e_sync_never_commits_developer_changes_and_survives_unrelated_remote_commits() {
    if !node_available() {
        return;
    }
    let lab = Lab::new();
    let a_repo = lab.clone_as("a");
    let other = lab.clone_as("other");
    let bridge = start_bridge(&a_repo);

    // Trabalho em andamento do desenvolvedor no repositório do sync.
    std::fs::write(a_repo.join("README.md"), "# projeto\nwip\n").unwrap();
    std::fs::write(a_repo.join("staged.txt"), "no index\n").unwrap();
    git(&a_repo, &["add", "staged.txt"]);
    std::fs::write(a_repo.join("scratch.txt"), "novo\n").unwrap();

    let a = machine(&lab.root, "a");
    add_project(&a, "P", &folder(&lab.root, "p"));
    let status = sync::sync(&a, &bridge.bridge, None, &idle()).unwrap();
    assert!(status.pushed, "{status:?}");
    assert_eq!(
        git(
            &lab.remote,
            &["log", "-1", "--name-only", "--format=", "main"]
        ),
        "data/workspace.json"
    );
    assert_eq!(
        git(&a_repo, &["diff", "--cached", "--name-only"]),
        "staged.txt"
    );
    assert!(git(&a_repo, &["status", "--porcelain", "README.md"]).contains("M README.md"));
    assert!(a_repo.join("scratch.txt").exists());

    // Alguém empurra código: o remoto anda por outros arquivos. Com a árvore suja o
    // sync NÃO faz fast-forward na árvore do desenvolvedor.
    git(&other, &["pull", "--quiet", "--ff-only"]);
    std::fs::write(other.join("code.rs"), "fn main() {}\n").unwrap();
    git(&other, &["add", "code.rs"]);
    git(&other, &["commit", "--quiet", "-m", "feat: code"]);
    git(&other, &["push", "--quiet", "origin", "main"]);
    add_project(&a, "Q", &folder(&lab.root, "q"));
    let head = git(&lab.remote, &["rev-parse", "main"]);
    let status = sync::sync(&a, &bridge.bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Error, "{status:?}");
    assert_eq!(status.code.as_deref(), Some("LOCAL_CHANGES"));
    assert_eq!(git(&lab.remote, &["rev-parse", "main"]), head);
    assert_eq!(sync::local_status(&a).unwrap().state, SyncState::LocalDirty);
    assert_eq!(
        std::fs::read_to_string(a_repo.join("README.md")).unwrap(),
        "# projeto\nwip\n"
    );

    // Árvore limpa: o fast-forward é seguro e o sync conclui.
    git(&a_repo, &["reset", "--quiet"]);
    std::fs::remove_file(a_repo.join("staged.txt")).unwrap();
    std::fs::remove_file(a_repo.join("scratch.txt")).unwrap();
    git(&a_repo, &["checkout", "--quiet", "--", "README.md"]);
    let status = sync::sync(&a, &bridge.bridge, None, &idle()).unwrap();
    assert!(status.pushed, "{status:?}");
    assert!(a_repo.join("code.rs").exists());
    assert_eq!(
        git(
            &lab.remote,
            &["log", "-1", "--name-only", "--format=", "main"]
        ),
        "data/workspace.json"
    );
}
#[test]
fn e2e_git_in_progress_states_block_the_sync() {
    if !node_available() {
        return;
    }
    let lab = Lab::new();
    let a_repo = lab.clone_as("a");
    let bridge = start_bridge(&a_repo);
    let a = machine(&lab.root, "a");
    add_project(&a, "P", &folder(&lab.root, "p"));
    let head = git(&lab.remote, &["rev-parse", "main"]);

    // Merge em andamento (marcador do próprio Git).
    let marker = a_repo.join(".git").join("MERGE_HEAD");
    std::fs::write(&marker, git(&a_repo, &["rev-parse", "HEAD"])).unwrap();
    let status = sync::sync(&a, &bridge.bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Error, "{status:?}");
    assert_eq!(status.code.as_deref(), Some("IN_PROGRESS"));
    std::fs::remove_file(&marker).unwrap();

    // Rebase em andamento.
    std::fs::create_dir(a_repo.join(".git").join("rebase-merge")).unwrap();
    let status = sync::sync(&a, &bridge.bridge, None, &idle()).unwrap();
    assert_eq!(status.code.as_deref(), Some("IN_PROGRESS"));
    std::fs::remove_dir(a_repo.join(".git").join("rebase-merge")).unwrap();

    // HEAD destacado.
    git(&a_repo, &["checkout", "--quiet", "--detach"]);
    let status = sync::sync(&a, &bridge.bridge, None, &idle()).unwrap();
    assert_eq!(status.code.as_deref(), Some("DETACHED"));
    assert_eq!(git(&lab.remote, &["rev-parse", "main"]), head);
    assert_eq!(sync::local_status(&a).unwrap().state, SyncState::LocalDirty);
}
#[test]
fn e2e_git_unreachable_remote_is_offline_and_local_stays_dirty() {
    if !node_available() {
        return;
    }
    let lab = Lab::new();
    let a_repo = lab.clone_as("a");
    let bridge = start_bridge(&a_repo);
    let a = machine(&lab.root, "a");
    add_project(&a, "P", &folder(&lab.root, "p"));
    git(
        &a_repo,
        &[
            "remote",
            "set-url",
            "origin",
            lab.root.join("nao-existe.git").to_str().unwrap(),
        ],
    );
    let status = sync::sync(&a, &bridge.bridge, None, &idle()).unwrap();
    assert_ne!(status.state, SyncState::Clean, "{status:?}");
    assert_eq!(db_base(&a), None);
    assert_eq!(sync::local_status(&a).unwrap().state, SyncState::LocalDirty);
    assert_eq!(a.lock().unwrap().projects().unwrap().len(), 1);
}
fn db_base(db: &Mutex<Database>) -> Option<String> {
    db.lock().unwrap().sync_meta().unwrap().base_hash
}
#[test]
fn e2e_corrupted_or_future_workspace_file_in_the_remote_is_not_adopted() {
    if !node_available() {
        return;
    }
    let lab = Lab::new();
    let writer = lab.clone_as("w");
    let reader = lab.clone_as("r");
    let bridge = start_bridge(&reader);
    std::fs::create_dir_all(writer.join("data")).unwrap();
    let write = |text: &str, msg: &str| {
        std::fs::write(writer.join("data/workspace.json"), text).unwrap();
        git(&writer, &["add", "data/workspace.json"]);
        git(&writer, &["commit", "--quiet", "-m", msg]);
        git(&writer, &["push", "--quiet", "origin", "main"]);
    };
    let b = machine(&lab.root, "b");
    add_project(&b, "Local", &folder(&lab.root, "p"));
    let before = export(&b);

    write("{quebrado", "corrompido");
    let status = sync::sync(&b, &bridge.bridge, None, &idle()).unwrap();
    assert_eq!(status.state, SyncState::Error);
    assert_eq!(status.code.as_deref(), Some("REMOTE_INVALID"));

    write(
        r#"{"schemaVersion":9,"source":"lkr-lab","module":"workspace","state":{"version":9}}"#,
        "futuro",
    );
    let status = sync::sync(&b, &bridge.bridge, None, &idle()).unwrap();
    assert_eq!(status.code.as_deref(), Some("REMOTE_NEWER"));
    assert_eq!(export(&b), before);
    assert!(!reader.join("data/workspace.json").exists());
}
#[test]
fn e2e_bridge_down_does_not_break_local_use() {
    let db = machine(tempfile::tempdir().unwrap().path(), "x");
    let status = sync::local_status(&db).unwrap();
    assert_eq!(status.state, SyncState::Clean);
    let closed = HttpBridge::new(1);
    let status = sync::status(&db, &closed).unwrap();
    assert_eq!(status.state, SyncState::Offline);
    assert_eq!(status.code.as_deref(), Some("BRIDGE_DOWN"));
}
#[test]
fn prompts_travel_and_secrets_stay_out() {
    let tmp = tempfile::tempdir().unwrap();
    let a = machine(tmp.path(), "a");
    a.lock()
        .unwrap()
        .save_prompt(Prompt {
            id: "mine".into(),
            title: "T".into(),
            category: "C".into(),
            project_id: None,
            body: "corpo".into(),
        })
        .unwrap();
    let ws = export(&a);
    let json = serde_json::to_string(&ws).unwrap();
    assert!(json.contains("corpo"));
    for forbidden in ["token", "password", "localPath", "pid", "cwd"] {
        assert!(
            !json
                .to_lowercase()
                .contains(&format!("\"{}\"", forbidden.to_lowercase())),
            "{forbidden}"
        );
    }
}

// ---- forma canônica compartilhada com lkr-workspace.js (mesmo arquivo nos dois testes) ----

#[test]
fn golden_workspace_is_a_fixed_point_of_the_rust_canonical_form() {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace-golden.json"),
    )
    .unwrap();
    let expected: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut ws: PortableWorkspace = serde_json::from_str(&text).unwrap();
    portable::validate(&ws).unwrap();
    portable::normalize(&mut ws);
    // Normalizar não muda nada e a serialização reproduz exatamente os mesmos campos.
    assert_eq!(serde_json::to_value(&ws).unwrap(), expected);
    // O hash é estável (muda só se a forma canônica mudar de propósito).
    assert_eq!(
        portable::content_hash(&ws),
        portable::content_hash(&ws.clone())
    );
}
