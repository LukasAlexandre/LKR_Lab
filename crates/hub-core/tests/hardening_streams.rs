//! Block 10 — captura real de stdout e stderr por um subprocesso CONTROLADO (Node): streams
//! separados, ordem preservada, sequência crescente, código de saída e nenhum processo restante.
mod common;
use common::{alive, fixture, project, supervisor, wait_until};
use hub_core::supervisor::RunState;

const STREAMS: &str = r#"
const step = (stream, text, ms) => new Promise((resolve) => setTimeout(() => { (stream === "out" ? process.stdout : process.stderr).write(text + "\n"); resolve(); }, ms));
(async () => {
  await step("out", "OUT-1 conhecido", 80);
  await step("err", "ERR-1 conhecido", 80);
  await step("out", "OUT-2 conhecido", 80);
  await step("err", "ERR-2 conhecido", 80);
  process.exit(Number(process.env.EXIT_CODE || 0));
})();
"#;

fn tools() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn stdout_and_stderr_are_captured_separately_in_order_with_increasing_sequence() {
    if !tools() {
        eprintln!("node indisponível: cenário ignorado");
        return;
    }
    let (_tmp, dir) = fixture(
        "streams",
        &[
            (
                "package.json",
                r#"{"scripts":{"streams":"node streams.js"}}"#,
            ),
            ("package-lock.json", "{}"),
            ("streams.js", STREAMS),
        ],
    );
    let p = project("st", &dir);
    let (sup, _) = supervisor();
    let run = sup.start(&p, "streams").unwrap();
    let state = |id: &str| sup.runs_for("st").into_iter().find(|r| r.id == id).unwrap();
    wait_until("terminou", || !state(&run.id).state.is_active());
    let info = state(&run.id);
    assert_eq!((info.state, info.exit_code), (RunState::Completed, Some(0)));
    assert!(info.ended_at.is_some());

    let chunk = sup.logs(&run.id, 0).unwrap();
    let ours: Vec<(&str, &str, u64)> = chunk
        .lines
        .iter()
        .filter(|l| l.text.contains("conhecido"))
        .map(|l| (l.stream, l.text.as_str(), l.seq))
        .collect();
    assert_eq!(
        ours.iter().map(|(s, t, _)| (*s, *t)).collect::<Vec<_>>(),
        vec![
            ("out", "OUT-1 conhecido"),
            ("err", "ERR-1 conhecido"),
            ("out", "OUT-2 conhecido"),
            ("err", "ERR-2 conhecido"),
        ],
        "streams separados e na ordem em que o processo escreveu"
    );
    assert!(
        ours.windows(2).all(|w| w[0].2 < w[1].2),
        "sequência estritamente crescente: {ours:?}"
    );
    // nada de stdout marcado como stderr nem o contrário
    assert!(chunk
        .lines
        .iter()
        .filter(|l| l.stream == "err")
        .all(|l| !l.text.contains("OUT-")));
    assert!(chunk
        .lines
        .iter()
        .filter(|l| l.stream == "out")
        .all(|l| !l.text.contains("ERR-")));
    // ler de novo a partir do fim não repete linhas
    assert!(sup.logs(&run.id, chunk.next_seq).unwrap().lines.is_empty());
    assert!(
        sup.managed_pids().is_empty(),
        "nada ficou como processo gerenciado"
    );
}

#[test]
fn a_failing_controlled_process_keeps_its_exit_code_and_last_stderr() {
    if !tools() {
        return;
    }
    let (_tmp, dir) = fixture(
        "streamsfail",
        &[
            (
                "package.json",
                r#"{"scripts":{"streams":"node streams.js"}}"#,
            ),
            ("package-lock.json", "{}"),
            (
                "streams.js",
                &STREAMS.replace("process.env.EXIT_CODE || 0", "7"),
            ),
        ],
    );
    let p = project("sf", &dir);
    let (sup, _) = supervisor();
    let run = sup.start(&p, "streams").unwrap();
    wait_until("terminou", || {
        sup.runs_for("sf")
            .into_iter()
            .any(|r| r.id == run.id && !r.state.is_active())
    });
    let info = sup
        .runs_for("sf")
        .into_iter()
        .find(|r| r.id == run.id)
        .unwrap();
    assert_eq!((info.state, info.exit_code), (RunState::Failed, Some(7)));
    let text: Vec<String> = sup
        .logs(&run.id, 0)
        .unwrap()
        .lines
        .iter()
        .map(|l| format!("{}:{}", l.stream, l.text))
        .collect();
    assert!(text.iter().any(|l| l == "err:ERR-2 conhecido"), "{text:?}");
    let pid = info.pid;
    if let Some(pid) = pid {
        wait_until("processo encerrado", || !alive(pid));
    }
}
