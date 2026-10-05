//! Block 10 — Validation & Hardening. Tudo aqui prova um comportamento (não presume): migrações,
//! atomicidade, concorrência, persistência após restart, limites de histórico, falha de gravação,
//! atribuição com um Worktree real e redaction de formatos reais de linha de comando.
use hub_core::{
    alert_store::RESOLVED_KEEP,
    control_plane::{self, attribute, Confidence as Attribution, Context, ProjectRef, WorktreeRef},
    database::Database,
    diagnostic_runner::{catalog, RunRecord, RunResult},
    diagnostics::*,
    health::{self, HealthStatus},
    models::ProjectInput,
    system::{self, RawProcess},
};
use rusqlite::Connection;
use std::{
    collections::HashMap,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const NOW: i64 = 1_800_000_000_000;
const GIB: u64 = 1024 * 1024 * 1024;
const LATEST: i64 = 11;

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

fn version(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn tables(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    let names = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    names
}

// ------------------------------------------------------------------ migrações

#[test]
fn a_fresh_database_reaches_the_latest_schema_with_every_table() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    drop(Database::open(&path).unwrap());
    assert_eq!(version(&path), LATEST);
    let names = tables(&path);
    for table in [
        "projects",
        "machine",
        "ddae_sessions",
        "managed_worktrees",
        "worktree_bindings",
        "planning_items",
        "planning_events",
        "machine_alerts",
        "machine_diagnostic_runs",
    ] {
        assert!(
            names.iter().any(|n| n == table),
            "faltou {table} em {names:?}"
        );
    }
}

#[test]
fn v10_to_v11_adds_the_alert_tables_and_preserves_everything_else() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    let folder = tmp.path().join("proj");
    std::fs::create_dir_all(&folder).unwrap();
    let (project, session, item) = {
        let mut db = Database::open(&path).unwrap();
        let project = db.save(None, input("Projeto", &folder)).unwrap().id;
        let session = db
            .ddae_create_session(&project, "SESSION-001 legada", "obj")
            .unwrap()
            .id;
        let item = db
            .planning_create_item(&project, "Item", "descrição")
            .unwrap()
            .id;
        for sql in [
            "DROP TABLE machine_alerts",
            "DROP TABLE machine_diagnostic_runs",
            "PRAGMA user_version=10",
        ] {
            db.conn.execute_batch(sql).unwrap();
        }
        (project, session, item)
    };
    assert_eq!(version(&path), 10);
    let db = Database::open(&path).unwrap();
    assert_eq!(version(&path), LATEST);
    assert!(db.alerts_latest().unwrap().is_empty());
    assert!(db.diagnostic_runs(5).unwrap().is_empty());
    assert_eq!(db.project(&project).unwrap().name, "Projeto");
    assert_eq!(
        db.ddae_session(&session).unwrap().title,
        "SESSION-001 legada"
    );
    assert_eq!(db.planning_overview(&project).unwrap().items.len(), 1);
    assert_eq!(
        db.planning_overview(&project).unwrap().items[0].item.id,
        item
    );
    // e as tabelas novas funcionam logo depois da migração
    let snapshot = db.alerts_run(&low_disk(4), NOW, view()).unwrap();
    assert_eq!(snapshot.alerts.len(), 1);
}

#[test]
fn reopening_a_latest_database_is_idempotent_and_keeps_the_alerts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    {
        let db = Database::open(&path).unwrap();
        db.alerts_run(&low_disk(4), NOW, view()).unwrap();
    }
    let before = Database::open(&path).unwrap().alerts_latest().unwrap();
    for _ in 0..3 {
        drop(Database::open(&path).unwrap());
    }
    assert_eq!(version(&path), LATEST);
    let after = Database::open(&path).unwrap().alerts_latest().unwrap();
    assert_eq!(before, after);
    assert_eq!(after.len(), 1);
}

#[test]
fn a_failing_migration_rolls_back_atomically_and_reports_the_error() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    {
        let db = Database::open(&path).unwrap();
        for sql in [
            "DROP TABLE machine_alerts",
            "DROP TABLE machine_diagnostic_runs",
            // tabela de mesmo nome e incompatível: a 011 não pode ser aplicada por inteiro
            "CREATE TABLE machine_alerts(id TEXT)",
            "PRAGMA user_version=10",
        ] {
            db.conn.execute_batch(sql).unwrap();
        }
    }
    let error = match Database::open(&path) {
        Ok(_) => panic!("a migração incompatível deveria falhar"),
        Err(error) => error,
    };
    assert!(!error.is_empty(), "erro explícito");
    // nada parcial: a versão não avançou e a segunda tabela da 011 não foi criada
    assert_eq!(version(&path), 10);
    let names = tables(&path);
    assert!(
        !names.iter().any(|n| n == "machine_diagnostic_runs"),
        "{names:?}"
    );
    // e os dados anteriores continuam acessíveis a um build que entenda o estado
    assert!(names.iter().any(|n| n == "projects"));
}

#[test]
fn a_database_from_a_newer_build_is_refused_without_touching_the_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("future.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE marcador(x INTEGER); PRAGMA user_version=12;")
        .unwrap();
    let error = match Database::open(&path) {
        Ok(_) => panic!("banco futuro deveria ser recusado"),
        Err(error) => error,
    };
    assert!(error.contains("mais recente"), "{error}");
    assert_eq!(version(&path), 12);
    assert_eq!(
        tables(&path),
        vec!["marcador".to_string()],
        "nenhuma tabela foi criada ou alterada"
    );
}

// ------------------------------------------------------------------ fatos de alerta (reuso)

fn low_disk(available_gib: u64) -> Facts {
    low_disk_on("C:", available_gib)
}

fn low_disk_on(mount: &str, available_gib: u64) -> Facts {
    Facts {
        machine: Some(MachineFacts {
            volumes: Some(vec![VolumeFact {
                mount: mount.into(),
                total: 200 * GIB,
                available: available_gib * GIB,
            }]),
            load_ready: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn view() -> DiagnosticsView {
    DiagnosticsView {
        elevated: false,
        catalog: catalog(false),
        targets: vec![],
        current: None,
        history: vec![],
    }
}

// ------------------------------------------------------------------ SQLite: concorrência

#[test]
fn concurrent_connections_never_hit_database_is_locked() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    drop(Database::open(&path).unwrap());
    let started = Instant::now();
    let handles: Vec<_> = (0..4)
        .map(|worker| {
            let path = path.clone();
            std::thread::spawn(move || -> Result<(), String> {
                let db = Database::open(&path)?;
                // cada worker tem o próprio volume (fingerprint): o que se testa é o travamento do arquivo
                let mount = format!("{}:", (b'C' + worker as u8) as char);
                for round in 0..40i64 {
                    let gib = if round % 2 == 0 { 4 } else { 150 };
                    db.alerts_run(&low_disk_on(&mount, gib), NOW + round * 100_000, view())?;
                    db.diagnostic_run_save(&RunRecord {
                        id: format!("run-{worker}-{round}"),
                        diagnostic: "chkdsk_scan".into(),
                        label: "x".into(),
                        target: Some(mount.clone()),
                        started_at: NOW + round,
                        finished_at: Some(NOW + round + 1),
                        running: false,
                        result: Some(RunResult::Clean),
                        exit_code: Some(0),
                        summary: "ok".into(),
                        output_tail: vec![],
                    })?;
                    db.diagnostic_runs(5)?;
                    db.alerts_visible(NOW)?;
                }
                Ok(())
            })
        })
        .collect();
    for handle in handles {
        handle
            .join()
            .expect("thread sem panic")
            .expect("nenhum erro, em especial 'database is locked'");
    }
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "sem esperas anormais"
    );
    let db = Database::open(&path).unwrap();
    assert_eq!(
        db.diagnostic_runs(1000).unwrap().len(),
        hub_core::diagnostic_runner::HISTORY_KEEP
    );
    // cada volume ficou com exatamente uma linha aberta ou resolvida por ocorrência
    let latest = db.alerts_latest().unwrap();
    assert_eq!(latest.len(), 4);
}

#[test]
fn the_database_uses_wal_and_a_busy_timeout() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let mode: String = db
        .conn
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    let timeout: i64 = db
        .conn
        .pragma_query_value(None, "busy_timeout", |r| r.get(0))
        .unwrap();
    assert!(timeout >= 5000, "busy_timeout = {timeout}");
    let fk: i64 = db
        .conn
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1);
}

// ------------------------------------------------------------------ restart / recovery

#[test]
fn alert_lifecycle_survives_restarts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    // 1ª execução do app: abre o alerta e o usuário reconhece
    let id = {
        let db = Database::open(&path).unwrap();
        let snapshot = db.alerts_run(&low_disk(4), NOW, view()).unwrap();
        let id = snapshot.alerts[0].alert_id();
        db.alert_acknowledge(&id, NOW + 1000).unwrap();
        id
    };
    // restart: reconhecido permanece, dedup continua, ocorrência não zera
    {
        let db = Database::open(&path).unwrap();
        let snapshot = db.alerts_run(&low_disk(4), NOW + 60_000, view()).unwrap();
        assert_eq!(snapshot.alerts.len(), 1);
        let r = &snapshot.alerts[0];
        assert_eq!(
            (r.alert_id(), r.status, r.occurrence_count, r.observations),
            (id.clone(), AlertStatus::Acknowledged, 1, 2)
        );
        assert_eq!(r.acknowledged_at, Some(NOW + 1000));
        assert_eq!(r.finding.fingerprint, "machine.disk.low_space@C:");
    }
    // restart: resolve depois do atraso; o histórico resolvido permanece
    {
        let db = Database::open(&path).unwrap();
        let snapshot = db
            .alerts_run(&low_disk(150), NOW + 60_000 + RESOLVE_DELAY_MS + 1, view())
            .unwrap();
        assert_eq!(snapshot.alerts[0].status, AlertStatus::Resolved);
    }
    // restart: segue resolvido; ao voltar, é a ocorrência 2 (a contagem não zerou)
    {
        let db = Database::open(&path).unwrap();
        let resolved_at = NOW + 60_000 + RESOLVE_DELAY_MS + 1;
        let still = db
            .alerts_run(&low_disk(150), resolved_at + 60_000, view())
            .unwrap();
        assert_eq!(still.alerts.len(), 1);
        assert_eq!(still.alerts[0].status, AlertStatus::Resolved);
        let back = db
            .alerts_run(&low_disk(4), resolved_at + 120_000, view())
            .unwrap();
        assert_eq!(
            back.alerts.len(),
            2,
            "a ocorrência anterior continua no histórico"
        );
        let open = back.alerts.iter().find(|r| r.status.is_open()).unwrap();
        assert_eq!(open.occurrence_count, 2);
    }
}

#[test]
fn diagnostic_history_survives_restarts_and_never_persists_a_running_state() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    {
        let db = Database::open(&path).unwrap();
        db.diagnostic_run_save(&RunRecord {
            id: "run-1".into(),
            diagnostic: "sfc_verifyonly".into(),
            label: "SFC".into(),
            target: None,
            started_at: NOW,
            finished_at: Some(NOW + 5),
            running: false,
            result: Some(RunResult::ProblemsFound),
            exit_code: Some(0),
            summary: "resumo".into(),
            output_tail: vec!["a".into(), "b".into()],
        })
        .unwrap();
    }
    let db = Database::open(&path).unwrap();
    let runs = db.diagnostic_runs(10).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        (runs[0].result, runs[0].exit_code, runs[0].output_tail.len()),
        (Some(RunResult::ProblemsFound), Some(0), 2)
    );
    assert!(runs.iter().all(|r| !r.running));
    // Só o término é gravado: um app que cai com um diagnóstico em curso não deixa nada "em execução".
    let columns: Vec<String> = {
        let mut stmt = db
            .conn
            .prepare("SELECT name FROM pragma_table_info('machine_diagnostic_runs')")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert!(
        !columns.iter().any(|c| c == "running" || c == "pid"),
        "{columns:?}"
    );
}

// ------------------------------------------------------------------ limites de histórico

fn resolved_alert(index: usize, resolved_at: i64) -> AlertRecord {
    let fingerprint = format!("machine.disk.low_space@V{index}");
    AlertRecord {
        id: format!("id-{index}"),
        finding: Finding {
            id: fingerprint.clone(),
            fingerprint,
            rule_id: "machine.disk.low_space".into(),
            title: "t".into(),
            summary: "s".into(),
            severity: Severity::Attention,
            confidence: Confidence::High,
            domain: Domain::Machine,
            source: "machine.disk".into(),
            resource: format!("V{index}"),
            evidence: vec![],
            reason: "r".into(),
            recommended_next_step: "n".into(),
            diagnostic_action: None,
            cta: None,
        },
        status: AlertStatus::Resolved,
        first_seen: resolved_at - 1000,
        last_seen: resolved_at - 500,
        acknowledged_at: None,
        resolved_at: Some(resolved_at),
        occurrence_count: 1,
        observations: 1,
    }
}

trait AlertId {
    fn alert_id(&self) -> String;
}
impl AlertId for AlertRecord {
    fn alert_id(&self) -> String {
        self.id.clone()
    }
}

#[test]
fn resolved_alert_history_is_capped_and_keeps_the_newest() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let total = RESOLVED_KEEP as usize + 25;
    let changes: Vec<Change> = (0..total)
        .map(|i| Change::Insert(resolved_alert(i, NOW + i as i64 * 1000)))
        .collect();
    db.alerts_apply(&changes).unwrap();
    assert_eq!(db.alerts_latest().unwrap().len(), total);
    db.alerts_prune(NOW + total as i64 * 1000).unwrap();
    let kept = db.alerts_latest().unwrap();
    assert_eq!(kept.len(), RESOLVED_KEEP as usize);
    assert!(
        kept.iter().any(|r| r.id == format!("id-{}", total - 1)),
        "o mais novo fica"
    );
    assert!(!kept.iter().any(|r| r.id == "id-0"), "o mais antigo sai");
}

#[test]
fn open_alerts_are_never_pruned_however_old() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(&tmp.path().join("hub.db")).unwrap();
    db.alerts_run(&low_disk(4), NOW, view()).unwrap();
    db.alerts_prune(NOW + 400 * 24 * 3_600_000).unwrap();
    assert_eq!(db.alerts_latest().unwrap().len(), 1);
}

// ------------------------------------------------------------------ falha de gravação (sem panic)

#[test]
fn a_failing_alert_write_is_an_error_not_a_panic_and_the_database_keeps_working() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Database::open(&tmp.path().join("hub.db")).unwrap();
    db.conn.execute_batch("DROP TABLE machine_alerts").unwrap();
    assert!(db.alerts_run(&low_disk(4), NOW, view()).is_err());
    assert!(db.alert_acknowledge("x", NOW).is_err());
    // o resto do banco segue funcional: uma falha isolada não derruba as outras fontes
    let folder = tmp.path().join("p");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(db.save(None, input("Ainda funciona", &folder)).is_ok());
    assert!(db.diagnostic_runs(5).is_ok());
}

#[test]
fn partial_facts_still_evaluate_the_other_sources() {
    // só o disco respondeu: o resto está ausente e nada é inventado nem derruba a avaliação
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let snapshot = db.alerts_run(&low_disk(4), NOW, view()).unwrap();
    assert_eq!(snapshot.alerts.len(), 1);
    assert_eq!(snapshot.summary.critical, 1);
}

// ------------------------------------------------------------------ falsos positivos: limites exatos

#[test]
fn disk_thresholds_are_strict_and_cross_both_limits() {
    // exatamente no limite não alerta (estritamente menor)
    assert_eq!(
        health::space_severity(100 * GIB, 10 * GIB, None),
        None,
        "10% e 10 GiB: exatamente 10%"
    );
    assert_eq!(
        health::space_severity(1000 * GIB, 20 * GIB, None),
        None,
        "2% mas exatamente 20 GiB"
    );
    assert_eq!(
        health::space_severity(1000 * GIB, 20 * GIB - 1, None),
        Some(HealthStatus::Attention),
        "2% e um byte abaixo de 20 GiB"
    );
    assert_eq!(
        health::space_severity(100 * GIB, 9 * GIB, None),
        Some(HealthStatus::Attention)
    );
    assert_eq!(
        health::space_severity(100 * GIB, 5 * GIB, None),
        Some(HealthStatus::Attention),
        "5% exatos não é crítico"
    );
    assert_eq!(
        health::space_severity(100 * GIB, 4 * GIB, None),
        Some(HealthStatus::Critical)
    );
    // só uma das duas condições: nada
    assert_eq!(
        health::space_severity(2000 * GIB, 100 * GIB, None),
        None,
        "5%, mas 100 GiB livres"
    );
    assert_eq!(
        health::space_severity(20 * GIB, 19 * GIB, None),
        None,
        "19 GiB livres, mas 95%"
    );
    // um volume que acabou de passar de 20 GiB livres não oscila: a histerese segura o alerta
    assert_eq!(
        health::space_severity(100 * GIB, 10 * GIB, Some(HealthStatus::Attention)),
        Some(HealthStatus::Attention)
    );
    assert_eq!(
        health::space_severity(100 * GIB, 12 * GIB, Some(HealthStatus::Attention)),
        None
    );
    assert_eq!(
        health::space_severity(100 * GIB, 5 * GIB, Some(HealthStatus::Critical)),
        Some(HealthStatus::Critical)
    );
    assert_eq!(
        health::space_severity(100 * GIB, 7 * GIB, Some(HealthStatus::Critical)),
        Some(HealthStatus::Attention)
    );
}

// ------------------------------------------------------------------ redaction (formatos reais)

fn redacted(args: &[&str]) -> String {
    control_plane::redact(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn authorization_headers_and_cookies_are_redacted() {
    for (args, secret) in [
        (
            vec![
                "curl",
                "-H",
                "Authorization: Bearer abc.def.ghi",
                "https://api/x",
            ],
            "abc.def.ghi",
        ),
        (
            vec![
                "curl",
                "--header",
                "Authorization: Basic dXNlcjpzZW5oYQ==",
                "https://api/x",
            ],
            "dXNlcjpzZW5oYQ",
        ),
        (
            vec!["curl", "-H", "X-Api-Key: k-12345", "https://api/x"],
            "k-12345",
        ),
        (
            vec![
                "curl",
                "-H",
                "Cookie: sid=abcdef; theme=dark",
                "https://api/x",
            ],
            "abcdef",
        ),
        (
            vec!["curl", "-H", "Authorization:", "Bearer zzz-token"],
            "zzz-token",
        ),
    ] {
        let out = redacted(&args);
        assert!(!out.contains(secret), "{secret} vazou em {out}");
        assert!(out.contains("curl"), "o comando continua legível: {out}");
    }
}

#[test]
fn common_secret_formats_stay_redacted_and_normal_flags_are_kept() {
    let out = redacted(&[
        "node",
        "app.js",
        "--token=t1",
        "--password",
        "p2",
        "--api-key=a3",
        "SECRET_KEY=s4",
        "--db-url=postgres://admin:p5@db:5432/app",
        "NPM_TOKEN=n6",
        "--client-secret",
        "c7",
        "--port=3000",
        "--host",
        "0.0.0.0",
    ]);
    for leaked in ["t1", "p2", "a3", "s4", "p5", "n6", "c7"] {
        assert!(
            !out.split_whitespace()
                .any(|w| w.contains(leaked) && !w.contains("***")),
            "{leaked} vazou em {out}"
        );
    }
    assert!(
        out.contains("--port=3000") && out.contains("0.0.0.0") && out.contains("app.js"),
        "{out}"
    );
    assert!(out.contains("admin:***@db"), "{out}");
}

#[test]
fn headers_without_secret_names_are_left_alone() {
    let out = redacted(&[
        "curl",
        "-H",
        "Content-Type: application/json",
        "-H",
        "Accept: text/html",
    ]);
    assert!(
        out.contains("Content-Type: application/json") && out.contains("Accept: text/html"),
        "{out}"
    );
}

// ------------------------------------------------------------------ atribuição com um Worktree REAL

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn spawn_sleeper(cwd: &Path) -> Child {
    Command::new("ping")
        .args(["-n", "60", "127.0.0.1"])
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("ping")
}

fn wait_for_cwd(pid: u32) -> RawProcess {
    for _ in 0..80 {
        if let Some(p) = system::inventory()
            .into_iter()
            .find(|p| p.pid == pid && p.cwd.is_some())
        {
            return p;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!("o processo {pid} não apareceu com cwd no inventário");
}

#[cfg(windows)]
#[test]
fn a_real_git_worktree_attributes_worktree_and_session_and_lookalikes_stay_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    let repo = base.join("lab");
    let worktree = base.join("lab-wt-feature");
    let lookalike = base.join("lab-extra"); // prefixo igual ao do Project, mas NÃO é o Project
    let elsewhere = base.join("outro");
    for dir in [&repo, &lookalike, &elsewhere] {
        std::fs::create_dir_all(dir).unwrap();
    }
    if !git(&repo, &["init", "-q"]) {
        eprintln!("git indisponível: cenário ignorado");
        return;
    }
    std::fs::write(repo.join("a.txt"), "x").unwrap();
    assert!(git(&repo, &["add", "."]));
    assert!(git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "init"
        ]
    ));
    assert!(git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            worktree.to_str().unwrap(),
            "-b",
            "feature"
        ]
    ));

    let ctx = Context {
        projects: vec![ProjectRef {
            id: "lab".into(),
            name: "Lab".into(),
            root: repo.to_string_lossy().into(),
            ports: vec![],
        }],
        worktrees: vec![WorktreeRef {
            id: "w1".into(),
            project_id: "lab".into(),
            name: "feature".into(),
            root: worktree.to_string_lossy().into(),
            session_id: Some("sid-2".into()),
            session_label: Some("SESSION-002".into()),
            block_id: Some("b10".into()),
            block_title: Some("10 — Validation & Hardening".into()),
        }],
    };
    let mut children = vec![
        spawn_sleeper(&worktree.join(".")),
        spawn_sleeper(&repo),
        spawn_sleeper(&lookalike),
        spawn_sleeper(&elsewhere),
    ];
    let pids: Vec<u32> = children.iter().map(Child::id).collect();
    let processes: Vec<RawProcess> = pids.iter().map(|pid| wait_for_cwd(*pid)).collect();
    let result = attribute(&processes, &[], &ctx, &HashMap::new());

    // cleanup antes dos asserts: nada fica vivo nem registrado
    for child in &mut children {
        let _ = child.kill();
        let _ = child.wait();
    }
    let removed = git(
        &repo,
        &["worktree", "remove", "--force", worktree.to_str().unwrap()],
    );

    let wt = &result[&pids[0]];
    assert_eq!(
        (wt.project_id.as_deref(), wt.worktree_id.as_deref()),
        (Some("lab"), Some("w1")),
        "{wt:?}"
    );
    assert_eq!(wt.session_label.as_deref(), Some("SESSION-002"));
    assert_eq!(
        wt.block_title.as_deref(),
        Some("10 — Validation & Hardening")
    );
    assert_eq!(wt.confidence, Attribution::High);
    assert_eq!(wt.evidence[0].kind, "cwd_in_worktree");

    let main = &result[&pids[1]];
    assert_eq!(
        (main.project_id.as_deref(), main.worktree_id.as_deref()),
        (Some("lab"), None),
        "checkout principal: Project, sem Worktree"
    );
    assert_eq!(main.session_label, None, "Session só vem pelo Worktree");

    for pid in [pids[2], pids[3]] {
        let other = &result[&pid];
        assert_eq!(other.confidence, Attribution::Unknown, "{other:?}");
        assert!(
            other.project_id.is_none() && other.worktree_id.is_none() && other.session_id.is_none()
        );
    }
    assert!(removed, "o worktree de teste foi removido");
    assert!(
        !worktree.exists()
            || std::fs::read_dir(&worktree)
                .map(|d| d.count() == 0)
                .unwrap_or(true)
    );
    // o inventário tem cache de 1 s: espera o processo sair da lista
    for pid in pids {
        let gone = (0..60).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            !system::inventory()
                .iter()
                .any(|p| p.pid == pid && p.name.to_lowercase().starts_with("ping"))
        });
        assert!(gone, "processo {pid} ficou vivo");
    }
}

// ------------------------------------------------------------------ privilégio e segredos (varredura de código)

fn code_of(relative: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text =
        std::fs::read_to_string(root.join(relative)).unwrap_or_else(|_| panic!("{relative}"));
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn nothing_in_the_app_can_request_elevation_by_itself() {
    // Nenhum módulo da Session (nem o app Tauri) pede administrador: sem runas, sem ShellExecute "runas",
    // sem manifesto que force elevação na inicialização.
    let modules = [
        "src/control_plane.rs",
        "src/telemetry.rs",
        "src/sensors.rs",
        "src/windows_health.rs",
        "src/windows_native.rs",
        "src/network_security.rs",
        "src/security_native.rs",
        "src/diagnostics.rs",
        "src/diagnostic_runner.rs",
        "src/alert_store.rs",
        "src/supervisor.rs",
        "src/machine.rs",
        "../../src-tauri/src/main.rs",
    ];
    for module in modules {
        let code = code_of(module).to_lowercase();
        for banned in [
            "runas",
            "shellexecute",
            "requireadministrator",
            "highestavailable",
            "createprocessasuser",
            "-verb",
        ] {
            assert!(!code.contains(banned), "{module} contém {banned}");
        }
    }
    let config = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/tauri.conf.json"),
    )
    .unwrap()
    .to_lowercase();
    assert!(!config.contains("requireadministrator") && !config.contains("highestavailable"));
}

#[test]
fn session_modules_expose_no_secret_fields() {
    // Campos públicos serializáveis dos coletores nunca carregam segredos (senha, token, chave de
    // recuperação, segredo de Wi-Fi, credenciais).
    let banned = [
        "password",
        "passwd",
        "secret",
        "token",
        "recovery",
        "credential",
        "psk",
        "passphrase",
        "private_key",
    ];
    for module in [
        "src/control_plane.rs",
        "src/telemetry.rs",
        "src/windows_health.rs",
        "src/network_security.rs",
        "src/diagnostics.rs",
        "src/diagnostic_runner.rs",
        "src/alert_store.rs",
    ] {
        for line in code_of(module).lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("pub ") {
                if let Some(name) = rest.split(':').next().filter(|_| {
                    rest.contains(':')
                        && !rest.starts_with("fn ")
                        && !rest.starts_with("struct ")
                        && !rest.starts_with("enum ")
                        && !rest.starts_with("const ")
                        && !rest.starts_with("type ")
                }) {
                    let lower = name.to_lowercase();
                    assert!(
                        !banned.iter().any(|b| lower.contains(b)),
                        "{module}: campo '{name}' sugere segredo"
                    );
                }
            }
        }
    }
}

#[test]
fn diagnostic_runner_has_no_free_command_surface() {
    // A API pública só aceita ids do catálogo e uma letra de volume: nenhuma função recebe comando,
    // argumentos ou shell como texto livre.
    let code = code_of("src/diagnostic_runner.rs");
    assert!(code.contains("pub fn start(&self, id: &str, target: Option<&str>)"));
    for line in code.lines().filter(|l| l.contains("pub fn ")) {
        let lower = line.to_lowercase();
        for banned in ["command:", "args:", "shell", "program:", "cmdline"] {
            assert!(
                !lower.contains(banned),
                "assinatura pública com superfície livre: {line}"
            );
        }
    }
}
