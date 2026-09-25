use hub_core::HubResult;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::{collections::HashMap, io::{Read, Write}, path::Path, sync::{Arc, Condvar, Mutex}};
use tauri::{ipc::Channel, State};

#[derive(Default)]
pub struct TerminalState(pub Arc<Mutex<HashMap<String, Arc<Session>>>>);
#[derive(Default)]
struct Flow { ready: bool, closed: bool }
pub struct Session {
    flow: Arc<(Mutex<Flow>, Condvar)>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
}
impl Drop for Session {
    fn drop(&mut self) {
        let (lock, signal) = &*self.flow;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        signal.notify_all();
        if let Ok(child) = self.child.get_mut() { let _ = child.kill(); }
    }
}
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalEvent { Output { data: Vec<u8> }, End { error: Option<String> } }

fn size(rows: u16, cols: u16) -> HubResult<PtySize> {
    if !(2..=300).contains(&rows) || !(10..=500).contains(&cols) { return Err("Dimensões de terminal inválidas".into()); }
    Ok(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
}
fn session(state: &TerminalState, id: &str) -> HubResult<Arc<Session>> {
    state.0.lock().map_err(|_| "Terminal indisponível")?.get(id).cloned().ok_or_else(|| "Sessão encerrada".into())
}
pub fn close_all(state: &TerminalState) {
    let sessions = state.0.lock().map(|mut sessions| std::mem::take(&mut *sessions)).unwrap_or_default();
    // ConPTY closure may wait for its output pipe to drain; never do that on the UI thread.
    std::thread::spawn(move || drop(sessions));
}
fn start(state: &TerminalState, id: String, path: &Path, rows: u16, cols: u16, output: Channel<TerminalEvent>) -> HubResult<()> {
    uuid::Uuid::parse_str(&id).map_err(|_| "Identificador de sessão inválido")?;
    let cwd = path.canonicalize().map_err(|_| "Diretório do projeto indisponível")?;
    if !cwd.is_dir() { return Err("Diretório do projeto inválido".into()); }
    let mut registry = state.0.lock().map_err(|_| "Terminal indisponível")?;
    if registry.len() >= 8 { return Err("Limite de 8 sessões abertas; feche uma aba antes de continuar".into()); }
    if registry.contains_key(&id) { return Err("Sessão já existe".into()); }
    let pair = native_pty_system().openpty(size(rows, cols)?).map_err(|error| format!("PTY indisponível: {error}"))?;
    let shell = hub_core::commands::executable(if cfg!(windows) { "powershell" } else { "sh" }).ok_or("Shell nativo não encontrado")?;
    let mut command = CommandBuilder::new(shell);
    if cfg!(windows) { command.args(["-NoLogo", "-NoProfile"]); }
    command.cwd(cwd);
    command.env("TERM", "xterm-256color");
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let child = pair.slave.spawn_command(command).map_err(|error| format!("Falha ao iniciar shell: {error}"))?;
    let mut killer = child.clone_killer();
    drop(pair.slave);
    let flow = Arc::new((Mutex::new(Flow { ready: true, closed: false }), Condvar::new()));
    registry.insert(id, Arc::new(Session { flow: flow.clone(), child: Mutex::new(child), writer: Mutex::new(writer), master: Mutex::new(pair.master) }));
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => { let _ = output.send(TerminalEvent::End { error: None }); break; }
                Ok(count) => count,
                Err(error) => { let _ = output.send(TerminalEvent::End { error: Some(error.to_string()) }); break; }
            };
            let (lock, signal) = &*flow;
            let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
            while !state.ready && !state.closed { state = signal.wait(state).unwrap_or_else(|e| e.into_inner()); }
            // Continue draining after closure so ConPTY can shut down without a pipe deadlock.
            if state.closed { continue; }
            state.ready = false;
            drop(state);
            if output.send(TerminalEvent::Output { data: buffer[..count].to_vec() }).is_err() {
                lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
                let _ = killer.kill();
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn terminal_start(state: State<'_, super::AppState>, terminals: State<'_, TerminalState>, project_id: String, id: String, rows: u16, cols: u16, output: Channel<TerminalEvent>) -> HubResult<()> {
    let project = super::db(&state)?.project(&project_id)?;
    let terminals = TerminalState(terminals.0.clone());
    tauri::async_runtime::spawn_blocking(move || start(&terminals, id, Path::new(&project.local_path), rows, cols, output)).await.map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn terminal_write(terminals: State<'_, TerminalState>, id: String, data: String) -> HubResult<()> {
    if data.len() > 32 * 1024 { return Err("Envie no máximo 32 KB por entrada".into()); }
    let terminal = session(&terminals, &id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut writer = terminal.writer.lock().map_err(|_| "Entrada indisponível")?;
        writer.write_all(data.as_bytes()).and_then(|_| writer.flush()).map_err(|e| e.to_string())
    }).await.map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn terminal_ack(terminals: State<'_, TerminalState>, id: String) -> HubResult<()> {
    let terminal = session(&terminals, &id)?;
    let (lock, signal) = &*terminal.flow;
    lock.lock().map_err(|_| "Saída indisponível")?.ready = true;
    signal.notify_one();
    Ok(())
}
#[tauri::command]
pub async fn terminal_resize(terminals: State<'_, TerminalState>, id: String, rows: u16, cols: u16) -> HubResult<()> {
    let terminal = session(&terminals, &id)?;
    let dimensions = size(rows, cols)?;
    tauri::async_runtime::spawn_blocking(move || terminal.master.lock().map_err(|_| "Terminal indisponível")?.resize(dimensions).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn terminal_close(terminals: State<'_, TerminalState>, id: String, confirmed: bool) -> HubResult<()> {
    if !confirmed { return Err("Confirmação necessária para encerrar a sessão".into()); }
    let terminal = terminals.0.lock().map_err(|_| "Terminal indisponível")?.remove(&id);
    tauri::async_runtime::spawn_blocking(move || drop(terminal)).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_bounds_reject_invalid_geometry() { assert!(size(0, 80).is_err()); assert!(size(24, 1000).is_err()); assert!(size(24, 80).is_ok()); }
    #[test]
    #[ignore = "Starts an isolated native PTY; run explicitly on host"]
    fn native_pty_reads_real_output() {
        let pair = native_pty_system().openpty(size(24, 80).unwrap()).unwrap();
        let mut command = CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "sh" });
        if cfg!(windows) { command.args(["/D", "/C", "echo LK_PTY_VERIFIED"]); } else { command.args(["-c", "printf LK_PTY_VERIFIED"]); }
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || { let mut buffer = [0; 4096]; let mut output = Vec::new(); while let Ok(count) = reader.read(&mut buffer) { if count == 0 { break; } output.extend_from_slice(&buffer[..count]); if String::from_utf8_lossy(&output).contains("LK_PTY_VERIFIED") { let _ = tx.send(true); break; } } });
        let result = rx.recv_timeout(std::time::Duration::from_secs(15));
        let _ = child.kill();
        drop(pair.master);
        let _ = worker.join();
        assert_eq!(result.unwrap(), true);
    }
}
