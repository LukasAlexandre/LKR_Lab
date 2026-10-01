//! Execução REAL de Rust, Tauri e Docker Compose pelo supervisor único.
//!
//! * Rust: cargo de verdade em crates temporários (sem dependências → sem rede).
//! * Tauri: o fluxo `npm run tauri -- dev` de verdade, com uma CLI falsa (script Node) no lugar
//!   do build pesado — o que se testa é a resolução, as portas, os logs e o encerramento da árvore.
//! * Docker: smoke OPCIONAL (`LKR_DOCKER_SMOKE=1`, com o Docker Desktop aberto); nunca roda no CI.
mod common;
use common::{alive, fixture, have, project, state_of, supervisor, text_of, wait_until};
use hub_core::{
    runtime::{self, RuntimeStatus, ScriptKind},
    supervisor::{RunInfo, RunState, Supervisor},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const PACKAGE: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

fn finish(sup: &Supervisor, project: &str, run: &RunInfo) -> RunInfo {
    wait_until(&format!("{} terminar", run.command), || {
        !state_of(sup, project, &run.id).is_active()
    });
    sup.runs_for(project)
        .into_iter()
        .find(|r| r.id == run.id)
        .unwrap()
}
fn crate_dir(name: &str, main: &str, extra: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let mut files = vec![("Cargo.toml", PACKAGE), ("src/main.rs", main)];
    files.extend_from_slice(extra);
    fixture(name, &files)
}

// ------------------------------------------------------------------ Rust

#[test]
fn cargo_run_build_test_check_and_clippy_complete_with_exit_codes_and_logs() {
    if !have("cargo") {
        return;
    }
    let main = "fn main() { println!(\"ola do cargo run\"); }\n#[cfg(test)]\nmod t { #[test] fn soma() { assert_eq!(1 + 1, 2); } }\n";
    let (_t, dir) = crate_dir("app", main, &[]);
    let p = project("rs", &dir);
    let (sup, _) = supervisor();
    let clippy = hub_core::tools::Tools::probe(&hub_core::tools::Needs {
        rust: true,
        ..Default::default()
    })
    .clippy
    .available;
    for (id, expect) in [
        ("cargo:run", "ola do cargo run"),
        ("cargo:build", "Finished"),
        ("cargo:test", "test result: ok. 1 passed"),
        ("cargo:check", "Finished"),
        ("cargo:clippy", "Finished"),
    ] {
        if id == "cargo:clippy" && !clippy {
            continue;
        }
        let run = sup
            .start_command(&p, id, None)
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(
            (run.command_id.as_str(), run.source, run.observer),
            (id, "cargo", false)
        );
        let done = finish(&sup, "rs", &run);
        let text = text_of(&sup, &run.id);
        assert_eq!(
            (done.state, done.exit_code),
            (RunState::Completed, Some(0)),
            "{id}\n{text}"
        );
        assert!(text.contains(expect), "{id}: faltou {expect:?} em\n{text}");
        // Tarefas terminam: não ficam como "projeto em execução".
        let rt = runtime::inspect(
            &p,
            std::slice::from_ref(&p),
            sup.runs_for("rs"),
            &sup.managed_pids(),
        );
        assert_ne!(rt.status, RuntimeStatus::Running, "{id}");
        if id != "cargo:run" {
            assert_eq!(done.kind, ScriptKind::Task);
            let last = rt.last_task.expect("última tarefa");
            assert_eq!((last.state, last.exit_code), (RunState::Completed, Some(0)));
        }
    }
}

#[test]
fn a_failing_cargo_task_reports_the_exit_code_and_the_compiler_message() {
    if !have("cargo") {
        return;
    }
    let (_t, dir) = crate_dir("broken", "fn main() { let x = ; }\n", &[]);
    let p = project("bad", &dir);
    let (sup, _) = supervisor();
    let run = sup.start_command(&p, "cargo:build", None).unwrap();
    let done = finish(&sup, "bad", &run);
    assert_eq!(done.state, RunState::Failed);
    assert_eq!(
        done.exit_code,
        Some(101),
        "cargo sinaliza erro de compilação com 101"
    );
    assert!(
        text_of(&sup, &run.id).contains("error"),
        "o erro do compilador precisa aparecer no log"
    );
    let rt = runtime::inspect(
        &p,
        std::slice::from_ref(&p),
        sup.runs_for("bad"),
        &sup.managed_pids(),
    );
    let last = rt.last_task.unwrap();
    assert_eq!((last.state, last.exit_code), (RunState::Failed, Some(101)));
    // Falha de tarefa não é "serviço com erro": o status do projeto não vira Error por isso.
    assert_ne!(rt.status, RuntimeStatus::Error);
}

#[test]
fn cancelling_a_long_build_stops_cargo_rustc_and_the_build_script() {
    if !have("cargo") {
        return;
    }
    let slow = "fn main() { println!(\"cargo:warning=build.rs iniciou\"); std::thread::sleep(std::time::Duration::from_secs(300)); }\n";
    let (_t, dir) = crate_dir("slow", "fn main() {}\n", &[("build.rs", slow)]);
    let p = project("slow", &dir);
    let (sup, _) = supervisor();
    let run = sup.start_command(&p, "cargo:build", None).unwrap();
    // Espera o script de build (o processo lento) existir DENTRO da árvore gerenciada.
    let mut build_script: Option<u32> = None;
    wait_until("script de build rodando", || {
        let managed = sup.managed_pids();
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        build_script = managed.keys().copied().find(|pid| {
            system
                .process(sysinfo::Pid::from_u32(*pid))
                .is_some_and(|proc| {
                    proc.name()
                        .to_string_lossy()
                        .to_lowercase()
                        .contains("build-script-build")
                })
        });
        build_script.is_some()
    });
    let tree: Vec<u32> = sup.managed_pids().keys().copied().collect();
    assert!(tree.len() >= 2, "cargo + script de build: {tree:?}");
    sup.stop(&run.id).unwrap();
    let done = finish(&sup, "slow", &run);
    assert_eq!(
        done.state,
        RunState::Stopped,
        "cancelar é Parado/Cancelado, não falha"
    );
    wait_until("árvore encerrada", || tree.iter().all(|pid| !alive(*pid)));
    assert!(!alive(build_script.unwrap()));
    assert!(sup.managed_pids().is_empty(), "nada órfão");
}

#[test]
fn choosing_a_binary_runs_only_that_binary_and_restart_keeps_the_choice() {
    if !have("cargo") {
        return;
    }
    let manifest = format!("{PACKAGE}[[bin]]\nname = \"alpha\"\npath = \"src/a.rs\"\n[[bin]]\nname = \"beta\"\npath = \"src/b.rs\"\n");
    let (_t, dir) = fixture(
        "two",
        &[
            ("Cargo.toml", &manifest),
            ("src/a.rs", "fn main() { println!(\"sou o alpha\"); }"),
            ("src/b.rs", "fn main() { println!(\"sou o beta\"); }"),
        ],
    );
    let p = project("two", &dir);
    let (sup, _) = supervisor();
    // Sem escolha, nada roda (e nenhum binário é escolhido "no escuro").
    assert!(sup
        .start_command(&p, "cargo:run", None)
        .unwrap_err()
        .contains("mais de uma opção"));
    assert!(sup.runs_for("two").is_empty());
    let run = sup.start_command(&p, "cargo:run", Some("beta")).unwrap();
    assert_eq!(
        (run.selection.as_deref(), run.command.as_str()),
        (Some("beta"), "cargo run --bin beta")
    );
    let done = finish(&sup, "two", &run);
    let text = text_of(&sup, &run.id);
    assert!(
        done.exit_code == Some(0) && text.contains("sou o beta") && !text.contains("sou o alpha"),
        "{text}"
    );
    // Reiniciar reexecuta a MESMA escolha.
    let again = sup.restart(&p, &run.id).unwrap();
    assert_eq!(
        (again.command_id.as_str(), again.selection.as_deref()),
        ("cargo:run", Some("beta"))
    );
    finish(&sup, "two", &again);
    assert!(text_of(&sup, &again.id).contains("sou o beta"));
}

// ------------------------------------------------------------------ Tauri (CLI falsa)

const FAKE_TAURI: &str = r#"
const fs = require('fs');
const net = require('net');
const { spawn } = require('child_process');
const conf = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const port = Number(new URL(conf.build.devUrl).port);
const args = process.argv.slice(2);
console.log('        Info arguments: ' + args.join(' '));
console.log('Running BeforeDevCommand (`npm run dev`)');
console.log('  VITE v5.0.0  ready in 120 ms');
console.log('  \u279c  Local:   http://localhost:' + port + '/');
console.log('   Compiling desk v0.1.0 (C:\\fake)');
console.log('linha qualquer do app');
if (args[0] === 'dev') {
  net.createServer(() => {}).listen(port, '127.0.0.1');
  const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
  console.log('filho ' + child.pid);
  setInterval(() => {}, 1000);
} else {
  console.log('construindo ' + args.join(' '));
  process.exit(0);
}
"#;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn fake_tauri_project(port: u16) -> (tempfile::TempDir, PathBuf) {
    let conf =
        format!(r#"{{"identifier":"a.b.c","build":{{"devUrl":"http://localhost:{port}"}}}}"#);
    fixture(
        "desk",
        &[
            (
                "package.json",
                r#"{"name":"desk","scripts":{"tauri":"node fake-tauri.js","dev":"vite"},"devDependencies":{"@tauri-apps/cli":"2"}}"#,
            ),
            ("package-lock.json", "{}"),
            ("fake-tauri.js", FAKE_TAURI),
            ("src-tauri/tauri.conf.json", &conf),
            (
                "src-tauri/Cargo.toml",
                "[package]\nname = \"desk\"\nversion = \"0.1.0\"\n[dependencies]\ntauri = \"2\"\n",
            ),
        ],
    )
}

#[test]
fn tauri_dev_runs_through_the_supervisor_with_sources_ownership_and_a_clean_stop() {
    if !have("npm") || !have("node") {
        return;
    }
    let port = free_port();
    let (_t, dir) = fake_tauri_project(port);
    let p = project("desk", &dir);
    let (sup, events) = supervisor();
    let run = sup.start_command(&p, "tauri:dev", None).unwrap();
    assert_eq!(
        (run.command.as_str(), run.source, run.kind),
        ("npm run tauri -- dev", "tauri", ScriptKind::Service)
    );
    wait_until("tauri dev rodando", || {
        state_of(&sup, "desk", &run.id) == RunState::Running
    });
    wait_until("filho criado", || text_of(&sup, &run.id).contains("filho "));
    // Os argumentos chegaram à CLI (npm run tauri -- dev → `dev`).
    assert!(text_of(&sup, &run.id).contains("Info arguments: dev"));

    // Fonte de cada linha, só quando inequívoca; o resto fica sem etiqueta.
    let lines = sup.logs(&run.id, 0).unwrap().lines;
    let tag = |needle: &str| {
        lines
            .iter()
            .find(|l| l.text.contains(needle))
            .unwrap_or_else(|| panic!("sem linha {needle}"))
            .source
    };
    assert_eq!(tag("Info arguments"), Some("tauri"));
    assert_eq!(tag("Running BeforeDevCommand"), Some("tauri"));
    assert_eq!(tag("VITE v5"), Some("vite"));
    assert_eq!(tag("Local:"), Some("vite"));
    assert_eq!(tag("Compiling desk"), Some("cargo"));
    assert_eq!(tag("linha qualquer"), None);

    // A porta do dev server só é do projeto porque o dono (PID) está na árvore gerenciada.
    let mut label_ok = false;
    wait_until("porta com dono verificado", || {
        let rt = runtime::inspect_live(&p, std::slice::from_ref(&p), sup.runs_for("desk"), &|| {
            sup.managed_pids()
        });
        label_ok = rt
            .services
            .iter()
            .any(|s| s.port == Some(port) && s.managed && s.label == "Frontend (devUrl do Tauri)");
        label_ok
    });
    let rt = runtime::inspect_live(&p, std::slice::from_ref(&p), sup.runs_for("desk"), &|| {
        sup.managed_pids()
    });
    assert_eq!(rt.status, RuntimeStatus::Running);
    assert_eq!(rt.primary_command.as_deref(), Some("tauri:dev"));
    assert!(
        rt.ports.iter().all(|x| x.managed),
        "só portas de dono gerenciado: {:?}",
        rt.ports
    );
    // Duplicar o mesmo Tauri Dev é recusado.
    assert!(sup
        .start_command(&p, "tauri:dev", None)
        .unwrap_err()
        .contains("já está em execução"));

    let tree: Vec<u32> = sup.managed_pids().keys().copied().collect();
    assert!(tree.len() >= 3, "npm → node → filho: {tree:?}");
    sup.stop_and_wait(&run.id, Duration::from_secs(20)).unwrap();
    assert_eq!(state_of(&sup, "desk", &run.id), RunState::Stopped);
    wait_until("árvore inteira encerrada", || {
        tree.iter().all(|pid| !alive(*pid))
    });
    assert!(
        std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(300)
        )
        .is_err(),
        "a porta precisa ser liberada"
    );
    assert!(events.lock().unwrap().iter().any(|e| matches!(
        e,
        hub_core::supervisor::RuntimeEvent::State {
            state: RunState::Stopping,
            ..
        }
    )));
}

#[test]
fn tauri_build_is_a_task_with_an_exit_code_and_never_a_service() {
    if !have("npm") || !have("node") {
        return;
    }
    let (_t, dir) = fake_tauri_project(free_port());
    let p = project("desk", &dir);
    let (sup, _) = supervisor();
    let run = sup.start_command(&p, "tauri:build", None).unwrap();
    assert_eq!(
        (run.command.as_str(), run.kind),
        ("npm run tauri -- build", ScriptKind::Task)
    );
    let done = finish(&sup, "desk", &run);
    assert_eq!((done.state, done.exit_code), (RunState::Completed, Some(0)));
    assert!(text_of(&sup, &run.id).contains("construindo build"));
    let rt = runtime::inspect(
        &p,
        std::slice::from_ref(&p),
        sup.runs_for("desk"),
        &sup.managed_pids(),
    );
    assert_ne!(
        rt.status,
        RuntimeStatus::Running,
        "build não é serviço persistente"
    );
    assert_eq!(rt.last_task.unwrap().command, "npm run tauri -- build");
}

// ------------------------------------------------------------------ Docker (opt-in)

struct ComposeCleanup {
    name: String,
    dir: PathBuf,
}
impl Drop for ComposeCleanup {
    // Mesmo se o teste falhar no meio: derruba SÓ o projeto de teste (nome único), nunca outro.
    fn drop(&mut self) {
        let _ = std::process::Command::new("docker")
            .args([
                "compose",
                "-p",
                &self.name,
                "down",
                "--remove-orphans",
                "-t",
                "1",
            ])
            .current_dir(&self.dir)
            .output();
    }
}

#[test]
fn compose_lifecycle_up_logs_restart_and_down_with_real_docker() {
    if std::env::var("LKR_DOCKER_SMOKE").is_err() {
        eprintln!("ignorado: defina LKR_DOCKER_SMOKE=1 com o Docker Desktop aberto para rodar o smoke do Compose");
        return;
    }
    let needs = hub_core::tools::Needs {
        compose: true,
        ..Default::default()
    };
    if !hub_core::tools::Tools::probe(&needs).daemon.available {
        eprintln!("ignorado: Docker Desktop indisponível");
        return;
    }
    let name = format!(
        "lkr-smoke-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let yaml = format!(
        "name: {name}\nservices:\n  web:\n    image: node:24.19.0-alpine\n    command: [\"node\", \"-e\", \"console.log('web no ar'); setInterval(() => {{}}, 1000)\"]\n  extra:\n    image: node:24.19.0-alpine\n    command: [\"node\", \"-e\", \"setInterval(() => {{}}, 1000)\"]\n    profiles: [\"extra\"]\n    environment:\n      SEGREDO: nao-pode-vazar\n"
    );
    let (_t, dir) = fixture(&name, &[("compose.yaml", &yaml)]);
    let _cleanup = ComposeCleanup {
        name: name.clone(),
        dir: dir.clone(),
    };
    let p = project("smoke", &dir);
    let (sup, _) = supervisor();
    let inspect = || {
        runtime::inspect_live(&p, std::slice::from_ref(&p), sup.runs_for("smoke"), &|| {
            sup.managed_pids()
        })
    };

    // Antes de subir: serviços do config, sem containers, e nada para parar.
    let rt = inspect();
    let compose = rt.compose.as_ref().expect("projeto Compose");
    assert_eq!(
        (compose.containers, compose.running, compose.expected),
        (0, 0, 1),
        "o serviço com profile não conta como esperado"
    );
    assert_eq!(
        compose
            .services
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["extra", "web"]
    );
    assert!(
        !rt.commands
            .iter()
            .find(|c| c.id == "compose:down")
            .unwrap()
            .available
    );
    assert!(!serde_json::to_string(&rt)
        .unwrap()
        .contains("nao-pode-vazar"));

    // Subir: `up -d` é uma tarefa; o estado real vem do `ps`, não do processo do cliente.
    let up = sup.start_command(&p, "compose:up", None).unwrap();
    assert_eq!((up.kind, up.source), (ScriptKind::Task, "compose"));
    let done = finish(&sup, "smoke", &up);
    assert_eq!(
        (done.state, done.exit_code),
        (RunState::Completed, Some(0)),
        "{}",
        text_of(&sup, &up.id)
    );
    let rt = inspect();
    let compose = rt.compose.as_ref().unwrap();
    assert_eq!(
        (compose.containers, compose.running, compose.started_here),
        (1, 1, true)
    );
    assert_eq!(
        compose
            .services
            .iter()
            .find(|s| s.name == "web")
            .unwrap()
            .state,
        "running"
    );
    assert_eq!(
        compose
            .services
            .iter()
            .find(|s| s.name == "extra")
            .unwrap()
            .state,
        "absent"
    );
    assert_eq!(rt.status, RuntimeStatus::Running);
    assert!(
        rt.commands
            .iter()
            .find(|c| c.id == "compose:down")
            .unwrap()
            .available
    );

    // Logs ao vivo: observador de um serviço; não conta como projeto em execução.
    let logs = sup.start_command(&p, "compose:logs", Some("web")).unwrap();
    assert!(logs.observer);
    wait_until("log do serviço", || {
        text_of(&sup, &logs.id).contains("web no ar")
    });
    assert!(sup
        .start_command(&p, "compose:logs", Some("web"))
        .unwrap_err()
        .contains("já está em execução"));
    assert!(sup
        .start_command(&p, "compose:logs", Some("nao-existe"))
        .is_err());

    // Reiniciar containers: tarefa; os containers voltam a rodar.
    let restart = sup.start_command(&p, "compose:restart", None).unwrap();
    // Operações do Compose do mesmo projeto não se atropelam.
    if state_of(&sup, "smoke", &restart.id).is_active() {
        assert!(sup
            .start_command(&p, "compose:down", None)
            .unwrap_err()
            .contains("operação do Compose"));
    }
    assert_eq!(finish(&sup, "smoke", &restart).exit_code, Some(0));
    assert_eq!(inspect().compose.unwrap().running, 1);

    // Parar: `down` do MESMO projeto; encerra também o observador (nada de `logs -f` órfão).
    let down = sup.start_command(&p, "compose:down", None).unwrap();
    assert_eq!(finish(&sup, "smoke", &down).exit_code, Some(0));
    wait_until("observador encerrado", || {
        !state_of(&sup, "smoke", &logs.id).is_active()
    });
    wait_until("sem processos gerenciados", || {
        sup.managed_pids().is_empty()
    });
    let rt = inspect();
    let compose = rt.compose.as_ref().unwrap();
    assert_eq!(
        (compose.containers, compose.running, compose.started_here),
        (0, 0, false)
    );
    assert_ne!(rt.status, RuntimeStatus::Running);
    // O Docker em si continua de pé: nada global foi encerrado.
    assert!(
        hub_core::tools::Tools::probe(&needs).daemon.available || {
            hub_core::tools::forget();
            hub_core::tools::Tools::probe(&needs).daemon.available
        }
    );
    let _ = Path::new(&dir);
}
