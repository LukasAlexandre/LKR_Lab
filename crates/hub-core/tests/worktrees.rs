//! Worktrees do LKR LAB (Concept 08): identidade, descoberta passiva, adoção, lifecycle,
//! relações com Session/Block, eventos portáteis, remoção do Git e workspace v4.
//! Usa repositórios Git temporários REAIS (nunca o repositório do desenvolvedor).
use hub_core::{
    database::Database,
    ddae::{self, EventType as DdaeEvent},
    git,
    models::ProjectInput,
    portable::{self, PortableWorkspace},
    worktrees::{
        CreateMode, CreateRequest, ItemKind, ManagedWorktree, OperationalStatus as St,
        WorktreeEventType as WEv,
    },
};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

struct Ctx {
    tmp: tempfile::TempDir,
    db: Database,
    project: String,
    repo: PathBuf,
}

fn input(name: &str, path: &Path) -> ProjectInput {
    ProjectInput {
        name: name.into(),
        description: String::new(),
        local_path: path.to_string_lossy().into(),
        repository: String::new(),
        stack: vec![],
        tags: vec![],
        ports: vec![],
        commands: vec![],
    }
}

fn setup() -> Ctx {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git_out(&repo, &["init", "-q", "-b", "main"]);
    git_out(
        &repo,
        &["remote", "add", "origin", "https://github.com/org/lkr.git"],
    );
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    git_out(&repo, &["add", "."]);
    git_out(&repo, &["commit", "-q", "-m", "init"]);
    let mut db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let project = db.save(None, input("LKR_Lab", &repo)).unwrap().id;
    Ctx {
        tmp,
        db,
        project,
        repo,
    }
}

fn wt_path(c: &Ctx, name: &str) -> PathBuf {
    c.tmp.path().join(name)
}

/// Cria um worktree Git adicional DIRETAMENTE no Git (sem metadata do LKR LAB).
fn git_worktree(c: &Ctx, name: &str, branch: &str) -> PathBuf {
    let path = wt_path(c, name);
    git_out(
        &c.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            "--",
            &path.to_string_lossy(),
        ],
    );
    path
}

fn adopt(c: &mut Ctx, name: &str, branch: &str) -> ManagedWorktree {
    let path = git_worktree(c, name, branch);
    c.db.worktree_adopt(&c.project, &path.to_string_lossy(), None, None, None)
        .unwrap()
}

fn overview(c: &Ctx) -> hub_core::worktrees::WorktreeOverview {
    c.db.project_worktree_overview(&c.project, true).unwrap()
}

fn new_session(c: &mut Ctx, blocks: &[&str]) -> String {
    let s = c.db.ddae_create_session(&c.project, "Feature", "").unwrap();
    for b in blocks {
        c.db.ddae_add_block(&s.id, b, "").unwrap();
    }
    s.id
}

fn table_counts(c: &Ctx) -> Vec<i64> {
    [
        "managed_worktrees",
        "worktree_bindings",
        "worktree_events",
        "activities",
        "ddae_events",
    ]
    .iter()
    .map(|t| {
        c.db.conn
            .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect()
}

fn kinds(c: &Ctx, id: &str) -> Vec<WEv> {
    c.db.managed_worktree(id)
        .unwrap()
        .events
        .iter()
        .map(|e| e.kind)
        .collect()
}

// ---- migration ----

#[test]
fn migration_009_adds_worktree_tables_and_keeps_previous_data() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("p");
    std::fs::create_dir_all(&folder).unwrap();
    let path = tmp.path().join("hub.db");
    let (project, session) = {
        let mut db = Database::open(&path).unwrap();
        let p = db.save(None, input("P", &folder)).unwrap().id;
        let s = db.ddae_create_session(&p, "Antes", "").unwrap().id;
        // Simula um banco na v8: sem as tabelas/gatilhos de worktrees.
        for sql in [
            "DROP TABLE worktree_bindings",
            "DROP TABLE worktree_events",
            "DROP TABLE managed_worktrees",
            "PRAGMA user_version=8",
        ] {
            db.conn.execute_batch(sql).unwrap();
        }
        (p, s)
    };
    let db = Database::open(&path).unwrap();
    let v: i64 = db
        .conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(v, 9);
    assert_eq!(db.ddae_session(&session).unwrap().project_id, project);
    for t in ["managed_worktrees", "worktree_bindings", "worktree_events"] {
        let n: i64 = db
            .conn
            .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}

#[test]
fn worktree_events_are_append_only_in_the_database() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    let ev = &w.events[0].id;
    assert!(c
        .db
        .conn
        .execute("UPDATE worktree_events SET payload='{}' WHERE id=?1", [ev])
        .is_err());
    assert!(c
        .db
        .conn
        .execute(
            "UPDATE worktree_events SET event_type='X' WHERE id=?1",
            [ev]
        )
        .is_err());
}

// ---- parser do Git ----

#[test]
fn parser_reads_primary_branch_detached_locked_prunable_and_bare() {
    let text = "worktree C:/dev/repo\nHEAD aaaa111\nbranch refs/heads/main\n\n\
                worktree C:/dev/wt-branch\nHEAD bbbb222\nbranch refs/heads/feature/x\n\n\
                worktree C:/dev/wt-detached\nHEAD cccc333\ndetached\n\n\
                worktree C:/dev/wt-locked\nHEAD dddd444\nbranch refs/heads/locked\nlocked\n\n\
                worktree C:/dev/wt-locked-why\nHEAD eeee555\nbranch refs/heads/why\nlocked em uso por outra máquina\n\n\
                worktree C:/dev/wt-gone\nHEAD ffff666\nbranch refs/heads/gone\nprunable gitdir file points to non-existent location\n";
    let list = git::parse_worktrees(text);
    assert_eq!(list.len(), 6);
    assert!(
        list[0].is_primary && list[1..].iter().all(|w| !w.is_primary),
        "só o primeiro é o principal"
    );
    assert_eq!(
        (list[0].branch.as_str(), list[0].head.as_str()),
        ("main", "aaaa111")
    );
    assert_eq!(list[1].branch, "feature/x");
    assert!(
        list[2].detached && list[2].branch.is_empty(),
        "detached não inventa branch"
    );
    assert!(list[3].locked && list[3].locked_reason.is_none());
    assert!(list[4].locked);
    assert_eq!(
        list[4].locked_reason.as_deref(),
        Some("em uso por outra máquina")
    );
    assert!(list[5].prunable);
    assert!(list[5]
        .prunable_reason
        .as_deref()
        .unwrap()
        .contains("non-existent"));
    assert!(!list[1].locked && !list[1].prunable && !list[1].bare);
    let bare = git::parse_worktrees("worktree C:/dev/bare.git\nbare\n");
    assert!(bare[0].bare && bare[0].is_primary && bare[0].branch.is_empty());
    assert!(git::parse_worktrees("").is_empty());
    // CRLF
    assert_eq!(git::parse_worktrees("worktree C:/a\r\nHEAD 1\r\nbranch refs/heads/m\r\n\r\nworktree C:/b\r\nHEAD 2\r\ndetached\r\n").len(), 2);
}

#[test]
fn real_git_marks_the_main_checkout_as_primary_and_reads_locks() {
    let c = setup();
    let wt = git_worktree(&c, "wt-l", "feature/l");
    git_out(
        &c.repo,
        &[
            "worktree",
            "lock",
            "--reason",
            "teste",
            &wt.to_string_lossy(),
        ],
    );
    let list = git::worktrees(&c.repo).unwrap();
    assert_eq!(list.len(), 2);
    assert!(list[0].is_primary && list[0].branch == "main");
    assert!(list[1].locked);
    assert_eq!(list[1].locked_reason.as_deref(), Some("teste"));
}

// ---- identidade ----

#[test]
fn identity_is_a_stable_uuid_not_the_branch_or_the_path() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    assert_eq!(w.id.len(), 36);
    assert_eq!(w.branch_hint.as_deref(), Some("feature/a"));
    let wt = wt_path(&c, "wt-a");
    // Branch renomeada no Git: o UUID continua, a dica é só dica.
    git_out(&wt, &["branch", "-m", "feature/renomeada"]);
    let o = overview(&c);
    let item = o
        .items
        .iter()
        .find(|i| i.kind == ItemKind::ManagedAvailable)
        .unwrap();
    assert_eq!(item.managed.as_ref().unwrap().worktree.id, w.id);
    assert_eq!(item.git.as_ref().unwrap().branch, "feature/renomeada");
    assert_eq!(
        item.managed
            .as_ref()
            .unwrap()
            .worktree
            .branch_hint
            .as_deref(),
        Some("feature/a")
    );
    // Não há índice de unicidade de branch: o mesmo hint em outra metadata é possível via workspace.
    assert_eq!(c.db.managed_worktree(&w.id).unwrap().id, w.id);
}

#[test]
fn local_path_never_enters_the_portable_state() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    let ws = c.db.export_portable().unwrap();
    let json = serde_json::to_string(&ws.managed_worktrees).unwrap();
    assert!(!json.contains("wt-a"), "{json}");
    assert!(!json.contains(&*c.tmp.path().to_string_lossy()));
    assert!(!json.to_lowercase().contains("localpath") && !json.contains("local_path"));
    assert!(!hub_core::ddae::has_machine_path(&json));
    // Mudar o binding não muda o conteúdo portátil nem o hash.
    let before = portable::content_hash(&ws);
    c.db.conn
        .execute(
            "UPDATE worktree_bindings SET local_path='X:/outro/lugar' WHERE worktree_id=?1",
            [&w.id],
        )
        .unwrap();
    assert_eq!(
        portable::content_hash(&c.db.export_portable().unwrap()),
        before
    );
    for ev in &ws.managed_worktrees[0].events {
        assert!(!serde_json::to_string(&ev.payload).unwrap().contains("wt-a"));
    }
}

// ---- descoberta passiva ----

#[test]
fn discovery_lists_the_primary_and_unmanaged_worktrees_without_writing_anything() {
    let c = setup();
    let wt = git_worktree(&c, "wt-extra", "feature/extra");
    let git_before = git_out(&c.repo, &["worktree", "list", "--porcelain"]);
    let status_before = git_out(&wt, &["status", "--porcelain=v2", "--branch"]);
    let rows = table_counts(&c);
    let first = overview(&c);
    // Repetir e chamar a variante sem Git também não escreve.
    let again = overview(&c);
    let light = c.db.project_worktree_overview(&c.project, false).unwrap();
    assert_eq!(
        table_counts(&c),
        rows,
        "abrir a página não grava metadata, binding, evento nem atividade"
    );
    assert_eq!(
        git_out(&c.repo, &["worktree", "list", "--porcelain"]),
        git_before
    );
    assert_eq!(
        git_out(&wt, &["status", "--porcelain=v2", "--branch"]),
        status_before
    );

    assert_eq!(first.items.len(), 2);
    assert_eq!(first.items[0].kind, ItemKind::Primary);
    assert_eq!(first.items[1].kind, ItemKind::Unmanaged);
    assert!(
        first.items[0].managed.is_none() && first.items[1].managed.is_none(),
        "nenhuma metadata automática"
    );
    assert!(first.items[1].git_summary.is_some() && light.items[1].git_summary.is_none());
    assert_eq!(again.counts, first.counts);
    // O principal e os não gerenciados ficam fora dos contadores operacionais.
    assert_eq!(first.counts.git_total, 2);
    assert_eq!(
        (
            first.counts.managed,
            first.counts.active,
            first.counts.unmanaged
        ),
        (0, 0, 1)
    );
    assert!(first.project_available && first.git_error.is_none());
}

// ---- adoção ----

#[test]
fn adopting_an_additional_worktree_creates_metadata_binding_and_event() {
    let mut c = setup();
    let path = git_worktree(&c, "wt-a", "feature/a");
    let w =
        c.db.worktree_adopt(&c.project, &path.to_string_lossy(), None, None, None)
            .unwrap();
    assert_eq!(w.display_name, "feature/a", "nome padrão = branch");
    assert_eq!(w.status, St::Active);
    assert_eq!(w.session_id, None);
    assert_eq!(w.block_id, None);
    assert_eq!(
        w.repository_locator.as_ref().unwrap().remote,
        "github.com/org/lkr",
        "locator do Project"
    );
    assert_eq!(kinds(&c, &w.id), [WEv::WorktreeAdopted]);
    let o = overview(&c);
    assert_eq!(o.counts.managed, 1);
    assert_eq!(o.counts.active, 1);
    assert_eq!(o.counts.unmanaged, 0);
    let item = o
        .items
        .iter()
        .find(|i| i.kind == ItemKind::ManagedAvailable)
        .unwrap();
    assert!(
        item.managed.as_ref().unwrap().session.is_none(),
        "sem Session: nunca escolhe uma"
    );
    assert!(item.managed.as_ref().unwrap().last_event_at.is_some());
}

#[test]
fn the_primary_checkout_cannot_be_adopted() {
    let mut c = setup();
    let err =
        c.db.worktree_adopt(&c.project, &c.repo.to_string_lossy(), None, None, None)
            .unwrap_err();
    assert!(err.contains("principal"), "{err}");
    assert_eq!(table_counts(&c)[0], 0);
    assert!(c
        .db
        .worktree_adopt(
            &c.project,
            &c.tmp.path().join("nada").to_string_lossy(),
            None,
            None,
            None
        )
        .is_err());
}

#[test]
fn duplicate_adoption_and_duplicate_binding_are_rejected() {
    let mut c = setup();
    let path = git_worktree(&c, "wt-a", "feature/a");
    let w =
        c.db.worktree_adopt(&c.project, &path.to_string_lossy(), Some("Meu"), None, None)
            .unwrap();
    assert_eq!(w.display_name, "Meu");
    assert!(c
        .db
        .worktree_adopt(&c.project, &path.to_string_lossy(), None, None, None)
        .is_err());
    assert_eq!(table_counts(&c)[0], 1);
    // O banco também recusa o mesmo path em outro worktree.
    let other = adopt(&mut c, "wt-b", "feature/b");
    let local: String =
        c.db.conn
            .query_row(
                "SELECT local_path FROM worktree_bindings WHERE worktree_id=?1",
                [&w.id],
                |r| r.get(0),
            )
            .unwrap();
    assert!(c
        .db
        .conn
        .execute(
            "UPDATE worktree_bindings SET local_path=?2 WHERE worktree_id=?1",
            rusqlite::params![other.id, local]
        )
        .is_err());
}

#[test]
fn adopting_with_a_session_and_block_links_both_sides_with_history() {
    let mut c = setup();
    let s = new_session(&mut c, &["A", "B"]);
    let b = c.db.ddae_session(&s).unwrap().blocks[1].id.clone();
    let path = git_worktree(&c, "wt-a", "feature/a");
    let w =
        c.db.worktree_adopt(
            &c.project,
            &path.to_string_lossy(),
            None,
            Some(&s),
            Some(&b),
        )
        .unwrap();
    assert_eq!(
        (w.session_id.as_deref(), w.block_id.as_deref()),
        (Some(s.as_str()), Some(b.as_str()))
    );
    let k = kinds(&c, &w.id);
    assert!(
        k.contains(&WEv::WorktreeAdopted)
            && k.contains(&WEv::WorktreeSessionLinked)
            && k.contains(&WEv::WorktreeBlockLinked)
    );
    let session_events: Vec<DdaeEvent> =
        c.db.ddae_session(&s)
            .unwrap()
            .events
            .iter()
            .map(|e| e.kind)
            .collect();
    assert!(session_events.contains(&DdaeEvent::WorktreeLinked));
    assert!(session_events.contains(&DdaeEvent::WorktreeBlockLinked));
    let e =
        c.db.ddae_session(&s)
            .unwrap()
            .events
            .into_iter()
            .find(|e| e.kind == DdaeEvent::WorktreeLinked)
            .unwrap();
    assert_eq!(e.payload["worktreeId"], w.id.as_str());
    assert!(
        !serde_json::to_string(&e).unwrap().contains("wt-a"),
        "nenhum path no histórico da Session"
    );
}

// ---- Missing / Locate ----

fn second_machine(c: &Ctx) -> (tempfile::TempDir, Database) {
    let tmp = tempfile::tempdir().unwrap();
    let mut b = Database::open(&tmp.path().join("b.db")).unwrap();
    b.apply_portable(&c.db.export_portable().unwrap()).unwrap();
    (tmp, b)
}

#[test]
fn metadata_without_a_local_binding_is_missing_without_fake_git_or_runtime() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    let (_t, b) = second_machine(&c);
    // Em B o Project nem tem pasta: nada de Git; o worktree gerenciado aparece como não localizado.
    let o = b.project_worktree_overview(&c.project, true).unwrap();
    assert!(!o.project_available);
    assert_eq!(o.items.len(), 1);
    let item = &o.items[0];
    assert_eq!(item.kind, ItemKind::ManagedMissing);
    assert!(
        item.git.is_none() && item.git_summary.is_none(),
        "sem Git Clean falso"
    );
    assert_eq!(item.managed.as_ref().unwrap().worktree.id, w.id);
    assert_eq!(o.counts.missing, 1);
    assert_eq!(o.counts.managed, 1);
    assert_eq!(o.counts.git_total, 0);
}

#[test]
fn locate_binds_the_real_worktree_and_preserves_the_uuid() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    let (t2, mut b) = second_machine(&c);
    // Em B, o mesmo repositório (aqui, a mesma pasta) está vinculado ao Project.
    let r = b.bind(&c.project, &c.repo.to_string_lossy(), true).unwrap();
    assert!(r.bound, "{}", r.message);
    let o = b.project_worktree_overview(&c.project, false).unwrap();
    let kinds: Vec<ItemKind> = o.items.iter().map(|i| i.kind).collect();
    assert!(
        kinds.contains(&ItemKind::ManagedMissing) && kinds.contains(&ItemKind::Unmanaged),
        "{kinds:?}"
    );
    let path = wt_path(&c, "wt-a");
    let located = b.worktree_locate(&w.id, &path.to_string_lossy()).unwrap();
    assert_eq!(located.id, w.id, "o UUID continua o mesmo");
    let after = b.project_worktree_overview(&c.project, false).unwrap();
    assert_eq!(after.counts.managed, 1, "nenhuma metadata nova");
    assert_eq!(after.counts.unmanaged, 0);
    assert!(after
        .items
        .iter()
        .any(|i| i.kind == ItemKind::ManagedAvailable));
    drop(t2);
}

#[test]
fn locate_refuses_a_branch_that_contradicts_the_hint_and_creates_nothing() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    let (_t2, mut b) = second_machine(&c);
    b.bind(&c.project, &c.repo.to_string_lossy(), true).unwrap();
    let path = wt_path(&c, "wt-a");
    git_out(&path, &["switch", "-q", "-c", "outra-branch"]);
    let err = b
        .worktree_locate(&w.id, &path.to_string_lossy())
        .unwrap_err();
    assert!(
        err.contains("outra-branch") && err.contains("feature/a"),
        "{err}"
    );
    let n: i64 = b
        .conn
        .query_row("SELECT count(*) FROM managed_worktrees", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "nenhum UUID novo por divergência de branch");
    let bound: i64 = b
        .conn
        .query_row("SELECT count(*) FROM worktree_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(bound, 0);
    // O principal não pode ser o alvo.
    assert!(b.worktree_locate(&w.id, &c.repo.to_string_lossy()).is_err());
}

// ---- lifecycle ----

#[test]
fn operational_lifecycle_follows_the_allowed_transitions() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    let id = w.id.as_str();
    let frozen =
        c.db.worktree_set_state(id, St::Frozen, "Aguardando", "")
            .unwrap();
    assert_eq!(
        (frozen.status, frozen.state_reason.as_deref()),
        (St::Frozen, Some("Aguardando"))
    );
    let back = c.db.worktree_set_state(id, St::Active, "", "").unwrap();
    assert_eq!((back.status, back.state_reason), (St::Active, None));
    let stopped = c.db.worktree_set_state(id, St::Stopped, "", "").unwrap();
    assert_eq!(
        (stopped.status, stopped.state_reason),
        (St::Stopped, None),
        "motivo é opcional"
    );
    c.db.worktree_set_state(id, St::Active, "", "").unwrap();
    assert!(
        c.db.worktree_set_state(id, St::Active, "", "").is_err(),
        "já está ativo"
    );
    c.db.worktree_set_state(id, St::Frozen, "x", "").unwrap();
    let done =
        c.db.worktree_set_state(id, St::Completed, "", "Integrado")
            .unwrap();
    assert_eq!(
        (done.status, done.result.as_deref()),
        (St::Completed, Some("Integrado"))
    );
    assert!(done.completed_at.is_some());
    let k = kinds(&c, id);
    assert!(k.contains(&WEv::WorktreeStateChanged) && k.contains(&WEv::WorktreeCompleted));
}

#[test]
fn completed_is_terminal_in_the_core_and_in_the_database() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    c.db.worktree_set_state(&w.id, St::Completed, "", "")
        .unwrap();
    for target in [St::Active, St::Frozen, St::Stopped, St::Completed] {
        assert!(
            c.db.worktree_set_state(&w.id, target, "", "").is_err(),
            "{target:?}"
        );
    }
    assert!(c.db.worktree_update(&w.id, "Outro nome", "").is_err());
    assert!(c.db.worktree_set_relation(&w.id, None, None).is_err());
    assert!(c.db.conn.execute("UPDATE managed_worktrees SET operational_status='active', completed_at=NULL WHERE id=?1", [&w.id]).is_err());
    assert_eq!(c.db.managed_worktree(&w.id).unwrap().status, St::Completed);
}

#[test]
fn invalid_transitions_are_refused_and_the_database_checks_completed_at() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    c.db.worktree_set_state(&w.id, St::Frozen, "", "").unwrap();
    assert!(
        c.db.worktree_set_state(&w.id, St::Stopped, "", "").is_err(),
        "frozen não vai direto a stopped"
    );
    assert!(
        c.db.conn
            .execute(
                "UPDATE managed_worktrees SET operational_status='completed' WHERE id=?1",
                [&w.id]
            )
            .is_err(),
        "completed exige completed_at"
    );
    assert!(
        c.db.conn
            .execute(
                "UPDATE managed_worktrees SET completed_at='2026-01-01T00:00:00Z' WHERE id=?1",
                [&w.id]
            )
            .is_err(),
        "completed_at só em completed"
    );
    assert!(c
        .db
        .conn
        .execute(
            "UPDATE managed_worktrees SET operational_status='xyz' WHERE id=?1",
            [&w.id]
        )
        .is_err());
}

#[test]
fn several_worktrees_can_be_active_in_the_same_project_and_session() {
    let mut c = setup();
    let s = new_session(&mut c, &["A"]);
    let a = adopt(&mut c, "wt-a", "feature/a");
    let b = adopt(&mut c, "wt-b", "feature/b");
    let d = adopt(&mut c, "wt-c", "feature/c");
    for w in [&a, &b, &d] {
        c.db.worktree_set_relation(&w.id, Some(&s), None).unwrap();
    }
    let o = overview(&c);
    assert_eq!(o.counts.active, 3, "sem regra de uma ativa");
    assert_eq!(c.db.worktrees_for_session(&c.project, &s).unwrap().len(), 3);
    assert_eq!(
        o.counts.active + o.counts.frozen + o.counts.stopped + o.counts.completed,
        o.counts.managed
    );
}

#[test]
fn operational_state_is_independent_of_git_and_of_the_session() {
    let mut c = setup();
    let s = new_session(&mut c, &["A"]);
    let w = adopt(&mut c, "wt-a", "feature/a");
    c.db.worktree_set_relation(&w.id, Some(&s), None).unwrap();
    // Git sujo + ativo: permitido; finalizar sujo também (só estado).
    let wt = wt_path(&c, "wt-a");
    std::fs::write(wt.join("novo.txt"), "x").unwrap();
    let dirty = overview(&c);
    let item = dirty
        .items
        .iter()
        .find(|i| i.kind == ItemKind::ManagedAvailable)
        .unwrap();
    assert_eq!(item.managed.as_ref().unwrap().worktree.status, St::Active);
    assert!(
        item.git_summary
            .as_ref()
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .changes
            > 0
    );
    assert_eq!(dirty.counts.with_changes, 1);
    // Session ACTIVE + worktree FROZEN: nada muda sozinho.
    c.db.worktree_set_state(&w.id, St::Frozen, "", "").unwrap();
    assert_eq!(
        c.db.ddae_session(&s).unwrap().status,
        ddae::SessionStatus::Active
    );
    // Session COMPLETED + worktree aberto: aviso, sem alterar nenhum dos dois.
    let blocks = c.db.ddae_session(&s).unwrap().blocks;
    c.db.ddae_start_block(&s, &blocks[0].id).unwrap();
    c.db.ddae_complete_block(&s, &blocks[0].id).unwrap();
    c.db.ddae_complete(&s, "").unwrap();
    let o = overview(&c);
    let item = o.items.iter().find(|i| i.managed.is_some()).unwrap();
    assert_eq!(item.warnings, ["session_completed_worktree_open"]);
    assert_eq!(item.managed.as_ref().unwrap().worktree.status, St::Frozen);
    // Worktree COMPLETED + Session ACTIVE em outro cenário.
    let s2 = new_session(&mut c, &["B"]);
    let w2 = adopt(&mut c, "wt-b", "feature/b");
    c.db.worktree_set_relation(&w2.id, Some(&s2), None).unwrap();
    c.db.worktree_set_state(&w2.id, St::Completed, "", "")
        .unwrap();
    let o = overview(&c);
    let item = o
        .items
        .iter()
        .find(|i| i.managed.as_ref().is_some_and(|m| m.worktree.id == w2.id))
        .unwrap();
    assert_eq!(item.warnings, ["worktree_completed_session_active"]);
    assert_eq!(
        c.db.ddae_session(&s2).unwrap().status,
        ddae::SessionStatus::Active
    );
}

#[test]
fn finalizing_does_not_touch_git_or_the_filesystem() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    let wt = wt_path(&c, "wt-a");
    std::fs::write(wt.join("sujo.txt"), "x").unwrap();
    let list_before = git_out(&c.repo, &["worktree", "list", "--porcelain"]);
    let branches_before = git_out(&c.repo, &["branch", "--list"]);
    let head_before = git_out(&wt, &["rev-parse", "HEAD"]);
    c.db.worktree_set_state(&w.id, St::Completed, "", "Resultado")
        .unwrap();
    assert_eq!(
        git_out(&c.repo, &["worktree", "list", "--porcelain"]),
        list_before
    );
    assert_eq!(git_out(&c.repo, &["branch", "--list"]), branches_before);
    assert_eq!(git_out(&wt, &["rev-parse", "HEAD"]), head_before);
    assert!(
        wt.join("sujo.txt").exists() && wt.join("a.txt").exists(),
        "nenhum arquivo apagado"
    );
    assert!(
        git_out(&wt, &["status", "--porcelain"]).contains("sujo.txt"),
        "a alteração Git continua lá"
    );
}

// ---- relações ----

#[test]
fn session_must_belong_to_the_same_project_and_block_to_the_same_session() {
    let mut c = setup();
    let other_dir = c.tmp.path().join("outro");
    std::fs::create_dir_all(&other_dir).unwrap();
    let other = c.db.save(None, input("Outro", &other_dir)).unwrap().id;
    let foreign = c.db.ddae_create_session(&other, "De outro", "").unwrap().id;
    let s1 = new_session(&mut c, &["A"]);
    c.db.ddae_freeze(&s1, "").unwrap();
    let s2 = new_session(&mut c, &["B"]);
    let block_of_s1 = c.db.ddae_session(&s1).unwrap().blocks[0].id.clone();
    let w = adopt(&mut c, "wt-a", "feature/a");
    assert!(
        c.db.worktree_set_relation(&w.id, Some(&foreign), None)
            .is_err(),
        "Session de outro Project"
    );
    assert!(c
        .db
        .worktree_set_relation(&w.id, Some("fantasma"), None)
        .is_err());
    assert!(
        c.db.worktree_set_relation(&w.id, Some(&s2), Some(&block_of_s1))
            .is_err(),
        "Block de outra Session"
    );
    c.db.worktree_set_relation(&w.id, Some(&s2), None).unwrap();
    // Pedir só um bloco (sem Session) desvincula tudo: o bloco nunca fica sozinho.
    assert!(
        c.db.worktree_set_relation(&w.id, None, Some(&block_of_s1))
            .is_ok(),
        "sem Session o bloco é descartado"
    );
    let cleared = c.db.managed_worktree(&w.id).unwrap();
    assert_eq!((cleared.session_id, cleared.block_id), (None, None));
    // O banco recusa a relação inválida por SQL direto.
    assert!(c
        .db
        .conn
        .execute(
            "UPDATE managed_worktrees SET session_id=?2 WHERE id=?1",
            rusqlite::params![w.id, foreign]
        )
        .is_err());
    assert!(c
        .db
        .conn
        .execute(
            "UPDATE managed_worktrees SET block_id=?2, session_id=?3 WHERE id=?1",
            rusqlite::params![w.id, block_of_s1, s2]
        )
        .is_err());
    assert!(
        c.db.conn
            .execute(
                "UPDATE managed_worktrees SET block_id=?2 WHERE id=?1",
                rusqlite::params![w.id, block_of_s1]
            )
            .is_err(),
        "block exige session"
    );
    // Adotar com relação inválida não cria nada.
    let path = git_worktree(&c, "wt-b", "feature/b");
    let rows = table_counts(&c)[0];
    assert!(c
        .db
        .worktree_adopt(
            &c.project,
            &path.to_string_lossy(),
            None,
            Some(&foreign),
            None
        )
        .is_err());
    assert_eq!(table_counts(&c)[0], rows);
}

#[test]
fn linking_unlinking_and_block_changes_write_both_histories_without_changing_states() {
    let mut c = setup();
    let s = new_session(&mut c, &["A", "B"]);
    let blocks = c.db.ddae_session(&s).unwrap().blocks;
    let w = adopt(&mut c, "wt-a", "feature/a");
    c.db.worktree_set_relation(&w.id, Some(&s), None).unwrap();
    c.db.worktree_set_relation(&w.id, Some(&s), Some(&blocks[0].id))
        .unwrap();
    c.db.worktree_set_relation(&w.id, Some(&s), Some(&blocks[1].id))
        .unwrap();
    assert!(
        c.db.worktree_set_relation(&w.id, Some(&s), Some(&blocks[1].id))
            .is_err(),
        "sem mudança"
    );
    // Desassociar a Session também zera o bloco.
    let un = c.db.worktree_set_relation(&w.id, None, None).unwrap();
    assert_eq!((un.session_id, un.block_id), (None, None));
    let k = kinds(&c, &w.id);
    for e in [
        WEv::WorktreeSessionLinked,
        WEv::WorktreeBlockLinked,
        WEv::WorktreeBlockUnlinked,
        WEv::WorktreeSessionUnlinked,
    ] {
        assert!(k.contains(&e), "{e:?} em {k:?}");
    }
    let se: Vec<DdaeEvent> =
        c.db.ddae_session(&s)
            .unwrap()
            .events
            .iter()
            .map(|e| e.kind)
            .collect();
    for e in [
        DdaeEvent::WorktreeLinked,
        DdaeEvent::WorktreeUnlinked,
        DdaeEvent::WorktreeBlockLinked,
        DdaeEvent::WorktreeBlockUnlinked,
    ] {
        assert!(se.contains(&e), "{e:?} em {se:?}");
    }
    assert_eq!(
        c.db.ddae_session(&s).unwrap().status,
        ddae::SessionStatus::Active,
        "o estado da Session não muda"
    );
    assert_eq!(c.db.managed_worktree(&w.id).unwrap().status, St::Active);
    // Trocar de Session emite desvincular + vincular.
    let s2 = {
        c.db.ddae_freeze(&s, "").unwrap();
        new_session(&mut c, &["C"])
    };
    c.db.worktree_set_relation(&w.id, Some(&s), None).unwrap();
    c.db.worktree_set_relation(&w.id, Some(&s2), None).unwrap();
    let on_first: Vec<DdaeEvent> =
        c.db.ddae_session(&s)
            .unwrap()
            .events
            .iter()
            .map(|e| e.kind)
            .collect();
    assert!(
        on_first
            .iter()
            .filter(|e| **e == DdaeEvent::WorktreeUnlinked)
            .count()
            >= 2
    );
    assert!(c
        .db
        .ddae_session(&s2)
        .unwrap()
        .events
        .iter()
        .any(|e| e.kind == DdaeEvent::WorktreeLinked));
}

#[test]
fn removing_a_pending_block_clears_the_worktree_block_relation_only() {
    let mut c = setup();
    let s = new_session(&mut c, &["A", "B"]);
    let b = c.db.ddae_session(&s).unwrap().blocks[1].id.clone();
    let w = adopt(&mut c, "wt-a", "feature/a");
    c.db.worktree_set_relation(&w.id, Some(&s), Some(&b))
        .unwrap();
    c.db.ddae_remove_block(&s, &b).unwrap();
    let after = c.db.managed_worktree(&w.id).unwrap();
    assert_eq!(
        (after.session_id.as_deref(), after.block_id),
        (Some(s.as_str()), None)
    );
}

#[test]
fn card_never_borrows_the_session_current_block() {
    let mut c = setup();
    let s = new_session(&mut c, &["A"]);
    let a = c.db.ddae_session(&s).unwrap().blocks[0].id.clone();
    c.db.ddae_start_block(&s, &a).unwrap();
    let w = adopt(&mut c, "wt-a", "feature/a");
    c.db.worktree_set_relation(&w.id, Some(&s), None).unwrap();
    let o = overview(&c);
    let m = o.items.iter().find_map(|i| i.managed.as_ref()).unwrap();
    assert_eq!(m.session.as_ref().unwrap().label, "SESSION-001");
    assert!(
        m.block.is_none(),
        "o bloco atual da Session não vira vínculo do worktree"
    );
}

// ---- remover do Git ----

#[test]
fn git_remove_keeps_the_metadata_and_only_drops_the_local_binding() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    let path = wt_path(&c, "wt-a");
    let before = c.db.managed_worktree(&w.id).unwrap();
    c.db.worktree_git_remove(&c.project, &path.to_string_lossy(), true)
        .unwrap();
    assert!(!path.exists());
    assert!(
        git_out(&c.repo, &["branch", "--list"]).contains("feature/a"),
        "a branch fica"
    );
    let after = c.db.managed_worktree(&w.id).unwrap();
    assert_eq!(after.id, before.id);
    assert_eq!(after.status, St::Active, "remover do Git NÃO finaliza");
    assert_eq!(
        after.events.len(),
        before.events.len(),
        "histórico preservado"
    );
    assert_eq!(table_counts(&c)[1], 0, "binding removido");
    let o = overview(&c);
    assert_eq!(
        o.items
            .iter()
            .filter(|i| i.kind == ItemKind::ManagedMissing)
            .count(),
        1
    );
    assert_eq!(o.counts.missing, 1);
}

#[test]
fn git_remove_refuses_primary_locked_dirty_and_unconfirmed_and_never_forces() {
    let mut c = setup();
    let a = adopt(&mut c, "wt-a", "feature/a");
    let path = wt_path(&c, "wt-a");
    assert!(
        c.db.worktree_git_remove(&c.project, &c.repo.to_string_lossy(), true)
            .is_err(),
        "principal"
    );
    assert!(
        c.db.worktree_git_remove(&c.project, &path.to_string_lossy(), false)
            .is_err(),
        "sem confirmação"
    );
    std::fs::write(path.join("sujo.txt"), "x").unwrap();
    assert!(
        c.db.worktree_git_remove(&c.project, &path.to_string_lossy(), true)
            .is_err(),
        "sujo"
    );
    std::fs::remove_file(path.join("sujo.txt")).unwrap();
    git_out(&c.repo, &["worktree", "lock", &path.to_string_lossy()]);
    assert!(
        c.db.worktree_git_remove(&c.project, &path.to_string_lossy(), true)
            .is_err(),
        "bloqueado"
    );
    assert!(path.exists(), "nada foi removido nas recusas");
    assert_eq!(table_counts(&c)[1], 1, "binding intacto");
    assert_eq!(c.db.managed_worktree(&a.id).unwrap().status, St::Active);
}

// ---- criar ----

fn request(mode: CreateMode, branch: &str, path: &Path) -> CreateRequest {
    CreateRequest {
        mode,
        display_name: String::new(),
        description: String::new(),
        branch: branch.into(),
        base_ref: String::new(),
        path: path.to_string_lossy().into(),
        session_id: None,
        block_id: None,
    }
}

#[test]
fn create_with_a_new_branch_from_an_explicit_base_registers_everything() {
    let mut c = setup();
    git_out(&c.repo, &["branch", "base-x"]);
    let s = new_session(&mut c, &["A"]);
    let b = c.db.ddae_session(&s).unwrap().blocks[0].id.clone();
    let path = wt_path(&c, "novo");
    let mut req = request(CreateMode::NewBranch, "feature/novo", &path);
    req.base_ref = "base-x".into();
    req.display_name = "Meu novo".into();
    req.session_id = Some(s.clone());
    req.block_id = Some(b.clone());
    let w = c.db.worktree_create(&c.project, req).unwrap();
    assert_eq!(
        (w.display_name.as_str(), w.status),
        ("Meu novo", St::Active)
    );
    assert_eq!(
        (w.session_id.as_deref(), w.block_id.as_deref()),
        (Some(s.as_str()), Some(b.as_str()))
    );
    assert!(path.exists());
    assert_eq!(
        git_out(&path, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "feature/novo"
    );
    let k = kinds(&c, &w.id);
    assert!(k.contains(&WEv::WorktreeCreated) && k.contains(&WEv::WorktreeSessionLinked));
    assert_eq!(overview(&c).counts.active, 1);
}

#[test]
fn create_from_an_existing_branch_and_git_refuses_a_branch_already_checked_out() {
    let mut c = setup();
    git_out(&c.repo, &["branch", "existente"]);
    let w =
        c.db.worktree_create(
            &c.project,
            request(
                CreateMode::ExistingBranch,
                "existente",
                &wt_path(&c, "wt-e"),
            ),
        )
        .unwrap();
    assert_eq!(w.branch_hint.as_deref(), Some("existente"));
    assert_eq!(w.display_name, "existente");
    // Já em uso por outro worktree (e a main, no principal): o Git recusa e nada é registrado.
    let before = table_counts(&c);
    assert!(c
        .db
        .worktree_create(
            &c.project,
            request(
                CreateMode::ExistingBranch,
                "existente",
                &wt_path(&c, "wt-f")
            )
        )
        .is_err());
    assert!(c
        .db
        .worktree_create(
            &c.project,
            request(CreateMode::ExistingBranch, "main", &wt_path(&c, "wt-g"))
        )
        .is_err());
    assert!(c
        .db
        .worktree_create(
            &c.project,
            request(
                CreateMode::ExistingBranch,
                "nao-existe",
                &wt_path(&c, "wt-h")
            )
        )
        .is_err());
    assert_eq!(table_counts(&c), before);
}

#[test]
fn create_validates_base_branch_destination_and_relation_before_touching_git() {
    let mut c = setup();
    let list_before = git_out(&c.repo, &["worktree", "list", "--porcelain"]);
    let mut bad_base = request(CreateMode::NewBranch, "feature/x", &wt_path(&c, "x1"));
    bad_base.base_ref = "base-que-nao-existe".into();
    assert!(c
        .db
        .worktree_create(&c.project, bad_base)
        .unwrap_err()
        .contains("base"));
    assert!(c
        .db
        .worktree_create(
            &c.project,
            request(CreateMode::NewBranch, "-x", &wt_path(&c, "x2"))
        )
        .is_err());
    assert!(c
        .db
        .worktree_create(
            &c.project,
            request(
                CreateMode::NewBranch,
                "feature/y",
                Path::new("relativo/caminho")
            )
        )
        .is_err());
    assert!(
        c.db.worktree_create(
            &c.project,
            request(CreateMode::NewBranch, "feature/y", &c.repo.join("dentro"))
        )
        .is_err(),
        "fora do repositório"
    );
    let mut bad_rel = request(CreateMode::NewBranch, "feature/z", &wt_path(&c, "x3"));
    bad_rel.session_id = Some("fantasma".into());
    assert!(c.db.worktree_create(&c.project, bad_rel).is_err());
    assert!(
        !wt_path(&c, "x3").exists(),
        "relação inválida não toca no Git"
    );
    assert_eq!(
        git_out(&c.repo, &["worktree", "list", "--porcelain"]),
        list_before
    );
    assert!(!git_out(&c.repo, &["branch", "--list"]).contains("feature/z"));
}

#[test]
fn partial_failure_keeps_the_git_worktree_unmanaged_and_removes_nothing() {
    let mut c = setup();
    // Um binding de outro worktree já ocupa o destino: o Git cria, a metadata falha.
    let other = adopt(&mut c, "wt-a", "feature/a");
    let dest = wt_path(&c, "wt-novo");
    let local = {
        std::fs::create_dir_all(&dest).unwrap();
        let canon = dest.canonicalize().unwrap();
        let text = canon.to_string_lossy().to_string();
        std::fs::remove_dir_all(&dest).unwrap();
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
    };
    c.db.conn
        .execute(
            "UPDATE worktree_bindings SET local_path=?2 WHERE worktree_id=?1",
            rusqlite::params![other.id, local],
        )
        .unwrap();
    let managed_before = table_counts(&c)[0];
    let err =
        c.db.worktree_create(
            &c.project,
            request(CreateMode::NewBranch, "feature/novo", &dest),
        )
        .unwrap_err();
    assert!(
        err.contains("Git Worktree foi criado") && err.contains("NÃO GERENCIADO"),
        "{err}"
    );
    assert!(dest.exists(), "nada foi removido");
    assert!(git_out(&c.repo, &["branch", "--list"]).contains("feature/novo"));
    assert_eq!(
        table_counts(&c)[0],
        managed_before,
        "nenhuma metadata criada"
    );
    // Restaura o binding original: na próxima leitura o worktree novo aparece como NÃO GERENCIADO.
    let original = wt_path(&c, "wt-a");
    c.db.conn
        .execute(
            "UPDATE worktree_bindings SET local_path=?2 WHERE worktree_id=?1",
            rusqlite::params![other.id, original.to_string_lossy()],
        )
        .unwrap();
    let o = overview(&c);
    assert!(
        o.items
            .iter()
            .any(|i| i.kind == ItemKind::Unmanaged
                && i.git.as_ref().unwrap().branch == "feature/novo")
    );
}

// ---- renomear / dados ----

#[test]
fn rename_and_description_are_metadata_only() {
    let mut c = setup();
    let w = adopt(&mut c, "wt-a", "feature/a");
    let r =
        c.db.worktree_update(&w.id, "  Novo nome  ", "Descrição nova")
            .unwrap();
    assert_eq!(
        (r.display_name.as_str(), r.description.as_str()),
        ("Novo nome", "Descrição nova")
    );
    assert!(kinds(&c, &w.id).contains(&WEv::WorktreeRenamed));
    assert!(c.db.worktree_update(&w.id, "   ", "").is_err());
    assert!(c.db.worktree_update(&w.id, r"C:\Users\x\wt", "").is_err());
    assert!(
        c.db.worktree_update(&w.id, "Novo nome", "Descrição nova")
            .is_err(),
        "nada mudou"
    );
    assert_eq!(
        git_out(&wt_path(&c, "wt-a"), &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "feature/a",
        "o Git não é tocado"
    );
}

// ---- workspace v4 ----

#[test]
fn legacy_workspaces_v1_v2_v3_stay_readable_and_become_v4() {
    for version in [1, 2, 3] {
        let text = format!(r#"{{"version":{version},"projects":[],"prompts":[],"knowledge":[]}}"#);
        let mut ws: PortableWorkspace = serde_json::from_str(&text).unwrap();
        portable::validate(&ws).unwrap();
        portable::normalize(&mut ws);
        assert_eq!(ws.version, 4);
        assert!(ws.managed_worktrees.is_empty());
    }
    // v3 com DDAE e critérios em texto continua legível.
    let v3 = r#"{"version":3,"projects":[{"id":"p1","name":"P"}],"prompts":[],"knowledge":[],
        "ddae":[{"id":"s1","projectId":"p1","number":1,"title":"T","status":"active","criteria":["x"],"blocks":[{"id":"b1","title":"B","status":"pending"}]}]}"#;
    let mut ws: PortableWorkspace = serde_json::from_str(v3).unwrap();
    portable::normalize(&mut ws);
    portable::validate(&ws).unwrap();
    assert_eq!(ws.version, 4);
    // v1–v3 com a mesma metadata têm o mesmo hash de v4.
    let base = r#"{"version":V,"projects":[{"id":"p1","name":"P"}],"prompts":[],"knowledge":[]}"#;
    let hashes: Vec<String> = ["1", "2", "3", "4"]
        .iter()
        .map(|v| {
            let mut w: PortableWorkspace = serde_json::from_str(&base.replace("V", v)).unwrap();
            portable::normalize(&mut w);
            portable::content_hash(&w)
        })
        .collect();
    assert!(hashes.windows(2).all(|p| p[0] == p[1]));
    // Worktrees gerenciados exigem v4.
    let mut with: PortableWorkspace =
        serde_json::from_str(base.replace("V", "3").as_str()).unwrap();
    with.managed_worktrees.push(sample_worktree());
    assert!(portable::validate(&with).is_err());
}

fn sample_worktree() -> ManagedWorktree {
    ManagedWorktree {
        id: "w1".into(),
        project_id: "p1".into(),
        display_name: "feature/x".into(),
        description: String::new(),
        status: St::Active,
        state_reason: None,
        result: None,
        repository_locator: None,
        branch_hint: Some("feature/x".into()),
        detached_head_hint: None,
        session_id: None,
        block_id: None,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        completed_at: None,
        events: vec![],
    }
}

#[test]
fn v4_round_trip_events_and_deterministic_hash_without_bindings() {
    let mut c = setup();
    let s = new_session(&mut c, &["A"]);
    let a = adopt(&mut c, "wt-a", "feature/a");
    c.db.worktree_set_relation(&a.id, Some(&s), None).unwrap();
    c.db.worktree_set_state(&a.id, St::Frozen, "pausa", "")
        .unwrap();
    let ws = c.db.export_portable().unwrap();
    assert_eq!(ws.version, 4);
    portable::validate(&ws).unwrap();
    let text = serde_json::to_string(&ws).unwrap();
    let back: PortableWorkspace = serde_json::from_str(&text).unwrap();
    assert_eq!(back, ws);
    assert_eq!(portable::content_hash(&back), portable::content_hash(&ws));
    assert!(!text.contains("worktree_bindings") && !text.contains("localPath"));
    // Ordem dos eventos canônica, mesmo embaralhada.
    let mut shuffled = ws.clone();
    shuffled.managed_worktrees[0].events.reverse();
    portable::normalize(&mut shuffled);
    assert_eq!(
        shuffled.managed_worktrees[0].events,
        ws.managed_worktrees[0].events
    );
    // Timestamps ficam fora do hash; o conteúdo conta.
    let mut touched = ws.clone();
    touched.managed_worktrees[0].updated_at = "2099-01-01T00:00:00.000Z".into();
    assert_eq!(
        portable::content_hash(&touched),
        portable::content_hash(&ws)
    );
    touched.managed_worktrees[0].status = St::Active;
    touched.managed_worktrees[0].state_reason = None;
    assert_ne!(
        portable::content_hash(&touched),
        portable::content_hash(&ws)
    );
}

#[test]
fn applying_a_workspace_preserves_local_bindings_and_removes_dropped_worktrees() {
    let mut c = setup();
    let a = adopt(&mut c, "wt-a", "feature/a");
    let b = adopt(&mut c, "wt-b", "feature/b");
    let ws = c.db.export_portable().unwrap();
    c.db.apply_portable(&ws).unwrap();
    assert_eq!(
        table_counts(&c)[1],
        2,
        "reaplicar o mesmo workspace mantém os bindings locais"
    );
    let events_a = c.db.managed_worktree(&a.id).unwrap().events;
    assert_eq!(events_a.len(), 1);
    // Um workspace sem o worktree B remove a metadata e o binding dele.
    let mut smaller = ws.clone();
    smaller.managed_worktrees.retain(|w| w.id != b.id);
    c.db.apply_portable(&smaller).unwrap();
    assert!(c.db.managed_worktree(&b.id).is_err());
    assert_eq!(table_counts(&c)[1], 1);
    assert_eq!(c.db.managed_worktree(&a.id).unwrap().id, a.id);
}

#[test]
fn portable_validation_rejects_absolute_paths_and_bad_relations() {
    let mut c = setup();
    let s = new_session(&mut c, &["A"]);
    let a = adopt(&mut c, "wt-a", "feature/a");
    c.db.worktree_set_relation(&a.id, Some(&s), None).unwrap();
    let ws = c.db.export_portable().unwrap();
    let mutate = |f: &dyn Fn(&mut PortableWorkspace)| {
        let mut w = ws.clone();
        f(&mut w);
        portable::validate(&w)
    };
    assert!(mutate(&|_| {}).is_ok());
    assert!(mutate(&|w| w.managed_worktrees[0].display_name = r"C:\Users\x\wt".into()).is_err());
    assert!(mutate(&|w| w.managed_worktrees[0].description = "/home/x/wt".into()).is_err());
    assert!(mutate(&|w| w.managed_worktrees[0].branch_hint = Some("~/wt".into())).is_err());
    assert!(mutate(&|w| {
        w.managed_worktrees[0].events[0]
            .payload
            .insert("path".into(), serde_json::json!("D:/dev/wt"));
    })
    .is_err());
    assert!(mutate(&|w| w.managed_worktrees[0].project_id = "fantasma".into()).is_err());
    assert!(mutate(&|w| w.managed_worktrees[0].session_id = Some("fantasma".into())).is_err());
    assert!(mutate(&|w| w.managed_worktrees[0].block_id = Some("b-fantasma".into())).is_err());
    assert!(mutate(&|w| {
        w.managed_worktrees[0].session_id = None;
        w.managed_worktrees[0].block_id = Some("b1".into());
    })
    .is_err());
    assert!(
        mutate(&|w| w.managed_worktrees[0].status = St::Completed).is_err(),
        "completed exige completedAt"
    );
    assert!(
        mutate(&|w| w.managed_worktrees[0].completed_at = Some("2026-01-01T00:00:00Z".into()))
            .is_err()
    );
    assert!(mutate(&|w| {
        let dup = w.managed_worktrees[0].events[0].clone();
        w.managed_worktrees[0].events.push(dup);
    })
    .is_err());
    assert!(mutate(&|w| {
        let mut other = w.managed_worktrees[0].clone();
        other.id = "w-outro".into();
        other.events.clear();
        w.managed_worktrees.push(other); // vários ATIVOS na mesma Session são válidos
    })
    .is_ok());
}

#[test]
fn worktree_events_and_session_events_travel_together() {
    let mut a = setup();
    let s = new_session(&mut a, &["A"]);
    let w = adopt(&mut a, "wt-a", "feature/a");
    a.db.worktree_set_relation(&w.id, Some(&s), None).unwrap();
    let ws = a.db.export_portable().unwrap();
    let (_t, b) = second_machine(&a);
    assert_eq!(
        b.managed_worktree(&w.id).unwrap().events,
        ws.managed_worktrees[0].events
    );
    assert!(b
        .ddae_session(&s)
        .unwrap()
        .events
        .iter()
        .any(|e| e.kind == DdaeEvent::WorktreeLinked));
    assert_eq!(
        portable::content_hash(&b.export_portable().unwrap()),
        portable::content_hash(&ws)
    );
}

#[test]
fn session_detail_reads_only_real_relations() {
    let mut c = setup();
    let s = new_session(&mut c, &["A"]);
    let other = new_session_frozen(&mut c);
    let w = adopt(&mut c, "wt-a", "feature/a");
    assert!(
        c.db.worktrees_for_session(&c.project, &s)
            .unwrap()
            .is_empty(),
        "nenhuma relação inventada"
    );
    c.db.worktree_set_relation(&w.id, Some(&s), None).unwrap();
    let list = c.db.worktrees_for_session(&c.project, &s).unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].available);
    assert_eq!(list[0].display_name, "feature/a");
    assert!(c
        .db
        .worktrees_for_session(&c.project, &other)
        .unwrap()
        .is_empty());
}

fn new_session_frozen(c: &mut Ctx) -> String {
    c.db.ddae_list_first_for_test(&c.project)
}

trait FirstSession {
    fn ddae_list_first_for_test(&mut self, project: &str) -> String;
}
impl FirstSession for Database {
    fn ddae_list_first_for_test(&mut self, project: &str) -> String {
        let s = self.ddae_overview(project).unwrap().sessions[0]
            .session
            .id
            .clone();
        self.ddae_freeze(&s, "").unwrap();
        self.ddae_create_session(project, "Outra", "").unwrap().id
    }
}
