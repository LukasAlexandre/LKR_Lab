//! Planejamento (Concept 09): fila ordenada, estados derivados, vínculo com a Session, One Active,
//! eventos portáteis e workspace v5.
use hub_core::{
    database::Database,
    ddae::SessionStatus,
    models::ProjectInput,
    planning::{
        derive_phase, MoveTo, Phase, PlanningEventType as Ev, StoredStatus, ACTIVE_SESSION_REASON,
        POSITION_GAP,
    },
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

struct Ctx {
    tmp: tempfile::TempDir,
    db: Database,
    project: String,
}

fn setup() -> Ctx {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("proj");
    std::fs::create_dir_all(&folder).unwrap();
    let mut db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let project = db.save(None, input("Projeto", &folder)).unwrap().id;
    Ctx { tmp, db, project }
}

fn item(c: &mut Ctx, title: &str) -> String {
    c.db.planning_create_item(&c.project, title, "descrição")
        .unwrap()
        .id
}

/// Ids dos itens na ordem da fila (posição crescente).
fn order(c: &Ctx) -> Vec<String> {
    c.db.planning_overview(&c.project)
        .unwrap()
        .items
        .iter()
        .map(|r| r.item.title.clone())
        .collect()
}

fn kinds(c: &Ctx, id: &str) -> Vec<Ev> {
    c.db.planning_events(id)
        .unwrap()
        .iter()
        .map(|e| e.kind)
        .collect()
}

fn phase_of(c: &Ctx, id: &str) -> Phase {
    c.db.planning_overview(&c.project)
        .unwrap()
        .items
        .iter()
        .find(|r| r.item.id == id)
        .unwrap()
        .phase
}

/// Inicia o item (Session ativa vinculada) e devolve o id da Session.
fn start(c: &mut Ctx, id: &str) -> String {
    let draft = c.db.planning_prepare_start(id).unwrap();
    c.db.planning_start_session(id, &draft.title, &draft.objective)
        .unwrap()
        .id
}

fn finish(c: &mut Ctx, session: &str) {
    let s = c.db.ddae_add_block(session, "Único", "").unwrap();
    let block = s.blocks[0].id.clone();
    c.db.ddae_start_block(session, &block).unwrap();
    c.db.ddae_complete_block(session, &block).unwrap();
    c.db.ddae_complete(session, "feito").unwrap();
}

fn count(c: &Ctx, table: &str) -> i64 {
    c.db.conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn user_version(db: &Database) -> i64 {
    db.conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

// ---- migration ----

fn downgrade_to_v9(c: Ctx) -> std::path::PathBuf {
    let Ctx { tmp, db, .. } = c;
    for sql in [
        "DROP TRIGGER ddae_sessions_planning_insert",
        "DROP TRIGGER ddae_sessions_planning_update",
        "DROP TRIGGER ddae_sessions_planning_is_write_once",
        "DROP INDEX ddae_sessions_planning_item",
        "DROP TABLE planning_events",
        "DROP TABLE planning_items",
        "ALTER TABLE ddae_sessions DROP COLUMN planning_item_id",
        "PRAGMA user_version=9",
    ] {
        db.conn.execute_batch(sql).unwrap();
    }
    drop(db);
    let path = tmp.path().join("hub.db");
    std::mem::forget(tmp); // a pasta temporária vive até o fim do processo de teste
    path
}

#[test]
fn migration_v9_to_v10_creates_the_planning_schema() {
    let c = setup();
    let path = downgrade_to_v9(c);
    let db = Database::open(&path).unwrap();
    assert_eq!(user_version(&db), 11);
    for table in ["planning_items", "planning_events"] {
        let n: i64 = db
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{table} nasce vazia");
    }
    let has_column: bool = db
        .conn
        .query_row(
            "SELECT count(*) > 0 FROM pragma_table_info('ddae_sessions') WHERE name='planning_item_id'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(has_column);
}

#[test]
fn migration_preserves_ddae_and_session_without_planning_item() {
    let mut c = setup();
    let legacy = c.db.ddae_create_session(&c.project, "Legada", "x").unwrap();
    c.db.ddae_add_block(&legacy.id, "Bloco", "").unwrap();
    let path = downgrade_to_v9(c);
    let db = Database::open(&path).unwrap();
    let s = db.ddae_session(&legacy.id).unwrap();
    assert_eq!(s.title, "Legada");
    assert_eq!(s.blocks.len(), 1);
    assert_eq!(s.status, SessionStatus::Active);
    assert_eq!(s.planning_item_id, None, "nenhum item retroativo");
    assert!(db
        .planning_overview(&s.project_id)
        .unwrap()
        .items
        .is_empty());
}

// ---- modelo ----

#[test]
fn create_trims_validates_and_appends_to_the_queue() {
    let mut c = setup();
    let a =
        c.db.planning_create_item(&c.project, "  Primeiro  ", "")
            .unwrap();
    assert_eq!(a.title, "Primeiro");
    assert_eq!(a.stored_status, StoredStatus::Open);
    assert_eq!(a.position, POSITION_GAP);
    let b =
        c.db.planning_create_item(&c.project, "Segundo", "Algo")
            .unwrap();
    assert_eq!(b.position, 2 * POSITION_GAP);
    assert!(c.db.planning_create_item(&c.project, "   ", "").is_err());
    assert!(c
        .db
        .planning_create_item(&c.project, &"x".repeat(121), "")
        .is_err());
    assert!(c
        .db
        .planning_create_item(&c.project, "ok", r"C:\Users\x\proj")
        .is_err());
    assert!(c.db.planning_create_item("fantasma", "x", "").is_err());
    assert_eq!(order(&c), ["Primeiro", "Segundo"]);
}

#[test]
fn update_changes_title_and_description_only_while_planned() {
    let mut c = setup();
    let a = item(&mut c, "A");
    let u = c.db.planning_update_item(&a, "A2", "nova").unwrap();
    assert_eq!((u.title.as_str(), u.description.as_str()), ("A2", "nova"));
    assert_eq!(
        kinds(&c, &a),
        [Ev::PlanningItemCreated, Ev::PlanningItemUpdated]
    );
    // Sem mudança: nada novo é gravado.
    c.db.planning_update_item(&a, "A2", "nova").unwrap();
    assert_eq!(kinds(&c, &a).len(), 2);
    assert!(c.db.planning_update_item(&a, "  ", "").is_err());
    start(&mut c, &a);
    assert!(
        c.db.planning_update_item(&a, "Outro", "").is_err(),
        "com Session não edita"
    );
    let b = item(&mut c, "B");
    c.db.planning_cancel(&b, None).unwrap();
    assert!(
        c.db.planning_update_item(&b, "B2", "").is_err(),
        "cancelado não edita"
    );
}

#[test]
fn cancel_and_restore_round_trip_keep_position() {
    let mut c = setup();
    let a = item(&mut c, "A");
    let b = item(&mut c, "B");
    let before = c.db.planning_item(&a).unwrap().position;
    let cancelled = c.db.planning_cancel(&a, Some(" mudou o plano ")).unwrap();
    assert_eq!(cancelled.stored_status, StoredStatus::Cancelled);
    assert_eq!(cancelled.cancel_reason.as_deref(), Some("mudou o plano"));
    assert!(cancelled.cancelled_at.is_some());
    assert_eq!(phase_of(&c, &a), Phase::Cancelled);
    assert!(c.db.planning_cancel(&a, None).is_err(), "já cancelado");
    let restored = c.db.planning_restore(&a).unwrap();
    assert_eq!(restored.stored_status, StoredStatus::Open);
    assert_eq!(restored.cancel_reason, None);
    assert_eq!(restored.cancelled_at, None);
    assert_eq!(restored.position, before, "volta à mesma posição");
    assert!(c.db.planning_restore(&a).is_err(), "só cancelado restaura");
    assert_eq!(order(&c), ["A", "B"]);
    let _ = b;
}

#[test]
fn cancel_is_refused_once_a_session_is_linked() {
    let mut c = setup();
    let a = item(&mut c, "A");
    start(&mut c, &a);
    assert!(c.db.planning_cancel(&a, None).is_err());
    // O banco também recusa (SQL direto).
    assert!(c
        .db
        .conn
        .execute(
            "UPDATE planning_items SET stored_status='cancelled',cancelled_at='x' WHERE id=?1",
            [&a]
        )
        .is_err());
}

#[test]
fn ordering_move_up_down_and_top() {
    let mut c = setup();
    let [a, b, cc, d] = ["A", "B", "C", "D"].map(|t| item(&mut c, t));
    c.db.planning_move(&cc, MoveTo::Up).unwrap();
    assert_eq!(order(&c), ["A", "C", "B", "D"]);
    c.db.planning_move(&a, MoveTo::Down).unwrap();
    assert_eq!(order(&c), ["C", "A", "B", "D"]);
    c.db.planning_move(&d, MoveTo::Top).unwrap();
    assert_eq!(order(&c), ["D", "C", "A", "B"]);
    // Limites: já no topo / no fim.
    assert!(c.db.planning_move(&d, MoveTo::Up).is_err());
    assert!(c.db.planning_move(&d, MoveTo::Top).is_err());
    assert!(c.db.planning_move(&b, MoveTo::Down).is_err());
    c.db.planning_move(&d, MoveTo::Down).unwrap();
    assert_eq!(order(&c), ["C", "D", "A", "B"]);
}

#[test]
fn move_renormalizes_when_there_is_no_gap_and_never_emits_events() {
    let mut c = setup();
    let ids: Vec<String> = (0..5).map(|n| item(&mut c, &format!("I{n}"))).collect();
    // Aperta as posições para esgotar o espaço entre vizinhos.
    for (n, id) in ids.iter().enumerate() {
        c.db.conn
            .execute(
                "UPDATE planning_items SET position=?2 WHERE id=?1",
                rusqlite_params(id, 10 + n as i64),
            )
            .unwrap();
    }
    let events_before = count(&c, "planning_events");
    c.db.planning_move(&ids[4], MoveTo::Top).unwrap();
    assert_eq!(order(&c), ["I4", "I0", "I1", "I2", "I3"]);
    let positions: Vec<i64> =
        c.db.planning_overview(&c.project)
            .unwrap()
            .items
            .iter()
            .map(|r| r.item.position)
            .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    assert_eq!(
        count(&c, "planning_events"),
        events_before,
        "reordenar não gera evento"
    );
}

fn rusqlite_params(id: &str, position: i64) -> [Box<dyn rusqlite::ToSql>; 2] {
    [Box::new(id.to_string()), Box::new(position)]
}

#[test]
fn cancelled_items_keep_their_place_when_others_move() {
    let mut c = setup();
    let [a, b, d] = ["A", "B", "C"].map(|t| item(&mut c, t));
    c.db.planning_cancel(&b, None).unwrap();
    c.db.planning_move(&d, MoveTo::Up).unwrap();
    // C passa por cima de A (o cancelado B fica fora da fila de planejados).
    let o = c.db.planning_overview(&c.project).unwrap();
    let planned: Vec<&str> = o
        .items
        .iter()
        .filter(|r| r.phase == Phase::Planned)
        .map(|r| r.item.title.as_str())
        .collect();
    assert_eq!(planned, ["C", "A"]);
    assert!(
        c.db.planning_move(&b, MoveTo::Top).is_err(),
        "cancelado não entra na fila"
    );
    let _ = a;
}

// ---- estados derivados ----

#[test]
fn derive_phase_is_a_pure_function_of_stored_status_and_session() {
    use StoredStatus::*;
    assert_eq!(derive_phase(Open, None), Phase::Planned);
    assert_eq!(
        derive_phase(Open, Some(SessionStatus::Active)),
        Phase::Executing
    );
    assert_eq!(
        derive_phase(Open, Some(SessionStatus::Frozen)),
        Phase::Executing
    );
    assert_eq!(
        derive_phase(Open, Some(SessionStatus::Stopped)),
        Phase::Executing
    );
    assert_eq!(
        derive_phase(Open, Some(SessionStatus::Completed)),
        Phase::Completed
    );
    assert_eq!(derive_phase(Cancelled, None), Phase::Cancelled);
}

#[test]
fn overview_derives_planned_executing_frozen_stopped_completed_and_cancelled() {
    let mut c = setup();
    let [planned, active, frozen, stopped, done, cancelled] =
        ["P", "A", "F", "S", "D", "X"].map(|t| item(&mut c, t));
    assert_eq!(phase_of(&c, &planned), Phase::Planned);
    // Concluído: inicia, finaliza.
    let s_done = start(&mut c, &done);
    finish(&mut c, &s_done);
    assert_eq!(phase_of(&c, &done), Phase::Completed);
    // Parado e congelado (a ativa anterior já foi finalizada).
    let s_stopped = start(&mut c, &stopped);
    c.db.ddae_stop(&s_stopped, "pausa").unwrap();
    let s_frozen = start(&mut c, &frozen);
    c.db.ddae_freeze(&s_frozen, "").unwrap();
    let s_active = start(&mut c, &active);
    c.db.planning_cancel(&cancelled, None).unwrap();

    let o = c.db.planning_overview(&c.project).unwrap();
    let row = |id: &str| o.items.iter().find(|r| r.item.id == id).unwrap();
    assert_eq!(row(&active).phase, Phase::Executing);
    assert_eq!(
        row(&active).session.as_ref().unwrap().status,
        SessionStatus::Active
    );
    assert_eq!(row(&frozen).phase, Phase::Executing);
    assert_eq!(
        row(&frozen).session.as_ref().unwrap().status,
        SessionStatus::Frozen
    );
    assert_eq!(row(&stopped).phase, Phase::Executing);
    assert_eq!(
        row(&stopped).session.as_ref().unwrap().status,
        SessionStatus::Stopped
    );
    assert_eq!(row(&done).phase, Phase::Completed);
    assert_eq!(row(&done).session.as_ref().unwrap().progress.completed, 1);
    assert_eq!(row(&cancelled).phase, Phase::Cancelled);
    assert_eq!(row(&planned).session.as_ref().map(|s| s.id.clone()), None);
    assert_eq!(row(&active).session.as_ref().unwrap().id, s_active);
    // Cancelado fica fora do total operacional.
    assert_eq!(o.counts.planned, 1);
    assert_eq!(o.counts.executing, 3);
    assert_eq!(o.counts.completed, 1);
    assert_eq!(o.counts.cancelled, 1);
    assert_eq!(o.counts.operational, 5);
    // Nada disso é gravado no item.
    for r in &o.items {
        assert!(matches!(
            r.item.stored_status,
            StoredStatus::Open | StoredStatus::Cancelled
        ));
    }
}

// ---- relação com a Session ----

#[test]
fn start_creates_the_session_through_the_ddae_flow_already_linked() {
    let mut c = setup();
    let a =
        c.db.planning_create_item(&c.project, "Feature X", "Fazer X")
            .unwrap();
    let draft = c.db.planning_prepare_start(&a.id).unwrap();
    assert_eq!(
        (draft.title.as_str(), draft.objective.as_str()),
        ("Feature X", "Fazer X")
    );
    assert!(draft.can_start);
    assert_eq!(count(&c, "ddae_sessions"), 0, "preparar não cria nada");
    let s =
        c.db.planning_start_session(&a.id, "Título revisado", "Objetivo revisado")
            .unwrap();
    assert_eq!(s.title, "Título revisado");
    assert_eq!(s.planning_item_id.as_deref(), Some(a.id.as_str()));
    assert_eq!(s.number, 1);
    assert_eq!(s.status, SessionStatus::Active);
    // O histórico da Session registra a origem, sem evento extra.
    let created = &c.db.ddae_session(&s.id).unwrap().events[0];
    assert_eq!(
        created
            .payload
            .get("planningItemId")
            .and_then(|v| v.as_str()),
        Some(a.id.as_str())
    );
    assert_eq!(phase_of(&c, &a.id), Phase::Executing);
    // A view do DDAE traz a origem; sessão sem item não traz nada.
    let view = c.db.ddae_overview(&c.project).unwrap();
    assert_eq!(
        view.sessions[0].planning_item.as_ref().unwrap().title,
        "Feature X"
    );
    let detail = c.db.ddae_session_detail(&c.project, &s.id).unwrap();
    assert_eq!(detail.planning_item.unwrap().id, a.id);
}

#[test]
fn sessions_created_outside_planning_have_no_planning_item() {
    let mut c = setup();
    let s = c.db.ddae_create_session(&c.project, "Legada", "").unwrap();
    assert_eq!(s.planning_item_id, None);
    let detail = c.db.ddae_session_detail(&c.project, &s.id).unwrap();
    assert!(detail.planning_item.is_none());
    // O vínculo é só da criação: o banco recusa dar um item retroativo.
    let a = item(&mut c, "A");
    assert!(c
        .db
        .conn
        .execute(
            "UPDATE ddae_sessions SET planning_item_id=?2 WHERE id=?1",
            [&s.id, &a]
        )
        .is_err());
}

#[test]
fn wrong_project_is_rejected_by_the_database() {
    let mut c = setup();
    let other_dir = c.tmp.path().join("outro");
    std::fs::create_dir_all(&other_dir).unwrap();
    let other = c.db.save(None, input("Outro", &other_dir)).unwrap().id;
    let a = item(&mut c, "A");
    // Pelo SQL, uma Session de outro Project não pode apontar para o item.
    let r = c.db.conn.execute(
        "INSERT INTO ddae_sessions(id,project_id,number,title,objective,status,created_at,updated_at,planning_item_id) \
         VALUES('s-x',?1,1,'T','', 'frozen','2026-01-01','2026-01-01',?2)",
        [&other, &a],
    );
    assert!(r.is_err());
    // E uma Session do Project certo é aceita.
    c.db.conn
        .execute(
            "INSERT INTO ddae_sessions(id,project_id,number,title,objective,status,created_at,updated_at,planning_item_id) \
             VALUES('s-ok',?1,1,'T','', 'frozen','2026-01-01','2026-01-01',?2)",
            [&c.project, &a],
        )
        .unwrap();
}

#[test]
fn an_item_cannot_link_two_sessions() {
    let mut c = setup();
    let a = item(&mut c, "A");
    let s = start(&mut c, &a);
    c.db.ddae_freeze(&s, "").unwrap();
    // API: o item já foi iniciado.
    assert!(!c.db.planning_prepare_start(&a).unwrap().can_start);
    assert!(c.db.planning_start_session(&a, "Outra", "").is_err());
    // Banco: índice único parcial.
    assert!(c
        .db
        .conn
        .execute(
            "INSERT INTO ddae_sessions(id,project_id,number,title,objective,status,created_at,updated_at,planning_item_id) \
             VALUES('s-2',?1,2,'T','', 'frozen','2026-01-01','2026-01-01',?2)",
            [&c.project, &a],
        )
        .is_err());
}

#[test]
fn a_linked_session_cannot_be_repointed_to_a_second_item() {
    let mut c = setup();
    let a = item(&mut c, "A");
    let b = item(&mut c, "B");
    let s = start(&mut c, &a);
    assert!(c
        .db
        .conn
        .execute(
            "UPDATE ddae_sessions SET planning_item_id=?2 WHERE id=?1",
            [&s, &b]
        )
        .is_err());
    assert!(c
        .db
        .conn
        .execute(
            "UPDATE ddae_sessions SET planning_item_id=NULL WHERE id=?1",
            [&s]
        )
        .is_err());
    assert_eq!(
        c.db.ddae_session(&s).unwrap().planning_item_id.as_deref(),
        Some(a.as_str())
    );
}

#[test]
fn cancelled_item_cannot_link_a_session() {
    let mut c = setup();
    let a = item(&mut c, "A");
    c.db.planning_cancel(&a, None).unwrap();
    assert!(!c.db.planning_prepare_start(&a).unwrap().can_start);
    assert!(c.db.planning_start_session(&a, "T", "").is_err());
    assert_eq!(count(&c, "ddae_sessions"), 0);
    assert!(c
        .db
        .conn
        .execute(
            "INSERT INTO ddae_sessions(id,project_id,number,title,objective,status,created_at,updated_at,planning_item_id) \
             VALUES('s-x',?1,1,'T','', 'frozen','2026-01-01','2026-01-01',?2)",
            [&c.project, &a],
        )
        .is_err());
}

// ---- One Active ----

#[test]
fn start_is_blocked_while_the_project_has_an_active_session() {
    let mut c = setup();
    let first = item(&mut c, "Primeiro");
    let second = item(&mut c, "Segundo");
    start(&mut c, &first);
    let draft = c.db.planning_prepare_start(&second).unwrap();
    assert!(!draft.can_start);
    assert_eq!(
        draft.disabled_reason.as_deref(),
        Some(ACTIVE_SESSION_REASON)
    );
    assert!(c.db.planning_start_session(&second, "T", "").is_err());
    assert_eq!(
        count(&c, "ddae_sessions"),
        1,
        "nada foi criado nem congelado"
    );
    let o = c.db.planning_overview(&c.project).unwrap();
    // PRÓXIMO continua sendo o primeiro planejado, mas com Iniciar desabilitado.
    let next = o.next.unwrap();
    assert_eq!(next.id, second);
    assert!(!next.can_start);
    assert_eq!(next.disabled_reason.as_deref(), Some(ACTIVE_SESSION_REASON));
    assert!(o
        .items
        .iter()
        .filter(|r| r.phase == Phase::Planned)
        .all(|r| !r.can_start));
}

#[test]
fn a_session_without_planning_also_blocks_start() {
    let mut c = setup();
    c.db.ddae_create_session(&c.project, "SESSION-001 legada", "")
        .unwrap();
    let a = item(&mut c, "A");
    assert!(!c.db.planning_prepare_start(&a).unwrap().can_start);
    let o = c.db.planning_overview(&c.project).unwrap();
    assert_eq!(o.active_session.unwrap().label, "SESSION-001");
}

#[test]
fn start_is_allowed_without_an_active_session() {
    let mut c = setup();
    let a = item(&mut c, "A");
    let o = c.db.planning_overview(&c.project).unwrap();
    assert!(o.next.as_ref().unwrap().can_start);
    assert!(o.active_session.is_none());
    let s = start(&mut c, &a);
    c.db.ddae_freeze(&s, "").unwrap();
    // Congelada libera o próximo.
    let b = item(&mut c, "B");
    assert!(c.db.planning_prepare_start(&b).unwrap().can_start);
}

// ---- eventos ----

#[test]
fn events_cover_create_cancel_restore_and_link_and_are_append_only() {
    let mut c = setup();
    let a = item(&mut c, "A");
    c.db.planning_cancel(&a, Some("motivo")).unwrap();
    c.db.planning_restore(&a).unwrap();
    let s = start(&mut c, &a);
    assert_eq!(
        kinds(&c, &a),
        [
            Ev::PlanningItemCreated,
            Ev::PlanningItemCancelled,
            Ev::PlanningItemRestored,
            Ev::PlanningItemSessionLinked
        ]
    );
    let events = c.db.planning_events(&a).unwrap();
    assert_eq!(
        events[1].payload.get("reason").and_then(|v| v.as_str()),
        Some("motivo")
    );
    assert_eq!(
        events[3].payload.get("sessionId").and_then(|v| v.as_str()),
        Some(s.as_str())
    );
    assert_eq!(
        events[3].payload.get("label").and_then(|v| v.as_str()),
        Some("SESSION-001")
    );
    // Append-only: nenhum UPDATE.
    assert!(c
        .db
        .conn
        .execute("UPDATE planning_events SET event_type='X'", [])
        .is_err());
}

#[test]
fn events_are_portable_without_local_data() {
    let mut c = setup();
    let a = item(&mut c, "A");
    start(&mut c, &a);
    let ws = c.db.export_portable().unwrap();
    assert_eq!(ws.planning_events.len(), 2);
    let text = serde_json::to_string(&ws).unwrap();
    assert!(!text.contains(&c.tmp.path().to_string_lossy().to_string()));
    portable::validate(&ws).unwrap();
    // Evento com caminho local é recusado ao receber o workspace.
    let mut bad = ws.clone();
    bad.planning_events[0]
        .payload
        .insert("title".into(), serde_json::json!(r"C:\Users\x\proj"));
    assert!(portable::validate(&bad).is_err());
    // Evento de item inexistente também.
    let mut orphan = ws.clone();
    orphan.planning_events[0].item_id = "fantasma".into();
    assert!(portable::validate(&orphan).is_err());
}

// ---- workspace v5 ----

fn legacy(version: u32) -> PortableWorkspace {
    let text = format!(
        r#"{{"version":{version},"projects":[{{"id":"p1","name":"P"}}],"prompts":[],"knowledge":[]}}"#
    );
    serde_json::from_str(&text).unwrap()
}

#[test]
fn legacy_workspaces_v1_to_v4_stay_readable_with_empty_planning() {
    let mut hashes = vec![];
    for version in 1..=5 {
        let mut ws = legacy(version);
        portable::validate(&ws).unwrap();
        portable::normalize(&mut ws);
        assert_eq!(ws.version, 5, "v{version} vira v5 ao normalizar");
        assert!(ws.planning_items.is_empty() && ws.planning_events.is_empty());
        hashes.push(portable::content_hash(&ws));
    }
    assert!(
        hashes.windows(2).all(|p| p[0] == p[1]),
        "mesmo conteúdo, mesmo hash"
    );
    // v4 com Session sem planningItemId continua legível: o vínculo é nulo.
    let v4 = r#"{"version":4,"projects":[{"id":"p1","name":"P"}],"prompts":[],"knowledge":[],
        "ddae":[{"id":"s1","projectId":"p1","number":1,"title":"T","status":"active","blocks":[{"id":"b1","title":"B","status":"pending"}]}]}"#;
    let mut ws: PortableWorkspace = serde_json::from_str(v4).unwrap();
    portable::normalize(&mut ws);
    portable::validate(&ws).unwrap();
    assert_eq!(ws.ddae[0].planning_item_id, None);
}

fn sample() -> PortableWorkspace {
    let text = r#"{"version":5,"projects":[{"id":"p1","name":"P"}],"prompts":[],"knowledge":[],
      "planningItems":[
        {"id":"i2","projectId":"p1","title":"Segundo","position":2000,"storedStatus":"open","createdAt":"2026-01-02T00:00:00.000Z","updatedAt":"2026-01-02T00:00:00.000Z"},
        {"id":"i1","projectId":"p1","title":"Primeiro","description":"d","position":1000,"storedStatus":"open","createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z"},
        {"id":"i3","projectId":"p1","title":"Cancelado","position":3000,"storedStatus":"cancelled","cancelReason":"não vale","cancelledAt":"2026-01-03T00:00:00.000Z","createdAt":"2026-01-03T00:00:00.000Z","updatedAt":"2026-01-03T00:00:00.000Z"}],
      "planningEvents":[
        {"id":"e2","itemId":"i1","type":"PLANNING_ITEM_SESSION_LINKED","payload":{"label":"SESSION-001"},"createdAt":"2026-01-01T00:00:02.000Z"},
        {"id":"e1","itemId":"i1","type":"PLANNING_ITEM_CREATED","payload":{"title":"Primeiro"},"createdAt":"2026-01-01T00:00:01.000Z"}],
      "ddae":[{"id":"s1","projectId":"p1","number":1,"title":"Primeiro","status":"active","planningItemId":"i1","blocks":[{"id":"b1","title":"B","status":"pending"}]}]}"#;
    serde_json::from_str(text).unwrap()
}

#[test]
fn v5_validates_normalizes_canonically_and_hashes_deterministically() {
    let mut ws = sample();
    portable::normalize(&mut ws);
    portable::validate(&ws).unwrap();
    let ids: Vec<&str> = ws.planning_items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["i1", "i2", "i3"], "itens por (projeto, posição, id)");
    let events: Vec<&str> = ws.planning_events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(events, ["e1", "e2"], "eventos por (createdAt, id)");
    let h = portable::content_hash(&ws);
    // A ordem de entrada e os carimbos de data não mudam o hash.
    let mut shuffled = sample();
    shuffled.planning_items.reverse();
    shuffled.planning_events.reverse();
    shuffled.planning_items[0].updated_at = "2030-01-01T00:00:00.000Z".into();
    assert_eq!(portable::content_hash(&shuffled), h);
    // Mudar o CONTEÚDO (a posição) muda o hash.
    let mut moved = sample();
    moved.planning_items[0].position = 5000;
    assert_ne!(portable::content_hash(&moved), h);
}

#[test]
fn v5_rejects_inconsistent_planning_state() {
    let ok = |f: &dyn Fn(&mut PortableWorkspace)| {
        let mut ws = sample();
        f(&mut ws);
        portable::validate(&ws).is_ok()
    };
    assert!(ok(&|_| {}));
    assert!(!ok(&|w| w.version = 4), "planejamento exige v5");
    assert!(!ok(&|w| w.planning_items[0].project_id = "fantasma".into()));
    assert!(
        !ok(&|w| w.planning_items[1].id = "i2".into()),
        "id duplicado"
    );
    assert!(
        !ok(&|w| w.planning_items[0].position = 1000),
        "posição repetida"
    );
    assert!(!ok(&|w| w.planning_items[0].title = "  ".into()));
    assert!(
        !ok(&|w| w.planning_items[0].cancelled_at = Some("x".into())),
        "aberto com cancelledAt"
    );
    assert!(
        !ok(&|w| w.planning_items[2].cancelled_at = None),
        "cancelado sem cancelledAt"
    );
    assert!(
        !ok(&|w| w.ddae[0].planning_item_id = Some("i3".into())),
        "item cancelado com Session"
    );
    assert!(!ok(&|w| w.ddae[0].planning_item_id = Some("nada".into())));
    assert!(
        !ok(&|w| {
            let mut s2 = w.ddae[0].clone();
            s2.id = "s2".into();
            s2.number = 2;
            s2.status = SessionStatus::Frozen;
            w.ddae.push(s2);
        }),
        "dois Sessions no mesmo item"
    );
    assert!(ok(&|w| w.ddae[0].planning_item_id = None));
}

#[test]
fn v5_round_trips_through_sqlite_and_back() {
    let mut a = setup();
    let first = item(&mut a, "Primeiro");
    let second = item(&mut a, "Segundo");
    let third = item(&mut a, "Terceiro");
    let fourth = item(&mut a, "Quarto");
    start(&mut a, &first);
    a.db.planning_cancel(&third, Some("depois")).unwrap();
    a.db.planning_move(&fourth, MoveTo::Top).unwrap();
    let _ = second;
    let exported = a.db.export_portable().unwrap();
    portable::validate(&exported).unwrap();
    assert_eq!(exported.version, 5);
    assert_eq!(exported.planning_items.len(), 4);
    assert!(exported.ddae[0].planning_item_id.is_some());

    // Outra máquina aplica o workspace e exporta o mesmo conteúdo (mesmo hash).
    let tmp = tempfile::tempdir().unwrap();
    let mut b = Database::open(&tmp.path().join("b.db")).unwrap();
    let summary = b.apply_portable(&exported).unwrap();
    assert_eq!(summary.planning_items, 4);
    let again = b.export_portable().unwrap();
    assert_eq!(again, exported);
    assert_eq!(
        portable::content_hash(&again),
        portable::content_hash(&exported)
    );
    let o = b.planning_overview(&a.project).unwrap();
    assert_eq!(o.counts.executing, 1);
    assert_eq!(o.counts.planned, 2);
    assert_eq!(o.counts.cancelled, 1);
    // "Topo" é o topo da FILA de planejados: fica logo depois do item já em execução.
    assert_eq!(
        o.items
            .iter()
            .map(|r| r.item.title.as_str())
            .collect::<Vec<_>>(),
        ["Primeiro", "Quarto", "Segundo", "Terceiro"]
    );

    // Aplicar um workspace sem Planejamento limpa os itens (o workspace manda).
    let mut empty = exported.clone();
    empty.planning_items.clear();
    empty.planning_events.clear();
    empty.ddae[0].planning_item_id = None;
    b.apply_portable(&empty).unwrap();
    assert!(b.planning_overview(&a.project).unwrap().items.is_empty());
}

#[test]
fn applying_a_workspace_can_cancel_an_item_whose_session_link_was_dropped() {
    let mut a = setup();
    let first = item(&mut a, "Primeiro");
    start(&mut a, &first);
    let mut ws = a.db.export_portable().unwrap();
    // O novo estado cancela o item e solta o vínculo da Session.
    ws.ddae[0].planning_item_id = None;
    ws.planning_items[0].stored_status = StoredStatus::Cancelled;
    ws.planning_items[0].cancelled_at = Some("2026-02-01T00:00:00.000Z".into());
    portable::validate(&ws).unwrap();
    a.db.apply_portable(&ws).unwrap();
    assert_eq!(phase_of(&a, &first), Phase::Cancelled);
}

#[test]
fn exported_planning_state_has_no_local_data() {
    let mut c = setup();
    let a = item(&mut c, "A");
    start(&mut c, &a);
    let ws = c.db.export_portable().unwrap();
    let text = serde_json::to_string(&ws).unwrap().to_lowercase();
    for forbidden in [
        "localpath",
        "machine",
        "hostname",
        "binding",
        "c:\\\\",
        "/users/",
    ] {
        assert!(
            !text.contains(forbidden),
            "{forbidden} vazou para o workspace"
        );
    }
    assert!(!text.contains(
        &c.tmp
            .path()
            .to_string_lossy()
            .to_lowercase()
            .replace('\\', "\\\\")
    ));
}

// ---- passividade ----

#[test]
fn reading_the_planning_overview_writes_nothing() {
    let mut c = setup();
    let a = item(&mut c, "A");
    item(&mut c, "B");
    start(&mut c, &a);
    let snapshot = |c: &Ctx| {
        (
            count(c, "planning_items"),
            count(c, "planning_events"),
            count(c, "ddae_sessions"),
            count(c, "ddae_events"),
            count(c, "activities"),
            c.db.export_portable().unwrap(),
        )
    };
    let before = snapshot(&c);
    for _ in 0..3 {
        c.db.planning_overview(&c.project).unwrap();
        c.db.planning_summary(&c.project).unwrap();
        c.db.planning_prepare_start(&a).unwrap();
        c.db.planning_events(&a).unwrap();
        c.db.ddae_overview(&c.project).unwrap();
    }
    assert_eq!(snapshot(&c), before);
}

#[test]
fn next_item_is_the_first_planned_and_summary_matches_overview() {
    let mut c = setup();
    assert!(c.db.planning_overview(&c.project).unwrap().next.is_none());
    let a = item(&mut c, "A");
    let b = item(&mut c, "B");
    c.db.planning_cancel(&a, None).unwrap();
    let o = c.db.planning_overview(&c.project).unwrap();
    assert_eq!(
        o.next.as_ref().unwrap().id,
        b,
        "cancelado nunca é o PRÓXIMO"
    );
    let s = c.db.planning_summary(&c.project).unwrap();
    assert_eq!(s.counts, o.counts);
    assert_eq!(s.next.unwrap().id, b);
}
