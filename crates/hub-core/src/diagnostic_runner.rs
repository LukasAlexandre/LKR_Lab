//! Diagnostic Runner (SESSION-002, Block 09): diagnósticos EXPLÍCITOS, somente leitura, só sob ação
//! do usuário. Nunca roda sozinho, nunca repara.
//!
//! * **Allowlist estrita:** cada diagnóstico é um id conhecido (`DiagnosticId`) que mapeia para um
//!   comando e argumentos FIXOS definidos aqui. Não existe campo para comando, argumentos ou
//!   texto de shell: o único parâmetro livre é a letra do volume, validada (`C:`), e o processo é
//!   iniciado sem shell (`Command` com argumentos separados), então não há injeção de comando.
//! * **Só verificação:** `sfc /verifyonly`, `DISM /CheckHealth`, `DISM /ScanHealth` e
//!   `CHKDSK <vol> /scan`. `sfc /scannow`, `DISM /RestoreHealth`, `chkdsk /f` e `/r` e qualquer
//!   outro reparo NÃO existem neste catálogo.
//! * **Elevação:** todos exigem administrador. O LKR LAB não pede UAC nem cria processo
//!   privilegiado: sem elevação o diagnóstico fica `requires_elevation` e não é executado.
//! * **Resultado:** não se infere sucesso só pelo código de saída. Cada ferramenta tem um parser
//!   próprio do texto (inglês e português) e o que não for reconhecido é `inconclusive`.
//! * **Domínio próprio:** não é um Project Runtime. A saída é estado da MÁQUINA (pode conter
//!   caminhos locais): não vai ao workspace portátil, ao sync nem ao contexto de IA, e só um resumo
//!   e uma cauda curta são guardados no histórico.
use crate::windows_health::Millis;
use serde::Serialize;
use std::{
    collections::VecDeque,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

/// Teto de bytes guardados por fluxo (o resto é descartado do começo).
pub const MAX_STREAM_BYTES: usize = 1024 * 1024;
/// A cauda da saída que fica no histórico.
pub const TAIL_LINES: usize = 30;
pub const TAIL_LINE_CHARS: usize = 200;
pub const TAIL_BYTES: usize = 4096;
pub const HISTORY_KEEP: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticId {
    SfcVerifyOnly,
    DismCheckHealth,
    DismScanHealth,
    ChkdskScan,
}

impl DiagnosticId {
    pub const ALL: [Self; 4] = [
        Self::SfcVerifyOnly,
        Self::DismCheckHealth,
        Self::DismScanHealth,
        Self::ChkdskScan,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SfcVerifyOnly => "sfc_verifyonly",
            Self::DismCheckHealth => "dism_checkhealth",
            Self::DismScanHealth => "dism_scanhealth",
            Self::ChkdskScan => "chkdsk_scan",
        }
    }
    /// Só aceita os ids exatos do catálogo (nada de comando livre).
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.as_str() == text)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::SfcVerifyOnly => "Verificar arquivos do sistema (SFC, somente verificação)",
            Self::DismCheckHealth => "Verificar o component store (DISM CheckHealth)",
            Self::DismScanHealth => "Examinar o component store (DISM ScanHealth)",
            Self::ChkdskScan => "Examinar um volume (CHKDSK, somente leitura)",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::SfcVerifyOnly => "Compara os arquivos protegidos do Windows com a cópia original e só informa; não repara nada. Pode levar vários minutos.",
            Self::DismCheckHealth => "Consulta se o component store já foi marcado como corrompido. É rápido e não repara nada.",
            Self::DismScanHealth => "Examina o component store em busca de corrupção, sem reparar. Pode levar vários minutos.",
            Self::ChkdskScan => "Examina o sistema de arquivos de um volume online, sem corrigir nada (/scan). Pode levar vários minutos.",
        }
    }
    pub fn needs_target(self) -> bool {
        self == Self::ChkdskScan
    }
    /// Todos os diagnósticos do catálogo precisam de administrador.
    pub fn requires_elevation(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagError {
    UnknownDiagnostic(String),
    InvalidTarget(String),
    /// Exige administrador e o app não está elevado (nunca pede UAC).
    RequiresElevation,
    Busy,
    Spawn(String),
}
impl std::fmt::Display for DiagError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownDiagnostic(_) => write!(f, "Diagnóstico desconhecido: só os do catálogo podem ser executados."),
            Self::InvalidTarget(why) => write!(f, "Alvo inválido: {why}"),
            Self::RequiresElevation => write!(f, "Requer administrador: o LKR LAB não solicita elevação automaticamente. Abra o app como administrador para executar este diagnóstico."),
            Self::Busy => write!(f, "Já existe um diagnóstico em execução."),
            Self::Spawn(why) => write!(f, "Não foi possível iniciar o diagnóstico: {why}"),
        }
    }
}
impl From<DiagError> for String {
    fn from(error: DiagError) -> Self {
        error.to_string()
    }
}

/// `C:` (uma letra de unidade). Qualquer outra coisa — caminho, flags, texto de shell — é rejeitada.
pub fn validate_volume(target: &str) -> Result<String, DiagError> {
    let bytes = target.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        Ok(format!("{}:", (bytes[0] as char).to_ascii_uppercase()))
    } else {
        Err(DiagError::InvalidTarget(
            "informe somente a letra do volume, como C:".into(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Resolve um id do catálogo em um comando fixo. Os testes injetam um catálogo inofensivo.
pub trait Catalog: Send + Sync {
    fn resolve(&self, id: DiagnosticId, target: Option<&str>) -> Result<CommandSpec, DiagError>;
}

/// Catálogo real: executáveis do System32 por caminho absoluto (sem busca no PATH) e argumentos fixos.
pub struct SystemCatalog;
impl Catalog for SystemCatalog {
    fn resolve(&self, id: DiagnosticId, target: Option<&str>) -> Result<CommandSpec, DiagError> {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let system32 = PathBuf::from(root).join("System32");
        let (exe, args): (&str, Vec<String>) = match id {
            DiagnosticId::SfcVerifyOnly => ("sfc.exe", vec!["/verifyonly".into()]),
            DiagnosticId::DismCheckHealth => (
                "dism.exe",
                vec![
                    "/Online".into(),
                    "/Cleanup-Image".into(),
                    "/CheckHealth".into(),
                ],
            ),
            DiagnosticId::DismScanHealth => (
                "dism.exe",
                vec![
                    "/Online".into(),
                    "/Cleanup-Image".into(),
                    "/ScanHealth".into(),
                ],
            ),
            DiagnosticId::ChkdskScan => {
                let volume = validate_volume(target.unwrap_or(""))?;
                ("chkdsk.exe", vec![volume, "/scan".into()])
            }
        };
        Ok(CommandSpec {
            program: system32.join(exe),
            args,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunResult {
    /// A ferramenta concluiu e não encontrou problemas.
    Clean,
    /// A ferramenta concluiu e encontrou problemas.
    ProblemsFound,
    /// Terminou, mas o texto não permite afirmar nada.
    Inconclusive,
    /// A ferramenta não conseguiu executar (erro, sem permissão).
    Failed,
    Cancelled,
}

impl RunResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::ProblemsFound => "problems_found",
            Self::Inconclusive => "inconclusive",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Clean,
            Self::ProblemsFound,
            Self::Inconclusive,
            Self::Failed,
            Self::Cancelled,
        ]
        .into_iter()
        .find(|r| r.as_str() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub diagnostic: String,
    pub label: String,
    pub target: Option<String>,
    pub started_at: Millis,
    pub finished_at: Option<Millis>,
    pub running: bool,
    /// `None` enquanto roda.
    pub result: Option<RunResult>,
    pub exit_code: Option<i32>,
    pub summary: String,
    /// Cauda curta da saída (machine-local; nunca a saída inteira).
    pub output_tail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub needs_target: bool,
    pub requires_elevation: bool,
    /// Pode ser iniciado agora.
    pub available: bool,
    pub reason: Option<String>,
}

pub fn catalog(elevated: bool) -> Vec<DiagnosticInfo> {
    DiagnosticId::ALL
        .into_iter()
        .map(|id| {
            let blocked = id.requires_elevation() && !elevated;
            DiagnosticInfo {
                id: id.as_str(),
                label: id.label(),
                description: id.description(),
                needs_target: id.needs_target(),
                requires_elevation: id.requires_elevation(),
                available: !blocked,
                reason: blocked.then(|| DiagError::RequiresElevation.to_string()),
            }
        })
        .collect()
}

// ------------------------------------------------------------------ saída e resultado

/// Texto do console: UTF-16LE (SFC escreve assim), senão a página de código OEM do console.
pub fn decode_console(bytes: &[u8]) -> String {
    let zeros = bytes.iter().filter(|b| **b == 0).count();
    if bytes.len() >= 2 && zeros * 4 >= bytes.len() {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16_lossy(&units);
        return text.trim_start_matches('\u{feff}').to_string();
    }
    oem_to_string(bytes)
}

#[cfg(windows)]
fn oem_to_string(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::MultiByteToWideChar;
    const CP_OEMCP: u32 = 1;
    if bytes.is_empty() {
        return String::new();
    }
    // SAFETY: `bytes` é válido para `len` bytes; a primeira chamada só mede o buffer necessário.
    let len = unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if len <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut buffer = vec![0u16; len as usize];
    // SAFETY: `buffer` tem exatamente `len` unidades.
    let written = unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            buffer.as_mut_ptr(),
            len,
        )
    };
    String::from_utf16_lossy(&buffer[..written.max(0) as usize])
}
#[cfg(not(windows))]
fn oem_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Minúsculas, sem acentos e com espaços normalizados, para comparar texto em inglês e português.
pub fn normalize(text: &str) -> String {
    let folded: String = text
        .chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Interpreta o texto da ferramenta. O código de saída sozinho nunca decide "sucesso".
pub fn parse_output(id: DiagnosticId, output: &str, exit_code: Option<i32>) -> (RunResult, String) {
    let text = normalize(output);
    let has = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
    let failed = |what: &str| (RunResult::Failed, what.to_string());
    match id {
        DiagnosticId::SfcVerifyOnly => {
            if has(&[
                "did not find any integrity violations",
                "nao encontrou nenhuma violacao de integridade",
            ]) {
                (
                    RunResult::Clean,
                    "Nenhuma violação de integridade encontrada.".into(),
                )
            } else if has(&[
                "found integrity violations",
                "encontrou violacoes de integridade",
            ]) {
                (RunResult::ProblemsFound, "O SFC encontrou violações de integridade nos arquivos do sistema. Nada foi reparado.".into())
            } else if has(&[
                "could not perform the requested operation",
                "nao pode executar a operacao solicitada",
                "pending",
                "pendente",
            ]) {
                (RunResult::Inconclusive, "O SFC não pôde concluir a verificação (operação pendente ou impossível agora).".into())
            } else if has(&["administrator", "administrador"]) && exit_code != Some(0) {
                failed("O SFC exige administrador.")
            } else if exit_code.is_some_and(|c| c != 0) {
                failed("O SFC terminou com erro.")
            } else {
                (
                    RunResult::Inconclusive,
                    "A saída do SFC não permite concluir o resultado.".into(),
                )
            }
        }
        DiagnosticId::DismCheckHealth | DiagnosticId::DismScanHealth => {
            // O DISM usa códigos de saída de erro de forma confiável (740 = sem elevação).
            if exit_code.is_some_and(|c| c != 0) {
                return if has(&["740", "elevated permissions", "permissoes elevadas"]) {
                    failed("O DISM exige administrador.")
                } else {
                    failed("O DISM terminou com erro.")
                };
            }
            if has(&[
                "no component store corruption detected",
                "nenhuma corrupcao",
            ]) {
                (
                    RunResult::Clean,
                    "Nenhuma corrupção do component store detectada.".into(),
                )
            } else if has(&[
                "repairable",
                "reparavel",
                "pode ser reparado",
                "corruption was detected",
                "corrupcao foi detectada",
            ]) {
                (
                    RunResult::ProblemsFound,
                    "O DISM detectou corrupção no component store. Nada foi reparado.".into(),
                )
            } else {
                (
                    RunResult::Inconclusive,
                    "A saída do DISM não permite concluir o resultado.".into(),
                )
            }
        }
        DiagnosticId::ChkdskScan => {
            if exit_code.is_some_and(|c| c >= 3) {
                return failed("O CHKDSK não conseguiu verificar o volume.");
            }
            if has(&[
                "found no problems",
                "nao encontrou problemas",
                "no problems found",
            ]) {
                (
                    RunResult::Clean,
                    "O volume foi examinado e nenhum problema foi encontrado.".into(),
                )
            } else if has(&[
                "found problems",
                "encontrou problemas",
                "errors found",
                "erros encontrados",
                "corrupt",
            ]) {
                (
                    RunResult::ProblemsFound,
                    "O CHKDSK encontrou problemas no volume. Nada foi corrigido.".into(),
                )
            } else if has(&[
                "access denied",
                "acesso negado",
                "administrator",
                "administrador",
            ]) {
                failed("O CHKDSK exige administrador.")
            } else {
                (
                    RunResult::Inconclusive,
                    "A saída do CHKDSK não permite concluir o resultado.".into(),
                )
            }
        }
    }
}

/// Últimas linhas, curtas, sem linhas vazias (para a interface e o histórico).
pub fn tail_of(output: &str) -> Vec<String> {
    let mut lines: Vec<String> = output
        .lines()
        .map(|l| {
            l.trim_end()
                .chars()
                .take(TAIL_LINE_CHARS)
                .collect::<String>()
        })
        .filter(|l| !l.trim().is_empty())
        .collect();
    let skip = lines.len().saturating_sub(TAIL_LINES);
    lines.drain(..skip);
    while lines.iter().map(|l| l.len() + 1).sum::<usize>() > TAIL_BYTES && lines.len() > 1 {
        lines.remove(0);
    }
    lines
}

// ------------------------------------------------------------------ elevação

/// O app está rodando como administrador? Somente consulta o token do próprio processo.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    // SAFETY: `token` recebe um handle que é fechado abaixo; a consulta é só de leitura.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}
#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

// ------------------------------------------------------------------ execução

type Sink = Arc<dyn Fn(&RunRecord) + Send + Sync>;
type Buffer = Arc<Mutex<Vec<u8>>>;

struct Live {
    record: RunRecord,
    cancel: Arc<AtomicBool>,
    stdout: Buffer,
    stderr: Buffer,
}

#[derive(Default)]
struct State {
    current: Option<Live>,
    last: Option<RunRecord>,
}

/// Um diagnóstico por vez. `sink` recebe o registro final (o app o grava no histórico local).
pub struct Runner {
    catalog: Box<dyn Catalog>,
    elevated: bool,
    sink: Sink,
    state: Arc<Mutex<State>>,
}

fn now_ms() -> Millis {
    crate::machine::now_ms()
}

fn pump(
    mut reader: impl std::io::Read + Send + 'static,
    buffer: Buffer,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut data = buffer.lock().unwrap_or_else(|e| e.into_inner());
            data.extend_from_slice(&chunk[..n]);
            if data.len() > MAX_STREAM_BYTES {
                let drop = data.len() - MAX_STREAM_BYTES;
                data.drain(..drop);
            }
        }
    })
}

fn combined(stdout: &Buffer, stderr: &Buffer) -> String {
    let out = decode_console(&stdout.lock().unwrap_or_else(|e| e.into_inner()));
    let err = decode_console(&stderr.lock().unwrap_or_else(|e| e.into_inner()));
    if err.trim().is_empty() {
        out
    } else {
        format!("{out}\n{err}")
    }
}

impl Runner {
    pub fn new(catalog: Box<dyn Catalog>, elevated: bool, sink: Sink) -> Self {
        Self {
            catalog,
            elevated,
            sink,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    pub fn elevated(&self) -> bool {
        self.elevated
    }

    /// Inicia o diagnóstico `id` (texto do catálogo). Rejeita o que não está na allowlist.
    pub fn start(&self, id: &str, target: Option<&str>) -> Result<RunRecord, DiagError> {
        let diagnostic = DiagnosticId::parse(id)
            .ok_or_else(|| DiagError::UnknownDiagnostic(id.chars().take(40).collect()))?;
        let target = match (diagnostic.needs_target(), target) {
            (true, Some(t)) => Some(validate_volume(t)?),
            (true, None) => {
                return Err(DiagError::InvalidTarget("informe o volume, como C:".into()))
            }
            (false, Some(_)) => {
                return Err(DiagError::InvalidTarget(
                    "este diagnóstico não aceita alvo".into(),
                ))
            }
            (false, None) => None,
        };
        if diagnostic.requires_elevation() && !self.elevated {
            return Err(DiagError::RequiresElevation);
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.current.is_some() {
            return Err(DiagError::Busy);
        }
        let spec = self.catalog.resolve(diagnostic, target.as_deref())?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| DiagError::Spawn(e.kind().to_string()))?;
        let stdout: Buffer = Arc::default();
        let stderr: Buffer = Arc::default();
        let readers: Vec<_> = [
            child.stdout.take().map(|s| pump(s, stdout.clone())),
            child.stderr.take().map(|s| pump(s, stderr.clone())),
        ]
        .into_iter()
        .flatten()
        .collect();
        let record = RunRecord {
            id: format!("{:x}-{:x}", now_ms(), std::process::id()),
            diagnostic: diagnostic.as_str().into(),
            label: diagnostic.label().into(),
            target: target.clone(),
            started_at: now_ms(),
            finished_at: None,
            running: true,
            result: None,
            exit_code: None,
            summary: "Em execução…".into(),
            output_tail: vec![],
        };
        let cancel = Arc::new(AtomicBool::new(false));
        state.current = Some(Live {
            record: record.clone(),
            cancel: cancel.clone(),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
        });
        drop(state);

        let shared = self.state.clone();
        let sink = self.sink.clone();
        let base = record.clone();
        std::thread::spawn(move || {
            let mut cancelled = false;
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) => {}
                    Err(_) => break None,
                }
                if cancel.load(Ordering::SeqCst) && !cancelled {
                    cancelled = true;
                    let _ = child.kill();
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            for reader in readers {
                let _ = reader.join();
            }
            let output = combined(&stdout, &stderr);
            let exit_code = status.and_then(|s| s.code());
            let (result, summary) = if cancelled {
                (RunResult::Cancelled, "Cancelado pelo usuário.".to_string())
            } else if status.is_none() {
                (
                    RunResult::Failed,
                    "Não foi possível acompanhar o processo.".to_string(),
                )
            } else {
                parse_output(diagnostic, &output, exit_code)
            };
            let finished = RunRecord {
                finished_at: Some(now_ms()),
                running: false,
                result: Some(result),
                exit_code,
                summary,
                output_tail: tail_of(&output),
                ..base
            };
            (sink)(&finished);
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            state.current = None;
            state.last = Some(finished);
        });
        Ok(record)
    }

    /// O diagnóstico em andamento (com a cauda parcial) ou o último que terminou.
    pub fn status(&self) -> Option<RunRecord> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match &state.current {
            Some(live) => {
                let mut record = live.record.clone();
                record.output_tail = tail_of(&combined(&live.stdout, &live.stderr));
                Some(record)
            }
            None => state.last.clone(),
        }
    }

    /// Cancela o diagnóstico em andamento (encerra só o processo dele). `false` se não há nenhum.
    pub fn cancel(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match &state.current {
            Some(live) => {
                live.cancel.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }
}

/// Ring do histórico em memória (usado pelos testes e como espelho do banco).
pub fn trim_history(history: &mut VecDeque<RunRecord>) {
    while history.len() > HISTORY_KEEP {
        history.pop_back();
    }
}
