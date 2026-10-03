//! DDAE: contexto determinístico da Session e Ready for AI (derivado, nunca gravado).
use hub_core::{
    database::Database,
    ddae::{self, ContextState, Details, Reference, ReferenceKind, SessionStatus},
    models::ProjectInput,
    portable,
};
use std::path::Path;

fn setup() -> (tempfile::TempDir, Database, String) {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("proj-folder-secreta");
    std::fs::create_dir_all(&folder).unwrap();
    let mut db = Database::open(&tmp.path().join("hub.db")).unwrap();
    let project = db
        .save(
            None,
            ProjectInput {
                name: "Projeto".into(),
                description: String::new(),
                local_path: folder.to_string_lossy().into(),
                repository: String::new(),
                stack: vec![],
                tags: vec![],
                ports: vec![],
                commands: vec![],
            },
        )
        .unwrap();
    (tmp, db, project.id)
}

fn complete_details() -> Details {
    Details {
        title: None,
        objective: "Entregar a feature".into(),
        desired_outcome: "Feature funcionando de ponta a ponta".into(),
        constraints: vec!["Sem LLM".into()],
        criteria: vec!["Testes verdes".into()],
        notes: vec!["Nota importante".into()],
        references: vec![
            Reference {
                kind: ReferenceKind::ProjectPath,
                value: "docs/ddae/sessions/SESSION-001.md".into(),
                label: None,
            },
            Reference {
                kind: ReferenceKind::Url,
                value: "https://github.com/org/repo".into(),
                label: None,
            },
        ],
    }
}

/// Session ativa com 1 bloco concluído, 1 pendente; detalhes completos.
fn ready_session(db: &mut Database, project: &str) -> String {
    let s = db.ddae_create_session(project, "Feature", "").unwrap();
    db.ddae_add_block(&s.id, "Bloco A", "").unwrap();
    db.ddae_add_block(&s.id, "Bloco B", "").unwrap();
    db.ddae_update_details(&s.id, complete_details()).unwrap();
    s.id
}

fn readiness(db: &Database, id: &str) -> ddae::ReadyForAi {
    ddae::ready_for_ai(&db.ddae_session(id).unwrap())
}

#[test]
fn context_is_deterministic() {
    let (_tmp, mut db, project) = setup();
    let id = ready_session(&mut db, &project);
    let first = db.ddae_generate_context(&id).unwrap();
    let second = db.ddae_generate_context(&id).unwrap();
    assert_eq!(first.markdown, second.markdown);
    let md = &first.markdown;
    assert!(md.starts_with("# DDAE — SESSION-001: Feature"));
    for expected in [
        "Projeto: Projeto",
        "Estado: active (ativa)",
        "Contexto IA: Pronto para continuar",
        "0 / 2 blocos concluídos",
        "## Objetivo\n\nEntregar a feature",
        "## Resultado desejado\n\nFeature funcionando de ponta a ponta",
        "- Sem LLM",
        "- [ ] Testes verdes",
        "1. [ ] Bloco A",
        "Bloco atual: nenhum",
        "Próximo bloco: Bloco A",
        "- Nota importante",
        "- project_path: docs/ddae/sessions/SESSION-001.md",
        "- url: https://github.com/org/repo",
    ] {
        assert!(md.contains(expected), "faltou {expected:?} em:\n{md}");
    }
    // Reflete o estado: mudar algo muda o contexto, e reverter volta aos mesmos bytes.
    let blocks = db.ddae_session(&id).unwrap().blocks;
    db.ddae_start_block(&id, &blocks[0].id).unwrap();
    let changed = db.ddae_generate_context(&id).unwrap().markdown;
    assert_ne!(changed, first.markdown);
    assert!(changed.contains("Bloco atual: Bloco A") && changed.contains("1. [~] Bloco A"));
}

#[test]
fn context_is_a_pure_read() {
    let (_tmp, mut db, project) = setup();
    let id = ready_session(&mut db, &project);
    let before = serde_json::to_string(&db.export_portable().unwrap()).unwrap();
    let activities = db.activities().unwrap().len();
    db.ddae_generate_context(&id).unwrap();
    assert_eq!(
        before,
        serde_json::to_string(&db.export_portable().unwrap()).unwrap()
    );
    assert_eq!(activities, db.activities().unwrap().len());
}

#[test]
fn ready_for_ai_requires_every_canonical_field() {
    let (_tmp, mut db, project) = setup();
    // Sessão nova: nada informado.
    let s = db.ddae_create_session(&project, "Feature", "").unwrap();
    let r = readiness(&db, &s.id);
    assert!(!r.ready);
    assert_eq!(r.state, ContextState::Incomplete);
    assert_eq!(
        r.missing,
        ["objective", "desired_outcome", "blocks", "criteria"]
    );

    db.ddae_add_block(&s.id, "A", "").unwrap();
    let full = complete_details();
    let with = |f: &dyn Fn(&mut Details)| {
        let mut d = full.clone();
        f(&mut d);
        d
    };

    // 3. objective ausente → false
    db.ddae_update_details(&s.id, with(&|d| d.objective.clear()))
        .unwrap();
    assert_eq!(readiness(&db, &s.id).missing, ["objective"]);
    // 4. resultado desejado ausente → false
    db.ddae_update_details(&s.id, with(&|d| d.desired_outcome = "  ".into()))
        .unwrap();
    assert_eq!(readiness(&db, &s.id).missing, ["desired_outcome"]);
    // 6. sem critério → false
    db.ddae_update_details(&s.id, with(&|d| d.criteria.clear()))
        .unwrap();
    assert_eq!(readiness(&db, &s.id).missing, ["criteria"]);
    // critério só com espaços não conta
    db.ddae_update_details(&s.id, with(&|d| d.criteria = vec!["   ".into()]))
        .unwrap();
    assert_eq!(readiness(&db, &s.id).missing, ["criteria"]);

    // 8. completo + bloco pendente → true
    db.ddae_update_details(&s.id, full.clone()).unwrap();
    let r = readiness(&db, &s.id);
    assert!(r.ready && r.missing.is_empty());
    assert_eq!(r.state, ContextState::Ready);
}

#[test]
fn ready_for_ai_without_blocks_is_false() {
    let (_tmp, mut db, project) = setup();
    let s = db.ddae_create_session(&project, "Feature", "").unwrap();
    db.ddae_update_details(&s.id, complete_details()).unwrap();
    let r = readiness(&db, &s.id);
    assert!(!r.ready);
    assert_eq!(r.missing, ["blocks"]);
}

#[test]
fn ready_for_ai_accepts_current_block_and_needs_an_actionable_one() {
    let (_tmp, mut db, project) = setup();
    let id = ready_session(&mut db, &project);
    let blocks = db.ddae_session(&id).unwrap().blocks;
    // 7. bloco atual (e um pendente) → true
    db.ddae_start_block(&id, &blocks[0].id).unwrap();
    assert!(readiness(&db, &id).ready);
    // só o atual, sem pendentes → true
    db.ddae_complete_block(&id, &blocks[0].id).unwrap();
    db.ddae_start_block(&id, &blocks[1].id).unwrap();
    let r = readiness(&db, &id);
    assert!(r.ready, "{r:?}");
    // todos concluídos e sessão ainda aberta: não há o que continuar
    db.ddae_complete_block(&id, &blocks[1].id).unwrap();
    let r = readiness(&db, &id);
    assert!(!r.ready);
    assert_eq!(r.missing, ["actionable_block"]);
}

#[test]
fn completed_sessions_still_generate_context() {
    let (_tmp, mut db, project) = setup();
    let id = ready_session(&mut db, &project);
    for b in db.ddae_session(&id).unwrap().blocks {
        db.ddae_start_block(&id, &b.id).unwrap();
        db.ddae_complete_block(&id, &b.id).unwrap();
    }
    // Finalizar exige os critérios concluídos (existe 1 critério em ready_session).
    let mut details = complete_details();
    let criterion = db.ddae_session(&id).unwrap().criteria[0].clone();
    details.criteria = vec![ddae::Criterion {
        completed: true,
        ..criterion
    }];
    db.ddae_update_details(&id, details).unwrap();
    db.ddae_complete(&id, "Entregue").unwrap();
    assert_eq!(
        db.ddae_session(&id).unwrap().status,
        SessionStatus::Completed
    );
    let ctx = db.ddae_generate_context(&id).unwrap();
    assert_eq!(ctx.ready_for_ai.state, ContextState::Available);
    assert!(!ctx.ready_for_ai.ready);
    assert!(ctx
        .markdown
        .contains("Contexto disponível (sessão finalizada)"));
    assert!(ctx.markdown.contains("Resultado: Entregue"));
    assert!(!ctx.markdown.contains("Pronto para continuar"));
    // E ainda é terminal: detalhes não mudam.
    assert!(db.ddae_update_details(&id, complete_details()).is_err());
}

#[test]
fn ready_for_ai_is_never_persisted() {
    let (_tmp, mut db, project) = setup();
    let id = ready_session(&mut db, &project);
    let columns: Vec<String> = db
        .conn
        .prepare("SELECT name FROM pragma_table_info('ddae_sessions')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!columns.iter().any(|c| c.contains("ready")), "{columns:?}");
    let json = serde_json::to_string(&db.export_portable().unwrap().ddae).unwrap();
    assert!(!json.to_lowercase().contains("ready"), "{json}");
    assert!(readiness(&db, &id).ready);
}

#[test]
fn context_has_no_local_data() {
    let (tmp, mut db, project) = setup();
    // Identidade da máquina presente no banco: nada dela pode vazar para o contexto.
    let machine_id = "11111111-2222-4333-8444-555555555555";
    db.conn
        .execute(
            "INSERT INTO machine(id,machine_id,name,usage,description) VALUES(1,?1,'PC-SECRETO','dev','')",
            [machine_id],
        )
        .unwrap();
    let id = ready_session(&mut db, &project);
    let blocks = db.ddae_session(&id).unwrap().blocks;
    db.ddae_start_block(&id, &blocks[0].id).unwrap();
    db.ddae_add_decision(&id, "Decisão", "corpo da decisão", None)
        .unwrap();
    let md = db.ddae_generate_context(&id).unwrap().markdown;

    let folder = tmp.path().to_string_lossy().to_string();
    assert!(!md.contains(&folder), "caminho absoluto no contexto");
    assert!(!md.contains("proj-folder-secreta"));
    assert!(!md.contains(machine_id), "Machine ID no contexto");
    assert!(!md.contains("PC-SECRETO"));
    assert!(!md.contains("\\\\"), "barra invertida/UNC");
    assert!(!ddae::has_machine_path(&md));
    for var in ["COMPUTERNAME", "HOSTNAME", "USERNAME", "USER"] {
        if let Ok(value) = std::env::var(var) {
            if value.len() >= 4 {
                assert!(!md.contains(&value), "{var}={value} vazou");
            }
        }
    }
    // Nem PID nem IP: o contexto só tem dados da Session.
    assert!(!md.to_lowercase().contains("pid"));
    assert!(!regex_ip(&md), "endereço IP no contexto");
}

fn regex_ip(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .any(|w| {
            let parts: Vec<&str> = w.split('.').collect();
            parts.len() == 4
                && parts.iter().all(|p| {
                    !p.is_empty() && p.len() <= 3 && p.parse::<u16>().is_ok_and(|n| n <= 255)
                })
        })
}

#[test]
fn details_reject_local_paths_and_bad_references() {
    let (_tmp, mut db, project) = setup();
    let s = db.ddae_create_session(&project, "Feature", "").unwrap();
    let bad_text = |f: &dyn Fn(&mut Details)| {
        let mut d = complete_details();
        f(&mut d);
        d
    };
    for path in [r"C:\Users\x\app", "/home/x/app", "~/dev", r"\\srv\share"] {
        assert!(
            db.ddae_update_details(&s.id, bad_text(&|d| d.desired_outcome = path.into()))
                .is_err(),
            "{path}"
        );
        assert!(
            db.ddae_update_details(&s.id, bad_text(&|d| d.notes = vec![path.into()]))
                .is_err(),
            "{path}"
        );
        assert!(
            db.ddae_update_details(
                &s.id,
                bad_text(&|d| d.references = vec![Reference {
                    kind: ReferenceKind::ProjectPath,
                    value: path.into(),
                    label: None
                }])
            )
            .is_err(),
            "{path}"
        );
    }
    for path in [
        "../fora", "a/../b", "/abs", "a//b", "a\\b", "C:/x", "./x", "",
    ] {
        let d = bad_text(&|d| {
            d.references = vec![Reference {
                kind: ReferenceKind::ProjectPath,
                value: path.into(),
                label: None,
            }]
        });
        assert!(db.ddae_update_details(&s.id, d).is_err(), "{path:?}");
    }
    for url in [
        "http://x.com/a",
        "https://user:pw@x.com/a",
        "ftp://x/y",
        "https://",
    ] {
        let d = bad_text(&|d| {
            d.references = vec![Reference {
                kind: ReferenceKind::Url,
                value: url.into(),
                label: None,
            }]
        });
        assert!(db.ddae_update_details(&s.id, d).is_err(), "{url}");
    }
    assert!(db
        .ddae_update_details(
            &s.id,
            bad_text(&|d| d.criteria = vec!["x".repeat(ddae::MAX_ITEM + 1).as_str().into()])
        )
        .is_err());
    assert!(db
        .ddae_update_details(
            &s.id,
            bad_text(&|d| d.notes = vec!["n".into(); ddae::MAX_ITEMS + 1])
        )
        .is_err());
    // Nada foi gravado pelas tentativas inválidas.
    assert!(db.ddae_session(&s.id).unwrap().desired_outcome.is_empty());
    // Itens vazios somem na forma canônica.
    let saved = db
        .ddae_update_details(
            &s.id,
            bad_text(&|d| d.constraints = vec!["  a  ".into(), "".into(), "   ".into()]),
        )
        .unwrap();
    assert_eq!(saved.constraints, ["a"]);
}

#[test]
fn details_travel_in_the_portable_workspace() {
    let (_tmp, mut a, project) = setup();
    let id = ready_session(&mut a, &project);
    let ws = a.export_portable().unwrap();
    portable::validate(&ws).unwrap();
    let t2 = tempfile::tempdir().unwrap();
    let mut b = Database::open(&t2.path().join("b.db")).unwrap();
    b.apply_portable(&ws).unwrap();
    let got = b.ddae_session(&id).unwrap();
    assert_eq!(got.desired_outcome, "Feature funcionando de ponta a ponta");
    assert_eq!(
        got.criteria
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>(),
        ["Testes verdes"]
    );
    assert_eq!(got.references.len(), 2);
    assert_eq!(
        b.ddae_generate_context(&id).unwrap().markdown,
        a.ddae_generate_context(&id).unwrap().markdown,
        "o mesmo estado portátil gera o mesmo contexto em qualquer máquina"
    );
    // Um workspace com referência absoluta é recusado.
    let mut bad = ws.clone();
    bad.ddae[0].references[0].value = "C:\\Users\\x\\f.md".into();
    assert!(portable::validate(&bad).is_err());
    let mut bad = ws;
    bad.ddae[0].references[0].value = "../fora".into();
    assert!(portable::validate(&bad).is_err());
}

#[test]
fn legacy_session_001_is_not_ready_and_says_why() {
    // O Markdown legado não traz resultado desejado nem critérios: nada é inventado.
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
    let project = db
        .save(
            None,
            ProjectInput {
                name: "LKR_Lab".into(),
                description: String::new(),
                local_path: folder.to_string_lossy().into(),
                repository: String::new(),
                stack: vec![],
                tags: vec![],
                ports: vec![],
                commands: vec![],
            },
        )
        .unwrap();
    db.ddae_import_legacy(&project.id).unwrap();
    let view = db.ddae_overview(&project.id).unwrap().sessions.remove(0);
    assert!(!view.ready_for_ai.ready);
    assert_eq!(view.ready_for_ai.state, ContextState::Incomplete);
    assert_eq!(view.ready_for_ai.missing, ["desired_outcome", "criteria"]);
    assert!(view.session.desired_outcome.is_empty() && view.session.criteria.is_empty());
    // O contexto ainda é gerável e diz o que falta, sem completar conteúdo.
    let ctx = db.ddae_generate_context(&view.session.id).unwrap().markdown;
    assert!(ctx.contains("Incompleto — falta: resultado desejado, critérios de conclusão"));
    assert!(ctx.contains("## Resultado desejado\n\n_Não informado._"));
}
