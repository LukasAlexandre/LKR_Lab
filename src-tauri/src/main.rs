#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use hub_core::{database::Database, models::*, HubResult};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};
struct AppState(Mutex<Database>);
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceState {
    system: hub_core::system::SystemState,
    ports: Vec<hub_core::ports::PortInfo>,
    processes: Vec<hub_core::system::ProcessInfo>,
    port_error: Option<String>,
}
fn db<'a>(state: &'a State<'_, AppState>) -> HubResult<std::sync::MutexGuard<'a, Database>> {
    state
        .0
        .lock()
        .map_err(|_| "Banco temporariamente indisponível".into())
}
/// Machine Registry: o gate global e a detecção pendente antes do cadastro.
struct MachineState(hub_core::machine::Registry);
#[tauri::command]
fn machine_status(app: tauri::AppHandle) -> HubResult<hub_core::machine::MachineStatus> {
    let state = app.state::<AppState>();
    app.state::<MachineState>()
        .0
        .status(&state.0, hub_core::machine::now_ms())
}
/// Ao abrir, ao voltar à janela e em "Atualizar detecção" (`force`). Só detecta de novo
/// se o snapshot tiver 6h ou mais, ou se `force`.
#[tauri::command]
async fn machine_refresh(
    app: tauri::AppHandle,
    force: bool,
) -> HubResult<hub_core::machine::MachineStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        app.state::<MachineState>().0.refresh(
            &state.0,
            &hub_core::machine::SystemDetector,
            hub_core::machine::now_ms(),
            force,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn machine_register(
    app: tauri::AppHandle,
    input: hub_core::machine::MachineInput,
) -> HubResult<hub_core::machine::MachineStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        app.state::<MachineState>().0.register(
            &state.0,
            &hub_core::machine::SystemDetector,
            input,
            hub_core::machine::now_ms(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Edita nome, uso e descrição. Só aceita `MachineInput`: identidade e snapshot
/// não vêm da interface.
#[tauri::command]
fn machine_update(
    app: tauri::AppHandle,
    input: hub_core::machine::MachineInput,
) -> HubResult<hub_core::machine::MachineStatus> {
    let state = app.state::<AppState>();
    app.state::<MachineState>()
        .0
        .update(&state.0, input, hub_core::machine::now_ms())
}
/// Machine Telemetry: um único sampler por app (hub_core::telemetry::Service).
struct TelemetryState(hub_core::telemetry::Service);
/// Estado atual e o buffer curto das sparklines (para quem abre o Dashboard agora).
#[tauri::command]
fn machine_telemetry(app: tauri::AppHandle) -> hub_core::telemetry::TelemetryState {
    app.state::<TelemetryState>().0.snapshot()
}
/// O Dashboard renova o interesse enquanto está na tela; sem renovação, o sampler desacelera.
#[tauri::command]
fn machine_telemetry_watch(app: tauri::AppHandle) {
    app.state::<TelemetryState>()
        .0
        .watch(hub_core::machine::now_ms());
}
/// "Atualizar agora": amostra completa imediata (a do inventário é `machine_refresh`).
#[tauri::command]
fn machine_telemetry_refresh(app: tauri::AppHandle) {
    app.state::<TelemetryState>().0.poke();
}
/// Windows Health (SESSION-002, Block 07): coletor passivo com cache por domínio. Só leitura:
/// nada é reparado, reiniciado, instalado, iniciado ou parado.
struct WindowsHealthState(
    hub_core::windows_health::Collector<hub_core::windows_health::WindowsSources>,
);
/// Snapshot atual; cada domínio só é relido quando o cache dele expira. `force` ("Atualizar agora")
/// relê tudo. Roda fora do thread da interface (o Event Log pode levar algumas centenas de ms).
#[tauri::command]
async fn windows_health_snapshot(
    app: tauri::AppHandle,
    force: bool,
) -> HubResult<hub_core::windows_health::WindowsHealthSnapshot> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<WindowsHealthState>()
            .0
            .snapshot(hub_core::machine::now_ms(), force)
    })
    .await
    .map_err(|e| e.to_string())
}
/// Network & Security (SESSION-002, Block 08): coletor passivo com cache por domínio. Só leitura:
/// nada é ativado, bloqueado, encerrado, escaneado ou alterado, e nenhuma consulta externa é feita.
struct NetworkSecurityState(
    hub_core::network_security::Collector<hub_core::network_security::LiveSources>,
);
#[tauri::command]
async fn network_security_snapshot(
    app: tauri::AppHandle,
    force: bool,
) -> HubResult<hub_core::network_security::NetworkSecuritySnapshot> {
    tauri::async_runtime::spawn_blocking(move || {
        // O contexto só serve para atribuir portas em escuta a Projects; nada é gravado.
        let (ctx, managed, _) = control_plane_inputs(&app)?;
        Ok(app.state::<NetworkSecurityState>().0.snapshot(
            hub_core::machine::now_ms(),
            force,
            &ctx,
            &managed,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn list_projects(state: State<AppState>) -> HubResult<Vec<ProjectEntry>> {
    let projects = db(&state)?.projects()?;
    Ok(projects
        .into_iter()
        .map(hub_core::projects::entry)
        .collect())
}
/// Página Projetos: UMA chamada devolve cada projeto com disponibilidade, Git, runtime e stack
/// (somente leitura, paralelismo limitado, falhas isoladas por projeto).
#[tauri::command]
async fn project_overviews(
    app: tauri::AppHandle,
) -> HubResult<hub_core::overview::ProjectsOverview> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (all, last) = {
            let guard = db(&state)?;
            (guard.projects()?, guard.last_activity_by_project()?)
        };
        let supervisor = &app.state::<RuntimeState>().0;
        let runs = all
            .iter()
            .map(|p| (p.id.clone(), supervisor.runs_for(&p.id)))
            .collect();
        Ok(hub_core::overview::collect(all, last, runs, &|| {
            supervisor.managed_pids()
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn save_project(
    state: State<AppState>,
    id: Option<String>,
    input: ProjectInput,
) -> HubResult<Project> {
    db(&state)?.save(id.as_deref(), input)
}
#[tauri::command]
fn delete_project(state: State<AppState>, id: String, confirmed: bool) -> HubResult<()> {
    db(&state)?.delete(&id, confirmed)
}
#[tauri::command]
async fn choose_folder() -> HubResult<Option<String>> {
    Ok(rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|p| p.path().to_string_lossy().to_string()))
}
#[tauri::command]
async fn discover_project(path: String) -> HubResult<hub_core::projects::Discovery> {
    tauri::async_runtime::spawn_blocking(move || hub_core::projects::discover(&path))
        .await
        .map_err(|e| e.to_string())?
}
/// Inspeção passiva do cadastro: só leitura de arquivos e Git somente leitura; nada é executado.
#[tauri::command]
async fn inspect_project_folder(
    state: State<'_, AppState>,
    path: String,
) -> HubResult<hub_core::inspect::ProjectInspection> {
    let known = db(&state)?.projects_for_matching()?;
    tauri::async_runtime::spawn_blocking(move || hub_core::inspect::inspect_folder(&path, &known))
        .await
        .map_err(|e| e.to_string())
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterProjectInput {
    path: String,
    name: String,
    #[serde(default)]
    description: String,
}
/// Cadastro: a pasta é reinspecionada AGORA e locator/stack/repositório vêm do backend, nunca da
/// interface; a decisão final (novo / já conhecido / já cadastrado) é tomada sob a trava do banco.
#[tauri::command]
async fn register_project(
    state: State<'_, AppState>,
    input: RegisterProjectInput,
) -> HubResult<hub_core::database::RegisterResult> {
    let path = input.path.clone();
    let inspection =
        tauri::async_runtime::spawn_blocking(move || hub_core::inspect::inspect_folder(&path, &[]))
            .await
            .map_err(|e| e.to_string())?;
    if !inspection.valid {
        return Err(inspection
            .error
            .unwrap_or_else(|| "Pasta inválida.".to_string()));
    }
    let request = hub_core::database::RegisterRequest {
        folder: std::path::PathBuf::from(&inspection.folder),
        locator: inspection.locator,
        repository: inspection.repository,
        stack: inspection
            .stack
            .iter()
            .map(|s| s.label.to_string())
            .collect(),
        name: input.name,
        description: input.description,
    };
    db(&state)?.register(request)
}
#[tauri::command]
async fn system_state() -> HubResult<hub_core::system::SystemState> {
    tauri::async_runtime::spawn_blocking(hub_core::system::inspect)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn workspace_state(state: State<'_, AppState>) -> HubResult<WorkspaceState> {
    let projects = db(&state)?.projects()?;
    tauri::async_runtime::spawn_blocking(move || -> HubResult<WorkspaceState> {
        let processes = hub_core::system::processes(&projects);
        let (ports, port_error) =
            match hub_core::ports::inspect_with_processes(&projects, &processes) {
                Ok(ports) => (ports, None),
                Err(error) => (Vec::new(), Some(error)),
            };
        Ok(WorkspaceState {
            system: hub_core::system::inspect(),
            ports,
            processes,
            port_error,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}
#[tauri::command]
async fn git_state(state: State<'_, AppState>, id: String) -> HubResult<hub_core::git::GitState> {
    let p = db(&state)?.project(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        hub_core::git::inspect(&hub_core::projects::local_dir(&p)?)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn github_state(
    state: State<'_, AppState>,
    id: String,
) -> HubResult<hub_core::github::HostingState> {
    use hub_core::github::GitHostingProvider;
    let p = db(&state)?.project(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        hub_core::github::GitHubProvider.inspect(&hub_core::projects::local_dir(&p)?)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn list_ports(state: State<'_, AppState>) -> HubResult<Vec<hub_core::ports::PortInfo>> {
    let p = db(&state)?.projects()?;
    tauri::async_runtime::spawn_blocking(move || hub_core::ports::inspect(&p))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn list_processes(
    state: State<'_, AppState>,
) -> HubResult<Vec<hub_core::system::ProcessInfo>> {
    let p = db(&state)?.projects()?;
    tauri::async_runtime::spawn_blocking(move || hub_core::system::processes(&p))
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn kill_process(pid: u32, start_time: u64, confirmed: bool) -> HubResult<()> {
    tauri::async_runtime::spawn_blocking(move || hub_core::ports::kill(pid, start_time, confirmed))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn launch_project(state: State<AppState>, id: String, action: String) -> HubResult<()> {
    hub_core::launchers::launch(&db(&state)?.project(&id)?, &action)
}
#[tauri::command]
fn bind_project(
    state: State<AppState>,
    id: String,
    path: String,
    confirmed: bool,
) -> HubResult<BindResult> {
    db(&state)?.bind(&id, &path, confirmed)
}
/// Control Plane (SESSION-002): observação da máquina. 100% leitura — nada é encerrado, fechado ou alterado.
type ControlPlaneInputs = (
    hub_core::control_plane::Context,
    std::collections::HashMap<u32, (String, String)>,
    Vec<hub_core::supervisor::RunInfo>,
);
fn control_plane_inputs(app: &tauri::AppHandle) -> HubResult<ControlPlaneInputs> {
    let state = app.state::<AppState>();
    let ctx = db(&state)?.control_plane_context()?;
    let supervisor = &app.state::<RuntimeState>().0;
    Ok((ctx, supervisor.managed_runs(), supervisor.all_runs()))
}
#[tauri::command]
async fn control_plane_snapshot(
    app: tauri::AppHandle,
) -> HubResult<hub_core::control_plane::ControlPlaneSnapshot> {
    tauri::async_runtime::spawn_blocking(move || {
        let (ctx, managed, runs) = control_plane_inputs(&app)?;
        hub_core::control_plane::observe(&ctx, managed, runs)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn control_plane_processes(
    app: tauri::AppHandle,
    include_all: bool,
) -> HubResult<Vec<hub_core::control_plane::ProcessEntry>> {
    tauri::async_runtime::spawn_blocking(move || {
        let (ctx, managed, _) = control_plane_inputs(&app)?;
        hub_core::control_plane::observe_processes(&ctx, &managed, include_all)
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Planejamento (Concept 09). Leitura agregada: 100% passiva (nada é gravado ao abrir a página).
#[tauri::command]
fn planning_overview(
    state: State<AppState>,
    project_id: String,
) -> HubResult<hub_core::planning::PlanningOverview> {
    db(&state)?.planning_overview(&project_id)
}
/// Só contagens, PRÓXIMO e Session ativa (Project Control Center / Próxima ação).
#[tauri::command]
fn planning_summary(
    state: State<AppState>,
    project_id: String,
) -> HubResult<hub_core::planning::PlanningSummary> {
    db(&state)?.planning_summary(&project_id)
}
#[tauri::command]
fn planning_events(
    state: State<AppState>,
    item_id: String,
) -> HubResult<Vec<hub_core::planning::PlanningEvent>> {
    db(&state)?.planning_events(&item_id)
}
#[tauri::command]
fn planning_create_item(
    state: State<AppState>,
    project_id: String,
    title: String,
    description: Option<String>,
) -> HubResult<hub_core::planning::PlanningItem> {
    db(&state)?.planning_create_item(&project_id, &title, description.as_deref().unwrap_or(""))
}
#[tauri::command]
fn planning_update_item(
    state: State<AppState>,
    id: String,
    title: String,
    description: Option<String>,
) -> HubResult<hub_core::planning::PlanningItem> {
    db(&state)?.planning_update_item(&id, &title, description.as_deref().unwrap_or(""))
}
#[tauri::command]
fn planning_cancel(
    state: State<AppState>,
    id: String,
    reason: Option<String>,
) -> HubResult<hub_core::planning::PlanningItem> {
    db(&state)?.planning_cancel(&id, reason.as_deref())
}
#[tauri::command]
fn planning_restore(
    state: State<AppState>,
    id: String,
) -> HubResult<hub_core::planning::PlanningItem> {
    db(&state)?.planning_restore(&id)
}
#[tauri::command]
fn planning_move(
    state: State<AppState>,
    id: String,
    to: hub_core::planning::MoveTo,
) -> HubResult<hub_core::planning::PlanningItem> {
    db(&state)?.planning_move(&id, to)
}
/// INICIAR, passo 1: o rascunho do formulário de Nova Session (não cria nada).
#[tauri::command]
fn planning_prepare_start(
    state: State<AppState>,
    id: String,
) -> HubResult<hub_core::planning::StartDraft> {
    db(&state)?.planning_prepare_start(&id)
}
/// INICIAR, passo 2 (explícito, após o usuário revisar o formulário): cria a Session já vinculada.
#[tauri::command]
fn planning_start_session(
    state: State<AppState>,
    id: String,
    title: String,
    objective: String,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.planning_start_session(&id, &title, &objective)
}
/// DDAE (Concept 06): lista, derivados e importação idempotente da SESSION-001 histórica.
#[tauri::command]
fn ddae_overview(
    state: State<AppState>,
    project_id: String,
) -> HubResult<hub_core::ddae::DdaeOverview> {
    db(&state)?.ddae_overview_with_legacy(&project_id)
}
#[tauri::command]
fn ddae_create_session(
    state: State<AppState>,
    project_id: String,
    title: String,
    objective: String,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_create_session(&project_id, &title, &objective)
}
#[tauri::command]
fn ddae_add_block(
    state: State<AppState>,
    session_id: String,
    title: String,
    description: Option<String>,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_add_block(&session_id, &title, description.as_deref().unwrap_or(""))
}
/// Renomeia um bloco pendente ou em andamento (concluído é histórico).
#[tauri::command]
fn ddae_rename_block(
    state: State<AppState>,
    session_id: String,
    block_id: String,
    title: String,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_rename_block(&session_id, &block_id, &title)
}
/// Remove um bloco SOMENTE se pendente.
#[tauri::command]
fn ddae_remove_block(
    state: State<AppState>,
    session_id: String,
    block_id: String,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_remove_block(&session_id, &block_id)
}
/// Detalhe de UMA Session validando o par (Project, Session) para a rota do Concept 07.
#[tauri::command]
fn ddae_session_detail(
    state: State<AppState>,
    project_id: String,
    session_id: String,
) -> HubResult<hub_core::ddae::SessionView> {
    db(&state)?.ddae_session_detail(&project_id, &session_id)
}
/// Referência da sessão; um caminho absoluto de arquivo do projeto vira relativo (fora = recusa).
#[tauri::command]
fn ddae_add_reference(
    state: State<AppState>,
    session_id: String,
    kind: hub_core::ddae::ReferenceKind,
    value: String,
    label: Option<String>,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_add_reference(&session_id, kind, &value, label.as_deref())
}
#[tauri::command]
fn ddae_start_block(
    state: State<AppState>,
    session_id: String,
    block_id: String,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_start_block(&session_id, &block_id)
}
#[tauri::command]
fn ddae_complete_block(
    state: State<AppState>,
    session_id: String,
    block_id: String,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_complete_block(&session_id, &block_id)
}
#[tauri::command]
fn ddae_freeze(
    state: State<AppState>,
    session_id: String,
    reason: String,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_freeze(&session_id, &reason)
}
#[tauri::command]
fn ddae_stop(
    state: State<AppState>,
    session_id: String,
    reason: String,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_stop(&session_id, &reason)
}
#[tauri::command]
fn ddae_resume(state: State<AppState>, session_id: String) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_resume(&session_id)
}
#[tauri::command]
fn ddae_complete(
    state: State<AppState>,
    session_id: String,
    result: String,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_complete(&session_id, &result)
}
#[tauri::command]
fn ddae_add_decision(
    state: State<AppState>,
    session_id: String,
    title: String,
    body: String,
    block_id: Option<String>,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_add_decision(&session_id, &title, &body, block_id.as_deref())
}
/// Define objetivo, resultado desejado, restrições, critérios, notas e referências da Session.
#[tauri::command]
fn ddae_update_details(
    state: State<AppState>,
    session_id: String,
    details: hub_core::ddae::Details,
) -> HubResult<hub_core::ddae::Session> {
    db(&state)?.ddae_update_details(&session_id, details)
}
/// Contexto determinístico da Session (Markdown) + Ready for AI derivado. Sem LLM.
#[tauri::command]
fn ddae_generate_context(
    state: State<AppState>,
    session_id: String,
) -> HubResult<hub_core::ddae::SessionContext> {
    db(&state)?.ddae_generate_context(&session_id)
}
#[tauri::command]
fn export_portable(state: State<AppState>) -> HubResult<hub_core::portable::PortableWorkspace> {
    db(&state)?.export_portable()
}
#[tauri::command]
fn apply_portable(
    state: State<AppState>,
    workspace: hub_core::portable::PortableWorkspace,
) -> HubResult<hub_core::portable::ApplySummary> {
    db(&state)?.apply_portable(&workspace)
}
/// Estado de sync (comandos abaixo): a interface nunca fala com o bridge nem com o Git.
#[derive(Default)]
struct SyncRuntime {
    running: std::sync::atomic::AtomicBool,
}
fn save_preferences(
    app: &tauri::AppHandle,
    preferences: Option<hub_core::portable::PortablePreferences>,
) -> HubResult<()> {
    if let Some(preferences) = preferences {
        let state = app.state::<AppState>();
        db(&state)?.save_preferences(preferences)?;
    }
    Ok(())
}
/// Estado local (sem rede) ou, com `check_remote`, comparado ao repositório via bridge.
#[tauri::command]
async fn sync_status(
    app: tauri::AppHandle,
    preferences: Option<hub_core::portable::PortablePreferences>,
    check_remote: bool,
) -> HubResult<hub_core::sync::SyncStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        save_preferences(&app, preferences)?;
        let state = app.state::<AppState>();
        if check_remote {
            hub_core::sync::status(&state.0, &hub_core::bridge::HttpBridge::from_env())
        } else {
            hub_core::sync::local_status(&state.0)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Ação "Sincronizar". `resolution` ("local" | "remote") só vem de uma escolha explícita.
#[tauri::command]
async fn sync_run(
    app: tauri::AppHandle,
    preferences: Option<hub_core::portable::PortablePreferences>,
    resolution: Option<String>,
) -> HubResult<hub_core::sync::SyncStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        save_preferences(&app, preferences)?;
        let resolution = resolution
            .as_deref()
            .map(hub_core::sync::Resolution::parse)
            .transpose()?;
        let state = app.state::<AppState>();
        let runtime = app.state::<SyncRuntime>();
        hub_core::sync::sync(
            &state.0,
            &hub_core::bridge::HttpBridge::from_env(),
            resolution,
            &runtime.running,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn open_localhost(port: u16) -> HubResult<()> {
    hub_core::launchers::open_url(&format!("http://127.0.0.1:{port}"))
}
/// Processos iniciados pelo LKR LAB ("Rodar"). O resto é externo e nunca é parado daqui.
struct RuntimeState(hub_core::supervisor::Supervisor);
fn runtime_of(
    app: &tauri::AppHandle,
    id: &str,
) -> HubResult<(hub_core::models::Project, hub_core::runtime::ProjectRuntime)> {
    let state = app.state::<AppState>();
    let (project, all) = {
        let guard = db(&state)?;
        (guard.project(id)?, guard.projects()?)
    };
    let supervisor = &app.state::<RuntimeState>().0;
    let runtime = hub_core::runtime::inspect_live(&project, &all, supervisor.runs_for(id), &|| {
        supervisor.managed_pids()
    });
    Ok((project, runtime))
}
#[tauri::command]
async fn project_runtime(
    app: tauri::AppHandle,
    id: String,
) -> HubResult<hub_core::runtime::ProjectRuntime> {
    tauri::async_runtime::spawn_blocking(move || runtime_of(&app, &id).map(|(_, runtime)| runtime))
        .await
        .map_err(|e| e.to_string())?
}
/// Executa uma ação do projeto. `command_id` e `selection` são só chaves: o comando é
/// resolvido no backend, dos arquivos locais da pasta vinculada (`actions::resolve`).
#[tauri::command]
async fn runtime_start(
    app: tauri::AppHandle,
    id: String,
    command_id: String,
    selection: Option<String>,
) -> HubResult<hub_core::supervisor::RunInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let project = db(&state)?.project(&id)?;
        let run = app.state::<RuntimeState>().0.start_command(
            &project,
            &command_id,
            selection.as_deref(),
        )?;
        db(&state)?.activity(&id, &format!("Execução iniciada: {}", run.command))?;
        Ok(run)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn runtime_stop(app: tauri::AppHandle, run_id: String) -> HubResult<()> {
    app.state::<RuntimeState>().0.stop(&run_id)
}
#[tauri::command]
async fn runtime_restart(
    app: tauri::AppHandle,
    id: String,
    run_id: String,
) -> HubResult<hub_core::supervisor::RunInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let project = db(&state)?.project(&id)?;
        let run = app.state::<RuntimeState>().0.restart(&project, &run_id)?;
        db(&state)?.activity(&id, &format!("Execução reiniciada: {}", run.command))?;
        Ok(run)
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Encerra os acompanhamentos de logs (observadores) do projeto: nunca sobrevivem à tela.
#[tauri::command]
fn runtime_stop_observers(app: tauri::AppHandle, id: String) {
    app.state::<RuntimeState>().0.stop_observers(&id);
}
#[tauri::command]
fn runtime_logs(
    app: tauri::AppHandle,
    run_id: String,
    since: u64,
) -> HubResult<hub_core::supervisor::LogChunk> {
    app.state::<RuntimeState>().0.logs(&run_id, since)
}
#[tauri::command]
async fn generate_context(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> HubResult<String> {
    let task_id = id.clone();
    let text = tauri::async_runtime::spawn_blocking(move || {
        let (project, runtime) = runtime_of(&app, &task_id)?;
        hub_core::snapshot::generate(&project, &runtime)
    })
    .await
    .map_err(|e| e.to_string())??;
    db(&state)?.activity(&id, hub_core::database::CONTEXT_ACTIVITY)?;
    Ok(text)
}
#[tauri::command]
async fn save_context(app: tauri::AppHandle, id: String) -> HubResult<bool> {
    let text = tauri::async_runtime::spawn_blocking(move || {
        let (project, runtime) = runtime_of(&app, &id)?;
        hub_core::snapshot::generate(&project, &runtime)
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(file) = rfd::AsyncFileDialog::new()
        .set_file_name("development-context.md")
        .add_filter("Markdown", &["md"])
        .save_file()
        .await
    {
        file.write(text.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}
#[tauri::command]
fn list_prompts(state: State<AppState>) -> HubResult<Vec<Prompt>> {
    db(&state)?.prompts()
}
#[tauri::command]
fn list_knowledge(state: State<AppState>) -> HubResult<Vec<KnowledgeEntry>> {
    db(&state)?.knowledge()
}
#[tauri::command]
fn save_knowledge(state: State<AppState>, entry: KnowledgeEntry) -> HubResult<String> {
    db(&state)?.save_knowledge(entry)
}
#[tauri::command]
fn save_prompt(state: State<AppState>, prompt: Prompt) -> HubResult<String> {
    db(&state)?.save_prompt(prompt)
}
/// Atividades recentes de um projeto (Visão geral do Project Control Center).
#[tauri::command]
fn project_activity(
    state: State<AppState>,
    id: String,
) -> HubResult<hub_core::database::ProjectActivity> {
    db(&state)?.project_activity(&id, 8)
}
#[tauri::command]
fn list_activities(state: State<AppState>) -> HubResult<Vec<Activity>> {
    db(&state)?.activities()
}
#[tauri::command]
fn agent_context(state: State<AppState>, id: String) -> HubResult<hub_core::agents::AgentContext> {
    use hub_core::agents::AgentProvider;
    let p = db(&state)?.project(&id)?;
    Ok(hub_core::agents::ClaudeProvider.context(&hub_core::projects::local_dir(&p)?))
}
#[tauri::command]
fn agent_providers() -> Vec<hub_core::agents::AgentProviderStatus> {
    hub_core::agents::providers()
}
#[tauri::command]
async fn list_worktrees(
    state: State<'_, AppState>,
    id: String,
) -> HubResult<Vec<hub_core::git::Worktree>> {
    let p = db(&state)?.project(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        hub_core::git::worktrees(&hub_core::projects::local_dir(&p)?)
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Worktrees do LKR LAB (Concept 08). Leitura agregada: só `git worktree list` e `git status`;
/// NUNCA grava metadata, binding ou evento (abrir a página não escreve).
#[tauri::command]
async fn project_worktree_overview(
    app: tauri::AppHandle,
    id: String,
) -> HubResult<hub_core::worktrees::WorktreeOverview> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut overview = db(&state)?.project_worktree_overview(&id, false)?;
        // Fora do lock do banco: cada git status pode demorar.
        overview.attach_git();
        Ok(overview)
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Só as contagens (Project Control Center): sem estado Git de cada worktree.
#[tauri::command]
async fn worktree_summary(
    app: tauri::AppHandle,
    id: String,
) -> HubResult<hub_core::worktrees::WorktreeCounts> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let counts = db(&state)?.project_worktree_overview(&id, false)?.counts;
        Ok(counts)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn worktrees_for_session(
    state: State<AppState>,
    project_id: String,
    session_id: String,
) -> HubResult<Vec<hub_core::worktrees::SessionWorktree>> {
    db(&state)?.worktrees_for_session(&project_id, &session_id)
}
/// ADOTAR (explícito): cria a metadata de um worktree adicional real. Nunca o principal.
#[tauri::command]
async fn worktree_adopt(
    app: tauri::AppHandle,
    project_id: String,
    path: String,
    name: Option<String>,
    session_id: Option<String>,
    block_id: Option<String>,
) -> HubResult<hub_core::worktrees::ManagedWorktree> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let result = db(&state)?.worktree_adopt(
            &project_id,
            &path,
            name.as_deref(),
            session_id.as_deref(),
            block_id.as_deref(),
        );
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
/// NOVO WORKTREE (explícito e mutante): `git worktree add` + metadata ACTIVE + binding local.
#[tauri::command]
async fn worktree_create(
    app: tauri::AppHandle,
    project_id: String,
    request: hub_core::worktrees::CreateRequest,
) -> HubResult<hub_core::worktrees::ManagedWorktree> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let result = db(&state)?.worktree_create(&project_id, request);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Estado operacional: metadata do LKR LAB. Não executa nenhum comando Git.
#[tauri::command]
fn worktree_set_state(
    state: State<AppState>,
    id: String,
    status: hub_core::worktrees::OperationalStatus,
    reason: Option<String>,
    result: Option<String>,
) -> HubResult<hub_core::worktrees::ManagedWorktree> {
    db(&state)?.worktree_set_state(
        &id,
        status,
        reason.as_deref().unwrap_or(""),
        result.as_deref().unwrap_or(""),
    )
}
#[tauri::command]
fn worktree_update(
    state: State<AppState>,
    id: String,
    display_name: String,
    description: Option<String>,
) -> HubResult<hub_core::worktrees::ManagedWorktree> {
    db(&state)?.worktree_update(&id, &display_name, description.as_deref().unwrap_or(""))
}
#[tauri::command]
fn worktree_set_relation(
    state: State<AppState>,
    id: String,
    session_id: Option<String>,
    block_id: Option<String>,
) -> HubResult<hub_core::worktrees::ManagedWorktree> {
    db(&state)?.worktree_set_relation(&id, session_id.as_deref(), block_id.as_deref())
}
/// LOCALIZAR: casa um worktree real desta máquina com a metadata do workspace (UUID preservado).
#[tauri::command]
async fn worktree_locate(
    app: tauri::AppHandle,
    id: String,
    path: String,
) -> HubResult<hub_core::worktrees::ManagedWorktree> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let result = db(&state)?.worktree_locate(&id, &path);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
/// REMOVER DO GIT (explícito; diferente de FINALIZAR). Preserva metadata e eventos.
#[tauri::command]
async fn worktree_git_remove(
    app: tauri::AppHandle,
    project_id: String,
    path: String,
    confirmed: bool,
) -> HubResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let result = db(&state)?.worktree_git_remove(&project_id, &path, confirmed);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn launch_worktree(
    state: State<'_, AppState>,
    id: String,
    path: String,
    action: String,
) -> HubResult<()> {
    let mut project = db(&state)?.project(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = hub_core::git::worktree_path(&hub_core::projects::local_dir(&project)?, &path)?;
        project.local_path = path.to_string_lossy().into();
        hub_core::launchers::launch(&project, &action)
    })
    .await
    .map_err(|error| error.to_string())?
}
/// Gate global no backend: antes do cadastro do computador, todo comando que não
/// seja de cadastro é recusado aqui, seja qual for a tela ou o atalho que o chamou.
/// Comandos de plugin (controles da janela) não passam por este handler.
fn gated<R: tauri::Runtime>(
    handler: impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        let allowed = invoke
            .message
            .webview_ref()
            .state::<MachineState>()
            .0
            .allows(invoke.message.command());
        if !allowed {
            invoke.resolver.reject(hub_core::machine::NOT_REGISTERED);
            return true;
        }
        handler(invoke)
    }
}
fn main() {
    let result = tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let database = Database::open(&dir.join("hub.db")).map_err(std::io::Error::other)?;
            let registry =
                hub_core::machine::Registry::load(&database).map_err(std::io::Error::other)?;
            app.manage(MachineState(registry));
            app.manage(AppState(Mutex::new(database)));
            app.manage(SyncRuntime::default());
            let handle = app.handle().clone();
            let sink: hub_core::supervisor::EventSink = std::sync::Arc::new(move |event| {
                let _ = handle.emit("runtime://event", event);
            });
            app.manage(RuntimeState(hub_core::supervisor::Supervisor::new(sink)));
            app.manage(WindowsHealthState(
                hub_core::windows_health::Collector::new(hub_core::windows_health::WindowsSources),
            ));
            app.manage(NetworkSecurityState(
                hub_core::network_security::Collector::new(hub_core::network_security::LiveSources),
            ));
            let emitter = app.handle().clone();
            let window = app.handle().clone();
            app.manage(TelemetryState(hub_core::telemetry::Service::start(
                std::sync::Arc::new(move |telemetry| {
                    let _ = emitter.emit(hub_core::telemetry::EVENT, telemetry);
                }),
                std::sync::Arc::new(move || {
                    window.get_webview_window("main").is_some_and(|w| {
                        w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
                    })
                }),
            )));
            Ok(())
        })
        .invoke_handler(gated(tauri::generate_handler![
            machine_status,
            machine_refresh,
            machine_register,
            machine_update,
            machine_telemetry,
            machine_telemetry_watch,
            machine_telemetry_refresh,
            windows_health_snapshot,
            network_security_snapshot,
            list_projects,
            project_overviews,
            save_project,
            delete_project,
            choose_folder,
            discover_project,
            inspect_project_folder,
            register_project,
            system_state,
            workspace_state,
            git_state,
            github_state,
            list_ports,
            list_processes,
            kill_process,
            launch_project,
            bind_project,
            export_portable,
            apply_portable,
            sync_status,
            sync_run,
            project_runtime,
            runtime_start,
            runtime_stop,
            runtime_restart,
            runtime_stop_observers,
            runtime_logs,
            open_localhost,
            generate_context,
            save_context,
            list_prompts,
            list_knowledge,
            save_knowledge,
            save_prompt,
            list_activities,
            project_activity,
            agent_context,
            agent_providers,
            list_worktrees,
            launch_worktree,
            project_worktree_overview,
            worktree_summary,
            worktrees_for_session,
            worktree_adopt,
            worktree_create,
            worktree_set_state,
            worktree_update,
            worktree_set_relation,
            worktree_locate,
            worktree_git_remove,
            control_plane_snapshot,
            control_plane_processes,
            planning_overview,
            planning_summary,
            planning_events,
            planning_create_item,
            planning_update_item,
            planning_cancel,
            planning_restore,
            planning_move,
            planning_prepare_start,
            planning_start_session,
            ddae_overview,
            ddae_create_session,
            ddae_add_block,
            ddae_start_block,
            ddae_complete_block,
            ddae_freeze,
            ddae_stop,
            ddae_resume,
            ddae_complete,
            ddae_add_decision,
            ddae_update_details,
            ddae_generate_context,
            ddae_rename_block,
            ddae_remove_block,
            ddae_session_detail,
            ddae_add_reference
        ]))
        .build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(|handle, event| {
            // Sair do app encerra as árvores gerenciadas: nada fica órfão.
            if let tauri::RunEvent::Exit = event {
                handle.state::<RuntimeState>().0.stop_all();
                handle.state::<TelemetryState>().0.stop();
            }
        }),
        Err(error) => {
            eprintln!("Falha ao iniciar LKR LAB: {error}");
            std::process::exit(1);
        }
    }
}
