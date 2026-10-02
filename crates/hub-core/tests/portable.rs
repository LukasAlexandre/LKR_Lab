//! Workspace portátil: identidade != caminho local (docs/STATE.md).
use hub_core::{
    database::Database,
    models::{KnowledgeEntry, Location, Project, ProjectCommand, ProjectInput, Prompt},
    portable::{self, PortableWorkspace},
    projects, system,
};
use rusqlite::Connection;
use std::{path::Path, process::Command};

fn input(name: &str, path: &Path, repository: &str) -> ProjectInput {
    ProjectInput {
        name: name.into(),
        description: "d".into(),
        local_path: path.to_string_lossy().into(),
        repository: repository.into(),
        stack: vec!["Rust".into()],
        tags: vec!["t".into()],
        ports: vec![],
        commands: vec![],
    }
}
fn git(path: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(args)
        .current_dir(path)
        .status()
        .unwrap()
        .success());
}
/// Pasta com repositório Git cujo origin é `remote`.
fn repo(root: &Path, name: &str, remote: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["remote", "add", "origin", remote]);
    dir
}
fn open(root: &Path, file: &str) -> Database {
    Database::open(&root.join(file)).unwrap()
}
fn folder(root: &Path, name: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    std::fs::create_dir(&dir).unwrap();
    dir
}
fn project(id: &str, path: &str) -> Project {
    Project {
        id: id.into(),
        name: id.into(),
        slug: id.into(),
        description: String::new(),
        local_path: path.into(),
        repository: String::new(),
        stack: vec![],
        tags: vec![],
        ports: vec![],
        commands: vec![],
        created_at: String::new(),
        updated_at: String::new(),
    }
}

// ---- system.rs: cwd.starts_with("") ----

#[test]
fn unbound_project_never_owns_a_process() {
    let cwd = Path::new(if cfg!(windows) {
        r"C:\qualquer\coisa"
    } else {
        "/qualquer/coisa"
    });
    assert!(system::owner_of(cwd, &[project("a", "")]).is_none());
}
#[test]
fn bound_project_owns_descendant_cwd_and_most_specific_wins() {
    let (outer, inner, cwd) = if cfg!(windows) {
        (r"C:\dev\app", r"C:\dev\app\sub", r"C:\dev\app\sub\src")
    } else {
        ("/dev/app", "/dev/app/sub", "/dev/app/sub/src")
    };
    let projects = [
        project("unbound", ""),
        project("outer", outer),
        project("inner", inner),
    ];
    assert_eq!(
        system::owner_of(Path::new(cwd), &projects).unwrap().id,
        "inner"
    );
    assert_eq!(
        system::owner_of(Path::new(outer), &projects).unwrap().id,
        "outer"
    );
}
#[test]
fn other_project_directory_is_not_a_false_positive() {
    let (dir, sibling) = if cfg!(windows) {
        (r"C:\dev\app", r"C:\dev\app-two\src")
    } else {
        ("/dev/app", "/dev/app-two/src")
    };
    assert!(system::owner_of(Path::new(sibling), &[project("a", dir)]).is_none());
}

// ---- remote identity ----

#[test]
fn https_and_ssh_forms_are_the_same_repository() {
    for same in [
        "https://github.com/Org/Repo.git",
        "https://github.com/org/repo",
        "git@github.com:org/repo.git",
        "ssh://git@github.com/org/repo.git",
        "https://user:tok@github.com/org/repo/",
    ] {
        assert_eq!(
            portable::remote_identity(same),
            "github.com/org/repo",
            "{same}"
        );
    }
    assert!(portable::same_repository(
        "git@github.com:org/repo.git",
        "https://github.com/org/repo"
    ));
}
#[test]
fn different_repositories_are_never_the_same() {
    let repo = "https://github.com/org/repo";
    assert!(!portable::same_repository(
        repo,
        "https://github.com/org/repo2"
    ));
    assert!(!portable::same_repository(
        repo,
        "https://github.com/other/repo"
    ));
    assert!(!portable::same_repository(
        repo,
        "https://gitlab.com/org/repo"
    ));
    assert!(!portable::same_repository("", ""));
    assert!(!portable::same_repository("", repo));
}

// ---- export / apply ----

#[test]
fn export_has_no_machine_data_and_keeps_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "marcador_local_unico");
    let mut db = open(tmp.path(), "a.db");
    let p = db
        .save(None, input("LKR_Lab", &dir, "https://github.com/org/lkr"))
        .unwrap();
    let ws = db.export_portable().unwrap();
    let json = serde_json::to_string(&ws).unwrap();
    assert!(!json.contains("localPath") && !json.contains("local_path"));
    assert!(!json.contains("marcador_local_unico"));
    assert!(!json.contains("pathAvailable") && !json.contains("location"));
    assert_eq!(ws.version, portable::SCHEMA_VERSION);
    assert_eq!(ws.projects[0].id, p.id);
    assert_eq!(ws.prompts.len(), 6);
}
#[test]
fn prompts_and_knowledge_metadata_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "p");
    let mut a = open(tmp.path(), "a.db");
    let p = a.save(None, input("P", &dir, "")).unwrap();
    a.save_prompt(Prompt {
        id: "mine".into(),
        title: "T".into(),
        category: "C".into(),
        project_id: Some(p.id.clone()),
        body: "corpo".into(),
    })
    .unwrap();
    a.save_knowledge(KnowledgeEntry {
        id: "k1".into(),
        project_id: Some(p.id.clone()),
        title: "K".into(),
        kind: "note".into(),
        body: "b".into(),
        tags: "x".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    })
    .unwrap();
    let ws = a.export_portable().unwrap();
    assert_eq!(ws.knowledge.len(), 1);

    let mut b = open(tmp.path(), "b.db");
    b.apply_portable(&ws).unwrap();
    let again = b.export_portable().unwrap();
    let prompt = again.prompts.iter().find(|x| x.id == "mine").unwrap();
    assert_eq!(prompt.project_id.as_deref(), Some(p.id.as_str()));
    assert_eq!(prompt.body, "corpo");
    assert_eq!(again.knowledge, ws.knowledge);
    assert_eq!(again.projects[0].id, p.id);
}
#[test]
fn round_trip_state_export_apply_effective_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "p");
    let mut a = open(tmp.path(), "a.db");
    let p = a
        .save(None, input("P", &dir, "https://github.com/org/p"))
        .unwrap();
    let ws = a.export_portable().unwrap();
    let mut b = open(tmp.path(), "b.db");
    b.apply_portable(&ws).unwrap();
    assert_eq!(b.export_portable().unwrap(), ws);
    let entry = projects::entry(b.project(&p.id).unwrap());
    assert_eq!(entry.location, Location::Unbound);
    assert_eq!(entry.project.id, p.id);
    assert_eq!(entry.project.repository, "https://github.com/org/p");
}
#[test]
fn apply_preserves_local_bindings() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "p");
    let mut db = open(tmp.path(), "a.db");
    let p = db.save(None, input("Antigo", &dir, "")).unwrap();
    let mut ws = db.export_portable().unwrap();
    ws.projects[0].name = "Novo nome".into();
    db.apply_portable(&ws).unwrap();
    let after = db.project(&p.id).unwrap();
    assert_eq!(after.name, "Novo nome");
    assert_eq!(after.local_path, p.local_path);
    assert_eq!(projects::location(&after), Location::Available);
}
#[test]
fn apply_removes_projects_absent_from_workspace_but_never_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "p");
    let mut db = open(tmp.path(), "a.db");
    let p = db.save(None, input("P", &dir, "")).unwrap();
    let mut ws = db.export_portable().unwrap();
    ws.projects.clear();
    let summary = db.apply_portable(&ws).unwrap();
    assert_eq!(summary.removed_projects, 1);
    assert!(db.project(&p.id).is_err());
    assert!(dir.is_dir());
    let bindings: i64 = db
        .conn
        .query_row("SELECT count(*) FROM project_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(bindings, 0);
}
#[test]
fn invalid_workspaces_are_rejected_without_touching_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "p");
    let mut db = open(tmp.path(), "a.db");
    db.save(None, input("P", &dir, "")).unwrap();
    let good = db.export_portable().unwrap();
    let before = db.export_portable().unwrap();
    let mutate = |f: &dyn Fn(&mut PortableWorkspace)| {
        let mut ws = good.clone();
        f(&mut ws);
        ws
    };
    let bad = [
        mutate(&|w| w.version = 99),
        mutate(&|w| w.version = 0),
        mutate(&|w| w.projects[0].id = "../x".into()),
        mutate(&|w| w.projects.push(w.projects[0].clone())),
        mutate(&|w| w.projects[0].repository = "https://u:p@github.com/a/b".into()),
        mutate(&|w| w.projects[0].repository = "git@github.com:a/b.git".into()),
        mutate(&|w| w.prompts[0].project_id = Some("fantasma".into())),
        mutate(&|w| {
            w.knowledge.push(KnowledgeEntry {
                id: "k".into(),
                project_id: None,
                title: "t".into(),
                kind: "malware".into(),
                body: "b".into(),
                tags: String::new(),
                updated_at: String::new(),
            })
        }),
        mutate(&|w| {
            w.projects[0].commands.push(ProjectCommand {
                name: "x".into(),
                program: r"C:\Windows\cmd.exe".into(),
                args: vec![],
            })
        }),
    ];
    for ws in &bad {
        assert!(db.apply_portable(ws).is_err());
    }
    assert_eq!(db.export_portable().unwrap(), before);
    assert!(serde_json::from_str::<PortableWorkspace>("{ nao é json").is_err());
}

// ---- bind ----

#[test]
fn bind_requires_matching_repository_and_keeps_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = repo(tmp.path(), "a", "https://github.com/org/x.git");
    let mut a = open(tmp.path(), "a.db");
    let p = a
        .save(None, input("X", &origin, "https://github.com/org/x"))
        .unwrap();
    let ws = a.export_portable().unwrap();

    // Máquina B: mesmo projeto, outra pasta, remote em SSH.
    let mut b = open(tmp.path(), "b.db");
    b.apply_portable(&ws).unwrap();
    assert_eq!(
        projects::location(&b.project(&p.id).unwrap()),
        Location::Unbound
    );
    assert!(projects::local_dir(&b.project(&p.id).unwrap()).is_err());

    let other = repo(tmp.path(), "other", "git@github.com:org/outro.git");
    let err = b.bind(&p.id, other.to_str().unwrap(), true).unwrap_err();
    assert!(err.contains("outro repositório"), "{err}");
    assert_eq!(
        projects::location(&b.project(&p.id).unwrap()),
        Location::Unbound
    );

    let none = folder(tmp.path(), "sem-git");
    assert!(b.bind(&p.id, none.to_str().unwrap(), true).is_err());

    let mine = repo(tmp.path(), "b", "git@github.com:org/x.git");
    let result = b.bind(&p.id, mine.to_str().unwrap(), false).unwrap();
    assert!(result.bound);
    let bound = b.project(&p.id).unwrap();
    assert_eq!(bound.id, p.id);
    assert_eq!(projects::location(&bound), Location::Available);
    assert_ne!(bound.local_path, p.local_path); // dois caminhos, mesma identidade
    assert_eq!(b.export_portable().unwrap(), ws); // o portátil não mudou
}
#[test]
fn bind_without_repository_needs_confirmation_and_refuses_invalid_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "p");
    let mut db = open(tmp.path(), "a.db");
    let p = db.save(None, input("P", &dir, "")).unwrap();
    let ws = db.export_portable().unwrap();
    let mut b = open(tmp.path(), "b.db");
    b.apply_portable(&ws).unwrap();
    let target = folder(tmp.path(), "t");
    let first = b.bind(&p.id, target.to_str().unwrap(), false).unwrap();
    assert!(!first.bound && first.needs_confirmation);
    assert_eq!(
        projects::location(&b.project(&p.id).unwrap()),
        Location::Unbound
    );
    assert!(b.bind(&p.id, target.to_str().unwrap(), true).unwrap().bound);
    assert!(b.bind(&p.id, "relativo", true).is_err());
    let missing = tmp.path().join("nao-existe");
    assert!(b.bind(&p.id, missing.to_str().unwrap(), true).is_err());
    assert!(b.bind("fantasma", target.to_str().unwrap(), true).is_err());
}
#[test]
fn one_folder_cannot_belong_to_two_projects() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "p");
    let mut db = open(tmp.path(), "a.db");
    let one = db.save(None, input("Um", &dir, "")).unwrap();
    let mut ws = db.export_portable().unwrap();
    let mut second = ws.projects[0].clone();
    second.id = "segundo".into();
    second.name = "Dois".into();
    ws.projects.push(second);
    db.apply_portable(&ws).unwrap();
    assert!(db.bind("segundo", dir.to_str().unwrap(), true).is_err());
    assert_eq!(db.project(&one.id).unwrap().local_path, one.local_path);
    assert_eq!(
        projects::location(&db.project("segundo").unwrap()),
        Location::Unbound
    );
}
#[test]
fn missing_binding_is_reported_and_blocks_local_operations() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "p");
    let mut db = open(tmp.path(), "a.db");
    let p = db.save(None, input("P", &dir, "")).unwrap();
    std::fs::remove_dir(&dir).unwrap();
    let after = db.project(&p.id).unwrap();
    assert_eq!(projects::location(&after), Location::Missing);
    assert!(projects::local_dir(&after)
        .unwrap_err()
        .contains("Localizar"));
    assert!(hub_core::snapshot::generate(
        &after,
        &hub_core::runtime::inspect(
            &after,
            std::slice::from_ref(&after),
            vec![],
            &Default::default()
        )
    )
    .is_err());
    assert!(hub_core::launchers::launch(&after, "folder").is_err());
}
#[test]
fn editing_an_unbound_project_keeps_it_unbound_and_new_projects_need_a_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = folder(tmp.path(), "p");
    let mut a = open(tmp.path(), "a.db");
    let p = a.save(None, input("P", &dir, "")).unwrap();
    let ws = a.export_portable().unwrap();
    let mut b = open(tmp.path(), "b.db");
    b.apply_portable(&ws).unwrap();
    let mut edit = input("P2", &dir, "");
    edit.local_path = String::new();
    let saved = b.save(Some(&p.id), edit).unwrap();
    assert_eq!(saved.name, "P2");
    assert_eq!(
        projects::location(&b.project(&p.id).unwrap()),
        Location::Unbound
    );
    let mut fresh = input("Novo", &dir, "");
    fresh.local_path = String::new();
    assert!(b.save(None, fresh).is_err());
}

// ---- migration 003 ----

fn legacy_db(path: &Path, folders: &[(&str, &Path)]) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for file in ["001_initial.sql", "002_knowledge.sql"] {
        conn.execute_batch(&std::fs::read_to_string(dir.join(file)).unwrap())
            .unwrap();
    }
    for (id, folder) in folders {
        let local = folder.to_string_lossy().to_string();
        let data = serde_json::json!({
            "id": id, "name": format!("Projeto {id}"), "slug": id, "description": "legado",
            "localPath": local, "repository": "https://github.com/org/legado", "stack": ["Rust"],
            "tags": [], "ports": [{"name":"web","port":3000}], "commands": [],
            "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z"
        });
        conn.execute(
            "INSERT INTO projects(id,local_path,data,updated_at) VALUES(?1,?2,?3,?4)",
            rusqlite::params![id, local, data.to_string(), "2026-01-02T00:00:00Z"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO prompt_templates VALUES(?1,'Meu','Dev',?2,'corpo')",
            rusqlite::params![format!("pt-{id}"), id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO knowledge(id,project_id,title,kind,body,tags) VALUES(?1,?2,'T','note','b','x')",
            rusqlite::params![format!("k-{id}"), id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activities(project_id,action) VALUES(?1,'legado')",
            [id],
        )
        .unwrap();
    }
}
#[test]
fn migration_003_preserves_legacy_data_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let a = folder(tmp.path(), "a");
    let b = folder(tmp.path(), "b");
    let path = tmp.path().join("legacy.db");
    legacy_db(&path, &[("id-a", &a), ("id-b", &b)]);

    let db = Database::open(&path).unwrap();
    let version: i64 = db
        .conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 5);
    assert_eq!(db.projects().unwrap().len(), 2);
    let pa = db.project("id-a").unwrap();
    assert_eq!(pa.local_path, a.to_string_lossy());
    assert_eq!(pa.name, "Projeto id-a");
    assert_eq!(pa.ports[0].port, 3000);
    assert_eq!(projects::location(&pa), Location::Available);
    // O cadastro não guarda mais o caminho.
    let stored: String = db
        .conn
        .query_row("SELECT data FROM projects WHERE id='id-a'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!stored.contains("localPath"));
    let has_local: i64 = db
        .conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('projects') WHERE name='local_path'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(has_local, 0);
    // Filhos continuam ligados.
    assert_eq!(db.knowledge().unwrap().len(), 2);
    assert_eq!(
        db.prompts()
            .unwrap()
            .iter()
            .filter(|p| p.project_id.is_some())
            .count(),
        2
    );
    assert_eq!(db.activities().unwrap().len(), 2);
    let broken = db
        .conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |_| Ok(()))
        .unwrap()
        .count();
    assert_eq!(broken, 0);
    let fk_on: i64 = db
        .conn
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .unwrap();
    assert_eq!(fk_on, 1);
    drop(db);

    // Já atualizado: abrir de novo não muda nada.
    let mut db = Database::open(&path).unwrap();
    assert_eq!(db.projects().unwrap().len(), 2);
    assert_eq!(db.project("id-b").unwrap().local_path, b.to_string_lossy());
    // O vínculo acompanha o projeto removido (cascata na tabela reconstruída).
    db.delete("id-a", true).unwrap();
    let left: i64 = db
        .conn
        .query_row("SELECT count(*) FROM project_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 1);
}
#[test]
fn migration_003_rolls_back_on_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let a = folder(tmp.path(), "a");
    let path = tmp.path().join("legacy.db");
    legacy_db(&path, &[("id-a", &a)]);
    // Referência órfã (FK estava desligada): a conferência final precisa abortar.
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.execute(
            "INSERT INTO knowledge(id,project_id,title,kind,body,tags) VALUES('orfao','nao-existe','T','note','b','x')",
            [],
        )
        .unwrap();
    }
    assert!(Database::open(&path).is_err());
    let conn = Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let has_local: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('projects') WHERE name='local_path'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(has_local, 1);
    let tables: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('project_bindings','projects_portable')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
}
#[test]
fn newer_database_versions_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("future.db");
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    assert!(Database::open(&path).is_err());
}
