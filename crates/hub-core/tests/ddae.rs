//! DDAE (Concept 06): invariantes, derivados, portabilidade e importação da SESSION-001.
use hub_core::{
    database::Database,
    ddae::{self, BlockStatus, LegacyImport, SessionStatus},
    models::ProjectInput,
    portable::{self, PortableWorkspace},
};
use std::path::Path;

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

/// Banco novo com um projeto cadastrado numa pasta temporária.
fn setup() -> (tempfile::TempDir, Database, String) {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("proj");
    std::fs::create_dir_all(&folder).unwrap();
    let mut db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let project = db.save(None, input("Projeto", &folder)).unwrap();
    (tmp, db, project.id)
}

fn with_blocks(db: &mut Database, project: &str, titles: &[&str]) -> String {
    let s = db
        .ddae_create_session(project, "Feature", "Objetivo")
        .unwrap();
    for t in titles {
        db.ddae_add_block(&s.id, t, "").unwrap();
    }
    s.id
}

fn block_id(db: &Database, session: &str, index: usize) -> String {
    db.ddae_session(session).unwrap().blocks[index].id.clone()
}

fn user_version(db: &Database) -> i64 {
    db.conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

// ---- schema ----

#[test]
fn migration_creates_the_ddae_tables_at_version_8() {
    let (_tmp, db, _) = setup();
    assert_eq!(user_version(&db), 8);
    for table in [
        "ddae_sessions",
        "ddae_blocks",
        "ddae_decisions",
        "ddae_events",
    ] {
        let n: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "{table}");
    }
}

#[test]
fn ddae_tables_never_store_paths_or_machine_identity() {
    let (_tmp, db, _) = setup();
    for table in [
        "ddae_sessions",
        "ddae_blocks",
        "ddae_decisions",
        "ddae_events",
    ] {
        let columns: Vec<String> = db
            .conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            !columns.iter().any(|c| c == "ip" || c.starts_with("ip_")),
            "{table}: coluna de IP"
        );
        for forbidden in ["path", "machine", "host", "percent", "progress"] {
            assert!(
                !columns.iter().any(|c| c.contains(forbidden)),
                "{table} tem coluna {forbidden}: {columns:?}"
            );
        }
    }
}

// ---- Session: criação, numeração, uma ativa ----

#[test]
fn creates_numbered_active_sessions_per_project() {
    let (_tmp, mut db, project) = setup();
    let first = db
        .ddae_create_session(&project, "  Primeira  ", "obj")
        .unwrap();
    assert_eq!(first.number, 1);
    assert_eq!(first.label(), "SESSION-001");
    assert_eq!(first.title, "Primeira");
    assert_eq!(first.status, SessionStatus::Active);
    assert_eq!(first.id.len(), 36, "UUID interno estável");

    // Com uma ativa, outra não nasce ativa: recusa com explicação.
    let err = db.ddae_create_session(&project, "Segunda", "").unwrap_err();
    assert!(
        err.contains("SESSION-001") && err.contains("ativa"),
        "{err}"
    );

    db.ddae_freeze(&first.id, "Aguardando revisão").unwrap();
    let second = db.ddae_create_session(&project, "Segunda", "").unwrap();
    assert_eq!(second.number, 2);
}

#[test]
fn numbering_is_per_project_and_never_reused() {
    let (tmp, mut db, project) = setup();
    let other_dir = tmp.path().join("other");
    std::fs::create_dir_all(&other_dir).unwrap();
    let other = db.save(None, input("Outro", &other_dir)).unwrap().id;
    let a = db.ddae_create_session(&project, "A", "").unwrap();
    let b = db.ddae_create_session(&other, "B", "").unwrap();
    assert_eq!((a.number, b.number), (1, 1));
    db.ddae_stop(&a.id, "Pausa").unwrap();
    let a2 = db.ddae_create_session(&project, "A2", "").unwrap();
    assert_eq!(a2.number, 2);
}

#[test]
fn one_active_session_per_project_is_enforced_by_the_database() {
    let (_tmp, mut db, project) = setup();
    db.ddae_create_session(&project, "Ativa", "").unwrap();
    let err = db.conn.execute(
        "INSERT INTO ddae_sessions(id,project_id,number,title,status,created_at,updated_at) \
         VALUES('x1',?1,2,'Outra','active','t','t')",
        [&project],
    );
    assert!(
        err.is_err(),
        "o índice único parcial deve impedir a 2ª ativa"
    );
}

#[test]
fn one_in_progress_block_per_session_is_enforced_by_the_database() {
    let (_tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &["A", "B"]);
    let a = block_id(&db, &s, 0);
    let b = block_id(&db, &s, 1);
    db.conn
        .execute(
            "UPDATE ddae_blocks SET status='in_progress' WHERE id=?1",
            [&a],
        )
        .unwrap();
    assert!(db
        .conn
        .execute(
            "UPDATE ddae_blocks SET status='in_progress' WHERE id=?1",
            [&b]
        )
        .is_err());
}

#[test]
fn validates_title_objective_and_project() {
    let (_tmp, mut db, project) = setup();
    assert!(db.ddae_create_session(&project, "   ", "").is_err());
    assert!(db
        .ddae_create_session(&project, &"x".repeat(ddae::MAX_TITLE + 1), "")
        .is_err());
    assert!(db
        .ddae_create_session(&project, "ok", &"x".repeat(ddae::MAX_OBJECTIVE + 1))
        .is_err());
    assert!(db.ddae_create_session("inexistente", "ok", "").is_err());
    assert!(db.ddae_overview("inexistente").is_err());
}

#[test]
fn rejects_absolute_paths_in_any_text() {
    let (_tmp, mut db, project) = setup();
    for bad in [
        r"C:\Users\fulano\repo",
        "D:/dev/app",
        r"\\servidor\share",
        "/home/fulano/app",
        "~/dev",
    ] {
        let err = db.ddae_create_session(&project, bad, "").unwrap_err();
        assert!(err.contains("caminho"), "{bad}: {err}");
        assert!(
            db.ddae_create_session(&project, "ok", bad).is_err(),
            "{bad}"
        );
    }
    // URLs e prosa comum passam.
    let s = db
        .ddae_create_session(
            &project,
            "Usar https://github.com/org/repo",
            "ver docs/ddae/x.md",
        )
        .unwrap();
    assert!(db.ddae_add_block(&s.id, "Bloco 1: C#", "").is_ok());
}

// ---- Blocks ----

#[test]
fn block_flow_derives_current_next_and_progress() {
    let (_tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &["A", "B", "C"]);
    let view = db.ddae_overview(&project).unwrap().sessions.remove(0);
    assert_eq!((view.progress.completed, view.progress.total), (0, 3));
    assert!(view.current_block.is_none());
    assert_eq!(view.next_block.as_ref().unwrap().title, "A");

    let (a, b) = (block_id(&db, &s, 0), block_id(&db, &s, 1));
    db.ddae_start_block(&s, &a).unwrap();
    let view = db.ddae_overview(&project).unwrap().sessions.remove(0);
    assert_eq!(view.current_block.as_ref().unwrap().title, "A");
    assert_eq!(view.next_block.as_ref().unwrap().title, "B");

    // Concluir NÃO inicia o próximo bloco automaticamente.
    db.ddae_complete_block(&s, &a).unwrap();
    let view = db.ddae_overview(&project).unwrap().sessions.remove(0);
    assert!(view.current_block.is_none());
    assert_eq!((view.progress.completed, view.progress.total), (1, 3));
    assert_eq!(view.next_block.as_ref().unwrap().title, "B");
    assert_eq!(
        db.ddae_session(&s).unwrap().blocks[1].status,
        BlockStatus::Pending
    );
    db.ddae_start_block(&s, &b).unwrap();
}

#[test]
fn only_one_block_in_progress_and_only_valid_transitions() {
    let (_tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &["A", "B"]);
    let (a, b) = (block_id(&db, &s, 0), block_id(&db, &s, 1));
    assert!(
        db.ddae_complete_block(&s, &a).is_err(),
        "pending não conclui"
    );
    db.ddae_start_block(&s, &a).unwrap();
    assert!(db.ddae_start_block(&s, &b).is_err(), "só um em andamento");
    assert!(db.ddae_start_block(&s, &a).is_err(), "já em andamento");
    assert!(
        db.ddae_complete_block(&s, &b).is_err(),
        "B não está em andamento"
    );
    db.ddae_complete_block(&s, &a).unwrap();
    assert!(db.ddae_complete_block(&s, &a).is_err(), "já concluído");
    assert!(db.ddae_start_block(&s, "nao-existe").is_err());
}

#[test]
fn blocks_only_move_in_an_active_session() {
    let (_tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &["A", "B"]);
    let (a, b) = (block_id(&db, &s, 0), block_id(&db, &s, 1));
    db.ddae_start_block(&s, &a).unwrap();
    db.ddae_freeze(&s, "Aguardando").unwrap();
    // O bloco atual é preservado enquanto congelada, mas nada se move.
    assert_eq!(
        db.ddae_session(&s).unwrap().current_block().unwrap().title,
        "A"
    );
    assert!(db.ddae_complete_block(&s, &a).is_err());
    assert!(db.ddae_start_block(&s, &b).is_err());
    db.ddae_resume(&s).unwrap();
    db.ddae_complete_block(&s, &a).unwrap();
}

// ---- Lifecycle ----

#[test]
fn freeze_and_stop_take_an_optional_reason_and_resume_clears_it() {
    let (_tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &["A"]);
    // O motivo é opcional no backend (a UI o incentiva): vazio não bloqueia e não grava texto.
    let bare = db.ddae_freeze(&s, "  ").unwrap();
    assert_eq!(bare.status, SessionStatus::Frozen);
    assert_eq!(bare.pause_reason, None);
    db.ddae_resume(&s).unwrap();
    let frozen = db.ddae_freeze(&s, "Aguardando API").unwrap();
    assert_eq!(frozen.status, SessionStatus::Frozen);
    assert_eq!(frozen.pause_reason.as_deref(), Some("Aguardando API"));
    assert!(db.ddae_stop(&s, "x").is_err(), "só a ativa pausa");
    let active = db.ddae_resume(&s).unwrap();
    assert_eq!(active.status, SessionStatus::Active);
    assert_eq!(active.pause_reason, None);
    let stopped = db.ddae_stop(&s, "Sem previsão").unwrap();
    assert_eq!(stopped.status, SessionStatus::Stopped);
    assert!(db.ddae_resume(&s).is_ok());
    assert!(db.ddae_resume(&s).is_err(), "já está ativa");
}

#[test]
fn resume_is_refused_while_another_session_is_active() {
    let (_tmp, mut db, project) = setup();
    let first = db.ddae_create_session(&project, "A", "").unwrap();
    db.ddae_freeze(&first.id, "pausa").unwrap();
    db.ddae_create_session(&project, "B", "").unwrap();
    let err = db.ddae_resume(&first.id).unwrap_err();
    assert!(err.contains("SESSION-002"), "{err}");
    assert_eq!(
        db.ddae_session(&first.id).unwrap().status,
        SessionStatus::Frozen
    );
}

#[test]
fn completion_needs_every_block_completed_and_none_in_progress() {
    let (_tmp, mut db, project) = setup();
    let empty = db.ddae_create_session(&project, "Vazia", "").unwrap();
    assert!(
        db.ddae_complete(&empty.id, "").is_err(),
        "sem blocos não finaliza"
    );
    let s = empty.id;
    db.ddae_add_block(&s, "A", "").unwrap();
    db.ddae_add_block(&s, "B", "").unwrap();
    let (a, b) = (block_id(&db, &s, 0), block_id(&db, &s, 1));
    assert!(db.ddae_complete(&s, "").is_err(), "pendentes");
    db.ddae_start_block(&s, &a).unwrap();
    assert!(db.ddae_complete(&s, "").is_err(), "em andamento");
    db.ddae_complete_block(&s, &a).unwrap();
    assert!(db.ddae_complete(&s, "").is_err(), "ainda há pendente");
    db.ddae_start_block(&s, &b).unwrap();
    db.ddae_complete_block(&s, &b).unwrap();
    let done = db.ddae_complete(&s, "Entregue").unwrap();
    assert_eq!(done.status, SessionStatus::Completed);
    assert_eq!(done.result.as_deref(), Some("Entregue"));
    assert!(done.completed_at.is_some());
    assert!(done.can_complete());
}

#[test]
fn completed_is_terminal() {
    let (_tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &["A"]);
    let a = block_id(&db, &s, 0);
    db.ddae_start_block(&s, &a).unwrap();
    db.ddae_complete_block(&s, &a).unwrap();
    db.ddae_complete(&s, "").unwrap();

    assert!(db.ddae_resume(&s).is_err());
    assert!(db.ddae_freeze(&s, "x").is_err());
    assert!(db.ddae_stop(&s, "x").is_err());
    assert!(db.ddae_add_block(&s, "novo", "").is_err());
    assert!(db.ddae_add_decision(&s, "d", "", None).is_err());
    assert!(db.ddae_complete(&s, "").is_err());
    // Nem por SQL direto: o gatilho do banco recusa.
    assert!(db
        .conn
        .execute("UPDATE ddae_sessions SET status='active' WHERE id=?1", [&s])
        .is_err());
    assert_eq!(
        db.ddae_session(&s).unwrap().status,
        SessionStatus::Completed
    );
    // Uma finalizada não conta como ativa: o projeto pode abrir outra sessão.
    assert!(db.ddae_create_session(&project, "Próxima", "").is_ok());
}

#[test]
fn decisions_are_appended_in_order() {
    let (_tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &[]);
    db.ddae_add_decision(&s, "Usar SQLite", "fonte de verdade", None)
        .unwrap();
    let session = db
        .ddae_add_decision(&s, "Markdown é export", "", None)
        .unwrap();
    assert_eq!(
        session
            .decisions
            .iter()
            .map(|d| d.title.as_str())
            .collect::<Vec<_>>(),
        ["Usar SQLite", "Markdown é export"]
    );
    assert!(db.ddae_add_decision(&s, "  ", "", None).is_err());
    let view = db.ddae_overview(&project).unwrap().sessions.remove(0);
    assert_eq!(view.recent_decision.unwrap().title, "Markdown é export");
}

// ---- Overview ----

#[test]
fn overview_counts_close_with_the_total() {
    let (_tmp, mut db, project) = setup();
    let a = with_blocks(&mut db, &project, &["A", "B"]);
    db.ddae_freeze(&a, "x").unwrap();
    let b = with_blocks(&mut db, &project, &["C"]);
    db.ddae_stop(&b, "y").unwrap();
    let c = with_blocks(&mut db, &project, &["D"]);
    let (d0,) = (block_id(&db, &c, 0),);
    db.ddae_start_block(&c, &d0).unwrap();
    db.ddae_complete_block(&c, &d0).unwrap();
    db.ddae_complete(&c, "ok").unwrap();
    let e = with_blocks(&mut db, &project, &["E"]);

    let o = db.ddae_overview(&project).unwrap();
    assert_eq!(o.counts.total, 4);
    assert_eq!(
        (
            o.counts.active,
            o.counts.frozen,
            o.counts.stopped,
            o.counts.completed
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(
        o.counts.active + o.counts.frozen + o.counts.stopped + o.counts.completed,
        o.counts.total
    );
    assert_eq!(o.blocks_total, 5);
    assert_eq!(o.active_session_id.as_deref(), Some(e.as_str()));
    // Mais recente primeiro.
    assert_eq!(
        o.sessions
            .iter()
            .map(|s| s.session.number)
            .collect::<Vec<_>>(),
        [4, 3, 2, 1]
    );
    assert_eq!(o.sessions[0].label, "SESSION-004");
}

// ---- Portabilidade ----

fn project_ws(db: &Database) -> PortableWorkspace {
    db.export_portable().unwrap()
}

#[test]
fn export_carries_sessions_and_stays_portable() {
    let (tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &["A"]);
    db.ddae_add_decision(&s, "Decisão", "corpo", None).unwrap();
    let ws = project_ws(&db);
    assert_eq!(ws.version, portable::SCHEMA_VERSION);
    assert_eq!(ws.ddae.len(), 1);
    portable::validate(&ws).unwrap();
    let json = serde_json::to_string(&ws).unwrap();
    let folder = tmp.path().to_string_lossy().replace('\\', "\\\\");
    assert!(
        !json.contains(&folder),
        "nenhum caminho local no estado portátil"
    );
    assert!(!json.contains("machine"), "nenhuma identidade de máquina");
}

#[test]
fn sessions_travel_through_the_workspace_with_the_same_identity() {
    let (_tmp, mut a, project) = setup();
    let s = with_blocks(&mut a, &project, &["A", "B"]);
    let first = block_id(&a, &s, 0);
    a.ddae_start_block(&s, &first).unwrap();
    a.ddae_add_decision(&s, "D", "", None).unwrap();
    let ws = project_ws(&a);

    let tmp2 = tempfile::tempdir().unwrap();
    let mut b = Database::open(&tmp2.path().join("b.db")).unwrap();
    let summary = b.apply_portable(&ws).unwrap();
    assert_eq!(summary.sessions, 1);
    let got = b.ddae_session(&s).unwrap();
    let want = a.ddae_session(&s).unwrap();
    assert_eq!(got.id, want.id);
    assert_eq!(got.number, 1);
    assert_eq!(got.blocks, want.blocks);
    assert_eq!(got.decisions.len(), 1);
    assert_eq!(got.current_block().unwrap().title, "A");
    // Round-trip: o mesmo conteúdo, o mesmo hash.
    assert_eq!(
        portable::content_hash(&project_ws(&b)),
        portable::content_hash(&ws)
    );
}

#[test]
fn apply_replaces_ddae_even_when_the_active_session_changes() {
    let (_tmp, mut a, project) = setup();
    let first = a.ddae_create_session(&project, "A", "").unwrap();
    let ws_with_a_active = project_ws(&a);
    a.ddae_freeze(&first.id, "pausa").unwrap();
    let second = a.ddae_create_session(&project, "B", "").unwrap();
    let ws_with_b_active = project_ws(&a);
    // Local tem B ativa; o workspace diz que A é a ativa: troca sem violar "uma ativa".
    a.apply_portable(&ws_with_a_active).unwrap();
    assert_eq!(a.ddae_overview(&project).unwrap().counts.total, 1);
    assert!(a.ddae_session(&second.id).is_err());
    a.apply_portable(&ws_with_b_active).unwrap();
    assert_eq!(
        a.ddae_overview(&project).unwrap().active_session_id,
        Some(second.id)
    );
}

#[test]
fn removing_a_project_from_the_workspace_removes_its_sessions() {
    let (_tmp, mut a, project) = setup();
    with_blocks(&mut a, &project, &["A"]);
    let mut ws = project_ws(&a);
    ws.projects.clear();
    ws.ddae.clear();
    a.apply_portable(&ws).unwrap();
    let n: i64 = a
        .conn
        .query_row("SELECT count(*) FROM ddae_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn hash_ignores_timestamps_but_not_content() {
    let (_tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &["A"]);
    let ws = project_ws(&db);
    let mut touched = ws.clone();
    touched.ddae[0].updated_at = "2099-01-01T00:00:00.000Z".into();
    touched.ddae[0].created_at = "2000-01-01T00:00:00.000Z".into();
    assert_eq!(
        portable::content_hash(&ws),
        portable::content_hash(&touched)
    );
    let a = block_id(&db, &s, 0);
    db.ddae_start_block(&s, &a).unwrap();
    assert_ne!(
        portable::content_hash(&ws),
        portable::content_hash(&project_ws(&db))
    );
}

#[test]
fn v1_workspaces_without_ddae_stay_readable_and_become_v2() {
    let (_tmp, db, _) = setup();
    let mut ws = project_ws(&db);
    ws.version = 1;
    portable::validate(&ws).unwrap();
    let h = portable::content_hash(&ws);
    portable::normalize(&mut ws);
    assert_eq!(ws.version, 3);
    assert_eq!(
        portable::content_hash(&ws),
        h,
        "v1 e v2 sem sessões têm o mesmo hash"
    );
    let text = serde_json::to_string(&ws).unwrap();
    assert!(
        !text.contains("\"ddae\""),
        "sem sessões a chave não aparece"
    );
    // Texto v1 antigo (sem a chave) carrega.
    let old: PortableWorkspace =
        serde_json::from_str(r#"{"version":1,"projects":[],"prompts":[],"knowledge":[]}"#).unwrap();
    assert!(old.ddae.is_empty());
    portable::validate(&old).unwrap();
}

#[test]
fn validation_rejects_broken_ddae_state() {
    let (_tmp, mut db, project) = setup();
    let s = with_blocks(&mut db, &project, &["A", "B"]);
    let ws = project_ws(&db);
    let mutate = |f: &dyn Fn(&mut PortableWorkspace)| {
        let mut w = ws.clone();
        f(&mut w);
        portable::validate(&w)
    };
    assert!(mutate(&|_| {}).is_ok());
    assert!(mutate(&|w| w.version = 1).is_err(), "DDAE exige v2+");
    assert!(mutate(&|w| w.version = 2).is_ok(), "v2 continua legível");
    assert!(mutate(&|w| w.version = 4).is_err());
    assert!(mutate(&|w| w.ddae[0].project_id = "fantasma".into()).is_err());
    assert!(mutate(&|w| w.ddae[0].title = r"C:\Users\x\proj".into()).is_err());
    assert!(mutate(&|w| w.ddae[0].objective = "/home/x/app".into()).is_err());
    assert!(mutate(&|w| w.ddae[0].number = 0).is_err());
    assert!(mutate(&|w| w.ddae[0].id = "id com espaço".into()).is_err());
    assert!(mutate(&|w| {
        // dois blocos em andamento
        w.ddae[0].blocks[0].status = BlockStatus::InProgress;
        w.ddae[0].blocks[1].status = BlockStatus::InProgress;
    })
    .is_err());
    assert!(mutate(&|w| w.ddae[0].status = SessionStatus::Completed).is_err());
    assert!(mutate(&|w| {
        // duas ativas no mesmo projeto
        let mut other = w.ddae[0].clone();
        other.id = "outra-uuid".into();
        other.number = 2;
        other
            .blocks
            .iter_mut()
            .for_each(|b| b.id = format!("{}-x", b.id));
        w.ddae.push(other);
    })
    .is_err());
    assert!(mutate(&|w| {
        let mut other = w.ddae[0].clone();
        other.id = "outra-uuid".into();
        other.status = SessionStatus::Frozen;
        other
            .blocks
            .iter_mut()
            .for_each(|b| b.id = format!("{}-x", b.id));
        w.ddae.push(other); // mesmo número
    })
    .is_err());
    assert!(mutate(&|w| {
        let dup = w.ddae[0].blocks[0].clone();
        w.ddae[0].blocks.push(dup); // id de bloco repetido
    })
    .is_err());
    let _ = s;
}

// ---- SESSION-001 histórica ----

const SAMPLE: &str = "# SESSION-001 — Machine Context & Project Workspace Foundation\n\n\
**PT:** Cadastro\n**Tipo:** Feature\n**Status:** ACTIVE / ATIVA\n\n\
## Objetivo\n\nTransformar o LKR LAB em um assistente consciente.\n\nSegundo parágrafo ignorado.\n\n\
## Blocos\n\n\
| # | Bloco | Status | Referência |\n|---|-------|--------|------------|\n\
| 01 | Product Architecture | CONCLUÍDO | abaixo |\n\
| 02 | Concept 01 — Primeiro acesso / Computador não cadastrado | CONCLUÍDO (visual APROVADO) | [CONCEPT-01](x.md) |\n\
| 03 | Concept 02 — Machine Health | EM ANDAMENTO | x |\n\
| 04 | Concept 03 — Projetos | PENDENTE | x |\n\n\
**Bloco atual:** 03\n\n## Desenvolvimento\n\n| 99 | Ignorado | CONCLUÍDO | x |\n";

#[test]
fn parses_the_legacy_markdown() {
    let s = ddae::parse_legacy(SAMPLE).unwrap();
    assert_eq!(s.number, 1);
    assert_eq!(s.title, "Machine Context & Project Workspace Foundation");
    assert_eq!(
        s.objective,
        "Transformar o LKR LAB em um assistente consciente."
    );
    assert_eq!(s.status, SessionStatus::Active);
    assert_eq!(s.blocks.len(), 4, "só a tabela de Blocos");
    assert_eq!(s.blocks[0].1, BlockStatus::Completed);
    assert_eq!(
        s.blocks[1].0,
        "Concept 01 — Primeiro acesso / Computador não cadastrado"
    );
    assert_eq!(s.blocks[2].1, BlockStatus::InProgress);
    assert_eq!(s.blocks[3].1, BlockStatus::Pending);
}

#[test]
fn legacy_parser_rejects_ambiguous_documents() {
    assert!(ddae::parse_legacy("sem cabeçalho").is_err());
    let two = SAMPLE.replace(
        "| 04 | Concept 03 — Projetos | PENDENTE",
        "| 04 | Concept 03 | EM ANDAMENTO",
    );
    assert!(ddae::parse_legacy(&two).is_err());
    let no_status = SAMPLE.replace("**Status:** ACTIVE / ATIVA", "");
    assert!(ddae::parse_legacy(&no_status).is_err());
}

/// O arquivo real do repositório continua legível (estrutura, não números que evoluem).
#[test]
fn the_real_session_001_document_parses() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/ddae/sessions/SESSION-001-machine-context-workspace-foundation.md");
    let text = std::fs::read_to_string(path).unwrap();
    let s = ddae::parse_legacy(&text).unwrap();
    assert_eq!(s.number, 1);
    assert!(s.title.starts_with("Machine Context"));
    assert!(s.blocks.len() >= 10);
    assert!(
        s.blocks
            .iter()
            .filter(|(_, st)| *st == BlockStatus::InProgress)
            .count()
            <= 1
    );
}

fn legacy_project(tmp: &Path, doc: &str) -> (Database, String) {
    let folder = tmp.join("repo");
    std::fs::create_dir_all(folder.join("docs/ddae/sessions")).unwrap();
    std::fs::write(
        folder.join("docs/ddae/sessions/SESSION-001-machine-context.md"),
        doc,
    )
    .unwrap();
    // Arquivos "de exemplo" que NÃO são a SESSION-001 não podem virar dados reais.
    for n in [2, 3, 4] {
        std::fs::write(
            folder.join(format!("docs/ddae/sessions/SESSION-00{n}-exemplo.md")),
            SAMPLE.replace("SESSION-001", &format!("SESSION-00{n}")),
        )
        .unwrap();
    }
    let mut db = Database::open(&tmp.join("hub.db")).unwrap();
    let project = db.save(None, input("LKR_Lab", &folder)).unwrap();
    (db, project.id)
}

#[test]
fn imports_session_001_once_without_duplicating_or_importing_examples() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut db, project) = legacy_project(tmp.path(), SAMPLE);
    assert_eq!(
        db.ddae_import_legacy(&project).unwrap(),
        LegacyImport::Imported
    );
    assert_eq!(
        db.ddae_import_legacy(&project).unwrap(),
        LegacyImport::AlreadyImported
    );
    let overview = db.ddae_overview_with_legacy(&project).unwrap();
    assert_eq!(overview.legacy_import, LegacyImport::AlreadyImported);
    assert_eq!(
        overview.counts.total, 1,
        "SESSION-002/003/004 não são importadas"
    );
    let s = &overview.sessions[0];
    assert_eq!(s.label, "SESSION-001");
    assert_eq!(s.session.status, SessionStatus::Active);
    assert_eq!((s.progress.completed, s.progress.total), (2, 4));
    assert_eq!(
        s.current_block.as_ref().unwrap().title,
        "Concept 02 — Machine Health"
    );
    assert_eq!(
        s.next_block.as_ref().unwrap().title,
        "Concept 03 — Projetos"
    );
    // Identidade determinística: UUID de 36 caracteres, versão 5.
    assert_eq!(s.session.id.len(), 36);
    assert_eq!(&s.session.id[14..15], "5");
    // Sem caminho no que foi importado.
    let json = serde_json::to_string(&s.session).unwrap();
    assert!(!json.contains("repo"), "{json}");
}

#[test]
fn legacy_import_ids_are_the_same_on_every_machine() {
    // Mesmo Project (mesmo UUID, como depois de um sync) em dois bancos independentes.
    let t1 = tempfile::tempdir().unwrap();
    let (mut a, project) = legacy_project(t1.path(), SAMPLE);
    a.ddae_import_legacy(&project).unwrap();
    let ws = a.export_portable().unwrap();

    let t2 = tempfile::tempdir().unwrap();
    let folder2 = t2.path().join("casa");
    std::fs::create_dir_all(folder2.join("docs/ddae/sessions")).unwrap();
    std::fs::write(
        folder2.join("docs/ddae/sessions/SESSION-001-machine-context.md"),
        SAMPLE,
    )
    .unwrap();
    let mut b = Database::open(&t2.path().join("b.db")).unwrap();
    // O workspace traz o projeto (sem pasta) e a Session já importada no outro PC.
    b.apply_portable(&ws).unwrap();
    // Localiza a pasta desta máquina; o import vira no-op: nada duplica.
    b.bind(&project, &folder2.to_string_lossy(), true).unwrap();
    assert_eq!(
        b.ddae_import_legacy(&project).unwrap(),
        LegacyImport::AlreadyImported
    );
    assert_eq!(b.ddae_overview(&project).unwrap().counts.total, 1);

    // E o inverso: importar nos dois ANTES de sincronizar gera a mesma identidade.
    let t3 = tempfile::tempdir().unwrap();
    let folder3 = t3.path().join("outra");
    std::fs::create_dir_all(folder3.join("docs/ddae/sessions")).unwrap();
    std::fs::write(
        folder3.join("docs/ddae/sessions/SESSION-001-machine-context.md"),
        SAMPLE,
    )
    .unwrap();
    let mut c = Database::open(&t3.path().join("c.db")).unwrap();
    let mut portable_only = ws.clone();
    portable_only.ddae.clear();
    portable_only.version = 2;
    c.apply_portable(&portable_only).unwrap();
    c.bind(&project, &folder3.to_string_lossy(), true).unwrap();
    assert_eq!(
        c.ddae_import_legacy(&project).unwrap(),
        LegacyImport::Imported
    );
    assert_eq!(
        c.ddae_overview(&project).unwrap().sessions[0].session.id,
        a.ddae_overview(&project).unwrap().sessions[0].session.id
    );
    assert_eq!(
        portable::content_hash(&c.export_portable().unwrap()),
        portable::content_hash(&ws),
        "importar em máquinas diferentes converge para o mesmo conteúdo portátil"
    );
}

#[test]
fn legacy_import_is_skipped_instead_of_breaking_a_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut db, project) = legacy_project(tmp.path(), SAMPLE);
    // Já existe uma sessão ativa criada pelo usuário: a histórica (ativa) não entra por cima.
    db.ddae_create_session(&project, "Minha", "").unwrap();
    assert_eq!(
        db.ddae_import_legacy(&project).unwrap(),
        LegacyImport::Skipped
    );
    assert_eq!(db.ddae_overview(&project).unwrap().counts.total, 1);
}

#[test]
fn legacy_import_is_not_applicable_without_a_local_folder_or_document() {
    let (_tmp, mut db, project) = setup();
    assert_eq!(
        db.ddae_import_legacy(&project).unwrap(),
        LegacyImport::NotApplicable
    );
    assert_eq!(db.ddae_overview(&project).unwrap().counts.total, 0);
}
