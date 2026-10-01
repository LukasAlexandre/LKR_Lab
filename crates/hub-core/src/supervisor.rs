//! Processos GERENCIADOS pelo LKR LAB (iniciados por "Rodar").
//!
//! Só o que o supervisor iniciou é "managed"; o resto é externo e nunca é
//! encerrado daqui. A árvore de cada execução vive num grupo (Job Object no
//! Windows): parar uma execução encerra npm → node → vite juntos, por
//! handle — nunca por nome de processo e imune a reutilização de PID.
//! Ao sair do app o grupo fecha e nada fica órfão.
use crate::{
    models::Project,
    runtime::{self, ScriptKind},
    HubResult,
};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        mpsc::{self, RecvTimeoutError},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_LOG_LINES: usize = 2000;
const MAX_LINE: usize = 4000;
const KEEP_FINISHED: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Completed,
}
impl RunState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub id: String,
    pub project_id: String,
    pub script: String,
    /// Exibição do comando ("npm run dev"); nunca é executado como texto.
    pub command: String,
    pub kind: ScriptKind,
    pub state: RunState,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    /// Milissegundos desde a época Unix.
    pub started_at: u64,
    pub last_seq: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RuntimeEvent {
    State {
        project_id: String,
        run_id: String,
        state: RunState,
        pid: Option<u32>,
        exit_code: Option<i32>,
    },
    /// Há saída nova (a interface busca com `logs`); no máximo ~10 por segundo.
    Output {
        project_id: String,
        run_id: String,
        seq: u64,
    },
}
pub type EventSink = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub seq: u64,
    /// "out" | "err"
    pub stream: &'static str,
    pub text: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogChunk {
    pub lines: Vec<LogLine>,
    pub next_seq: u64,
    /// Linhas antigas foram descartadas pelo limite do buffer.
    pub truncated: bool,
}

#[derive(Default)]
struct LogBuffer {
    lines: VecDeque<LogLine>,
    next_seq: u64,
}
impl LogBuffer {
    fn push(&mut self, stream: &'static str, text: String) {
        self.lines.push_back(LogLine {
            seq: self.next_seq,
            stream,
            text,
        });
        self.next_seq += 1;
        while self.lines.len() > MAX_LOG_LINES {
            self.lines.pop_front();
        }
    }
}

struct Status {
    state: RunState,
    pid: Option<u32>,
    exit_code: Option<i32>,
    stop_requested: bool,
}

struct Run {
    id: String,
    project_id: String,
    script: String,
    command: String,
    kind: ScriptKind,
    started_at: u64,
    status: Mutex<Status>,
    changed: Condvar,
    logs: Mutex<LogBuffer>,
    group: group::Group,
}
impl Run {
    fn info(&self) -> RunInfo {
        let status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        RunInfo {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            script: self.script.clone(),
            command: self.command.clone(),
            kind: self.kind,
            state: status.state,
            pid: status.pid,
            exit_code: status.exit_code,
            started_at: self.started_at,
            last_seq: self.logs.lock().unwrap_or_else(|e| e.into_inner()).next_seq,
        }
    }
    fn state_event(&self) -> RuntimeEvent {
        let status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        RuntimeEvent::State {
            project_id: self.project_id.clone(),
            run_id: self.id.clone(),
            state: status.state,
            pid: status.pid,
            exit_code: status.exit_code,
        }
    }
}

#[derive(Clone)]
pub struct Supervisor {
    runs: Arc<Mutex<Vec<Arc<Run>>>>,
    sink: EventSink,
    /// Serializa montar+entregar cada evento de estado: a ordem entregue é a ordem real.
    emit_lock: Arc<Mutex<()>>,
}

fn emit_state(sink: &EventSink, lock: &Mutex<()>, run: &Run) {
    let _order = lock.lock().unwrap_or_else(|e| e.into_inner());
    sink(run.state_event());
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Remove sequências ANSI (cores, cursor, título) para exibir o log como texto.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if c != '\r' {
                out.push(c);
            }
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(n) = chars.next() {
                    if n == '\u{7}' {
                        break;
                    }
                    if n == '\u{1b}' {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {
                chars.next();
            }
        }
    }
    out
}

/// Lê um fluxo em linhas (com limite por linha) e as envia ao pump.
fn read_lines(mut stream: impl Read, kind: &'static str, tx: mpsc::Sender<(&'static str, String)>) {
    let mut chunk = [0u8; 8192];
    let mut line: Vec<u8> = Vec::new();
    let flush = |line: &mut Vec<u8>| {
        let text = strip_ansi(&String::from_utf8_lossy(line));
        line.clear();
        let text: String = text.chars().take(MAX_LINE).collect();
        let _ = tx.send((kind, text));
    };
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                for &byte in &chunk[..n] {
                    if byte == b'\n' {
                        flush(&mut line);
                    } else {
                        line.push(byte);
                        if line.len() >= MAX_LINE {
                            flush(&mut line);
                        }
                    }
                }
            }
        }
    }
    if !line.is_empty() {
        flush(&mut line);
    }
}

impl Supervisor {
    pub fn new(sink: EventSink) -> Self {
        Self {
            runs: Arc::new(Mutex::new(Vec::new())),
            sink,
            emit_lock: Arc::new(Mutex::new(())),
        }
    }

    fn all(&self) -> Vec<Arc<Run>> {
        self.runs.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn find(&self, run_id: &str) -> HubResult<Arc<Run>> {
        self.all()
            .into_iter()
            .find(|r| r.id == run_id)
            .ok_or_else(|| "Execução não encontrada.".to_string())
    }

    /// Inicia `script` do projeto. O comando vem SOMENTE do package.json local
    /// (validado em `runtime::launch_spec`); nada vem do workspace portátil.
    pub fn start(&self, project: &Project, script: &str) -> HubResult<RunInfo> {
        let spec = runtime::launch_spec(project, script)?;
        {
            let runs = self.runs.lock().unwrap_or_else(|e| e.into_inner());
            if runs.iter().any(|r| {
                r.project_id == project.id
                    && r.script == script
                    && (r
                        .status
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .state
                        .is_active()
                        || !r.group.pids().is_empty())
            }) {
                return Err(format!("“{script}” já está em execução neste projeto."));
            }
        }
        let group = group::Group::new()?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NO_COLOR", "1")
            .env("FORCE_COLOR", "0");
        group::prepare(&mut command);
        let mut child: Child = command
            .spawn()
            .map_err(|e| format!("Não foi possível iniciar o comando: {e}"))?;
        let pid = child.id();
        group.adopt(&child);
        let run = Arc::new(Run {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project.id.clone(),
            script: script.to_string(),
            command: spec.display,
            kind: spec.kind,
            started_at: now_ms(),
            status: Mutex::new(Status {
                state: RunState::Starting,
                pid: Some(pid),
                exit_code: None,
                stop_requested: false,
            }),
            changed: Condvar::new(),
            logs: Mutex::new(LogBuffer::default()),
            group,
        });
        {
            let mut runs = self.runs.lock().unwrap_or_else(|e| e.into_inner());
            runs.push(run.clone());
            // Mantém as ativas e só as últimas execuções terminadas.
            let finished: Vec<String> = runs
                .iter()
                .filter(|r| {
                    !r.status
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .state
                        .is_active()
                        && r.group.pids().is_empty()
                })
                .map(|r| r.id.clone())
                .collect();
            if finished.len() > KEEP_FINISHED {
                let drop_ids: Vec<_> = finished[..finished.len() - KEEP_FINISHED].to_vec();
                runs.retain(|r| !drop_ids.contains(&r.id));
            }
        }
        crate::system::invalidate_process_cache();
        emit_state(&self.sink, &self.emit_lock, &run);

        let (tx, rx) = mpsc::channel();
        if let Some(out) = child.stdout.take() {
            let tx = tx.clone();
            thread::spawn(move || read_lines(out, "out", tx));
        }
        if let Some(err) = child.stderr.take() {
            let tx = tx.clone();
            thread::spawn(move || read_lines(err, "err", tx));
        }
        drop(tx);

        // Espera o processo: único dono do Child.
        {
            let run = run.clone();
            let sink = self.sink.clone();
            let lock = self.emit_lock.clone();
            thread::spawn(move || {
                let code = child.wait().ok().and_then(|s| s.code());
                {
                    let mut status = run.status.lock().unwrap_or_else(|e| e.into_inner());
                    status.exit_code = code;
                    status.state = if status.stop_requested {
                        RunState::Stopped
                    } else if code == Some(0) {
                        RunState::Completed
                    } else {
                        RunState::Failed
                    };
                }
                run.changed.notify_all();
                crate::system::invalidate_process_cache();
                emit_state(&sink, &lock, &run);
            });
        }

        // Pump: junta stdout/stderr no buffer, promove Starting → Running e emite
        // avisos de saída com limite de frequência (com descarga final).
        {
            let run = run.clone();
            let sink = self.sink.clone();
            let lock = self.emit_lock.clone();
            thread::spawn(move || {
                let started = Instant::now();
                let mut last_emit = Instant::now() - Duration::from_secs(1);
                let mut dirty = false;
                let mut closed_at: Option<Instant> = None;
                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok((stream, text)) => {
                            run.logs
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .push(stream, text);
                            dirty = true;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            closed_at.get_or_insert_with(Instant::now);
                        }
                    }
                    let promote = {
                        let mut status = run.status.lock().unwrap_or_else(|e| e.into_inner());
                        let ready = status.state == RunState::Starting
                            && (dirty || started.elapsed() > Duration::from_millis(600));
                        if ready {
                            status.state = RunState::Running;
                        }
                        ready
                    };
                    if promote {
                        emit_state(&sink, &lock, &run);
                    }
                    if dirty && last_emit.elapsed() >= Duration::from_millis(100) {
                        let seq = run.logs.lock().unwrap_or_else(|e| e.into_inner()).next_seq;
                        sink(RuntimeEvent::Output {
                            project_id: run.project_id.clone(),
                            run_id: run.id.clone(),
                            seq,
                        });
                        last_emit = Instant::now();
                        dirty = false;
                    }
                    let finished = !run
                        .status
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .state
                        .is_active();
                    if closed_at.is_some() && !dirty {
                        break;
                    }
                    // Pipes herdados por um neto solto não prendem o pump para sempre.
                    if finished && closed_at.is_none() && started.elapsed() > Duration::from_secs(3)
                    {
                        let idle = last_emit.elapsed() > Duration::from_millis(500);
                        if idle && !dirty {
                            break;
                        }
                    }
                }
            });
        }
        Ok(run.info())
    }

    /// Pede o encerramento e retorna; o estado final chega por evento.
    pub fn stop(&self, run_id: &str) -> HubResult<()> {
        let run = self.find(run_id)?;
        let live = {
            let mut status = run.status.lock().unwrap_or_else(|e| e.into_inner());
            let alive = status.state.is_active();
            if alive {
                status.stop_requested = true;
                status.state = RunState::Stopping;
            }
            alive
        };
        if live {
            emit_state(&self.sink, &self.emit_lock, &run);
        }
        if live || !run.group.pids().is_empty() {
            run.group.terminate();
            crate::system::invalidate_process_cache();
        }
        Ok(())
    }

    /// Para e espera a árvore inteira desaparecer.
    pub fn stop_and_wait(&self, run_id: &str, timeout: Duration) -> HubResult<()> {
        let run = self.find(run_id)?;
        self.stop(run_id)?;
        let deadline = Instant::now() + timeout;
        let mut status = run.status.lock().unwrap_or_else(|e| e.into_inner());
        while status.state.is_active() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("O processo não encerrou a tempo.".into());
            }
            status = run
                .changed
                .wait_timeout(status, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        drop(status);
        while !run.group.pids().is_empty() {
            if Instant::now() >= deadline {
                return Err("Ainda há processos filhos ativos.".into());
            }
            run.group.terminate();
            thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }

    /// Para (esperando a árvore sumir) e inicia de novo — nunca duas instâncias juntas.
    pub fn restart(&self, project: &Project, run_id: &str) -> HubResult<RunInfo> {
        let run = self.find(run_id)?;
        if run.project_id != project.id {
            return Err("Execução de outro projeto.".into());
        }
        let script = run.script.clone();
        self.stop_and_wait(run_id, Duration::from_secs(15))?;
        self.start(project, &script)
    }

    pub fn logs(&self, run_id: &str, since: u64) -> HubResult<LogChunk> {
        let run = self.find(run_id)?;
        let logs = run.logs.lock().unwrap_or_else(|e| e.into_inner());
        let first = logs.lines.front().map(|l| l.seq).unwrap_or(logs.next_seq);
        Ok(LogChunk {
            lines: logs
                .lines
                .iter()
                .filter(|l| l.seq >= since)
                .cloned()
                .collect(),
            next_seq: logs.next_seq,
            truncated: since < first,
        })
    }

    pub fn runs_for(&self, project_id: &str) -> Vec<RunInfo> {
        let mut runs: Vec<_> = self
            .all()
            .into_iter()
            .filter(|r| r.project_id == project_id)
            .map(|r| r.info())
            .collect();
        runs.sort_by_key(|r| std::cmp::Reverse(r.started_at));
        runs
    }

    /// PID → projeto de todo processo vivo nas árvores gerenciadas.
    pub fn managed_pids(&self) -> HashMap<u32, String> {
        let mut map = HashMap::new();
        for run in self.all() {
            for pid in run.group.pids() {
                map.insert(pid, run.project_id.clone());
            }
        }
        map
    }

    /// Ao sair do app: encerra todas as árvores gerenciadas.
    pub fn stop_all(&self) {
        for run in self.all() {
            let _ = self.stop(&run.id);
        }
    }
}

#[cfg(windows)]
mod group {
    use std::{
        ffi::c_void,
        os::windows::{io::AsRawHandle, process::CommandExt},
        process::{Child, Command},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
        },
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn prepare(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    /// Job Object com KILL_ON_JOB_CLOSE: fechar o handle (ou sair do app) mata a árvore.
    pub struct Group(usize);
    impl Group {
        pub fn new() -> Result<Self, String> {
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err("Não foi possível criar o grupo de processos.".into());
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    CloseHandle(job);
                    return Err("Não foi possível configurar o grupo de processos.".into());
                }
                Ok(Self(job as usize))
            }
        }
        fn handle(&self) -> HANDLE {
            self.0 as HANDLE
        }
        /// Coloca o processo recém-criado (e filhos que já tenham nascido) no grupo.
        pub fn adopt(&self, child: &Child) {
            unsafe {
                AssignProcessToJobObject(self.handle(), child.as_raw_handle() as HANDLE);
            }
            // Um filho criado antes da atribuição ainda é descendente: recolhe-o também.
            let mut system = sysinfo::System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            let mut frontier = vec![child.id()];
            let mut seen = std::collections::HashSet::new();
            while let Some(parent) = frontier.pop() {
                for (pid, process) in system.processes() {
                    if process.parent().map(|p| p.as_u32()) == Some(parent)
                        && seen.insert(pid.as_u32())
                    {
                        self.assign_pid(pid.as_u32());
                        frontier.push(pid.as_u32());
                    }
                }
            }
        }
        fn assign_pid(&self, pid: u32) {
            unsafe {
                let handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
                if !handle.is_null() {
                    AssignProcessToJobObject(self.handle(), handle);
                    CloseHandle(handle);
                }
            }
        }
        pub fn pids(&self) -> Vec<u32> {
            // Cabeçalho (2 × u32) + até 256 PIDs.
            let mut buffer = vec![0usize; 1 + 256];
            let ok = unsafe {
                QueryInformationJobObject(
                    self.handle(),
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr() as *mut c_void,
                    (buffer.len() * std::mem::size_of::<usize>()) as u32,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Vec::new();
            }
            let in_list = (buffer[0] >> 32) as u32 as usize;
            buffer[1..]
                .iter()
                .take(in_list.min(256))
                .map(|p| *p as u32)
                .collect()
        }
        pub fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.handle(), 1);
            }
        }
    }
    impl Drop for Group {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle());
            }
        }
    }
    unsafe impl Send for Group {}
    unsafe impl Sync for Group {}
}

#[cfg(not(windows))]
mod group {
    use std::process::{Child, Command};
    pub fn prepare(command: &mut Command) {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    /// Sem Job Object: a árvore é a de descendentes do processo raiz.
    pub struct Group(std::sync::Mutex<Option<u32>>);
    impl Group {
        pub fn new() -> Result<Self, String> {
            Ok(Self(std::sync::Mutex::new(None)))
        }
        pub fn adopt(&self, child: &Child) {
            *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(child.id());
        }
        fn descendants(root: u32) -> Vec<u32> {
            let mut system = sysinfo::System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            let mut found = vec![root];
            let mut index = 0;
            while index < found.len() {
                let parent = found[index];
                for (pid, process) in system.processes() {
                    if process.parent().map(|p| p.as_u32()) == Some(parent)
                        && !found.contains(&pid.as_u32())
                    {
                        found.push(pid.as_u32());
                    }
                }
                index += 1;
            }
            let alive: std::collections::HashSet<u32> =
                system.processes().keys().map(|p| p.as_u32()).collect();
            found.into_iter().filter(|p| alive.contains(p)).collect()
        }
        pub fn pids(&self) -> Vec<u32> {
            match *self.0.lock().unwrap_or_else(|e| e.into_inner()) {
                Some(root) => Self::descendants(root),
                None => Vec::new(),
            }
        }
        pub fn terminate(&self) {
            let pids = self.pids();
            let system = sysinfo::System::new_all();
            // Folhas primeiro, raiz por último.
            for pid in pids.iter().rev() {
                if let Some(process) = system.process(sysinfo::Pid::from_u32(*pid)) {
                    process.kill();
                }
            }
        }
    }
}
