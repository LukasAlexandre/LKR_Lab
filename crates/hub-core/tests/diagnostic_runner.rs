//! Block 09 — Diagnostic Runner e armazenamento. O catálogo real (SFC/DISM/CHKDSK) NUNCA é executado
//! aqui: os testes de execução usam um catálogo inofensivo injetado (`cmd.exe` com texto fixo).
use hub_core::{database::Database, diagnostic_runner::*, diagnostics::*, windows_health::Millis};
use std::sync::{Arc, Mutex};

const NOW: Millis = 1_800_000_000_000;

// ------------------------------------------------------------------ allowlist

#[test]
fn only_known_diagnostic_ids_parse() {
    for id in DiagnosticId::ALL {
        assert_eq!(DiagnosticId::parse(id.as_str()), Some(id));
    }
    for bad in [
        "",
        "sfc",
        "sfc /scannow",
        "sfc_scannow",
        "sfc_verifyonly & calc",
        "sfc_verifyonly /scannow",
        "dism_restorehealth",
        "DISM_CHECKHEALTH",
        "chkdsk /f",
        "chkdsk_f",
        "chkdsk_r",
        "cmd /c whoami",
        "../sfc_verifyonly",
    ] {
        assert_eq!(
            DiagnosticId::parse(bad),
            None,
            "{bad:?} não pode ser aceito"
        );
    }
}

#[test]
fn the_catalog_has_only_read_only_diagnostics() {
    let ids: Vec<_> = DiagnosticId::ALL.iter().map(|d| d.as_str()).collect();
    assert_eq!(
        ids,
        [
            "sfc_verifyonly",
            "dism_checkhealth",
            "dism_scanhealth",
            "chkdsk_scan"
        ]
    );
}

#[test]
fn the_real_catalog_builds_fixed_commands_without_a_shell() {
    let catalog = SystemCatalog;
    let spec = |id, target: Option<&str>| catalog.resolve(id, target).unwrap();
    let sfc = spec(DiagnosticId::SfcVerifyOnly, None);
    assert_eq!(sfc.args, ["/verifyonly"]);
    assert!(
        sfc.program.ends_with("System32/sfc.exe") || sfc.program.ends_with("System32\\sfc.exe")
    );
    assert!(
        sfc.program.is_absolute(),
        "caminho absoluto: sem busca no PATH"
    );
    assert_eq!(
        spec(DiagnosticId::DismCheckHealth, None).args,
        ["/Online", "/Cleanup-Image", "/CheckHealth"]
    );
    assert_eq!(
        spec(DiagnosticId::DismScanHealth, None).args,
        ["/Online", "/Cleanup-Image", "/ScanHealth"]
    );
    let chkdsk = spec(DiagnosticId::ChkdskScan, Some("d:"));
    assert_eq!(chkdsk.args, ["D:", "/scan"]);
    for id in DiagnosticId::ALL {
        let target = id.needs_target().then_some("C:");
        let s = spec(id, target);
        let all = s.args.join(" ").to_lowercase();
        for repair in [
            "/scannow",
            "/restorehealth",
            "/startcomponentcleanup",
            "/f",
            "/r",
            "/x",
            "/b",
            "/spotfix",
            "&",
            "|",
            ">",
            ";",
        ] {
            assert!(
                !s.args.iter().any(|a| a.to_lowercase() == repair),
                "{id:?} usa {repair}"
            );
        }
        assert!(!all.contains("scannow") && !all.contains("restorehealth"));
        let program = s.program.to_string_lossy().to_lowercase();
        assert!(["sfc.exe", "dism.exe", "chkdsk.exe"]
            .iter()
            .any(|exe| program.ends_with(exe)));
    }
}

#[test]
fn the_runner_source_has_no_repair_commands_or_shell() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("diagnostic_runner.rs"),
    )
    .unwrap();
    let code: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    for banned in [
        "\"/scannow\"",
        "restorehealth\"",
        "\"/f\"",
        "\"/r\"",
        "cmd.exe",
        "powershell",
        "\"/c\"",
        "shellexecute",
        "runas",
    ] {
        assert!(
            !code.contains(banned),
            "diagnostic_runner.rs contém {banned}"
        );
    }
}

#[test]
fn volume_target_is_a_single_drive_letter() {
    assert_eq!(validate_volume("c:").unwrap(), "C:");
    assert_eq!(validate_volume("Z:").unwrap(), "Z:");
    for bad in [
        "",
        "C",
        "C:\\",
        "C:/f",
        "C: /f",
        "C: & calc",
        "\\\\.\\C:",
        "CC:",
        "1:",
        "C:;whoami",
        "C:\n/f",
        "-:",
        "C:$(calc)",
        "%SystemRoot%",
    ] {
        assert!(
            validate_volume(bad).is_err(),
            "{bad:?} deveria ser rejeitado"
        );
    }
}

// ------------------------------------------------------------------ catálogo e elevação

#[test]
fn catalog_view_reflects_elevation_and_never_offers_repairs() {
    let blocked = catalog(false);
    assert_eq!(blocked.len(), 4);
    assert!(blocked.iter().all(|d| d.requires_elevation && !d.available));
    assert!(blocked.iter().all(|d| d
        .reason
        .as_deref()
        .unwrap()
        .contains("Requer administrador")));
    assert!(
        blocked
            .iter()
            .find(|d| d.id == "chkdsk_scan")
            .unwrap()
            .needs_target
    );
    let open = catalog(true);
    assert!(open.iter().all(|d| d.available && d.reason.is_none()));
    let text = serde_json::to_string(&open).unwrap().to_lowercase();
    assert!(
        !text.contains("scannow")
            && !text.contains("restorehealth")
            && !text.contains("reparar automaticamente")
    );
}

struct Fake(Vec<String>);
impl Catalog for Fake {
    fn resolve(&self, _id: DiagnosticId, _target: Option<&str>) -> Result<CommandSpec, DiagError> {
        Ok(CommandSpec {
            program: std::env::var_os("ComSpec")
                .unwrap_or_else(|| "cmd.exe".into())
                .into(),
            args: self.0.clone(),
        })
    }
}

type Seen = Arc<Mutex<Vec<RunRecord>>>;
fn runner(args: &[&str], elevated: bool) -> (Runner, Seen) {
    let seen: Seen = Arc::default();
    let sink = seen.clone();
    let runner = Runner::new(
        Box::new(Fake(args.iter().map(|a| a.to_string()).collect())),
        elevated,
        Arc::new(move |record: &RunRecord| sink.lock().unwrap().push(record.clone())),
    );
    (runner, seen)
}

fn wait_done(runner: &Runner) -> RunRecord {
    for _ in 0..200 {
        if let Some(record) = runner.status() {
            if !record.running {
                return record;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("o diagnóstico não terminou");
}

#[test]
fn requires_elevation_is_reported_and_nothing_runs() {
    let (runner, seen) = runner(&["/C", "echo ola"], false);
    assert_eq!(
        runner.start("sfc_verifyonly", None).unwrap_err(),
        DiagError::RequiresElevation
    );
    assert!(runner.status().is_none());
    assert!(seen.lock().unwrap().is_empty());
    assert!(DiagError::RequiresElevation
        .to_string()
        .contains("não solicita elevação automaticamente"));
}

#[test]
fn unknown_diagnostics_and_bad_targets_are_rejected_before_anything_runs() {
    let (runner, seen) = runner(&["/C", "echo nao-deve-rodar"], true);
    for bad in [
        "sfc /scannow",
        "chkdsk /f",
        "sfc_verifyonly & calc",
        "",
        "dism_restorehealth",
    ] {
        assert!(
            matches!(
                runner.start(bad, None),
                Err(DiagError::UnknownDiagnostic(_))
            ),
            "{bad}"
        );
    }
    assert!(matches!(
        runner.start("chkdsk_scan", None),
        Err(DiagError::InvalidTarget(_))
    ));
    assert!(matches!(
        runner.start("chkdsk_scan", Some("C: & calc")),
        Err(DiagError::InvalidTarget(_))
    ));
    assert!(matches!(
        runner.start("chkdsk_scan", Some("C:\\ /f")),
        Err(DiagError::InvalidTarget(_))
    ));
    // sem argumentos arbitrários: diagnóstico sem alvo não aceita nenhum
    assert!(matches!(
        runner.start("sfc_verifyonly", Some("/scannow")),
        Err(DiagError::InvalidTarget(_))
    ));
    assert!(runner.status().is_none());
    assert!(seen.lock().unwrap().is_empty());
}

// ------------------------------------------------------------------ execução (comando inofensivo injetado)

#[cfg(windows)]
#[test]
fn runs_captures_stdout_stderr_and_exit_code_and_records_history() {
    let (runner, seen) = runner(
        &["/C", "echo linha-um& echo linha-erro 1>&2& exit /b 3"],
        true,
    );
    let started = runner.start("dism_checkhealth", None).unwrap();
    assert!(started.running && started.result.is_none());
    assert_eq!(started.diagnostic, "dism_checkhealth");
    let done = wait_done(&runner);
    assert!(!done.running && done.finished_at.is_some());
    assert_eq!(done.exit_code, Some(3));
    assert!(
        done.output_tail.iter().any(|l| l.contains("linha-um")),
        "stdout: {:?}",
        done.output_tail
    );
    assert!(
        done.output_tail.iter().any(|l| l.contains("linha-erro")),
        "stderr: {:?}",
        done.output_tail
    );
    // código de saída diferente de zero no DISM = falha (não é inferido como sucesso)
    assert_eq!(done.result, Some(RunResult::Failed));
    let recorded = seen.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].id, started.id);
}

#[cfg(windows)]
#[test]
fn a_clean_run_is_classified_by_the_tool_text_not_only_by_exit_code() {
    let (runner, _) = runner(
        &[
            "/C",
            "echo Windows Resource Protection did not find any integrity violations.",
        ],
        true,
    );
    runner.start("sfc_verifyonly", None).unwrap();
    let done = wait_done(&runner);
    assert_eq!(done.exit_code, Some(0));
    assert_eq!(done.result, Some(RunResult::Clean));
    // exit 0 sem texto reconhecível é inconclusivo, não "sucesso"
    let (runner, _) = runner_with("echo qualquer coisa");
    runner.start("sfc_verifyonly", None).unwrap();
    assert_eq!(wait_done(&runner).result, Some(RunResult::Inconclusive));
}

#[cfg(windows)]
fn runner_with(line: &str) -> (Runner, Seen) {
    runner(&["/C", line], true)
}

#[cfg(windows)]
#[test]
fn only_one_diagnostic_at_a_time() {
    let (runner, _) = runner(&["/C", "ping -n 20 127.0.0.1 >nul"], true);
    runner.start("dism_scanhealth", None).unwrap();
    assert_eq!(
        runner.start("dism_checkhealth", None).unwrap_err(),
        DiagError::Busy
    );
    assert!(runner.cancel());
    wait_done(&runner);
}

#[cfg(windows)]
#[test]
fn cancel_stops_only_the_diagnostic_process() {
    let (runner, seen) = runner(&["/C", "ping -n 60 127.0.0.1 >nul"], true);
    runner.start("dism_scanhealth", None).unwrap();
    assert!(runner.status().unwrap().running);
    let begun = std::time::Instant::now();
    assert!(runner.cancel());
    let done = wait_done(&runner);
    assert!(begun.elapsed().as_secs() < 10, "cancelamento rápido");
    assert_eq!(done.result, Some(RunResult::Cancelled));
    assert_eq!(done.summary, "Cancelado pelo usuário.");
    assert_eq!(
        seen.lock().unwrap().last().unwrap().result,
        Some(RunResult::Cancelled)
    );
    assert!(!runner.cancel(), "nada em andamento");
    // pode iniciar outro depois
    assert!(runner.start("dism_checkhealth", None).is_ok());
    runner.cancel();
    wait_done(&runner);
}

#[cfg(windows)]
#[test]
fn a_program_that_cannot_start_is_reported_without_panicking() {
    struct Missing;
    impl Catalog for Missing {
        fn resolve(&self, _: DiagnosticId, _: Option<&str>) -> Result<CommandSpec, DiagError> {
            Ok(CommandSpec {
                program: "C:\\nao-existe\\ferramenta.exe".into(),
                args: vec![],
            })
        }
    }
    let runner = Runner::new(Box::new(Missing), true, Arc::new(|_| {}));
    assert!(matches!(
        runner.start("sfc_verifyonly", None),
        Err(DiagError::Spawn(_))
    ));
    assert!(runner.status().is_none());
}

// ------------------------------------------------------------------ parsers

const SFC_CLEAN_EN: &str = "Beginning verification phase of system scan.\nVerification 100% complete.\n\nWindows Resource Protection did not find any integrity violations.";
const SFC_CLEAN_PT: &str = "Iniciando a fase de verificação da verificação do sistema.\nA Proteção de Recursos do Windows não encontrou nenhuma violação de integridade.";
const SFC_BAD_EN: &str = "Windows Resource Protection found integrity violations.";
const SFC_BAD_PT: &str = "A Proteção de Recursos do Windows encontrou violações de integridade.";
const SFC_PENDING: &str = "There is a system repair pending which requires reboot to complete. Restart Windows and run sfc again.";

#[test]
fn sfc_results_in_english_and_portuguese() {
    let id = DiagnosticId::SfcVerifyOnly;
    assert_eq!(parse_output(id, SFC_CLEAN_EN, Some(0)).0, RunResult::Clean);
    assert_eq!(parse_output(id, SFC_CLEAN_PT, Some(0)).0, RunResult::Clean);
    assert_eq!(
        parse_output(id, SFC_BAD_EN, Some(0)).0,
        RunResult::ProblemsFound
    );
    assert_eq!(
        parse_output(id, SFC_BAD_PT, Some(1)).0,
        RunResult::ProblemsFound
    );
    assert_eq!(
        parse_output(id, SFC_PENDING, Some(0)).0,
        RunResult::Inconclusive
    );
    assert_eq!(parse_output(id, "", Some(0)).0, RunResult::Inconclusive);
    assert_eq!(parse_output(id, "", Some(1)).0, RunResult::Failed);
    assert_eq!(
        parse_output(
            id,
            "Você deve ser um administrador para executar sfc.",
            Some(1)
        )
        .0,
        RunResult::Failed
    );
    let summary = parse_output(id, SFC_BAD_EN, Some(0)).1;
    assert!(summary.contains("Nada foi reparado"));
}

#[test]
fn dism_results() {
    for id in [DiagnosticId::DismCheckHealth, DiagnosticId::DismScanHealth] {
        assert_eq!(
            parse_output(
                id,
                "No component store corruption detected.\nThe operation completed successfully.",
                Some(0)
            )
            .0,
            RunResult::Clean
        );
        assert_eq!(
            parse_output(
                id,
                "Nenhuma corrupção do repositório de componentes foi detectada.",
                Some(0)
            )
            .0,
            RunResult::Clean
        );
        assert_eq!(
            parse_output(id, "The component store is repairable.", Some(0)).0,
            RunResult::ProblemsFound
        );
        assert_eq!(
            parse_output(
                id,
                "O repositório de componentes pode ser reparado.",
                Some(0)
            )
            .0,
            RunResult::ProblemsFound
        );
        assert_eq!(
            parse_output(
                id,
                "Error: 740\nElevated permissions are required to run DISM.",
                Some(740)
            )
            .1,
            "O DISM exige administrador."
        );
        assert_eq!(parse_output(id, "Error: 87", Some(87)).0, RunResult::Failed);
        assert_eq!(
            parse_output(id, "nada reconhecível", Some(0)).0,
            RunResult::Inconclusive
        );
        // Sucesso textual com código de erro não é sucesso.
        assert_eq!(
            parse_output(id, "No component store corruption detected.", Some(1)).0,
            RunResult::Failed
        );
    }
}

#[test]
fn chkdsk_results() {
    let id = DiagnosticId::ChkdskScan;
    assert_eq!(parse_output(id, "Windows has scanned the file system and found no problems.\nNo further action is required.", Some(0)).0, RunResult::Clean);
    assert_eq!(
        parse_output(
            id,
            "O Windows examinou o sistema de arquivos e não encontrou problemas.",
            Some(0)
        )
        .0,
        RunResult::Clean
    );
    assert_eq!(
        parse_output(
            id,
            "Windows has scanned the file system and found problems.",
            Some(0)
        )
        .0,
        RunResult::ProblemsFound
    );
    assert_eq!(
        parse_output(
            id,
            "O Windows examinou o sistema de arquivos e encontrou problemas.",
            Some(0)
        )
        .0,
        RunResult::ProblemsFound
    );
    assert_eq!(
        parse_output(
            id,
            "Access Denied as you do not have sufficient privileges.",
            Some(0)
        )
        .0,
        RunResult::Failed
    );
    assert_eq!(
        parse_output(id, "Could not check", Some(3)).0,
        RunResult::Failed
    );
    assert_eq!(
        parse_output(id, "texto estranho", Some(0)).0,
        RunResult::Inconclusive
    );
}

#[test]
fn console_text_is_decoded_from_utf16_and_oem() {
    let utf16: Vec<u8> = "Proteção de Recursos\r\n"
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    assert_eq!(decode_console(&utf16).trim(), "Proteção de Recursos");
    let with_bom: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain("ok".encode_utf16().flat_map(|u| u.to_le_bytes()))
        .collect();
    assert_eq!(decode_console(&with_bom), "ok");
    assert_eq!(decode_console(b"plain ascii"), "plain ascii");
    assert_eq!(decode_console(b""), "");
    assert_eq!(
        normalize("Não   Encontrou  VIOLAÇÕES"),
        "nao encontrou violacoes"
    );
}

#[test]
fn the_tail_is_short_clean_and_bounded() {
    let long = (0..200)
        .map(|i| format!("linha {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let tail = tail_of(&long);
    assert_eq!(tail.len(), TAIL_LINES);
    assert_eq!(tail.last().unwrap(), "linha 199");
    assert!(tail_of("a\n\n   \nb").len() == 2);
    let wide = "x".repeat(5000);
    assert!(tail_of(&wide)[0].chars().count() <= TAIL_LINE_CHARS);
    let huge = (0..30)
        .map(|_| "y".repeat(TAIL_LINE_CHARS))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        tail_of(&huge).iter().map(|l| l.len() + 1).sum::<usize>() <= TAIL_BYTES + TAIL_LINE_CHARS
    );
}

// ------------------------------------------------------------------ banco local

fn temp_db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join("hub.db")).unwrap();
    (dir, db)
}

fn finding(rule: &str, resource: &str, severity: Severity, source: &str) -> Finding {
    Finding {
        id: fingerprint(rule, resource),
        fingerprint: fingerprint(rule, resource),
        rule_id: rule.into(),
        title: "Título".into(),
        summary: "Resumo".into(),
        severity,
        confidence: Confidence::High,
        domain: Domain::Machine,
        source: source.into(),
        resource: resource.into(),
        evidence: vec![Evidence {
            label: "Volume".into(),
            value: resource.into(),
            source: source.into(),
        }],
        reason: "Motivo".into(),
        recommended_next_step: "Próximo passo".into(),
        diagnostic_action: Some(DiagnosticRef {
            id: "chkdsk_scan".into(),
            target: Some(resource.into()),
            label: "CHKDSK".into(),
        }),
        cta: Some(Cta {
            kind: "machine".into(),
            target: None,
        }),
    }
}

fn evaluation(findings: Vec<Finding>, evaluated: &[&str]) -> Evaluation {
    Evaluation {
        findings,
        evaluated: evaluated.iter().map(|s| s.to_string()).collect(),
        statuses: vec![],
    }
}

fn step(db: &Database, evaluation: &Evaluation, now: Millis) -> Vec<AlertRecord> {
    let mut n = 0u32;
    let latest = db.alerts_latest().unwrap();
    let changes = reconcile(&latest, evaluation, now, &mut || {
        n += 1;
        format!("id-{now}-{n}")
    });
    db.alerts_apply(&changes).unwrap();
    db.alerts_latest().unwrap()
}

#[test]
fn migration_creates_the_local_tables() {
    let (_dir, db) = temp_db();
    assert!(db.alerts_latest().unwrap().is_empty());
    assert!(db.diagnostic_runs(10).unwrap().is_empty());
}

#[test]
fn alerts_round_trip_with_evidence_action_and_cta() {
    let (_dir, db) = temp_db();
    let f = finding(
        "machine.disk.low_space",
        "C:",
        Severity::Critical,
        "machine.disk",
    );
    let latest = step(&db, &evaluation(vec![f.clone()], &["machine.disk"]), NOW);
    assert_eq!(latest.len(), 1);
    let r = &latest[0];
    assert_eq!(r.finding, f);
    assert_eq!(
        (
            r.status,
            r.first_seen,
            r.last_seen,
            r.occurrence_count,
            r.observations
        ),
        (AlertStatus::Active, NOW, NOW, 1, 1)
    );
}

#[test]
fn persistence_dedups_across_evaluations_and_restarts() {
    let (dir, db) = temp_db();
    let f = finding(
        "machine.disk.low_space",
        "C:",
        Severity::Attention,
        "machine.disk",
    );
    let e = evaluation(vec![f], &["machine.disk"]);
    step(&db, &e, NOW);
    step(&db, &e, NOW + 30_000);
    drop(db);
    // "reiniciar o app": reabre o mesmo banco
    let db = Database::open(&dir.path().join("hub.db")).unwrap();
    let latest = step(&db, &e, NOW + 60_000);
    assert_eq!(latest.len(), 1);
    assert_eq!(
        (
            latest[0].observations,
            latest[0].first_seen,
            latest[0].last_seen
        ),
        (3, NOW, NOW + 60_000)
    );
    assert_eq!(db.alerts_visible(NOW + 60_000).unwrap().len(), 1);
}

#[test]
fn acknowledge_resolve_and_reopen_keep_the_history() {
    let (_dir, db) = temp_db();
    let f = finding(
        "machine.disk.low_space",
        "C:",
        Severity::Critical,
        "machine.disk",
    );
    let present = evaluation(vec![f], &["machine.disk"]);
    let gone = evaluation(vec![], &["machine.disk"]);
    let latest = step(&db, &present, NOW);
    let acked = db.alert_acknowledge(&latest[0].id, NOW + 1000).unwrap();
    assert_eq!(
        (acked.status, acked.acknowledged_at),
        (AlertStatus::Acknowledged, Some(NOW + 1000))
    );
    // reconhecer duas vezes é inofensivo
    assert_eq!(
        db.alert_acknowledge(&latest[0].id, NOW + 2000)
            .unwrap()
            .acknowledged_at,
        Some(NOW + 1000)
    );
    let latest = step(&db, &gone, NOW + RESOLVE_DELAY_MS + 1000);
    assert_eq!(latest[0].status, AlertStatus::Resolved);
    assert!(
        db.alert_acknowledge(&latest[0].id, NOW + 999_999).is_err(),
        "resolvido não é reconhecido"
    );
    assert!(db.alert_acknowledge("inexistente", NOW).is_err());
    // volta: nova ocorrência, a anterior continua visível como resolvida
    let present = evaluation(
        vec![finding(
            "machine.disk.low_space",
            "C:",
            Severity::Critical,
            "machine.disk",
        )],
        &["machine.disk"],
    );
    let latest = step(&db, &present, NOW + 10 * 60_000);
    assert_eq!(
        (latest[0].occurrence_count, latest[0].status),
        (2, AlertStatus::Active)
    );
    let visible = db.alerts_visible(NOW + 10 * 60_000).unwrap();
    assert_eq!(visible.len(), 2);
    assert_eq!(
        visible
            .iter()
            .filter(|r| r.status == AlertStatus::Resolved)
            .count(),
        1
    );
}

#[test]
fn only_one_open_alert_per_fingerprint_is_enforced_by_the_database() {
    let (_dir, db) = temp_db();
    let f = finding(
        "machine.disk.low_space",
        "C:",
        Severity::Attention,
        "machine.disk",
    );
    let latest = step(&db, &evaluation(vec![f], &["machine.disk"]), NOW);
    let mut duplicate = latest[0].clone();
    duplicate.id = "outro-id".into();
    duplicate.occurrence_count = 2;
    assert!(db.alerts_apply(&[Change::Insert(duplicate)]).is_err());
}

#[test]
fn old_resolved_alerts_are_pruned_but_open_ones_never_are() {
    let (_dir, db) = temp_db();
    let old = finding(
        "machine.disk.low_space",
        "C:",
        Severity::Attention,
        "machine.disk",
    );
    let open = finding(
        "windows.reboot.pending",
        "system",
        Severity::Attention,
        "windows.restart",
    );
    step(
        &db,
        &evaluation(
            vec![old, open.clone()],
            &["machine.disk", "windows.restart"],
        ),
        NOW,
    );
    step(
        &db,
        &evaluation(vec![open], &["machine.disk", "windows.restart"]),
        NOW + 5 * 60_000,
    );
    let later = NOW + 40 * 24 * 3_600_000;
    db.alerts_prune(later).unwrap();
    let all = db.alerts_latest().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].finding.rule_id, "windows.reboot.pending");
    // o resolvido sai da janela visível antes de ser podado
    let (_dir2, db2) = temp_db();
    step(
        &db2,
        &evaluation(
            vec![finding(
                "machine.disk.low_space",
                "C:",
                Severity::Attention,
                "machine.disk",
            )],
            &["machine.disk"],
        ),
        NOW,
    );
    step(
        &db2,
        &evaluation(vec![], &["machine.disk"]),
        NOW + 5 * 60_000,
    );
    assert_eq!(db2.alerts_visible(NOW + 5 * 60_000).unwrap().len(), 1);
    assert_eq!(
        db2.alerts_visible(NOW + 5 * 60_000 + RESOLVED_VISIBLE_MS + 1)
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn diagnostic_history_is_persisted_summarized_and_bounded() {
    let (_dir, db) = temp_db();
    for i in 0..(HISTORY_KEEP as i64 + 5) {
        let run = RunRecord {
            id: format!("run-{i:03}"),
            diagnostic: "chkdsk_scan".into(),
            label: "x".into(),
            target: Some("C:".into()),
            started_at: NOW + i * 1000,
            finished_at: Some(NOW + i * 1000 + 500),
            running: false,
            result: Some(RunResult::Clean),
            exit_code: Some(0),
            summary: "O volume foi examinado e nenhum problema foi encontrado.".into(),
            output_tail: vec!["linha curta".into()],
        };
        db.diagnostic_run_save(&run).unwrap();
    }
    let history = db.diagnostic_runs(100).unwrap();
    assert_eq!(history.len(), HISTORY_KEEP);
    assert_eq!(
        history[0].id,
        format!("run-{:03}", HISTORY_KEEP + 4),
        "mais recente primeiro"
    );
    assert_eq!(history[0].result, Some(RunResult::Clean));
    assert_eq!(history[0].target.as_deref(), Some("C:"));
    assert_eq!(history[0].exit_code, Some(0));
    assert!(!history[0].running);
    assert_eq!(history[0].output_tail, ["linha curta"]);
    assert!(history[0].label.contains("CHKDSK"));
    assert_eq!(db.diagnostic_runs(3).unwrap().len(), 3);
}

#[test]
fn diagnostic_output_never_reaches_portable_workspace_or_ai_context() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in [
        "portable.rs",
        "sync.rs",
        "snapshot.rs",
        "planning.rs",
        "ddae.rs",
        "bridge.rs",
        "agents.rs",
        "tools.rs",
    ] {
        let Ok(text) = std::fs::read_to_string(src.join(file)) else {
            continue;
        };
        for word in [
            "machine_diagnostic_runs",
            "diagnostic_run",
            "output_tail",
            "diagnostic_runner",
        ] {
            assert!(!text.contains(word), "{file} não pode tocar {word}");
        }
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

fn low_disk_facts(available_gib: u64) -> Facts {
    const GIB: u64 = 1024 * 1024 * 1024;
    Facts {
        machine: Some(MachineFacts {
            volumes: Some(vec![VolumeFact {
                mount: "C:".into(),
                total: 200 * GIB,
                available: available_gib * GIB,
            }]),
            load_ready: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn alerts_run_applies_the_whole_lifecycle_to_the_local_database() {
    let (_dir, db) = temp_db();
    // primeira avaliação: abre
    let first = db.alerts_run(&low_disk_facts(4), NOW, view()).unwrap();
    assert_eq!(
        (
            first.summary.critical,
            first.summary.attention,
            first.summary.info
        ),
        (1, 0, 0)
    );
    assert_eq!(first.alerts.len(), 1);
    assert_eq!(first.alerts[0].finding.rule_id, "machine.disk.low_space");
    // repete: mesmo alerta, sem duplicar
    let again = db
        .alerts_run(&low_disk_facts(4), NOW + 30_000, view())
        .unwrap();
    assert_eq!(again.alerts.len(), 1);
    assert_eq!(again.alerts[0].observations, 2);
    // reconhece: o resumo continua contando como aberto
    let acked = db
        .alert_acknowledge(&again.alerts[0].id, NOW + 40_000)
        .unwrap();
    let after = db
        .alerts_run(&low_disk_facts(4), NOW + 60_000, view())
        .unwrap();
    assert_eq!(after.alerts[0].status, AlertStatus::Acknowledged);
    assert_eq!((after.summary.critical, after.summary.acknowledged), (1, 1));
    assert_eq!(after.alerts[0].id, acked.id);
    // some: resolve depois do atraso e fica no histórico recente
    let resolved = db
        .alerts_run(
            &low_disk_facts(150),
            NOW + 60_000 + RESOLVE_DELAY_MS + 1,
            view(),
        )
        .unwrap();
    assert_eq!(resolved.alerts[0].status, AlertStatus::Resolved);
    assert_eq!(
        (
            resolved.summary.critical,
            resolved.summary.resolved_recently
        ),
        (0, 1)
    );
    // telemetria velha: alerta novo não nasce e nada muda
    let stale = db
        .alerts_run(&Facts::default(), NOW + 10 * 60_000, view())
        .unwrap();
    assert_eq!(stale.alerts.len(), 1);
    assert_eq!(stale.alerts[0].status, AlertStatus::Resolved);
}

#[test]
fn alerts_run_does_not_invent_alerts_on_an_empty_or_healthy_machine() {
    let (_dir, db) = temp_db();
    let snapshot = db.alerts_run(&Facts::default(), NOW, view()).unwrap();
    assert!(snapshot.alerts.is_empty());
    assert_eq!(snapshot.summary, AlertSummary::default());
    let healthy = db
        .alerts_run(&low_disk_facts(150), NOW + 1000, view())
        .unwrap();
    assert!(healthy.alerts.is_empty());
}
