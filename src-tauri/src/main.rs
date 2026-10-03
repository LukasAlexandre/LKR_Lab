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
#[tauri::command]
fn list_projects(state: State<AppState>) -> HubResult<Vec<ProjectEntry>> {
    let projects = db(&state)?.projects()?;
    Ok(projects
        .into_iter()
        .map(hub_core::projects::entry)
        .collect())
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
    db(&state)?.activity(&id, "Contexto de desenvolvimento gerado")?;
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
#[tauri::command]
async fn create_worktree(
    state: State<'_, AppState>,
    id: String,
    path: String,
    branch: String,
) -> HubResult<()> {
    let project = db(&state)?.project(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        hub_core::git::create_worktree(&hub_core::projects::local_dir(&project)?, &path, &branch)
    })
    .await
    .map_err(|error| error.to_string())??;
    db(&state)?.activity(&id, "Worktree criada")?;
    Ok(())
}
#[tauri::command]
async fn remove_worktree(
    state: State<'_, AppState>,
    id: String,
    path: String,
    confirmed: bool,
) -> HubResult<()> {
    let project = db(&state)?.project(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        hub_core::git::remove_worktree(&hub_core::projects::local_dir(&project)?, &path, confirmed)
    })
    .await
    .map_err(|error| error.to_string())??;
    db(&state)?.activity(&id, "Worktree removida")?;
    Ok(())
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
            list_projects,
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
            agent_context,
            agent_providers,
            list_worktrees,
            create_worktree,
            remove_worktree,
            launch_worktree
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
