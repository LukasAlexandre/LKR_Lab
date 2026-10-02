//! Machine Registry (SESSION-001): identidade, política de 6h, gate e migration 005.
use hub_core::{
    database::Database,
    machine::{
        command_allowed, is_stale, validate, Detector, GpuInfo, MachineInput, MachineSnapshot,
        NetworkInterface, Registry, SystemDetector, SNAPSHOT_TTL_MS,
    },
};
use rusqlite::Connection;
use std::{
    cell::{Cell, RefCell},
    path::Path,
    sync::Mutex,
};

const HOUR: i64 = 60 * 60 * 1000;
const T0: i64 = 1_790_000_000_000;

/// Detector falso: devolve o snapshot configurado e conta as detecções.
struct Fake {
    snapshot: RefCell<MachineSnapshot>,
    calls: Cell<u32>,
}
impl Fake {
    fn new(snapshot: MachineSnapshot) -> Self {
        Self {
            snapshot: RefCell::new(snapshot),
            calls: Cell::new(0),
        }
    }
    fn set(&self, change: impl FnOnce(&mut MachineSnapshot)) {
        change(&mut self.snapshot.borrow_mut());
    }
}
impl Detector for Fake {
    fn detect(&self, now: i64) -> MachineSnapshot {
        self.calls.set(self.calls.get() + 1);
        MachineSnapshot {
            detected_at: now,
            ..self.snapshot.borrow().clone()
        }
    }
}

fn workstation() -> MachineSnapshot {
    MachineSnapshot {
        hostname: Some("DESKTOP-LKR".into()),
        os_name: Some("Windows 11 Pro".into()),
        os_version: Some("24H2".into()),
        os_build: Some("26200.6584".into()),
        cpu_model: Some("AMD Ryzen 7 5800X".into()),
        cpu_cores: Some(8),
        cpu_threads: Some(16),
        memory_total: Some(32 * 1024 * 1024 * 1024),
        gpus: vec![GpuInfo {
            name: "NVIDIA GeForce RTX 3060".into(),
            memory: Some(12 * 1024 * 1024 * 1024),
        }],
        network_interfaces: vec![NetworkInterface {
            name: "Ethernet".into(),
            ipv4: vec!["192.168.1.24".into()],
        }],
        active_interface: Some("Ethernet".into()),
        local_ipv4: Some("192.168.1.24".into()),
        uptime: Some(3600),
        ..Default::default()
    }
}

fn input(name: &str) -> MachineInput {
    MachineInput {
        name: name.into(),
        usage: "home".into(),
        description: String::new(),
    }
}

fn open(path: &Path) -> (Mutex<Database>, Registry) {
    let db = Database::open(path).unwrap();
    let registry = Registry::load(&db).unwrap();
    (Mutex::new(db), registry)
}

fn machine_rows(db: &Mutex<Database>) -> i64 {
    db.lock()
        .unwrap()
        .conn
        .query_row("SELECT count(*) FROM machine", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn unregistered_machine_blocks_everything_but_setup() {
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    assert!(!registry.is_registered());
    for command in ["machine_status", "machine_refresh", "machine_register"] {
        assert!(registry.allows(command), "{command} libera o cadastro");
    }
    for command in [
        "list_projects",
        "workspace_state",
        "sync_run",
        "runtime_start",
        "kill_process",
        "list_worktrees",
        "",
    ] {
        assert!(
            !registry.allows(command),
            "{command} deveria estar bloqueado"
        );
    }
    let status = registry.status(&db, T0).unwrap();
    assert!(!status.registered);
    assert!(status.machine.is_none());
    assert!(status.stale, "sem detecção ainda");
}

#[test]
fn registered_machine_unlocks_the_environment() {
    assert!(command_allowed("list_projects", true));
    assert!(!command_allowed("list_projects", false));
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    let fake = Fake::new(workstation());
    let status = registry.register(&db, &fake, input("PC Casa"), T0).unwrap();
    assert!(status.registered);
    assert!(registry.allows("list_projects"));
    assert!(registry.allows("sync_run"));
    let machine = status.machine.unwrap();
    assert_eq!(machine.name, "PC Casa");
    assert_eq!(machine.usage, "home");
    assert_eq!(machine.last_detected_at, Some(T0));
    assert_eq!(
        status.snapshot.unwrap().hostname.as_deref(),
        Some("DESKTOP-LKR")
    );
}

#[test]
fn machine_id_is_generated_once_and_persisted() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    let fake = Fake::new(workstation());
    let first = {
        let (db, registry) = open(&path);
        let id = registry
            .register(&db, &fake, input("PC Casa"), T0)
            .unwrap()
            .machine
            .unwrap()
            .machine_id;
        assert_eq!(uuid_version(&id), Some(4));
        // Um segundo cadastro na mesma instalação é recusado.
        assert!(registry.register(&db, &fake, input("Outro"), T0).is_err());
        id
    };
    // Reabrir o app: reconhece a máquina pelo machine_id local e não pede cadastro.
    let (db, registry) = open(&path);
    assert!(registry.is_registered());
    let status = registry.status(&db, T0 + HOUR).unwrap();
    let machine = status.machine.unwrap();
    assert_eq!(machine.machine_id, first);
    assert_eq!(machine.name, "PC Casa");
    assert_eq!(machine_rows(&db), 1);
    // Outra instalação (outro banco) gera outra identidade.
    let other = tmp.path().join("other.db");
    let (db2, registry2) = open(&other);
    let second = registry2
        .register(&db2, &fake, input("Notebook"), T0)
        .unwrap()
        .machine
        .unwrap()
        .machine_id;
    assert_ne!(first, second);
}

fn uuid_version(id: &str) -> Option<u32> {
    let parts: Vec<&str> = id.split('-').collect();
    (parts.len() == 5 && id.len() == 36)
        .then(|| parts[2].chars().next()?.to_digit(16))
        .flatten()
}

#[test]
fn snapshot_validity_is_six_hours() {
    assert_eq!(SNAPSHOT_TTL_MS, 6 * HOUR);
    assert!(is_stale(None, T0));
    assert!(!is_stale(Some(T0), T0));
    assert!(!is_stale(Some(T0), T0 + 6 * HOUR - 1));
    assert!(is_stale(Some(T0), T0 + 6 * HOUR));
    assert!(is_stale(Some(T0), T0 + 30 * HOUR));
    // Relógio que voltou no tempo não congela um snapshot antigo.
    assert!(is_stale(Some(T0), T0 - 1));
}

#[test]
fn fresh_snapshot_is_not_detected_again_and_expired_one_is() {
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    let fake = Fake::new(workstation());
    registry.register(&db, &fake, input("PC Casa"), T0).unwrap();
    assert_eq!(fake.calls.get(), 1);

    // < 6h: nenhuma coleta.
    let status = registry.refresh(&db, &fake, T0 + 5 * HOUR, false).unwrap();
    assert_eq!(fake.calls.get(), 1);
    assert!(!status.stale);
    assert_eq!(status.machine.unwrap().last_detected_at, Some(T0));

    // >= 6h (ex.: volta de suspensão): coleta de novo e persiste.
    let status = registry.refresh(&db, &fake, T0 + 6 * HOUR, false).unwrap();
    assert_eq!(fake.calls.get(), 2);
    assert!(!status.stale);
    assert_eq!(
        status.machine.unwrap().last_detected_at,
        Some(T0 + 6 * HOUR)
    );
}

#[test]
fn manual_refresh_always_detects() {
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    let fake = Fake::new(workstation());
    registry.register(&db, &fake, input("PC Casa"), T0).unwrap();
    fake.set(|s| s.uptime = Some(7200));
    let status = registry.refresh(&db, &fake, T0 + 60_000, true).unwrap();
    assert_eq!(fake.calls.get(), 2);
    assert_eq!(status.snapshot.unwrap().uptime, Some(7200));
    assert_eq!(status.machine.unwrap().last_detected_at, Some(T0 + 60_000));
}

#[test]
fn ip_change_keeps_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    let fake = Fake::new(workstation());
    let id = registry
        .register(&db, &fake, input("PC Casa"), T0)
        .unwrap()
        .machine
        .unwrap()
        .machine_id;
    fake.set(|s| {
        s.local_ipv4 = Some("10.0.0.5".into());
        s.active_interface = Some("Wi-Fi".into());
        s.network_interfaces = vec![NetworkInterface {
            name: "Wi-Fi".into(),
            ipv4: vec!["10.0.0.5".into()],
        }];
        s.hostname = Some("NOVO-HOSTNAME".into());
    });
    let status = registry.refresh(&db, &fake, T0 + 7 * HOUR, false).unwrap();
    let machine = status.machine.unwrap();
    let snapshot = status.snapshot.unwrap();
    assert_eq!(machine.machine_id, id);
    assert_eq!(machine.name, "PC Casa");
    assert_eq!(snapshot.local_ipv4.as_deref(), Some("10.0.0.5"));
    assert_eq!(snapshot.active_interface.as_deref(), Some("Wi-Fi"));
    assert_eq!(snapshot.hostname.as_deref(), Some("NOVO-HOSTNAME"));
    assert_eq!(machine_rows(&db), 1);
}

#[test]
fn hardware_change_keeps_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    let fake = Fake::new(workstation());
    let id = registry
        .register(&db, &fake, input("PC Casa"), T0)
        .unwrap()
        .machine
        .unwrap()
        .machine_id;
    fake.set(|s| {
        s.memory_total = Some(64 * 1024 * 1024 * 1024);
        s.gpus = vec![GpuInfo {
            name: "NVIDIA GeForce RTX 4070".into(),
            memory: Some(12 * 1024 * 1024 * 1024),
        }];
        s.cpu_model = Some("AMD Ryzen 9 7950X".into());
    });
    let status = registry.refresh(&db, &fake, T0, true).unwrap();
    let snapshot = status.snapshot.unwrap();
    assert_eq!(status.machine.unwrap().machine_id, id);
    assert_eq!(snapshot.memory_total, Some(64 * 1024 * 1024 * 1024));
    assert_eq!(snapshot.gpus[0].name, "NVIDIA GeForce RTX 4070");
    assert_eq!(machine_rows(&db), 1);
}

#[test]
fn unavailable_optional_data_does_not_block_registration() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    let fake = Fake::new(MachineSnapshot::default());
    {
        let (db, registry) = open(&path);
        let status = registry.refresh(&db, &fake, T0, false).unwrap();
        let snapshot = status.snapshot.unwrap();
        assert!(snapshot.gpus.is_empty());
        assert!(snapshot.hostname.is_none());
        assert!(snapshot.local_ipv4.is_none());
        registry
            .register(&db, &fake, input("Notebook"), T0)
            .unwrap();
    }
    let (db, registry) = open(&path);
    let snapshot = registry.status(&db, T0).unwrap().snapshot.unwrap();
    assert_eq!(snapshot.gpus, vec![]);
    assert_eq!(snapshot.memory_total, None);
}

#[test]
fn pending_detection_is_reused_on_registration_and_never_persisted_before() {
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    let fake = Fake::new(workstation());
    // Antes do cadastro: detecção só em memória.
    registry.refresh(&db, &fake, T0, false).unwrap();
    registry.refresh(&db, &fake, T0 + HOUR, false).unwrap();
    assert_eq!(fake.calls.get(), 1, "detecção pendente ainda válida");
    assert_eq!(machine_rows(&db), 0);
    // "Atualizar detecção" antes do cadastro.
    fake.set(|s| s.hostname = Some("REVISADO".into()));
    registry.refresh(&db, &fake, T0 + HOUR, true).unwrap();
    assert_eq!(fake.calls.get(), 2);
    // O cadastro persiste o snapshot revisado, sem detectar de novo.
    let status = registry
        .register(&db, &fake, input("PC Casa"), T0 + 2 * HOUR)
        .unwrap();
    assert_eq!(fake.calls.get(), 2);
    assert_eq!(
        status.snapshot.unwrap().hostname.as_deref(),
        Some("REVISADO")
    );
    assert_eq!(status.machine.unwrap().last_detected_at, Some(T0 + HOUR));
}

#[test]
fn registration_input_is_validated() {
    let ok = validate(MachineInput {
        name: "  PC Casa  ".into(),
        usage: "work".into(),
        description: "  Principal  ".into(),
    })
    .unwrap();
    assert_eq!(ok.name, "PC Casa");
    assert_eq!(ok.description, "Principal");
    for bad in [
        input(""),
        input("   "),
        input(&"x".repeat(61)),
        input("PC\nCasa"),
        MachineInput {
            usage: "garagem".into(),
            ..input("PC")
        },
        MachineInput {
            description: "d".repeat(121),
            ..input("PC")
        },
    ] {
        assert!(validate(bad.clone()).is_err(), "{bad:?}");
    }
    // Nada é gravado com entrada inválida.
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    let fake = Fake::new(workstation());
    assert!(registry.register(&db, &fake, input(" "), T0).is_err());
    assert!(!registry.is_registered());
    assert_eq!(machine_rows(&db), 0);
}

#[test]
fn machine_state_never_enters_the_portable_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    let fake = Fake::new(workstation());
    registry.register(&db, &fake, input("PC Casa"), T0).unwrap();
    let ws = db.lock().unwrap().export_portable().unwrap();
    let text = serde_json::to_string(&ws).unwrap();
    for leaked in ["PC Casa", "DESKTOP-LKR", "192.168.1.24", "Ryzen", "machine"] {
        assert!(!text.contains(leaked), "{leaked} vazou para o workspace");
    }
}

#[test]
fn real_detection_is_passive_and_tolerates_missing_data() {
    let snapshot = SystemDetector.detect(T0);
    assert_eq!(snapshot.detected_at, T0);
    assert!(snapshot.cpu_threads.unwrap_or(1) >= 1);
    if let (Some(cores), Some(threads)) = (snapshot.cpu_cores, snapshot.cpu_threads) {
        assert!(cores <= threads);
    }
    // IPv4 da rota, quando existe, pertence à interface ativa.
    if let (Some(ip), Some(active)) = (&snapshot.local_ipv4, &snapshot.active_interface) {
        let interface = snapshot
            .network_interfaces
            .iter()
            .find(|i| &i.name == active)
            .unwrap();
        assert!(interface.ipv4.contains(ip));
    }
    // Nada além dos campos operacionais é serializado.
    let json = serde_json::to_value(&snapshot).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for key in &keys {
        assert!(
            [
                "hostname",
                "osName",
                "osVersion",
                "osBuild",
                "cpuModel",
                "cpuCores",
                "cpuThreads",
                "memoryTotal",
                "gpus",
                "storage",
                "networkInterfaces",
                "activeInterface",
                "localIpv4",
                "uptime",
                "detectedAt"
            ]
            .contains(key),
            "campo inesperado: {key}"
        );
    }
    println!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
}

// ---- migration 005 ----

/// Banco v4 (antes do Machine Registry) com dados em todas as tabelas.
fn legacy_v4(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for file in [
        "001_initial.sql",
        "002_knowledge.sql",
        "003_project_bindings.sql",
        "004_sync_state.sql",
    ] {
        conn.execute_batch(&std::fs::read_to_string(dir.join(file)).unwrap())
            .unwrap();
    }
    let data = serde_json::json!({
        "id": "p1", "name": "Legado", "slug": "legado", "description": "antes da 005",
        "repository": "https://github.com/org/legado", "stack": ["Rust"], "tags": ["a"],
        "ports": [{"name": "web", "port": 3000}], "commands": [],
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z"
    });
    conn.execute(
        "INSERT INTO projects(id,data,updated_at) VALUES('p1',?1,'2026-01-02T00:00:00Z')",
        [data.to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO project_bindings(project_id,local_path) VALUES('p1','C:/legado')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO prompt_templates VALUES('pt1','Meu','Dev','p1','corpo')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO knowledge(id,project_id,title,kind,body,tags) VALUES('k1','p1','T','note','b','x')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO activities(project_id,action) VALUES('p1','legado')",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE sync_state SET base_hash='abc',last_applied_hash='def',last_synced_at='2026-01-03T00:00:00Z' WHERE id=1",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO portable_preferences(id,data) VALUES(1,'{\"sidebarCompact\":true}')",
        [],
    )
    .unwrap();
}

#[test]
fn migration_005_preserves_legacy_data_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy.db");
    legacy_v4(&path);
    for _ in 0..2 {
        let db = Database::open(&path).unwrap();
        let version: i64 = db
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 5);
        let project = db.project("p1").unwrap();
        assert_eq!(project.name, "Legado");
        assert_eq!(project.local_path, "C:/legado");
        assert_eq!(project.ports[0].port, 3000);
        assert_eq!(
            db.prompts()
                .unwrap()
                .iter()
                .filter(|p| p.id == "pt1")
                .count(),
            1
        );
        assert_eq!(db.knowledge().unwrap().len(), 1);
        assert!(db
            .activities()
            .unwrap()
            .iter()
            .any(|a| a.action == "legado"));
        let sync = db.sync_meta().unwrap();
        assert_eq!(sync.base_hash.as_deref(), Some("abc"));
        assert_eq!(sync.last_applied_hash.as_deref(), Some("def"));
        assert!(db.preferences().unwrap().sidebar_compact);
        // Banco legado: nenhuma máquina, então o ambiente começa bloqueado.
        assert!(db.machine().unwrap().is_none());
        assert!(!Registry::load(&db).unwrap().is_registered());
    }
}

#[test]
fn databases_newer_than_005_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("future.db");
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 6)
        .unwrap();
    assert!(Database::open(&path).is_err());
}

// ---- edição da metadata (nome / uso / descrição) ----

fn row(db: &Mutex<Database>) -> (String, String, Option<String>, Option<i64>, String) {
    db.lock()
        .unwrap()
        .conn
        .query_row(
            "SELECT machine_id,created_at,snapshot,last_detected_at,updated_at FROM machine WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
}

/// Cadastrada em T0 e com updated_at antigo, para a edição ter o que mudar.
fn registered(path: &Path, fake: &Fake) -> (Mutex<Database>, Registry) {
    let (db, registry) = open(path);
    registry.register(&db, fake, input("PC Casa"), T0).unwrap();
    db.lock()
        .unwrap()
        .conn
        .execute(
            "UPDATE machine SET updated_at='2026-01-01T00:00:00.000Z'",
            [],
        )
        .unwrap();
    (db, registry)
}

#[test]
fn metadata_update_changes_only_name_usage_description_and_updated_at() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Fake::new(workstation());
    let (db, registry) = registered(&tmp.path().join("hub.db"), &fake);
    let (id, created, snapshot, detected, updated) = row(&db);

    let status = registry
        .update(
            &db,
            MachineInput {
                name: "  PC Casa Teste  ".into(),
                usage: "work".into(),
                description: "  Notebook de testes  ".into(),
            },
            T0 + HOUR,
        )
        .unwrap();
    let machine = status.machine.unwrap();
    assert_eq!(machine.name, "PC Casa Teste", "nome com trim");
    assert_eq!(machine.usage, "work");
    assert_eq!(machine.description, "Notebook de testes");

    let (id2, created2, snapshot2, detected2, updated2) = row(&db);
    assert_eq!(id2, id, "machine_id preservado");
    assert_eq!(created2, created, "created_at preservado");
    assert_eq!(snapshot2, snapshot, "snapshot preservado");
    assert_eq!(detected2, detected, "last_detected_at preservado");
    assert_ne!(updated2, updated, "updated_at alterado");
    assert_eq!(machine_rows(&db), 1);
    // Salvar metadata não é detectar.
    assert_eq!(fake.calls.get(), 1);
}

#[test]
fn metadata_update_reuses_registration_rules_and_never_writes_invalid_input() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Fake::new(workstation());
    let (db, registry) = registered(&tmp.path().join("hub.db"), &fake);
    let before = row(&db);
    for bad in [
        input(""),
        input("   "),
        MachineInput {
            usage: "garagem".into(),
            ..input("PC")
        },
        MachineInput {
            description: "d".repeat(121),
            ..input("PC")
        },
    ] {
        assert!(registry.update(&db, bad.clone(), T0).is_err(), "{bad:?}");
    }
    assert_eq!(row(&db), before, "nada gravado");
    let machine = registry.status(&db, T0).unwrap().machine.unwrap();
    assert_eq!(machine.name, "PC Casa");
    // Descrição no limite é aceita; vazia também (é opcional).
    let ok = MachineInput {
        description: "d".repeat(120),
        ..input("PC Casa")
    };
    assert!(registry.update(&db, ok, T0).is_ok());
    assert!(registry.update(&db, input("PC Casa"), T0).is_ok());
}

#[test]
fn metadata_update_requires_a_registered_machine_and_is_gated() {
    let tmp = tempfile::tempdir().unwrap();
    let (db, registry) = open(&tmp.path().join("hub.db"));
    assert!(!registry.allows("machine_update"));
    assert!(registry.update(&db, input("PC Casa"), T0).is_err());
    assert_eq!(machine_rows(&db), 0, "edição não cria máquina");
}

#[test]
fn technical_refresh_after_edit_keeps_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Fake::new(workstation());
    let (db, registry) = registered(&tmp.path().join("hub.db"), &fake);
    registry
        .update(
            &db,
            MachineInput {
                name: "PC Casa Teste".into(),
                usage: "other".into(),
                description: "editado".into(),
            },
            T0,
        )
        .unwrap();
    let updated = row(&db).4;
    fake.set(|s| {
        s.local_ipv4 = Some("10.0.0.9".into());
        s.hostname = Some("OUTRO".into());
    });
    let status = registry.refresh(&db, &fake, T0 + 2 * HOUR, true).unwrap();
    let machine = status.machine.unwrap();
    assert_eq!(machine.name, "PC Casa Teste");
    assert_eq!(machine.usage, "other");
    assert_eq!(machine.description, "editado");
    assert_eq!(machine.last_detected_at, Some(T0 + 2 * HOUR));
    assert_eq!(
        status.snapshot.unwrap().local_ipv4.as_deref(),
        Some("10.0.0.9")
    );
    assert_eq!(row(&db).4, updated, "refresh técnico não toca updated_at");
}

#[test]
fn metadata_survives_reopening_the_app() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.db");
    let fake = Fake::new(workstation());
    let id = {
        let (db, registry) = registered(&path, &fake);
        registry
            .update(&db, input("PC Casa Teste"), T0)
            .unwrap()
            .machine
            .unwrap()
            .machine_id
    };
    let (db, registry) = open(&path);
    let machine = registry.status(&db, T0).unwrap().machine.unwrap();
    assert_eq!(machine.name, "PC Casa Teste");
    assert_eq!(machine.machine_id, id);
}
