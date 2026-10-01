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
#[tauri::command]
async fn runtime_start(
    app: tauri::AppHandle,
    id: String,
    script: String,
) -> HubResult<hub_core::supervisor::RunInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let project = db(&state)?.project(&id)?;
        let run = app.state::<RuntimeState>().0.start(&project, &script)?;
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
fn main() {
    let result = tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let database = Database::open(&dir.join("hub.db")).map_err(std::io::Error::other)?;
            app.manage(AppState(Mutex::new(database)));
            app.manage(SyncRuntime::default());
            let handle = app.handle().clone();
            let sink: hub_core::supervisor::EventSink = std::sync::Arc::new(move |event| {
                let _ = handle.emit("runtime://event", event);
            });
            app.manage(RuntimeState(hub_core::supervisor::Supervisor::new(sink)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            save_project,
            delete_project,
            choose_folder,
            discover_project,
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
        ])
        .build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(|handle, event| {
            // Sair do app encerra as árvores gerenciadas: nada fica órfão.
            if let tauri::RunEvent::Exit = event {
                handle.state::<RuntimeState>().0.stop_all();
            }
        }),
        Err(error) => {
            eprintln!("Falha ao iniciar LKR LAB: {error}");
            std::process::exit(1);
        }
    }
}
