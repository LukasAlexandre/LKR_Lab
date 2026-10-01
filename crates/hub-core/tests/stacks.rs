//! Rust, Tauri e Docker Compose no Project Runtime Manager — resolução de comandos.
//!
//! Estes testes NÃO dependem de cargo, Tauri CLI ou Docker instalados: a detecção lê os
//! arquivos das fixtures e a disponibilidade das ferramentas é injetada (`Tools`). A execução
//! real fica em `stacks_exec.rs`.
mod common;
use common::{fixture, project, write};
use hub_core::{
    actions::{self, ActionGroup, ComposeView, RuntimeCommand},
    compose,
    runtime::{self, ScriptKind},
    rust_project,
    tools::{ToolStatus, Tools},
};
use std::path::{Path, PathBuf};

fn ok(id: &'static str, label: &'static str) -> ToolStatus {
    ToolStatus::ok(id, label, Some("teste".into()), Some(PathBuf::from(id)))
}
fn tools_with(pm: &'static str) -> Tools {
    Tools::all_available().with_package_manager(ok(pm, pm))
}
fn tools() -> Tools {
    tools_with("npm")
}
fn find<'a>(commands: &'a [RuntimeCommand], id: &str) -> &'a RuntimeCommand {
    commands.iter().find(|c| c.id == id).unwrap_or_else(|| {
        panic!(
            "ação {id} ausente em {:?}",
            commands.iter().map(|c| &c.id).collect::<Vec<_>>()
        )
    })
}
fn build(dir: &Path, tools: &Tools, view: Option<&ComposeView>) -> Vec<RuntimeCommand> {
    actions::build_commands(&runtime::detect(dir), tools, dir, view)
}

const PACKAGE: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

// ------------------------------------------------------------------ Rust

#[test]
fn rust_single_binary_offers_run_and_tasks_with_structured_args() {
    let (_t, dir) = fixture(
        "app",
        &[("Cargo.toml", PACKAGE), ("src/main.rs", "fn main() {}")],
    );
    let d = runtime::detect(&dir);
    let rust = d.rust.as_ref().unwrap();
    assert_eq!(
        (rust.dir.as_str(), rust.workspace, rust.bins.len()),
        (".", false, 1)
    );
    assert_eq!(rust.bins[0].name, "app");
    let commands = actions::build_commands(&d, &tools(), &dir, None);

    let run = find(&commands, "cargo:run");
    assert_eq!(
        (run.program.as_str(), run.args.clone()),
        ("cargo", vec!["run".to_string()])
    );
    assert_eq!(
        (run.kind, run.group, run.available, run.selection_required),
        (ScriptKind::Service, ActionGroup::Run, true, false)
    );
    for (id, arg, group) in [
        ("cargo:build", "build", ActionGroup::Build),
        ("cargo:test", "test", ActionGroup::Quality),
        ("cargo:check", "check", ActionGroup::Quality),
        ("cargo:clippy", "clippy", ActionGroup::Quality),
    ] {
        let c = find(&commands, id);
        assert_eq!(
            (c.args.clone(), c.kind, c.group, c.available),
            (vec![arg.to_string()], ScriptKind::Task, group, true),
            "{id}"
        );
        assert!(!c.long_running);
    }
    assert_eq!(
        actions::primary(&commands, &d).as_deref(),
        Some("cargo:run")
    );
    // Programa e argumentos separados: nenhum argumento carrega uma linha de comando.
    assert!(commands.iter().all(|c| c
        .args
        .iter()
        .all(|a| !a.contains(' ') && !a.contains('&') && !a.contains(';'))));
    // Ordem por intenção: Execução → Qualidade → Build.
    let groups: Vec<_> = commands.iter().map(|c| c.group).collect();
    assert!(groups.windows(2).all(|w| (w[0] as u8) <= (w[1] as u8)));
}

#[test]
fn rust_multiple_binaries_require_a_choice_and_never_pick_the_first() {
    let manifest = format!("{PACKAGE}[[bin]]\nname = \"alpha\"\npath = \"src/a.rs\"\n[[bin]]\nname = \"beta\"\npath = \"src/b.rs\"\n");
    let (_t, dir) = fixture(
        "multi",
        &[
            ("Cargo.toml", &manifest),
            ("src/a.rs", "fn main() {}"),
            ("src/b.rs", "fn main() {}"),
            ("src/bin/gamma.rs", "fn main() {}"),
        ],
    );
    let d = runtime::detect(&dir);
    let names: Vec<_> = d
        .rust
        .as_ref()
        .unwrap()
        .bins
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    let commands = actions::build_commands(&d, &tools(), &dir, None);
    let run = find(&commands, "cargo:run");
    assert!(run.selection_required && run.available);
    assert_eq!(
        run.choices
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta", "gamma"]
    );
    assert_eq!(
        run.args,
        vec!["run".to_string()],
        "sem escolha, o argumento --bin não existe"
    );
    assert_eq!(
        actions::primary(&commands, &d),
        None,
        "ambíguo: o botão vira 'Rodar ▾'"
    );
}

#[test]
fn rust_default_run_resolves_the_ambiguity_only_when_it_names_a_real_binary() {
    let with = |default: &str| {
        format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\ndefault-run = \"{default}\"\n[[bin]]\nname = \"alpha\"\npath = \"src/a.rs\"\n[[bin]]\nname = \"beta\"\npath = \"src/b.rs\"\n")
    };
    let files = |m: &str| {
        vec![
            ("Cargo.toml".to_string(), m.to_string()),
            ("src/a.rs".into(), "fn main() {}".into()),
            ("src/b.rs".into(), "fn main() {}".into()),
        ]
    };
    for (default, required) in [("beta", false), ("inexistente", true)] {
        let m = with(default);
        let owned = files(&m);
        let refs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let (_t, dir) = fixture("dr", &refs);
        let commands = build(&dir, &tools(), None);
        assert_eq!(
            find(&commands, "cargo:run").selection_required,
            required,
            "default-run = {default}"
        );
    }
}

#[test]
fn cargo_workspace_single_binary_is_explicit_and_multiple_packages_ask() {
    // Workspace virtual com 1 binário e 1 biblioteca.
    let (_t, dir) = fixture(
        "ws",
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n",
            ),
            (
                "crates/core/Cargo.toml",
                "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("crates/core/src/lib.rs", ""),
            (
                "crates/server/Cargo.toml",
                "[package]\nname = \"server\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("crates/server/src/main.rs", "fn main() {}"),
        ],
    );
    let d = runtime::detect(&dir);
    let rust = d.rust.as_ref().unwrap();
    assert!(rust.workspace && rust.virtual_manifest);
    assert_eq!(rust.packages, vec!["core", "server"]);
    let commands = actions::build_commands(&d, &tools(), &dir, None);
    let run = find(&commands, "cargo:run");
    assert_eq!(
        run.args,
        ["run", "--package", "server", "--bin", "server"].map(String::from)
    );
    assert!(!run.selection_required);

    // Dois pacotes com binário: precisa escolher, no formato pacote › binário.
    // (Outra pasta: a detecção em cache só percebe membros novos do workspace depois do TTL.)
    let pkg = |name: &str| {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")
    };
    let (_t2, dir2) = fixture(
        "ws2",
        &[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            ("crates/server/Cargo.toml", &pkg("server")),
            ("crates/server/src/main.rs", "fn main() {}"),
            ("crates/cli/Cargo.toml", &pkg("cli")),
            ("crates/cli/src/main.rs", "fn main() {}"),
        ],
    );
    let commands = build(&dir2, &tools(), None);
    let run = find(&commands, "cargo:run");
    assert!(run.selection_required);
    assert_eq!(
        run.choices
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["cli/cli", "server/server"]
    );
    assert_eq!(run.choices[0].label, "cli › cli");
}

#[test]
fn rust_library_only_has_no_run_but_keeps_the_tasks() {
    let (_t, dir) = fixture(
        "lib",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"lib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("src/lib.rs", ""),
        ],
    );
    let commands = build(&dir, &tools(), None);
    assert!(commands.iter().all(|c| c.id != "cargo:run"));
    assert!(commands.iter().any(|c| c.id == "cargo:test"));
}

#[test]
fn cargo_and_clippy_unavailable_block_commands_with_a_clear_reason() {
    let (_t, dir) = fixture(
        "app",
        &[("Cargo.toml", PACKAGE), ("src/main.rs", "fn main() {}")],
    );
    // Rust detectado, Cargo ausente: tudo bloqueado, com o motivo e sem caminho de execução.
    let mut none = tools();
    none.cargo = ToolStatus::missing(
        "cargo",
        "Cargo",
        "Cargo não encontrado no PATH. Instale o Rust (rustup.rs) e reinicie o aplicativo.",
    );
    none.clippy = ToolStatus::missing(
        "clippy",
        "Clippy",
        "Depende do Cargo, que não está disponível.",
    );
    let commands = build(&dir, &none, None);
    assert_eq!(
        runtime::detect(&dir)
            .stack
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        vec!["rust"]
    );
    for command in commands.iter().filter(|c| c.source == "cargo") {
        assert!(
            !command.available && command.program_path.is_none(),
            "{}",
            command.id
        );
        assert!(
            command
                .unavailable_reason
                .as_deref()
                .unwrap()
                .contains("Cargo não encontrado"),
            "{}",
            command.id
        );
    }
    // Só o Clippy ausente: os outros seguem disponíveis e o motivo é específico.
    let mut no_clippy = tools();
    no_clippy.clippy = ToolStatus::missing(
        "clippy",
        "Clippy",
        "Clippy não está instalado neste toolchain (rustup component add clippy).",
    );
    let commands = build(&dir, &no_clippy, None);
    assert!(find(&commands, "cargo:build").available && find(&commands, "cargo:test").available);
    let clippy = find(&commands, "cargo:clippy");
    assert!(
        !clippy.available
            && clippy
                .unavailable_reason
                .as_deref()
                .unwrap()
                .contains("rustup component add clippy")
    );
}

#[test]
fn rust_manifests_are_read_passively_and_hostile_members_are_ignored() {
    // Membros fora da pasta, absolutos e padrões desconhecidos não viram pacotes.
    let (_t, dir) = fixture(
        "hostile",
        &[
            ("Cargo.toml", "[workspace]\nmembers = [\"../outside\", \"/etc\", \"C:/Windows\", \"crates/**\", \"ok\"]\n"),
            ("ok/Cargo.toml", "[package]\nname = \"ok\"\nversion = \"0.1.0\"\n"),
            ("ok/src/main.rs", "fn main() {}"),
        ],
    );
    let rust = rust_project::detect(&dir).unwrap();
    assert_eq!(rust.packages, vec!["ok"]);
    assert!(
        rust.notes.iter().any(|n| n.contains("fora da pasta"))
            && rust.notes.iter().any(|n| n.contains("não suportado"))
    );
    // TOML inválido, grande demais ou vazio: nota, nunca pânico.
    let (_t2, broken) = fixture("broken", &[("Cargo.toml", "[package\nname = ")]);
    let info = rust_project::detect(&broken).unwrap();
    assert!(info.packages.is_empty() && info.notes[0].contains("sintaxe"));
    let (_t3, empty) = fixture("empty", &[("Cargo.toml", "")]);
    let info = rust_project::detect(&empty).unwrap();
    assert!(info.virtual_manifest && info.bins.is_empty());
    // Nome de binário com metacaracteres de shell é descartado na origem.
    let bad = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[[bin]]\nname = \"a b&calc\"\npath = \"src/x.rs\"\n[[bin]]\nname = \"fine\"\npath = \"src/y.rs\"\n";
    let (_t4, dir4) = fixture("badbin", &[("Cargo.toml", bad)]);
    let names: Vec<_> = rust_project::detect(&dir4)
        .unwrap()
        .bins
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, vec!["fine"]);
    // Sem Cargo.toml no lugar esperado: não é Rust.
    let (_t5, none) = fixture("none", &[("notes.txt", "x")]);
    assert!(rust_project::detect(&none).is_none());
}

// ------------------------------------------------------------------ Tauri

fn tauri_project(
    lock: &str,
    package_scripts: &str,
    extra: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf) {
    let package = format!(
        r#"{{"name":"app","devDependencies":{{"@tauri-apps/cli":"^2.0.0","vite":"5"}},"dependencies":{{"react":"19"}},"scripts":{package_scripts}}}"#
    );
    let mut files: Vec<(&str, &str)> = vec![
        ("package.json", package.as_str()),
        ("vite.config.ts", ""),
        ("src-tauri/tauri.conf.json", r#"{"identifier":"x.y.z","build":{"devUrl":"http://localhost:1420"}}"#),
        ("src-tauri/Cargo.toml", "[package]\nname = \"desk\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\ntauri = { version = \"2.11.5\", features = [] }\n"),
        ("src-tauri/src/main.rs", "fn main() {}"),
    ];
    if !lock.is_empty() {
        files.push((lock, "{}"));
    }
    files.extend_from_slice(extra);
    fixture("desk", &files)
}
const LOCAL_CLI: [(&str, &str); 2] = [
    ("node_modules/.bin/tauri", ""),
    ("node_modules/.bin/tauri.cmd", ""),
];

#[test]
fn tauri_uses_the_package_script_with_the_arguments_of_each_package_manager() {
    for (lock, pm, prefix) in [
        ("package-lock.json", "npm", vec!["run", "tauri", "--"]),
        ("pnpm-lock.yaml", "pnpm", vec!["run", "tauri"]),
        ("yarn.lock", "yarn", vec!["run", "tauri"]),
        ("bun.lock", "bun", vec!["run", "tauri"]),
    ] {
        let (_t, dir) = tauri_project(
            lock,
            r#"{"dev":"vite","build":"vite build","tauri":"tauri"}"#,
            &LOCAL_CLI,
        );
        let commands = build(&dir, &tools_with(pm), None);
        let dev = find(&commands, "tauri:dev");
        assert_eq!(dev.program, pm);
        assert_eq!(
            dev.args,
            prefix
                .iter()
                .chain(["dev"].iter())
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            "{pm}"
        );
        assert!(
            dev.available
                && dev.long_running
                && dev.kind == ScriptKind::Service
                && dev.group == ActionGroup::Run
        );
        let tauri_build = find(&commands, "tauri:build");
        assert_eq!(
            (tauri_build.kind, tauri_build.group),
            (ScriptKind::Task, ActionGroup::Build)
        );
        assert_eq!(tauri_build.args.last().map(String::as_str), Some("build"));
        // O script "tauri" sozinho só imprime ajuda: não aparece como ação Node.
        assert!(
            commands.iter().all(|c| c.id != "node:tauri")
                && commands.iter().any(|c| c.id == "node:dev")
        );
    }
}

#[test]
fn tauri_without_the_script_uses_the_local_cli_then_cargo_tauri_and_otherwise_explains() {
    // CLI local instalada, sem script: executa o binário DO PROJETO, não uma CLI global.
    let (_t, dir) = tauri_project("package-lock.json", r#"{"dev":"vite"}"#, &LOCAL_CLI);
    let dev = find(&build(&dir, &tools(), None), "tauri:dev").clone();
    assert!(dev.available && dev.args == ["dev".to_string()] && dev.program == "tauri");
    let path = dev.program_path.unwrap();
    assert!(
        path.starts_with(dir.join("node_modules"))
            && path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("tauri")
    );

    // Sem nada local: `cargo tauri` só se o subcomando existe de verdade.
    let (_t2, dir2) = fixture(
        "rustonly",
        &[
            ("src-tauri/tauri.conf.json", r#"{"identifier":"a.b.c"}"#),
            (
                "src-tauri/Cargo.toml",
                "[package]\nname = \"d\"\nversion = \"0.1.0\"\n",
            ),
        ],
    );
    let mut with_cargo_tauri = tools();
    with_cargo_tauri.cargo_tauri = ok("cargo-tauri", "cargo-tauri");
    let dev = find(&build(&dir2, &with_cargo_tauri, None), "tauri:dev").clone();
    assert!(
        dev.available
            && dev.program == "cargo"
            && dev.args == ["tauri".to_string(), "dev".to_string()]
    );

    // Nenhuma CLI: bloqueado, com o caminho para resolver — e nenhuma execução às cegas.
    let dev = find(&build(&dir2, &tools(), None), "tauri:dev").clone();
    assert!(!dev.available && dev.program_path.is_none());
    let reason = dev.unavailable_reason.unwrap();
    assert!(
        reason.contains("nenhuma CLI utilizável")
            && reason.contains("@tauri-apps/cli")
            && reason.contains("cargo install tauri-cli"),
        "{reason}"
    );
    assert!(!find(&build(&dir2, &tools(), None), "tauri:build").available);
}

#[test]
fn tauri_script_that_calls_the_local_cli_needs_installed_dependencies() {
    // package.json aponta para `tauri`, mas não há node_modules: explica em vez de falhar no log.
    let (_t, dir) = tauri_project(
        "package-lock.json",
        r#"{"dev":"vite","tauri":"tauri"}"#,
        &[],
    );
    let d = runtime::detect(&dir);
    let info = d.tauri.as_ref().unwrap();
    assert!(info.has_script && !info.local_cli);
    let dev = find(
        &actions::build_commands(&d, &tools(), &dir, None),
        "tauri:dev",
    )
    .clone();
    assert!(
        !dev.available
            && dev
                .unavailable_reason
                .as_deref()
                .unwrap()
                .contains("npm install")
    );
    // Um script próprio (não chama a CLI local) não exige node_modules.
    let (_t2, dir2) = tauri_project(
        "package-lock.json",
        r#"{"tauri":"node scripts/tauri.js"}"#,
        &[],
    );
    assert!(find(&build(&dir2, &tools(), None), "tauri:dev").available);
    // Instalando as dependências, o cache de detecção percebe e libera.
    write(&dir, "node_modules/.bin/tauri.cmd", "");
    write(&dir, "node_modules/.bin/tauri", "");
    assert!(find(&build(&dir, &tools(), None), "tauri:dev").available);
}

#[test]
fn tauri_multi_stack_is_one_system_and_tauri_dev_is_the_primary_action() {
    // O formato deste próprio repositório: workspace Cargo na raiz + src-tauri + frontend Vite.
    let (_t, dir) = tauri_project(
        "package-lock.json",
        r#"{"dev":"vite","build":"tsc && vite build","test":"vitest","tauri":"tauri"}"#,
        &[
            ("Cargo.toml", "[workspace]\nmembers = [\"src-tauri\"]\n"),
            ("node_modules/.bin/tauri", ""),
            ("node_modules/.bin/tauri.cmd", ""),
        ],
    );
    let d = runtime::detect(&dir);
    assert_eq!(d.composition.headline, "Tauri · Vite · Rust");
    let parts: Vec<_> = d
        .composition
        .parts
        .iter()
        .map(|p| (p.role, p.label.as_str()))
        .collect();
    assert_eq!(
        parts,
        vec![
            ("Shell", "Tauri v2"),
            ("Frontend", "Vite / npm"),
            ("Backend", "Rust / Cargo")
        ]
    );
    let tauri = d.tauri.as_ref().unwrap();
    assert_eq!(
        (
            tauri.version,
            tauri.version_evidence.as_deref(),
            tauri.dev_url_port
        ),
        (Some(2), Some("src-tauri/Cargo.toml › tauri"), Some(1420))
    );
    // O app do Tauri não roda com `cargo run`; as tarefas do cargo e os scripts continuam.
    let commands = actions::build_commands(&d, &tools(), &dir, None);
    assert!(commands.iter().all(|c| c.id != "cargo:run"));
    for id in [
        "cargo:test",
        "cargo:build",
        "node:dev",
        "node:test",
        "node:build",
        "tauri:dev",
        "tauri:build",
    ] {
        find(&commands, id);
    }
    // Mesmo existindo `npm run dev`, a principal é o fluxo oficial do Tauri.
    assert_eq!(
        actions::primary(&commands, &d).as_deref(),
        Some("tauri:dev")
    );
    // E continua sendo, mesmo bloqueada (CLI ausente): nunca troca silenciosamente por `npm run dev`.
    let (_t2, dir2) = tauri_project("package-lock.json", r#"{"dev":"vite"}"#, &[]);
    let d2 = runtime::detect(&dir2);
    let blocked = actions::build_commands(&d2, &tools(), &dir2, None);
    assert!(!find(&blocked, "tauri:dev").available);
    assert_eq!(
        actions::primary(&blocked, &d2).as_deref(),
        Some("tauri:dev")
    );
}

#[test]
fn tauri_version_and_dev_server_port_come_from_the_project_files() {
    // Cada caso: o que o projeto tem e o que a detecção precisa concluir.
    struct Case {
        conf: &'static str,
        cargo: Option<&'static str>,
        package: &'static str,
        version: Option<u8>,
        evidence: &'static str,
        port: Option<u16>,
    }
    let cases = [
        Case {
            conf: r#"{"tauri":{},"build":{"devPath":"http://localhost:3000"}}"#,
            cargo: None,
            package: "{}",
            version: Some(1),
            evidence: "tauri.conf.json › formato v1",
            port: Some(3000),
        },
        Case {
            conf: r#"{"$schema":"https://schema.tauri.app/config/2","build":{"devUrl":"http://127.0.0.1:5173"}}"#,
            cargo: None,
            package: "{}",
            version: Some(2),
            evidence: "tauri.conf.json › formato v2",
            port: Some(5173),
        },
        Case {
            conf: r#"{"identifier":"a.b.c","build":{"devUrl":"https://example.com:8443"}}"#,
            cargo: None,
            package: "{}",
            version: Some(2),
            evidence: "tauri.conf.json › formato v2",
            port: None,
        },
        Case {
            conf: "{}",
            cargo: None,
            package: r#"{"devDependencies":{"@tauri-apps/cli":"^2.1.0"}}"#,
            version: Some(2),
            evidence: "package.json › @tauri-apps/cli",
            port: None,
        },
        Case {
            conf: r#"{"tauri":{}}"#,
            cargo: Some(
                "[package]
name = \"a\"
[dependencies]
tauri = \"1.8\"
",
            ),
            package: "{}",
            version: Some(1),
            evidence: "src-tauri/Cargo.toml › tauri",
            port: None,
        },
    ];
    for (i, case) in cases.into_iter().enumerate() {
        let mut files: Vec<(&str, &str)> = vec![
            ("src-tauri/tauri.conf.json", case.conf),
            ("package.json", case.package),
        ];
        if let Some(cargo) = case.cargo {
            files.push(("src-tauri/Cargo.toml", cargo));
        }
        let (_t, dir) = fixture("v", &files);
        let tauri = runtime::detect(&dir).tauri.unwrap();
        assert_eq!(
            (
                tauri.version,
                tauri.version_evidence.as_deref(),
                tauri.dev_url_port
            ),
            (case.version, Some(case.evidence), case.port),
            "caso {i}"
        );
    }
}

// ------------------------------------------------------------------ Docker / Compose

const SAMPLE_CONFIG: &str = r#"{
  "name": "lkr-probe",
  "networks": {"default": {"name": "lkr-probe_default", "ipam": {}}},
  "services": {
    "api": {
      "environment": {"SECRET_TOKEN": "should-not-appear", "DB_PASSWORD": "hunter2"},
      "env_file": [{"path": "C:\\Users\\someone\\proj\\.env"}],
      "image": "node:24",
      "ports": [{"mode": "ingress", "host_ip": "127.0.0.1", "target": 8080, "published": "18080", "protocol": "tcp"},
                {"target": 9000, "published": 9001, "protocol": "udp"},
                {"target": 7000, "published": "8000-8010"}]
    },
    "db": {"profiles": ["data"], "image": "postgres:16-alpine", "environment": {"POSTGRES_PASSWORD": "top-secret"}},
    "bad name!": {"image": "x"},
    "-dash": {"image": "x"}
  }
}"#;

#[test]
fn dockerfile_alone_is_not_a_compose_project_and_has_no_container_actions() {
    let (_t, dir) = fixture("df", &[("Dockerfile", "FROM scratch")]);
    let d = runtime::detect(&dir);
    assert_eq!(d.docker.as_ref().unwrap().kind, "dockerfile");
    assert!(d.docker.as_ref().unwrap().compose_files.is_empty());
    assert!(build(&dir, &tools(), None)
        .iter()
        .all(|c| c.source != "compose"));
    assert!(
        d.composition.headline.contains("Dockerfile")
            && !d.composition.headline.contains("Compose")
    );

    let (_t2, dir2) = fixture(
        "cp",
        &[
            ("Dockerfile", ""),
            ("docker-compose.yml", ""),
            ("compose.yaml", ""),
            ("compose.override.yaml", ""),
        ],
    );
    let docker = runtime::detect(&dir2).docker.unwrap();
    assert_eq!(docker.kind, "compose");
    assert_eq!(
        docker.compose_files,
        vec!["compose.yaml", "docker-compose.yml"],
        "ordem de precedência do próprio Compose"
    );
    assert_eq!(docker.override_files, vec!["compose.override.yaml"]);
    assert!(docker.dockerfile);
}

#[test]
fn compose_config_parser_keeps_only_names_ports_and_profiles() {
    let config = compose::parse_config(SAMPLE_CONFIG).unwrap();
    assert_eq!(config.project_name.as_deref(), Some("lkr-probe"));
    let names: Vec<_> = config.services.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["api", "db"],
        "nomes inválidos não viram serviço"
    );
    let api = &config.services[0];
    assert_eq!(api.ports.len(), 3);
    assert_eq!(
        (
            api.ports[0].published,
            api.ports[0].target,
            api.ports[0].protocol.as_str()
        ),
        (Some(18080), 8080, "tcp")
    );
    assert_eq!(
        (api.ports[1].published, api.ports[1].protocol.as_str()),
        (Some(9001), "udp")
    );
    assert_eq!(
        api.ports[2].published, None,
        "faixa de portas não vira uma porta"
    );
    assert_eq!(config.services[1].profiles, vec!["data"]);
    // O `config` imprime environment/env_file em texto puro: NADA disso pode sobreviver ao parser.
    let everything = format!("{config:?} {}", serde_json::to_string(&config).unwrap());
    for secret in [
        "SECRET_TOKEN",
        "should-not-appear",
        "hunter2",
        "top-secret",
        "POSTGRES_PASSWORD",
        ".env",
        "someone",
    ] {
        assert!(!everything.contains(secret), "vazou {secret}");
    }
    assert!(compose::parse_config("não é json").is_err() && compose::parse_config("[]").is_err());
    assert!(compose::parse_config("{}").unwrap().services.is_empty());
}

#[test]
fn compose_ps_parser_reads_lines_arrays_and_rejects_garbage() {
    let lines = "{\"Service\":\"api\",\"State\":\"running\",\"Health\":\"Healthy\",\"ExitCode\":0,\"Name\":\"p-api-1\",\"Command\":\"node\"}\n{\"Service\":\"db\",\"State\":\"EXITED\",\"Health\":\"\",\"ExitCode\":137}\n";
    let containers = compose::parse_ps(lines).unwrap();
    assert_eq!(containers.len(), 2);
    assert_eq!(
        (
            containers[0].state.as_str(),
            containers[0].health.as_deref()
        ),
        ("running", Some("healthy"))
    );
    assert_eq!(
        (
            containers[1].state.as_str(),
            containers[1].exit_code,
            containers[1].health.clone()
        ),
        ("exited", Some(137), None)
    );
    assert_eq!(
        compose::parse_ps("[{\"Service\":\"api\",\"State\":\"running\"}]")
            .unwrap()
            .len(),
        1
    );
    assert!(compose::parse_ps("  \n").unwrap().is_empty());
    assert!(compose::parse_ps("Error: algo deu errado").is_err());
    let mixed = compose::parse_ps("{\"Service\":\"bad name\",\"State\":\"running\"}\n{\"Service\":\"ok\",\"State\":\"running\"}").unwrap();
    assert_eq!(
        mixed.iter().map(|c| c.service.as_str()).collect::<Vec<_>>(),
        vec!["ok"]
    );
}

fn compose_dir() -> (tempfile::TempDir, PathBuf) {
    fixture("stack", &[("compose.yaml", "services: {}")])
}

#[test]
fn compose_commands_separate_detected_from_available_and_explain_why() {
    let (_t, dir) = compose_dir();
    let d = runtime::detect(&dir);
    let view = ComposeView {
        services: vec!["api".into(), "db".into()],
        containers: Some(2),
        config_error: None,
    };
    let all = actions::build_commands(&d, &tools(), &dir, Some(&view));
    let up = find(&all, "compose:up");
    assert_eq!(
        (up.program.as_str(), up.args.clone()),
        ("docker", ["compose", "up", "-d"].map(String::from).to_vec())
    );
    assert_eq!(
        (up.kind, up.group, up.exclusive),
        (ScriptKind::Task, ActionGroup::Run, Some("compose"))
    );
    assert_eq!(
        find(&all, "compose:down").args,
        ["compose", "down"].map(String::from)
    );
    assert_eq!(
        find(&all, "compose:restart").args,
        ["compose", "restart"].map(String::from)
    );
    // Parar containers NUNCA apaga volumes, imagens nem órfãos.
    assert!(find(&all, "compose:down").args.iter().all(|a| ![
        "-v",
        "--volumes",
        "--rmi",
        "--remove-orphans"
    ]
    .contains(&a.as_str())));
    let logs = find(&all, "compose:logs");
    assert!(logs.observer && logs.kind == ScriptKind::Service && logs.exclusive.is_none());
    assert_eq!(
        logs.args,
        ["compose", "logs", "--follow", "--tail", "200"].map(String::from)
    );
    assert!(all
        .iter()
        .filter(|c| c.source == "compose")
        .all(|c| c.available));
    assert_eq!(actions::primary(&all, &d).as_deref(), Some("compose:up"));

    let blocked = |t: &Tools, view: Option<&ComposeView>| {
        actions::build_commands(&d, t, &dir, view)
            .into_iter()
            .filter(|c| c.source == "compose")
            .collect::<Vec<_>>()
    };
    // Docker CLI ausente: o projeto continua reconhecido, as ações ficam desabilitadas com motivo.
    let mut no_docker = tools();
    no_docker.docker = ToolStatus::missing(
        "docker",
        "Docker CLI",
        "Docker CLI não encontrado no PATH. Instale o Docker Desktop e reinicie o aplicativo.",
    );
    let list = blocked(&no_docker, None);
    assert_eq!(list.len(), 4);
    assert!(list.iter().all(|c| !c.available
        && c.unavailable_reason
            .as_deref()
            .unwrap()
            .contains("Docker CLI não encontrado")));
    // Docker instalado, Docker Desktop fechado.
    let mut no_daemon = tools();
    no_daemon.daemon = ToolStatus::missing("daemon", "Docker Desktop", "O Docker Desktop não está disponível (o daemon não respondeu). Abra o Docker Desktop e tente de novo.");
    assert!(blocked(&no_daemon, None).iter().all(|c| !c.available
        && c.unavailable_reason
            .as_deref()
            .unwrap()
            .contains("Docker Desktop")));
    // Plugin Compose ausente.
    let mut no_plugin = tools();
    no_plugin.compose = ToolStatus::missing(
        "compose",
        "Docker Compose",
        "O plugin Docker Compose (v2) não está disponível.",
    );
    assert!(blocked(&no_plugin, None).iter().all(|c| !c.available));
    // Arquivo inválido: tudo bloqueado com o motivo do próprio Compose.
    let invalid = ComposeView {
        config_error: Some("Arquivo Compose inválido: services.api must be a mapping".into()),
        ..ComposeView::default()
    };
    assert!(blocked(&tools(), Some(&invalid))
        .iter()
        .all(|c| !c.available
            && c.unavailable_reason
                .as_deref()
                .unwrap()
                .starts_with("Arquivo Compose inválido")));
    // Sem containers: subir continua possível; parar, reiniciar e logs não fazem sentido.
    let empty = ComposeView {
        containers: Some(0),
        ..ComposeView::default()
    };
    let list = blocked(&tools(), Some(&empty));
    let avail = |id: &str| list.iter().find(|c| c.id == id).unwrap().available;
    assert!(
        avail("compose:up")
            && !avail("compose:down")
            && !avail("compose:restart")
            && !avail("compose:logs")
    );
    // Estado desconhecido (daemon sem resposta ao ps): não bloqueia por suposição.
    let unknown = ComposeView {
        containers: None,
        ..ComposeView::default()
    };
    assert!(blocked(&tools(), Some(&unknown))
        .iter()
        .all(|c| c.available));
}

#[test]
fn compose_logs_choices_are_only_valid_service_names() {
    let (_t, dir) = compose_dir();
    let view = ComposeView {
        services: vec![
            "api".into(),
            "db".into(),
            "bad name".into(),
            "-x".into(),
            "a;b".into(),
        ],
        containers: Some(1),
        config_error: None,
    };
    let commands = build(&dir, &tools(), Some(&view));
    let logs = find(&commands, "compose:logs");
    assert!(!logs.selection_required);
    assert_eq!(
        logs.choices
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["api", "db"]
    );
    assert_eq!(logs.choices[0].args, vec!["api".to_string()]);
}

#[test]
fn primary_action_exists_only_when_unambiguous() {
    let node = |scripts: &str, extra: &[(&str, &str)]| {
        let package = format!(r#"{{"scripts":{scripts}}}"#);
        let mut files: Vec<(&str, &str)> = vec![
            ("package.json", package.as_str()),
            ("package-lock.json", "{}"),
        ];
        files.extend_from_slice(extra);
        let (t, dir) = fixture("p", &files);
        let d = runtime::detect(&dir);
        let c = actions::build_commands(&d, &tools(), &dir, None);
        let p = actions::primary(&c, &d);
        drop(t);
        p
    };
    let rust_main = [("Cargo.toml", PACKAGE), ("src/main.rs", "fn main() {}")];
    assert_eq!(
        node(r#"{"test":"x","dev":"vite","build":"x"}"#, &[]).as_deref(),
        Some("node:dev")
    );
    assert_eq!(
        node(r#"{"test":"x","build":"x"}"#, &[]),
        None,
        "só tarefas: nada de principal"
    );
    assert_eq!(
        node(r#"{"dev":"vite"}"#, &rust_main),
        None,
        "Node + Rust: o usuário escolhe"
    );
    assert_eq!(
        node(r#"{"build":"x"}"#, &rust_main).as_deref(),
        Some("cargo:run")
    );
    assert_eq!(
        node(r#"{"dev":"vite"}"#, &[("compose.yaml", "")]),
        None,
        "Node + Compose: o usuário escolhe"
    );
}

// ------------------------------------------------------------------ segurança

#[test]
fn ids_and_selections_are_lookup_keys_never_commands() {
    let (_t, dir) = fixture(
        "sec",
        &[
            ("package.json", r#"{"scripts":{"dev":"vite"}}"#),
            ("package-lock.json", "{}"),
        ],
    );
    let mut p = project("sec", &dir);
    // Metadado portátil (workspace.json) é dado: nunca vira ação, nem com nome que casa.
    p.commands = vec![hub_core::models::ProjectCommand {
        name: "evil".into(),
        program: "powershell".into(),
        args: vec!["-c".into(), "calc".into()],
    }];
    for bad in [
        "",
        "evil",
        "x",
        "node:",
        ":dev",
        "NODE:dev",
        "node:dev; calc",
        "node:dev && calc",
        "node:../dev",
        "node:dev\n",
        "node:dev\0",
        "node: dev",
        "cargo:run --release",
        "cargo:publish",
        "tauri:dev\r\ncalc",
        "compose:up && calc",
        "compose:down",
        "powershell:-c",
        "node:evil",
        "docker:run",
    ] {
        assert!(
            actions::resolve(&p, bad, None).is_err(),
            "id {bad:?} não pode resolver"
        );
    }
    for bad in ["dev; calc", "../x", "dev\n", &"a".repeat(200)] {
        assert!(
            actions::resolve(&p, "node:dev", Some(bad)).is_err(),
            "seleção {bad:?}"
        );
    }
    // Pasta não localizada nunca executa nada.
    let mut unbound = p.clone();
    unbound.local_path = String::new();
    assert!(actions::resolve(&unbound, "node:dev", None)
        .unwrap_err()
        .contains("Localizar"));
    // Selecionar algo que a ação não oferece também é recusado.
    if have_npm() {
        assert!(actions::resolve(&p, "node:dev", Some("outra")).is_err());
        assert!(actions::resolve(&p, "node:dev", None).is_ok());
    }
}
fn have_npm() -> bool {
    common::have("npm")
}

#[test]
fn cargo_selection_is_validated_against_the_detected_binaries() {
    if !common::have("cargo") {
        return;
    }
    let manifest = format!("{PACKAGE}[[bin]]\nname = \"alpha\"\npath = \"src/a.rs\"\n[[bin]]\nname = \"beta\"\npath = \"src/b.rs\"\n");
    let (_t, dir) = fixture(
        "sel",
        &[
            ("Cargo.toml", &manifest),
            ("src/a.rs", "fn main() {}"),
            ("src/b.rs", "fn main() {}"),
        ],
    );
    let p = project("sel", &dir);
    let missing = actions::resolve(&p, "cargo:run", None).unwrap_err();
    assert!(missing.contains("mais de uma opção"), "{missing}");
    for bad in [
        "alpha --release",
        "alpha;calc",
        "../alpha",
        "alpha\n",
        "ALPHA",
        "gamma",
        "--bin",
        "-p",
        "beta/../alpha",
        "alpha beta",
    ] {
        assert!(
            actions::resolve(&p, "cargo:run", Some(bad)).is_err(),
            "seleção {bad:?}"
        );
    }
    let spec = actions::resolve(&p, "cargo:run", Some("beta")).unwrap();
    assert_eq!(spec.args, ["run", "--bin", "beta"].map(String::from));
    assert_eq!(
        (
            spec.command_id.as_str(),
            spec.selection.as_deref(),
            spec.source
        ),
        ("cargo:run", Some("beta"), "cargo")
    );
    assert!(
        spec.program.is_absolute()
            && spec.cwd.is_absolute()
            && !spec.cwd.to_string_lossy().starts_with(r"\\?\")
    );
    assert_eq!(spec.display, "cargo run --bin beta");
}

#[test]
fn snapshot_and_ui_payload_carry_no_script_bodies_env_values_or_arguments() {
    let (_t, dir) = fixture(
        "ctx",
        &[
            ("package.json", r#"{"scripts":{"dev":"vite --token=abc123secret","build":"x"},"devDependencies":{"@tauri-apps/cli":"2"}}"#),
            ("package-lock.json", "{}"),
            ("src-tauri/tauri.conf.json", r#"{"identifier":"a.b.c"}"#),
            ("src-tauri/Cargo.toml", "[package]\nname = \"d\"\nversion = \"0.1.0\"\n"),
            ("compose.yaml", "services:\n  api:\n    image: node\n    environment:\n      SECRET_TOKEN: leak-me\n    env_file: .env\n"),
            (".env", "DB_PASSWORD=hunter2\n"),
        ],
    );
    let p = project("ctx", &dir);
    let rt = runtime::inspect(
        &p,
        std::slice::from_ref(&p),
        vec![],
        &std::collections::HashMap::new(),
    );
    let snapshot = hub_core::snapshot::generate(&p, &rt).unwrap();
    let json = serde_json::to_string(&rt).unwrap();
    // Contexto de IA: nem o corpo do script, nem valores/nomes de env do Compose ou do .env.
    for secret in [
        "abc123secret",
        "leak-me",
        "hunter2",
        "SECRET_TOKEN",
        "DB_PASSWORD",
    ] {
        assert!(!snapshot.contains(secret), "snapshot vazou {secret}");
    }
    // Payload da interface: o texto do script é exibição (já era); env do Compose nunca chega lá.
    for secret in ["leak-me", "hunter2", "SECRET_TOKEN", "DB_PASSWORD"] {
        assert!(!json.contains(secret), "payload vazou {secret}");
    }
    for section in [
        "Composição:",
        "Ferramentas:",
        "Ações disponíveis:",
        "Ações indisponíveis:",
        "Compose:",
        "Última tarefa:",
    ] {
        assert!(snapshot.contains(section), "falta {section}");
    }
    assert!(snapshot.contains("Tauri") && snapshot.contains("[node:dev]"));
}
