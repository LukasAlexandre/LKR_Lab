//! DDAE — detalhe da Session (Concept 07): par Project/Session, critérios marcáveis, histórico
//! portátil (ddae_events), blocos, detalhes, arquivos, notas e workspace v3.
use hub_core::{
    database::Database,
    ddae::{
        self, BlockStatus, Criterion, Details, EventType, LegacyImport, ReferenceKind,
        SessionStatus,
    },
    models::ProjectInput,
    portable::{self, PortableWorkspace},
};
use std::path::{Path, PathBuf};

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

struct Ctx {
    _tmp: tempfile::TempDir,
    db: Database,
    project: String,
    folder: PathBuf,
}

fn setup() -> Ctx {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("projeto");
    std::fs::create_dir_all(folder.join("docs")).unwrap();
    std::fs::write(folder.join("docs/plano.md"), "x").unwrap();
    let mut db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let project = db.save(None, input("Projeto", &folder)).unwrap().id;
    Ctx {
        _tmp: tmp,
        db,
        project,
        folder,
    }
}

fn session_with_blocks(c: &mut Ctx, titles: &[&str]) -> String {
    let s =
        c.db.ddae_create_session(&c.project, "Feature", "Obj")
            .unwrap();
    for t in titles {
        c.db.ddae_add_block(&s.id, t, "").unwrap();
    }
    s.id
}

fn blocks(c: &Ctx, id: &str) -> Vec<ddae::Block> {
    c.db.ddae_session(id).unwrap().blocks
}

fn types(c: &Ctx, id: &str) -> Vec<EventType> {
    c.db.ddae_session(id)
        .unwrap()
        .events
        .iter()
        .map(|e| e.kind)
        .collect()
}

fn details_of(c: &Ctx, id: &str) -> Details {
    let s = c.db.ddae_session(id).unwrap();
    Details {
        title: None,
        objective: s.objective,
        desired_outcome: s.desired_outcome,
        constraints: s.constraints,
        criteria: s.criteria,
        notes: s.notes,
        references: s.references,
    }
}

// ---- rota: par Project/Session ----

#[test]
fn project_scoped_get_returns_the_session_view() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A", "B"]);
    let view = c.db.ddae_session_detail(&c.project, &id).unwrap();
    assert_eq!(view.label, "SESSION-001");
    assert_eq!(view.session.id, id);
    assert_eq!((view.progress.completed, view.progress.total), (0, 2));
    assert_eq!(view.next_block.unwrap().title, "A");
    assert!(!view.session.events.is_empty());
}

#[test]
fn session_of_another_project_and_unknown_sessions_are_rejected() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    let other_dir = c.folder.parent().unwrap().join("outro");
    std::fs::create_dir_all(&other_dir).unwrap();
    let other = c.db.save(None, input("Outro", &other_dir)).unwrap().id;
    let err = c.db.ddae_session_detail(&other, &id).unwrap_err();
    assert_eq!(err, "Sessão não encontrada neste projeto.");
    assert_eq!(
        c.db.ddae_session_detail(&c.project, "nao-existe")
            .unwrap_err(),
        "Sessão não encontrada neste projeto."
    );
    assert!(c.db.ddae_session_detail("projeto-fantasma", &id).is_err());
    // SESSION-NNN nunca é identidade.
    assert!(c.db.ddae_session_detail(&c.project, "SESSION-001").is_err());
}

// ---- critérios ----

#[test]
fn legacy_string_criteria_load_as_objects_without_losing_any() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    // Formato da migration 007: lista de strings no JSON.
    c.db.conn
        .execute(
            "UPDATE ddae_sessions SET criteria=?2 WHERE id=?1",
            rusqlite::params![id, r#"["Build verde","Testes verdes","Build verde"]"#],
        )
        .unwrap();
    let first = c.db.ddae_session(&id).unwrap().criteria;
    assert_eq!(
        first.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        ["Build verde", "Testes verdes", "Build verde"]
    );
    assert!(first.iter().all(|c| !c.completed && !c.id.is_empty()));
    let ids: std::collections::HashSet<_> = first.iter().map(|c| c.id.clone()).collect();
    assert_eq!(ids.len(), 3, "ids distintos, mesmo com texto repetido");
    // Determinístico: relê igual, e o export portátil traz os mesmos ids.
    assert_eq!(first, c.db.ddae_session(&id).unwrap().criteria);
    let ws = c.db.export_portable().unwrap();
    assert_eq!(ws.ddae[0].criteria, first);
}

#[test]
fn v1_and_v2_workspaces_with_string_criteria_become_v3_objects() {
    let text = |version: u32| {
        format!(
            r#"{{"version":{version},"projects":[{{"id":"p1","name":"P"}}],"prompts":[],"knowledge":[],
            "ddae":[{{"id":"s1","projectId":"p1","number":1,"title":"T","status":"active",
            "criteria":["Build verde",{{"text":"Já objeto"}}],"blocks":[{{"id":"b1","title":"B","status":"pending"}}]}}]}}"#
        )
    };
    let mut hashes = Vec::new();
    for version in [2, 3] {
        let mut ws: PortableWorkspace = serde_json::from_str(&text(version)).unwrap();
        portable::normalize(&mut ws);
        portable::validate(&ws).unwrap();
        assert_eq!(ws.version, portable::SCHEMA_VERSION);
        assert_eq!(
            ws.ddae[0].criteria.len(),
            2,
            "nenhum critério antigo se perde"
        );
        assert_eq!(ws.ddae[0].criteria[0].text, "Build verde");
        assert!(!ws.ddae[0].criteria[0].completed && !ws.ddae[0].criteria[0].id.is_empty());
        hashes.push(portable::content_hash(&ws));
    }
    assert_eq!(
        hashes[0], hashes[1],
        "a mesma leitura em qualquer versão tem o mesmo hash"
    );
    // v1 sem DDAE continua válido.
    let mut v1: PortableWorkspace =
        serde_json::from_str(r#"{"version":1,"projects":[],"prompts":[],"knowledge":[]}"#).unwrap();
    portable::validate(&v1).unwrap();
    portable::normalize(&mut v1);
    assert_eq!(v1.version, 4);
}

#[test]
fn criteria_can_be_completed_and_reopened_keeping_their_ids() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    let mut d = details_of(&c, &id);
    d.criteria = vec!["Um".into(), "Dois".into()];
    let saved = c.db.ddae_update_details(&id, d).unwrap();
    let (one, two) = (saved.criteria[0].clone(), saved.criteria[1].clone());
    assert!(!one.id.is_empty() && one.id != two.id);

    let mut d = details_of(&c, &id);
    d.criteria[0].completed = true;
    let done = c.db.ddae_update_details(&id, d).unwrap();
    assert!(done.criteria[0].completed && !done.criteria[1].completed);
    assert_eq!(done.criteria[0].id, one.id, "o id é preservado");

    let mut d = details_of(&c, &id);
    d.criteria[0].completed = false;
    d.criteria[1].text = "Dois (editado)".into();
    let reopened = c.db.ddae_update_details(&id, d).unwrap();
    assert!(!reopened.criteria[0].completed);
    assert_eq!(reopened.criteria[1].id, two.id);
    assert_eq!(reopened.criteria[1].text, "Dois (editado)");

    let mut d = details_of(&c, &id);
    d.criteria.remove(0);
    assert_eq!(c.db.ddae_update_details(&id, d).unwrap().criteria.len(), 1);

    let kinds = types(&c, &id);
    for expected in [
        EventType::CriterionAdded,
        EventType::CriterionCompleted,
        EventType::CriterionReopened,
        EventType::CriterionRemoved,
        EventType::DetailsUpdated,
    ] {
        assert!(kinds.contains(&expected), "{expected:?} em {kinds:?}");
    }
}

fn finish_blocks(c: &mut Ctx, id: &str) {
    for b in blocks(c, id) {
        c.db.ddae_start_block(id, &b.id).unwrap();
        c.db.ddae_complete_block(id, &b.id).unwrap();
    }
}

#[test]
fn finalizing_needs_every_criterion_completed_when_there_are_any() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    let mut d = details_of(&c, &id);
    d.criteria = vec!["Um".into(), "Dois".into()];
    c.db.ddae_update_details(&id, d).unwrap();
    finish_blocks(&mut c, &id);
    // Blocos concluídos NÃO finalizam; critérios pendentes bloqueiam.
    let view = c.db.ddae_session_detail(&c.project, &id).unwrap();
    assert!(!view.can_complete);
    assert_eq!(view.completion_blockers, ["criteria_pending"]);
    let err = c.db.ddae_complete(&id, "").unwrap_err();
    assert!(
        err.contains("critérios") && err.contains("2 pendentes"),
        "{err}"
    );

    let mut d = details_of(&c, &id);
    d.criteria[0].completed = true;
    c.db.ddae_update_details(&id, d).unwrap();
    assert!(c
        .db
        .ddae_complete(&id, "")
        .unwrap_err()
        .contains("1 pendente"));
    // Marcar o último critério também NÃO finaliza sozinho.
    let mut d = details_of(&c, &id);
    d.criteria[1].completed = true;
    let s = c.db.ddae_update_details(&id, d).unwrap();
    assert_eq!(s.status, SessionStatus::Active);
    assert!(
        c.db.ddae_session_detail(&c.project, &id)
            .unwrap()
            .can_complete
    );
    assert_eq!(
        c.db.ddae_complete(&id, "ok").unwrap().status,
        SessionStatus::Completed
    );
}

#[test]
fn zero_criteria_do_not_block_finalizing() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    finish_blocks(&mut c, &id);
    assert!(c.db.ddae_session(&id).unwrap().criteria.is_empty());
    assert!(
        c.db.ddae_session_detail(&c.project, &id)
            .unwrap()
            .can_complete
    );
    assert_eq!(
        c.db.ddae_complete(&id, "").unwrap().status,
        SessionStatus::Completed
    );
}

#[test]
fn criteria_never_count_as_progress_or_ready_for_ai() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A", "B"]);
    let mut d = details_of(&c, &id);
    d.criteria = vec![Criterion {
        id: String::new(),
        text: "Feito".into(),
        completed: true,
    }];
    c.db.ddae_update_details(&id, d).unwrap();
    let view = c.db.ddae_session_detail(&c.project, &id).unwrap();
    assert_eq!((view.progress.completed, view.progress.total), (0, 2));
}

// ---- eventos ----

#[test]
fn lifecycle_and_block_operations_write_events_in_the_same_transaction() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A", "B"]);
    let (a, b) = (blocks(&c, &id)[0].id.clone(), blocks(&c, &id)[1].id.clone());
    c.db.ddae_start_block(&id, &a).unwrap();
    c.db.ddae_complete_block(&id, &a).unwrap();
    c.db.ddae_rename_block(&id, &b, "B2").unwrap();
    c.db.ddae_freeze(&id, "Aguardando").unwrap();
    c.db.ddae_resume(&id).unwrap();
    c.db.ddae_stop(&id, "").unwrap();
    c.db.ddae_resume(&id).unwrap();
    c.db.ddae_add_block(&id, "C", "").unwrap();
    let c_id = blocks(&c, &id)[2].id.clone();
    c.db.ddae_remove_block(&id, &c_id).unwrap();
    c.db.ddae_start_block(&id, &b).unwrap();
    c.db.ddae_complete_block(&id, &b).unwrap();
    c.db.ddae_complete(&id, "Entregue").unwrap();

    let kinds = types(&c, &id);
    assert_eq!(kinds[0], EventType::SessionCreated);
    for expected in [
        EventType::BlockAdded,
        EventType::BlockStarted,
        EventType::BlockCompleted,
        EventType::BlockRenamed,
        EventType::BlockRemoved,
        EventType::SessionFrozen,
        EventType::SessionStopped,
        EventType::SessionResumed,
        EventType::SessionCompleted,
    ] {
        assert!(kinds.contains(&expected), "{expected:?} em {kinds:?}");
    }
    let events = c.db.ddae_session(&id).unwrap().events;
    let frozen = events
        .iter()
        .find(|e| e.kind == EventType::SessionFrozen)
        .unwrap();
    assert_eq!(frozen.payload["reason"], "Aguardando");
    let stopped = events
        .iter()
        .find(|e| e.kind == EventType::SessionStopped)
        .unwrap();
    assert!(stopped.payload.is_empty(), "motivo vazio não vira texto");
    let ids: std::collections::HashSet<_> = events.iter().map(|e| e.id.clone()).collect();
    assert_eq!(ids.len(), events.len(), "UUID próprio por evento");
    assert!(events
        .windows(2)
        .all(|w| (&w[0].created_at, &w[0].id) <= (&w[1].created_at, &w[1].id)));
}

#[test]
fn events_belong_to_their_session_and_are_append_only() {
    let mut c = setup();
    let first = session_with_blocks(&mut c, &["A"]);
    c.db.ddae_freeze(&first, "x").unwrap();
    let second = session_with_blocks(&mut c, &["B"]);
    let of = |c: &Ctx, id: &str| c.db.ddae_session(id).unwrap().events;
    let first_events = of(&c, &first);
    let second_events = of(&c, &second);
    let overlap = first_events
        .iter()
        .any(|e| second_events.iter().any(|f| f.id == e.id));
    assert!(!overlap, "eventos de uma Session não aparecem na outra");
    // Append-only: o banco recusa alterar um evento.
    let id = &first_events[0].id;
    assert!(c
        .db
        .conn
        .execute("UPDATE ddae_events SET payload='{}' WHERE id=?1", [id])
        .is_err());
    assert!(c
        .db
        .conn
        .execute("UPDATE ddae_events SET event_type='X' WHERE id=?1", [id])
        .is_err());
    // Falha na operação não deixa evento órfão (mesma transação).
    let before = of(&c, &second).len();
    assert!(c.db.ddae_complete_block(&second, "inexistente").is_err());
    assert!(c.db.ddae_complete(&second, "").is_err());
    assert_eq!(of(&c, &second).len(), before);
}

#[test]
fn events_follow_the_session_when_it_is_removed_with_its_project() {
    let mut c = setup();
    session_with_blocks(&mut c, &["A"]);
    let mut ws = c.db.export_portable().unwrap();
    ws.projects.clear();
    ws.ddae.clear();
    c.db.apply_portable(&ws).unwrap();
    let n: i64 =
        c.db.conn
            .query_row("SELECT count(*) FROM ddae_events", [], |r| r.get(0))
            .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn events_travel_in_the_portable_workspace_and_dedupe_by_identity() {
    let mut a = setup();
    let id = session_with_blocks(&mut a, &["A"]);
    let blk = blocks(&a, &id)[0].id.clone();
    a.db.ddae_start_block(&id, &blk).unwrap();
    let ws = a.db.export_portable().unwrap();
    portable::validate(&ws).unwrap();
    let want = ws.ddae[0].events.clone();
    assert!(want.len() >= 3);

    let t2 = tempfile::tempdir().unwrap();
    let mut b = Database::open(&t2.path().join("b.db")).unwrap();
    b.apply_portable(&ws).unwrap();
    assert_eq!(
        b.ddae_session(&id).unwrap().events,
        want,
        "mesmos UUIDs, mesma ordem"
    );
    // Aplicar de novo não duplica.
    b.apply_portable(&ws).unwrap();
    assert_eq!(b.ddae_session(&id).unwrap().events.len(), want.len());
    assert_eq!(
        portable::content_hash(&b.export_portable().unwrap()),
        portable::content_hash(&ws)
    );

    // Dois eventos com o mesmo id no workspace são recusados.
    let mut bad = ws.clone();
    let dup = bad.ddae[0].events[0].clone();
    bad.ddae[0].events.push(dup);
    assert!(portable::validate(&bad).is_err());
}

#[test]
fn event_order_is_canonical_regardless_of_input_order() {
    let mut a = setup();
    let id = session_with_blocks(&mut a, &["A", "B"]);
    let ws = a.db.export_portable().unwrap();
    let mut shuffled = ws.clone();
    shuffled.ddae[0].events.reverse();
    portable::normalize(&mut shuffled);
    assert_eq!(shuffled.ddae[0].events, ws.ddae[0].events);
    assert_eq!(
        portable::content_hash(&shuffled),
        portable::content_hash(&ws)
    );
    let _ = id;
}

#[test]
fn events_carry_no_local_data() {
    let mut c = setup();
    c.db.conn
        .execute(
            "INSERT INTO machine(id,machine_id,name,usage,description) VALUES(1,'11111111-2222-4333-8444-555555555555','PC-SECRETO','dev','')",
            [],
        )
        .unwrap();
    let id = session_with_blocks(&mut c, &["A"]);
    let a = blocks(&c, &id)[0].id.clone();
    c.db.ddae_start_block(&id, &a).unwrap();
    c.db.ddae_freeze(&id, "pausa").unwrap();
    let json = serde_json::to_string(&c.db.ddae_session(&id).unwrap().events).unwrap();
    let folder = c.folder.to_string_lossy().to_string();
    assert!(
        !json.contains(&folder) && !json.contains("11111111-2222") && !json.contains("PC-SECRETO")
    );
    assert!(!ddae::has_machine_path(&json));
    // Um evento com caminho local é recusado no workspace.
    let mut ws = c.db.export_portable().unwrap();
    ws.ddae[0].events[0]
        .payload
        .insert("path".into(), serde_json::json!("C:\\Users\\x\\app"));
    assert!(portable::validate(&ws).is_err());
}

#[test]
fn legacy_import_records_one_deterministic_event_and_backfill_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("repo");
    std::fs::create_dir_all(folder.join("docs/ddae/sessions")).unwrap();
    let doc = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/ddae/sessions/SESSION-001-machine-context-workspace-foundation.md");
    std::fs::copy(
        doc,
        folder.join("docs/ddae/sessions/SESSION-001-machine-context-workspace-foundation.md"),
    )
    .unwrap();
    let mut db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let project = db.save(None, input("LKR_Lab", &folder)).unwrap().id;
    assert_eq!(
        db.ddae_import_legacy(&project).unwrap(),
        LegacyImport::Imported
    );
    let session = db
        .ddae_overview(&project)
        .unwrap()
        .sessions
        .remove(0)
        .session;
    assert_eq!(
        session.events.len(),
        1,
        "nenhum histórico detalhado é inventado"
    );
    assert_eq!(session.events[0].kind, EventType::LegacyImported);
    let legacy_id = session.events[0].id.clone();
    // Já importada antes dos eventos: o preenchimento recria o MESMO evento, uma única vez.
    db.conn.execute("DELETE FROM ddae_events", []).unwrap();
    ddae::backfill_legacy_events(&db.conn).unwrap();
    ddae::backfill_legacy_events(&db.conn).unwrap();
    let after = db.ddae_session(&session.id).unwrap().events;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, legacy_id);
    assert_eq!(
        db.ddae_session(&session.id)
            .unwrap()
            .blocks
            .iter()
            .filter(|b| b.status == BlockStatus::Completed)
            .count(),
        9,
        "nenhum BLOCK_COMPLETED fictício"
    );
}

// ---- blocos ----

#[test]
fn rename_pending_and_in_progress_blocks_but_never_completed() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A", "B", "C"]);
    let ids: Vec<_> = blocks(&c, &id).into_iter().map(|b| b.id).collect();
    c.db.ddae_rename_block(&id, &ids[2], "C novo").unwrap();
    assert_eq!(blocks(&c, &id)[2].title, "C novo");
    c.db.ddae_start_block(&id, &ids[0]).unwrap();
    c.db.ddae_rename_block(&id, &ids[0], "A em andamento")
        .unwrap();
    assert_eq!(blocks(&c, &id)[0].status, BlockStatus::InProgress);
    c.db.ddae_complete_block(&id, &ids[0]).unwrap();
    assert!(
        c.db.ddae_rename_block(&id, &ids[0], "Outro").is_err(),
        "concluído é histórico"
    );
    assert_eq!(blocks(&c, &id)[0].title, "A em andamento");
    assert!(c.db.ddae_rename_block(&id, &ids[1], "   ").is_err());
    assert!(c.db.ddae_rename_block(&id, "fantasma", "x").is_err());
    assert!(
        c.db.ddae_rename_block(&id, &ids[1], "B").is_err(),
        "mesmo nome"
    );
}

#[test]
fn only_pending_blocks_can_be_removed_and_positions_stay_packed() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A", "B", "C", "D"]);
    let ids: Vec<_> = blocks(&c, &id).into_iter().map(|b| b.id).collect();
    c.db.ddae_start_block(&id, &ids[0]).unwrap();
    assert!(
        c.db.ddae_remove_block(&id, &ids[0]).is_err(),
        "em andamento"
    );
    c.db.ddae_complete_block(&id, &ids[0]).unwrap();
    assert!(c.db.ddae_remove_block(&id, &ids[0]).is_err(), "concluído");
    c.db.ddae_decisions_for_test(&id, &ids[2]);
    let after = c.db.ddae_remove_block(&id, &ids[1]).unwrap();
    assert_eq!(
        after
            .blocks
            .iter()
            .map(|b| b.title.as_str())
            .collect::<Vec<_>>(),
        ["A", "C", "D"]
    );
    // Reordenação preservou a ordem e permite continuar acrescentando.
    c.db.ddae_add_block(&id, "E", "").unwrap();
    assert_eq!(blocks(&c, &id).len(), 4);
    let positions: Vec<i64> =
        c.db.conn
            .prepare("SELECT position FROM ddae_blocks WHERE session_id=?1 ORDER BY position")
            .unwrap()
            .query_map([&id], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
    assert_eq!(positions, [0, 1, 2, 3]);
    assert!(c.db.ddae_remove_block(&id, "fantasma").is_err());
}

trait DecisionHelper {
    fn ddae_decisions_for_test(&mut self, session: &str, block: &str);
}
impl DecisionHelper for Database {
    fn ddae_decisions_for_test(&mut self, session: &str, block: &str) {
        self.ddae_add_decision(session, "Decisão ligada", "", Some(block))
            .unwrap();
    }
}

#[test]
fn removing_a_pending_block_only_detaches_decisions() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A", "B"]);
    let b = blocks(&c, &id)[1].id.clone();
    c.db.ddae_add_decision(&id, "D", "corpo", Some(&b)).unwrap();
    assert_eq!(
        c.db.ddae_session(&id).unwrap().decisions[0]
            .block_id
            .as_deref(),
        Some(b.as_str())
    );
    c.db.ddae_remove_block(&id, &b).unwrap();
    let s = c.db.ddae_session(&id).unwrap();
    assert_eq!(s.decisions.len(), 1, "a decisão é histórico e permanece");
    assert_eq!(s.decisions[0].block_id, None);
}

#[test]
fn completing_the_last_block_does_not_finalize_the_session() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    finish_blocks(&mut c, &id);
    let view = c.db.ddae_session_detail(&c.project, &id).unwrap();
    assert_eq!((view.progress.completed, view.progress.total), (1, 1));
    assert_eq!(view.session.status, SessionStatus::Active);
    assert!(view.can_complete);
    assert!(!types(&c, &id).contains(&EventType::SessionCompleted));
}

#[test]
fn block_description_and_decision_block_validation() {
    let mut c = setup();
    let s = c.db.ddae_create_session(&c.project, "F", "").unwrap();
    let added =
        c.db.ddae_add_block(&s.id, "Com descrição", "Detalhe do bloco")
            .unwrap();
    assert_eq!(added.blocks[0].description, "Detalhe do bloco");
    assert!(c
        .db
        .ddae_add_block(&s.id, "x", &"d".repeat(ddae::MAX_BLOCK_DESCRIPTION + 1))
        .is_err());
    assert!(c.db.ddae_add_block(&s.id, "x", r"C:\Users\x").is_err());
    assert!(c
        .db
        .ddae_add_decision(&s.id, "D", "", Some("bloco-de-outra-session"))
        .is_err());
    assert_eq!(c.db.ddae_session(&s.id).unwrap().decisions.len(), 0);
}

// ---- detalhes: título, número, UUID ----

#[test]
fn title_can_change_but_number_and_uuid_never_do() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    let before = c.db.ddae_session(&id).unwrap();
    let mut d = details_of(&c, &id);
    d.title = Some("  Novo título  ".into());
    let after = c.db.ddae_update_details(&id, d).unwrap();
    assert_eq!(after.title, "Novo título");
    assert_eq!(after.number, before.number);
    assert_eq!(after.id, before.id);
    assert_eq!(after.label(), "SESSION-001");
    let mut d = details_of(&c, &id);
    d.title = Some("   ".into());
    assert!(c.db.ddae_update_details(&id, d).is_err());
    let mut d = details_of(&c, &id);
    d.title = Some(r"C:\Users\x\proj".into());
    assert!(c.db.ddae_update_details(&id, d).is_err());
    // Sem title informado, o título permanece.
    assert_eq!(
        c.db.ddae_update_details(&id, details_of(&c, &id))
            .unwrap()
            .title,
        "Novo título"
    );
    // Salvar sem mudar nada não gera DETAILS_UPDATED.
    let n = types(&c, &id)
        .iter()
        .filter(|t| **t == EventType::DetailsUpdated)
        .count();
    assert_eq!(n, 1, "só o da troca de título");
}

#[test]
fn ready_for_ai_is_recomputed_after_each_plan_edit() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    let ready = |c: &Ctx| ddae::ready_for_ai(&c.db.ddae_session(&id).unwrap());
    assert_eq!(ready(&c).missing, ["desired_outcome", "criteria"]);
    let mut d = details_of(&c, &id);
    d.desired_outcome = "Resultado".into();
    c.db.ddae_update_details(&id, d).unwrap();
    assert_eq!(ready(&c).missing, ["criteria"]);
    let mut d = details_of(&c, &id);
    d.criteria = vec!["Um".into()];
    c.db.ddae_update_details(&id, d).unwrap();
    assert!(ready(&c).ready, "pronto para IA sem nenhum boolean gravado");
}

// ---- arquivos / referências ----

#[test]
fn project_path_references_are_stored_relative_only() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    let s =
        c.db.ddae_add_reference(
            &id,
            ReferenceKind::ProjectPath,
            "docs/plano.md",
            Some("Plano"),
        )
        .unwrap();
    assert_eq!(s.references[0].value, "docs/plano.md");
    assert_eq!(s.references[0].label.as_deref(), Some("Plano"));
    // Um caminho ABSOLUTO dentro do projeto (como o de um seletor) é convertido em relativo.
    let abs = c.folder.join("docs").join("plano.md");
    assert!(
        c.db.ddae_add_reference(
            &id,
            ReferenceKind::ProjectPath,
            &abs.to_string_lossy(),
            None
        )
        .is_err(),
        "duplicada"
    );
    std::fs::write(c.folder.join("docs/outro.md"), "y").unwrap();
    let abs2 = c.folder.join("docs").join("outro.md");
    let s =
        c.db.ddae_add_reference(
            &id,
            ReferenceKind::ProjectPath,
            &abs2.to_string_lossy(),
            None,
        )
        .unwrap();
    assert_eq!(s.references[1].value, "docs/outro.md");
    let json = serde_json::to_string(&c.db.export_portable().unwrap().ddae).unwrap();
    assert!(
        !json.contains(&*c.folder.to_string_lossy()),
        "caminho absoluto nunca é persistido"
    );
}

#[test]
fn references_outside_the_project_or_unsafe_are_rejected() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    let outside = c.folder.parent().unwrap().join("fora.md");
    std::fs::write(&outside, "z").unwrap();
    let err =
        c.db.ddae_add_reference(
            &id,
            ReferenceKind::ProjectPath,
            &outside.to_string_lossy(),
            None,
        )
        .unwrap_err();
    assert!(err.contains("fora da pasta do projeto"), "{err}");
    for bad in [
        "../fora.md",
        "a/../b",
        "/etc/passwd",
        "a//b",
        "C:/Windows/x",
    ] {
        assert!(
            c.db.ddae_add_reference(&id, ReferenceKind::ProjectPath, bad, None)
                .is_err(),
            "{bad}"
        );
    }
    for url in ["http://x.com/a", "https://u:p@x.com/a", "ftp://x"] {
        assert!(
            c.db.ddae_add_reference(&id, ReferenceKind::Url, url, None)
                .is_err(),
            "{url}"
        );
    }
    assert!(c
        .db
        .ddae_add_reference(
            &id,
            ReferenceKind::Url,
            "https://github.com/org/repo",
            Some("Repo")
        )
        .is_ok());
    assert!(c.db.ddae_session(&id).unwrap().references.len() == 1);
    // remover
    let mut d = details_of(&c, &id);
    d.references.clear();
    assert!(c
        .db
        .ddae_update_details(&id, d)
        .unwrap()
        .references
        .is_empty());
}

// ---- notas ----

#[test]
fn notes_can_be_added_and_removed_with_events() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    let mut d = details_of(&c, &id);
    d.notes = vec!["Primeira".into(), "Segunda".into()];
    c.db.ddae_update_details(&id, d).unwrap();
    let mut d = details_of(&c, &id);
    d.notes.remove(0);
    let s = c.db.ddae_update_details(&id, d).unwrap();
    assert_eq!(s.notes, ["Segunda"]);
    let events = c.db.ddae_session(&id).unwrap().events;
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == EventType::NoteAdded)
            .count(),
        2
    );
    let removed = events
        .iter()
        .find(|e| e.kind == EventType::NoteRemoved)
        .unwrap();
    assert_eq!(removed.payload["text"], "Primeira");
    let mut d = details_of(&c, &id);
    d.notes = vec![r"C:\Users\x".into()];
    assert!(c.db.ddae_update_details(&id, d).is_err());
}

// ---- ciclo de vida / one-active ----

#[test]
fn resume_with_another_active_session_is_refused_and_nothing_changes() {
    let mut c = setup();
    let first = session_with_blocks(&mut c, &["A"]);
    c.db.ddae_freeze(&first, "pausa").unwrap();
    let second = session_with_blocks(&mut c, &["B"]);
    let events_before = c.db.ddae_session(&first).unwrap().events.len();
    let err = c.db.ddae_resume(&first).unwrap_err();
    assert!(err.contains("SESSION-002"), "{err}");
    assert_eq!(
        c.db.ddae_session(&first).unwrap().status,
        SessionStatus::Frozen
    );
    assert_eq!(
        c.db.ddae_session(&second).unwrap().status,
        SessionStatus::Active
    );
    assert_eq!(
        c.db.ddae_session(&first).unwrap().events.len(),
        events_before
    );
}

#[test]
fn completed_sessions_accept_no_detail_operation() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A"]);
    finish_blocks(&mut c, &id);
    c.db.ddae_complete(&id, "").unwrap();
    let b = blocks(&c, &id)[0].id.clone();
    assert!(c.db.ddae_update_details(&id, details_of(&c, &id)).is_err());
    assert!(c.db.ddae_rename_block(&id, &b, "x").is_err());
    assert!(c.db.ddae_remove_block(&id, &b).is_err());
    assert!(c
        .db
        .ddae_add_reference(&id, ReferenceKind::Url, "https://a.com/b", None)
        .is_err());
    assert!(c.db.ddae_add_decision(&id, "d", "", None).is_err());
    assert!(c.db.ddae_add_block(&id, "x", "").is_err());
}

#[test]
fn v3_workspace_round_trips_with_a_deterministic_hash() {
    let mut c = setup();
    let id = session_with_blocks(&mut c, &["A", "B"]);
    let mut d = details_of(&c, &id);
    d.desired_outcome = "R".into();
    d.constraints = vec!["K".into()];
    d.criteria = vec!["C1".into()];
    d.notes = vec!["N".into()];
    c.db.ddae_update_details(&id, d).unwrap();
    c.db.ddae_add_reference(
        &id,
        ReferenceKind::ProjectPath,
        "docs/plano.md",
        Some("Plano"),
    )
    .unwrap();
    let a = c.db.ddae_session(&id).unwrap().blocks[0].id.clone();
    c.db.ddae_add_decision(&id, "Decisão", "x", Some(&a))
        .unwrap();
    let ws = c.db.export_portable().unwrap();
    assert_eq!(ws.version, 4);
    portable::validate(&ws).unwrap();
    let text = serde_json::to_string(&ws).unwrap();
    let back: PortableWorkspace = serde_json::from_str(&text).unwrap();
    assert_eq!(back, ws);
    assert_eq!(portable::content_hash(&back), portable::content_hash(&ws));
    assert_eq!(
        portable::content_hash(&ws),
        portable::content_hash(&c.db.export_portable().unwrap())
    );
}
