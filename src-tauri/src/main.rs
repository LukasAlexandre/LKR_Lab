#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use hub_core::{database::Database, models::*, HubResult};
use serde::Serialize;
use std::{path::Path, sync::Mutex};
use tauri::{Manager, State};
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
    tauri::async_runtime::spawn_blocking(move || hub_core::git::inspect(Path::new(&p.local_path)))
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
        hub_core::github::GitHubProvider.inspect(Path::new(&p.local_path))
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
fn open_localhost(port: u16) -> HubResult<()> {
    hub_core::launchers::open_url(&format!("http://127.0.0.1:{port}"))
}
#[tauri::command]
async fn generate_context(state: State<'_, AppState>, id: String) -> HubResult<String> {
    let p = db(&state)?.project(&id)?;
    let text = tauri::async_runtime::spawn_blocking(move || hub_core::snapshot::generate(&p))
        .await
        .map_err(|e| e.to_string())?;
    db(&state)?.activity(&id, "Contexto de desenvolvimento gerado")?;
    Ok(text)
}
#[tauri::command]
async fn save_context(state: State<'_, AppState>, id: String) -> HubResult<bool> {
    let p = db(&state)?.project(&id)?;
    let text = tauri::async_runtime::spawn_blocking(move || hub_core::snapshot::generate(&p))
        .await
        .map_err(|e| e.to_string())?;
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
    Ok(hub_core::agents::ClaudeProvider.context(Path::new(&p.local_path)))
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
    tauri::async_runtime::spawn_blocking(move || hub_core::git::worktrees(Path::new(&p.local_path)))
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
        hub_core::git::create_worktree(Path::new(&project.local_path), &path, &branch)
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
        hub_core::git::remove_worktree(Path::new(&project.local_path), &path, confirmed)
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
        let path = hub_core::git::worktree_path(Path::new(&project.local_path), &path)?;
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
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Falha ao iniciar LKR LAB: {error}");
        std::process::exit(1);
    }
}
