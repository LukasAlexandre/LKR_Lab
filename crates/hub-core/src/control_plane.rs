//! Control Plane local do LKR LAB (SESSION-002): observação da máquina e atribuição de runtimes.
//!
//! Camadas (nenhuma é um sistema paralelo — todas reaproveitam o que já existe):
//!   1. **Observação** (`system::inventory`, `netstat2`): processos e portas como o SO os mostra.
//!      100% LEITURA: nunca encerra, fecha, bloqueia nem altera nada.
//!   2. **Atribuição** (`attribute`): relaciona PROCESSO → PROJECT → WORKTREE → SESSION *somente*
//!      quando há evidência, sempre com `Confidence` e a lista de evidências. Sem evidência
//!      suficiente o resultado é `Unknown` e a interface mostra "—": o Project ativo NUNCA é um palpite.
//!   3. **Runtimes** (`build_snapshot`): MANAGED (grupo/Job Object do `Supervisor`, console capturado)
//!      e DISCOVERED (processo externo: sem handle, logo sem stdout — dito claramente).
//!   4. **Contrato** (`ControlPlaneSnapshot`): a única forma que a interface consome.
//!
//! Estado de MÁQUINA (classe C): nada daqui vai ao workspace portátil, ao Git ou ao sync, e linhas de
//! comando são redigidas (tokens/senhas) antes de sair deste módulo.
//!
//! Decisão de arquitetura: o processo Tauri + snapshots sob demanda (cache de 1 s) bastam para o MVP.
//! Nenhum worker em segundo plano nem Windows Service é necessário; um agente privilegiado só será
//! considerado se uma informação concreta (ex.: linha de comando de processos protegidos) exigir.
use crate::{
    database::Database,
    supervisor::{RunInfo, RunState},
    system::{self, RawProcess},
    HubResult,
};
use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use rusqlite::params;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

// ------------------------------------------------------------------ contrato

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Sem evidência suficiente: Project, Worktree e Session ficam vazios.
    #[default]
    Unknown,
    /// Indício fraco (ex.: porta declarada por um único Project, caminho na linha de comando).
    Medium,
    /// Evidência forte (cwd/executável dentro do Project ou Worktree, descendente de atribuído).
    High,
    /// Processo na árvore (grupo) de uma execução iniciada pelo LKR LAB.
    Exact,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    /// managed_execution | cwd_in_worktree | cwd_in_project | executable_in_worktree |
    /// executable_in_project | ancestor | command_line_path | declared_port
    pub kind: &'static str,
    pub detail: String,
}

/// Relação PROCESSO → PROJECT → WORKTREE → SESSION. Sessão e bloco só vêm pelo Worktree.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Association {
    pub confidence: Confidence,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub worktree_id: Option<String>,
    pub worktree_name: Option<String>,
    pub session_id: Option<String>,
    pub session_label: Option<String>,
    pub block_id: Option<String>,
    pub block_title: Option<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessObservation {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub executable: Option<String>,
    /// Redigida: valores de token/senha/chave nunca saem daqui.
    pub command_line: String,
    pub cwd: Option<String>,
    /// ms desde a época Unix.
    pub started_at: u64,
    pub cpu: f32,
    pub memory: u64,
    pub read_bytes: u64,
    pub written_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortObservation {
    pub port: u16,
    pub address: String,
    pub protocol: &'static str,
    /// v4 | v6
    pub ip_version: &'static str,
    /// Ausente quando o SO não informa o dono (permissão).
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub depth: u32,
    pub cpu: f32,
    pub memory: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Iniciado pelo LKR LAB (grupo/Job Object, stdout/stderr capturados).
    Managed,
    /// Detectado externamente: sem handle, sem stdout.
    Discovered,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleStream {
    pub available: bool,
    /// Id da execução no Supervisor (para abrir o console). Só quando disponível.
    pub run_id: Option<String>,
    /// Por que não há console.
    pub reason: Option<String>,
}

pub const NO_CONSOLE_EXTERNAL: &str = "Console não disponível — processo iniciado fora do LKR LAB.";
pub const NO_CONSOLE_UNIDENTIFIED: &str =
    "Console não disponível — o sistema não informou o processo dono da porta.";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedExecution {
    pub run_id: String,
    pub command_id: String,
    pub command: String,
    pub project_id: String,
    pub exit_code: Option<i32>,
    pub observer: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeObservation {
    /// Managed: id da execução. Discovered: `pid:<raiz>:<início>` (imune a reutilização de PID).
    pub id: String,
    pub origin: Origin,
    /// dev | system | other (system fica oculto por padrão na interface).
    pub category: &'static str,
    pub label: String,
    /// Dica pela linha de comando/nome (Vite, Node.js, MySQL…). É descrição, não atribuição.
    pub technology: Option<String>,
    /// starting | running | stopping | stopped | failed | completed
    pub state: RunState,
    pub root_pid: Option<u32>,
    pub pids: Vec<u32>,
    pub ports: Vec<PortObservation>,
    pub tree: Vec<TreeNode>,
    pub command: String,
    pub cwd: Option<String>,
    pub started_at: Option<u64>,
    pub cpu: f32,
    pub memory: u64,
    pub association: Association,
    pub console: ConsoleStream,
    pub execution: Option<ManagedExecution>,
    /// O próprio LKR LAB (este app) — nunca encerrável por aqui.
    pub is_self: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

/// Sinal de saúde. O espaço está reservado para CPU, GPU, RAM, VRAM, temperaturas, discos, bateria,
/// rede e Windows (Blocks 06–08); hoje só nasce o que já é observado de verdade.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSignal {
    pub id: String,
    /// runtime | cpu | memory | gpu | disk | network | windows | security
    pub domain: &'static str,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limitation {
    pub id: &'static str,
    pub detail: String,
    pub requires_elevation: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlPlaneSnapshot {
    pub taken_at: u64,
    /// Sempre "local": nada é enviado para fora desta máquina.
    pub scope: &'static str,
    pub process_total: usize,
    pub listening_total: usize,
    pub runtimes: Vec<RuntimeObservation>,
    pub signals: Vec<HealthSignal>,
    pub limitations: Vec<Limitation>,
}

// ------------------------------------------------------------------ contexto (Projects/Worktrees)

#[derive(Debug, Clone)]
pub struct ProjectRef {
    pub id: String,
    pub name: String,
    /// Pasta local vinculada (vazia = sem vínculo: nunca é dona de nada).
    pub root: String,
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct WorktreeRef {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub root: String,
    pub session_id: Option<String>,
    pub session_label: Option<String>,
    pub block_id: Option<String>,
    pub block_title: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Context {
    pub projects: Vec<ProjectRef>,
    pub worktrees: Vec<WorktreeRef>,
}

impl Database {
    /// Projects (com pasta local) e Worktrees gerenciados com binding local, para a atribuição.
    pub fn control_plane_context(&self) -> HubResult<Context> {
        let projects = self
            .projects()?
            .into_iter()
            .map(|p| ProjectRef {
                id: p.id,
                name: p.name,
                root: p.local_path,
                ports: p.ports.iter().map(|x| x.port).collect(),
            })
            .collect();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT w.id,w.project_id,w.display_name,b.local_path,w.session_id,s.number,w.block_id,k.title \
                 FROM managed_worktrees w \
                 JOIN worktree_bindings b ON b.worktree_id=w.id \
                 LEFT JOIN ddae_sessions s ON s.id=w.session_id \
                 LEFT JOIN ddae_blocks k ON k.id=w.block_id \
                 WHERE w.operational_status <> 'completed'",
            )
            .map_err(|e| e.to_string())?;
        let worktrees = stmt
            .query_map(params![], |r| {
                Ok(WorktreeRef {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    name: r.get(2)?,
                    root: r.get(3)?,
                    session_id: r.get(4)?,
                    session_label: r.get::<_, Option<u32>>(5)?.map(crate::ddae::label),
                    block_id: r.get(6)?,
                    block_title: r.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(Context {
            projects,
            worktrees,
        })
    }
}

// ------------------------------------------------------------------ redação (segurança)

fn secret_key(key: &str) -> bool {
    let k = key.to_lowercase();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "pwd",
        "apikey",
        "api-key",
        "api_key",
        "authorization",
        "bearer",
        "credential",
        "private-key",
        "access-key",
    ]
    .iter()
    .any(|w| k.contains(w))
}

/// Substitui valores sensíveis de uma linha de comando por `***`: `--token=x`, `--token x`,
/// `KEY=valor` e credenciais embutidas em URLs (`https://user:senha@host`).
pub fn redact(args: &[String]) -> String {
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for arg in args {
        if hide_next {
            out.push("***".into());
            hide_next = false;
            continue;
        }
        let mut text = arg.clone();
        if let Some(at) = text.find("://") {
            let rest = &text[at + 3..];
            if let (Some(colon), Some(atsign)) = (rest.find(':'), rest.find('@')) {
                if colon < atsign && !rest[..atsign].contains('/') {
                    text = format!(
                        "{}{}:***{}",
                        &text[..at + 3],
                        &rest[..colon],
                        &rest[atsign..]
                    );
                }
            }
        }
        if let Some(eq) = text.find('=') {
            if secret_key(&text[..eq]) {
                text = format!("{}=***", &text[..eq]);
            }
        } else if text.starts_with('-') && secret_key(&text) {
            hide_next = true;
        }
        out.push(if text.contains(' ') {
            format!("\"{text}\"")
        } else {
            text
        });
    }
    out.join(" ")
}

// ------------------------------------------------------------------ caminhos

fn norm(path: &str) -> String {
    let p = path.strip_prefix("\\\\?\\").unwrap_or(path);
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// `child` é a própria `root` ou está dentro dela (fronteira de pasta, não de prefixo de texto).
fn within(child: &str, root: &str) -> bool {
    !root.is_empty()
        && (child == root
            || (child.starts_with(root) && child.as_bytes().get(root.len()) == Some(&b'/')))
}

struct Target<'a> {
    root: String,
    project: &'a ProjectRef,
    worktree: Option<&'a WorktreeRef>,
}

fn targets(ctx: &Context) -> Vec<Target<'_>> {
    let mut list: Vec<Target> = Vec::new();
    for project in ctx.projects.iter().filter(|p| !p.root.is_empty()) {
        list.push(Target {
            root: norm(&project.root),
            project,
            worktree: None,
        });
    }
    for worktree in ctx.worktrees.iter().filter(|w| !w.root.is_empty()) {
        if let Some(project) = ctx.projects.iter().find(|p| p.id == worktree.project_id) {
            list.push(Target {
                root: norm(&worktree.root),
                project,
                worktree: Some(worktree),
            });
        }
    }
    // O mais específico primeiro: um worktree dentro da pasta do projeto ganha do projeto.
    list.sort_by_key(|t| std::cmp::Reverse(t.root.len()));
    list
}

fn association_for(
    project: &ProjectRef,
    worktree: Option<&WorktreeRef>,
    confidence: Confidence,
    evidence: Evidence,
) -> Association {
    Association {
        confidence,
        project_id: Some(project.id.clone()),
        project_name: Some(project.name.clone()),
        worktree_id: worktree.map(|w| w.id.clone()),
        worktree_name: worktree.map(|w| w.name.clone()),
        // Session/Block só pelo Worktree vinculado: nunca por Project ativo.
        session_id: worktree.and_then(|w| w.session_id.clone()),
        session_label: worktree.and_then(|w| w.session_label.clone()),
        block_id: worktree.and_then(|w| w.block_id.clone()),
        block_title: worktree.and_then(|w| w.block_title.clone()),
        evidence: vec![evidence],
    }
}

// ------------------------------------------------------------------ atribuição

/// Evidência DIRETA de um processo (sem olhar ancestrais). `managed` = (projeto, execução) do grupo.
fn direct(
    process: &RawProcess,
    targets: &[Target],
    ctx: &Context,
    managed: Option<&(String, String)>,
    ports: &[u16],
) -> Association {
    let cwd = process.cwd.as_deref().map(norm);
    let exe = process.executable.as_deref().map(norm);
    let located = |path: &Option<String>| {
        path.as_deref()
            .and_then(|p| targets.iter().find(|t| within(p, &t.root)))
    };
    if let Some((project_id, run_id)) = managed {
        if let Some(project) = ctx.projects.iter().find(|p| &p.id == project_id) {
            // O worktree do runtime gerenciado vem do cwd real do processo (e só se for do mesmo Project).
            let worktree = located(&cwd)
                .filter(|t| t.project.id == project.id)
                .and_then(|t| t.worktree);
            return association_for(
                project,
                worktree,
                Confidence::Exact,
                Evidence {
                    kind: "managed_execution",
                    detail: format!("grupo da execução {run_id}"),
                },
            );
        }
    }
    if let Some(t) = located(&cwd) {
        let (kind, name) = match t.worktree {
            Some(w) => ("cwd_in_worktree", w.name.clone()),
            None => ("cwd_in_project", t.project.name.clone()),
        };
        return association_for(
            t.project,
            t.worktree,
            Confidence::High,
            Evidence {
                kind,
                detail: format!("pasta de trabalho dentro de {name}"),
            },
        );
    }
    if let Some(t) = located(&exe) {
        let (kind, name) = match t.worktree {
            Some(w) => ("executable_in_worktree", w.name.clone()),
            None => ("executable_in_project", t.project.name.clone()),
        };
        return association_for(
            t.project,
            t.worktree,
            Confidence::High,
            Evidence {
                kind,
                detail: format!("executável dentro de {name}"),
            },
        );
    }
    let line = norm(&process.cmd.join(" "));
    if let Some(t) = targets
        .iter()
        .find(|t| line.contains(&format!("{}/", t.root)) || line.ends_with(&t.root))
    {
        let name = t
            .worktree
            .map(|w| w.name.clone())
            .unwrap_or_else(|| t.project.name.clone());
        return association_for(
            t.project,
            t.worktree,
            Confidence::Medium,
            Evidence {
                kind: "command_line_path",
                detail: format!("a linha de comando cita um caminho de {name}"),
            },
        );
    }
    let declaring: Vec<&ProjectRef> = ctx
        .projects
        .iter()
        .filter(|p| ports.iter().any(|port| p.ports.contains(port)))
        .collect();
    if declaring.len() == 1 {
        return association_for(
            declaring[0],
            None,
            Confidence::Medium,
            Evidence {
                kind: "declared_port",
                detail: "porta declarada por este Project".into(),
            },
        );
    }
    Association::default()
}

/// Atribui todos os processos: evidência direta + herança de ancestrais (≤ 8 níveis, só de
/// ancestral com confiança ≥ High; Exact só para o que está no grupo gerenciado).
pub fn attribute(
    processes: &[RawProcess],
    ports: &[PortObservation],
    ctx: &Context,
    managed: &HashMap<u32, (String, String)>,
) -> HashMap<u32, Association> {
    let tg = targets(ctx);
    let mut ports_of: HashMap<u32, Vec<u16>> = HashMap::new();
    for p in ports {
        if let Some(pid) = p.pid {
            ports_of.entry(pid).or_default().push(p.port);
        }
    }
    let mut result: HashMap<u32, Association> = processes
        .iter()
        .map(|p| {
            (
                p.pid,
                direct(
                    p,
                    &tg,
                    ctx,
                    managed.get(&p.pid),
                    ports_of.get(&p.pid).map(Vec::as_slice).unwrap_or(&[]),
                ),
            )
        })
        .collect();
    let by_pid: HashMap<u32, &RawProcess> = processes.iter().map(|p| (p.pid, p)).collect();
    for process in processes {
        if result[&process.pid].confidence >= Confidence::High {
            continue;
        }
        let mut ancestor = process.parent;
        for _ in 0..8 {
            let Some(parent) = ancestor.and_then(|id| by_pid.get(&id)) else {
                break;
            };
            // PID reutilizado: o pai não pode ter nascido depois do filho.
            if parent.start_time > process.start_time {
                break;
            }
            let found = &result[&parent.pid];
            if found.confidence >= Confidence::High {
                let mut inherited = found.clone();
                inherited.confidence = Confidence::High;
                inherited.evidence = vec![Evidence {
                    kind: "ancestor",
                    detail: format!("descende de {} (PID {})", parent.name, parent.pid),
                }];
                if inherited.confidence > result[&process.pid].confidence {
                    result.insert(process.pid, inherited);
                }
                break;
            }
            ancestor = parent.parent;
        }
    }
    result
}

// ------------------------------------------------------------------ descrição do runtime

const BOUNDARY: [&str; 16] = [
    "explorer.exe",
    "services.exe",
    "svchost.exe",
    "wininit.exe",
    "winlogon.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "wt.exe",
    "windowsterminal.exe",
    "conhost.exe",
    "openconsole.exe",
    "bash.exe",
    "sh.exe",
    "mintty.exe",
    "code.exe",
];

const TECHNOLOGIES: [(&str, &str); 24] = [
    ("lk-dev-hub", "LKR LAB"),
    ("vite", "Vite"),
    ("next", "Next.js"),
    ("nuxt", "Nuxt"),
    ("webpack", "Webpack"),
    ("tauri", "Tauri"),
    ("uvicorn", "Uvicorn"),
    ("flask", "Flask"),
    ("django", "Django"),
    ("mysqld", "MySQL"),
    ("postgres", "PostgreSQL"),
    ("redis-server", "Redis"),
    ("mongod", "MongoDB"),
    ("ollama", "Ollama"),
    ("docker", "Docker"),
    ("cargo", "Cargo"),
    ("rustc", "Rust"),
    ("node", "Node.js"),
    ("npm", "npm"),
    ("python", "Python"),
    ("java", "Java"),
    ("dotnet", ".NET"),
    ("nginx", "nginx"),
    ("httpd", "Apache"),
];

fn technology(process: &RawProcess) -> Option<String> {
    let name = process.name.to_lowercase();
    let line = format!("{} {}", name, process.cmd.join(" ").to_lowercase());
    TECHNOLOGIES
        .iter()
        .find(|(key, _)| line.contains(key))
        .map(|(_, label)| (*label).to_string())
}

fn is_system(process: &RawProcess) -> bool {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let windir = norm(&windir);
    process.pid <= 4
        || process
            .executable
            .as_deref()
            .map(norm)
            .is_some_and(|e| within(&e, &windir))
        || [
            "system",
            "registry",
            "lsass.exe",
            "svchost.exe",
            "services.exe",
            "wininit.exe",
            "spoolsv.exe",
        ]
        .contains(&process.name.to_lowercase().as_str())
}

fn observation(process: &RawProcess) -> ProcessObservation {
    ProcessObservation {
        pid: process.pid,
        parent_pid: process.parent,
        name: process.name.clone(),
        executable: process.executable.clone(),
        command_line: redact(&process.cmd),
        cwd: process.cwd.clone(),
        started_at: process.start_time.saturating_mul(1000),
        cpu: process.cpu,
        memory: process.memory,
        read_bytes: process.read_bytes,
        written_bytes: process.written_bytes,
    }
}

/// Árvore (raiz + descendentes) em ordem de profundidade, só com `members`.
fn build_tree(
    root: u32,
    members: &HashSet<u32>,
    by_pid: &HashMap<u32, &RawProcess>,
    children: &HashMap<u32, Vec<u32>>,
) -> Vec<TreeNode> {
    let mut out = Vec::new();
    let mut stack = vec![(root, 0u32)];
    let mut seen = HashSet::new();
    while let Some((pid, depth)) = stack.pop() {
        if !seen.insert(pid) || !members.contains(&pid) {
            continue;
        }
        if let Some(p) = by_pid.get(&pid) {
            out.push(TreeNode {
                pid,
                parent_pid: p.parent,
                name: p.name.clone(),
                depth,
                cpu: p.cpu,
                memory: p.memory,
            });
        }
        let mut kids = children.get(&pid).cloned().unwrap_or_default();
        kids.sort_unstable_by(|a, b| b.cmp(a));
        for kid in kids {
            stack.push((kid, depth + 1));
        }
    }
    out
}

fn best(candidates: impl Iterator<Item = Association>) -> Association {
    candidates.max_by_key(|a| a.confidence).unwrap_or_default()
}

// ------------------------------------------------------------------ snapshot (função pura)

pub struct Inputs<'a> {
    pub now_ms: u64,
    pub processes: Vec<RawProcess>,
    pub ports: Vec<PortObservation>,
    pub ctx: &'a Context,
    /// PID → (projeto, execução) das árvores gerenciadas.
    pub managed: HashMap<u32, (String, String)>,
    pub runs: Vec<RunInfo>,
    pub self_pid: u32,
}

pub fn build_snapshot(input: Inputs) -> ControlPlaneSnapshot {
    let Inputs {
        now_ms,
        processes,
        ports,
        ctx,
        managed,
        runs,
        self_pid,
    } = input;
    let by_pid: HashMap<u32, &RawProcess> = processes.iter().map(|p| (p.pid, p)).collect();
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in &processes {
        if let Some(parent) = p.parent.filter(|parent| {
            by_pid
                .get(parent)
                .is_some_and(|pp| pp.start_time <= p.start_time)
        }) {
            children.entry(parent).or_default().push(p.pid);
        }
    }
    let attribution = attribute(&processes, &ports, ctx, &managed);
    let usage = |pids: &[u32]| {
        pids.iter()
            .filter_map(|pid| by_pid.get(pid))
            .fold((0f32, 0u64), |(c, m), p| (c + p.cpu, m + p.memory))
    };

    let mut runtimes: Vec<RuntimeObservation> = Vec::new();
    let mut claimed: HashSet<u32> = HashSet::new();

    // ---- MANAGED
    for run in runs.iter().filter(|r| !r.observer) {
        let pids: Vec<u32> = managed
            .iter()
            .filter(|(_, (_, id))| id == &run.id)
            .map(|(pid, _)| *pid)
            .collect();
        claimed.extend(&pids);
        let members: HashSet<u32> = pids.iter().copied().collect();
        let root = run
            .pid
            .filter(|p| members.contains(p))
            .or_else(|| pids.iter().copied().min());
        let tree = root
            .map(|r| build_tree(r, &members, &by_pid, &children))
            .unwrap_or_default();
        let mut sorted = pids.clone();
        sorted.sort_unstable();
        let own_ports: Vec<PortObservation> = ports
            .iter()
            .filter(|p| p.pid.is_some_and(|pid| members.contains(&pid)))
            .cloned()
            .collect();
        let root_process = root.and_then(|r| by_pid.get(&r));
        let mut association = root
            .and_then(|r| attribution.get(&r).cloned())
            .filter(|a| a.project_id.as_deref() == Some(run.project_id.as_str()))
            .unwrap_or_else(|| {
                // Execução já encerrada (sem processos vivos): o Project vem da própria execução.
                ctx.projects
                    .iter()
                    .find(|p| p.id == run.project_id)
                    .map(|p| {
                        association_for(
                            p,
                            None,
                            Confidence::Exact,
                            Evidence {
                                kind: "managed_execution",
                                detail: format!("execução {}", run.id),
                            },
                        )
                    })
                    .unwrap_or_default()
            });
        // O worktree pode estar em qualquer processo do grupo (ex.: o filho com cwd no worktree).
        if association.worktree_id.is_none() {
            if let Some(a) = pids.iter().filter_map(|p| attribution.get(p)).find(|a| {
                a.worktree_id.is_some() && a.project_id.as_deref() == Some(run.project_id.as_str())
            }) {
                association.worktree_id = a.worktree_id.clone();
                association.worktree_name = a.worktree_name.clone();
                association.session_id = a.session_id.clone();
                association.session_label = a.session_label.clone();
                association.block_id = a.block_id.clone();
                association.block_title = a.block_title.clone();
            }
        }
        let (cpu, memory) = usage(&sorted);
        runtimes.push(RuntimeObservation {
            id: run.id.clone(),
            origin: Origin::Managed,
            category: "dev",
            label: run.label.clone(),
            technology: root_process
                .and_then(|p| technology(p))
                .or_else(|| Some(run.source.to_string())),
            state: run.state,
            root_pid: root,
            pids: sorted,
            ports: own_ports,
            tree,
            command: run.command.clone(),
            cwd: root_process.and_then(|p| p.cwd.clone()),
            started_at: Some(run.started_at),
            cpu,
            memory,
            association,
            console: ConsoleStream {
                available: true,
                run_id: Some(run.id.clone()),
                reason: None,
            },
            execution: Some(ManagedExecution {
                run_id: run.id.clone(),
                command_id: run.command_id.clone(),
                command: run.command.clone(),
                project_id: run.project_id.clone(),
                exit_code: run.exit_code,
                observer: run.observer,
            }),
            is_self: false,
        });
    }

    // ---- DISCOVERED: um runtime por raiz de árvore que possui porta em escuta
    let boundary = |p: &RawProcess| BOUNDARY.contains(&p.name.to_lowercase().as_str());
    // Hospedeiros: pais de processos gerenciados (ex.: o próprio LKR LAB). Um serviço externo que também
    // é filho dele não pode ser agrupado com os gerenciados nem herdar a atribuição Exata deles.
    let hosts: HashSet<u32> = managed
        .keys()
        .filter_map(|pid| by_pid.get(pid).and_then(|p| p.parent))
        .collect();
    let mut roots: HashMap<u32, Vec<&PortObservation>> = HashMap::new();
    let mut orphans: Vec<&PortObservation> = Vec::new();
    for port in &ports {
        let Some(pid) = port.pid else {
            orphans.push(port);
            continue;
        };
        if claimed.contains(&pid) {
            continue;
        }
        let Some(start) = by_pid.get(&pid) else {
            orphans.push(port);
            continue;
        };
        let mut root = start.pid;
        let mut current: &RawProcess = start;
        for _ in 0..12 {
            let Some(parent) = current.parent.and_then(|id| by_pid.get(&id)) else {
                break;
            };
            if boundary(parent)
                || parent.pid <= 4
                || parent.start_time > current.start_time
                || claimed.contains(&parent.pid)
                || hosts.contains(&parent.pid)
            {
                break;
            }
            root = parent.pid;
            current = parent;
        }
        roots.entry(root).or_default().push(port);
    }
    for (root, root_ports) in roots {
        let Some(root_process) = by_pid.get(&root) else {
            continue;
        };
        // Membros: descendentes da raiz (a árvore inteira, não só quem escuta).
        let mut members: HashSet<u32> = HashSet::new();
        let mut queue = vec![root];
        while let Some(pid) = queue.pop() {
            if managed.contains_key(&pid) {
                continue;
            }
            if members.insert(pid) {
                queue.extend(children.get(&pid).cloned().unwrap_or_default());
            }
        }
        let mut pids: Vec<u32> = members.iter().copied().collect();
        pids.sort_unstable();
        let tree = build_tree(root, &members, &by_pid, &children);
        let listener = root_ports
            .first()
            .and_then(|p| p.pid)
            .and_then(|pid| by_pid.get(&pid))
            .copied()
            .unwrap_or(root_process);
        let mut own: Vec<PortObservation> = ports
            .iter()
            .filter(|p| p.pid.is_some_and(|pid| members.contains(&pid)))
            .cloned()
            .collect();
        own.sort_by_key(|p| p.port);
        let association = best(pids.iter().filter_map(|p| attribution.get(p).cloned()));
        let (cpu, memory) = usage(&pids);
        let system_proc = is_system(listener) || is_system(root_process);
        let tech = technology(listener).or_else(|| technology(root_process));
        let category = if system_proc {
            "system"
        } else if tech.is_some() || association.confidence >= Confidence::Medium {
            "dev"
        } else {
            "other"
        };
        let label = tech
            .clone()
            .unwrap_or_else(|| root_process.name.trim_end_matches(".exe").to_string());
        runtimes.push(RuntimeObservation {
            id: format!("pid:{root}:{}", root_process.start_time),
            origin: Origin::Discovered,
            category,
            label,
            technology: tech,
            state: RunState::Running,
            root_pid: Some(root),
            pids,
            ports: own,
            tree,
            command: redact(&root_process.cmd),
            cwd: root_process.cwd.clone(),
            started_at: Some(root_process.start_time.saturating_mul(1000)),
            cpu,
            memory,
            association,
            console: ConsoleStream {
                available: false,
                run_id: None,
                reason: Some(NO_CONSOLE_EXTERNAL.into()),
            },
            execution: None,
            is_self: members.contains(&self_pid),
        });
    }
    for port in orphans {
        runtimes.push(RuntimeObservation {
            id: format!("port:{}:{}", port.address, port.port),
            origin: Origin::Discovered,
            category: "other",
            label: "Processo não identificado".into(),
            technology: None,
            state: RunState::Running,
            root_pid: None,
            pids: vec![],
            ports: vec![port.clone()],
            tree: vec![],
            command: String::new(),
            cwd: None,
            started_at: None,
            cpu: 0.0,
            memory: 0,
            association: Association::default(),
            console: ConsoleStream {
                available: false,
                run_id: None,
                reason: Some(NO_CONSOLE_UNIDENTIFIED.into()),
            },
            execution: None,
            is_self: false,
        });
    }

    runtimes.sort_by(|a, b| {
        let key = |r: &RuntimeObservation| {
            (
                r.origin != Origin::Managed,
                r.category == "system",
                std::cmp::Reverse(r.association.confidence),
                r.ports.first().map(|p| p.port).unwrap_or(u16::MAX),
                r.id.clone(),
            )
        };
        key(a).cmp(&key(b))
    });

    let protected = processes
        .iter()
        .filter(|p| p.executable.is_none() && p.pid > 4)
        .count();
    let mut limitations = Vec::new();
    if protected > 0 {
        limitations.push(Limitation {
            id: "protected_processes",
            detail: format!("{protected} processos protegidos não expõem caminho nem linha de comando sem elevação; eles não podem ser associados a um Project."),
            requires_elevation: true,
        });
    }
    let signals = runs
        .iter()
        .filter(|r| !r.observer && r.state == RunState::Failed)
        .map(|r| HealthSignal {
            id: format!("run-failed:{}", r.id),
            domain: "runtime",
            severity: Severity::Warning,
            message: format!(
                "A execução \"{}\" falhou (código {}).",
                r.command,
                r.exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".into())
            ),
        })
        .collect();

    ControlPlaneSnapshot {
        taken_at: now_ms,
        scope: "local",
        process_total: processes.len(),
        listening_total: ports.len(),
        runtimes,
        signals,
        limitations,
    }
}

// ------------------------------------------------------------------ observação ao vivo

/// Portas TCP em escuta (IPv4 e IPv6) com o PID dono. Só leitura.
pub fn listening_ports() -> HubResult<Vec<PortObservation>> {
    let sockets = get_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP,
    )
    .map_err(|e| format!("Não foi possível enumerar portas: {e}"))?;
    let mut out = Vec::new();
    for socket in sockets {
        let ProtocolSocketInfo::Tcp(t) = socket.protocol_socket_info else {
            continue;
        };
        if t.state != TcpState::Listen {
            continue;
        }
        let version = if t.local_addr.is_ipv6() { "v6" } else { "v4" };
        let pids: Vec<Option<u32>> = if socket.associated_pids.is_empty() {
            vec![None]
        } else {
            socket.associated_pids.iter().map(|p| Some(*p)).collect()
        };
        for pid in pids {
            out.push(PortObservation {
                port: t.local_port,
                address: t.local_addr.to_string(),
                protocol: "TCP",
                ip_version: version,
                pid,
            });
        }
    }
    out.sort_by_key(|p| (p.port, p.ip_version));
    out.dedup_by(|a, b| a.port == b.port && a.address == b.address && a.pid == b.pid);
    Ok(out)
}

/// Observa a máquina agora. `managed`/`runs` vêm do `Supervisor`. Nunca altera nada.
pub fn observe(
    ctx: &Context,
    managed: HashMap<u32, (String, String)>,
    runs: Vec<RunInfo>,
) -> HubResult<ControlPlaneSnapshot> {
    let processes = system::inventory();
    let ports = listening_ports()?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Ok(build_snapshot(Inputs {
        now_ms,
        processes,
        ports,
        ctx,
        managed,
        runs,
        self_pid: std::process::id(),
    }))
}

// ------------------------------------------------------------------ inventário de processos

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    #[serde(flatten)]
    pub process: ProcessObservation,
    pub listening_ports: Vec<u16>,
    pub association: Association,
    pub managed: bool,
    pub is_self: bool,
}

/// Inventário de processos. Por padrão só os RELEVANTES: donos de porta em escuta, processos do
/// LKR LAB, qualquer um com atribuição ≥ Medium e os ancestrais deles (para a árvore fechar).
/// `include_all` devolve a máquina inteira. Só leitura.
pub fn process_inventory(
    processes: Vec<RawProcess>,
    ports: &[PortObservation],
    ctx: &Context,
    managed: &HashMap<u32, (String, String)>,
    include_all: bool,
    self_pid: u32,
) -> Vec<ProcessEntry> {
    let attribution = attribute(&processes, ports, ctx, managed);
    let by_pid: HashMap<u32, &RawProcess> = processes.iter().map(|p| (p.pid, p)).collect();
    let mut keep: HashSet<u32> = HashSet::new();
    for p in &processes {
        let relevant = include_all
            || managed.contains_key(&p.pid)
            || ports.iter().any(|port| port.pid == Some(p.pid))
            || attribution
                .get(&p.pid)
                .is_some_and(|a| a.confidence >= Confidence::Medium);
        if relevant {
            keep.insert(p.pid);
            let mut up = p.parent;
            for _ in 0..8 {
                let Some(parent) = up.and_then(|id| by_pid.get(&id)) else {
                    break;
                };
                if parent.start_time > p.start_time || !keep.insert(parent.pid) {
                    break;
                }
                up = parent.parent;
            }
        }
    }
    let mut out: Vec<ProcessEntry> = processes
        .iter()
        .filter(|p| keep.contains(&p.pid))
        .map(|p| ProcessEntry {
            process: observation(p),
            listening_ports: ports
                .iter()
                .filter(|port| port.pid == Some(p.pid))
                .map(|port| port.port)
                .collect(),
            association: attribution.get(&p.pid).cloned().unwrap_or_default(),
            managed: managed.contains_key(&p.pid),
            is_self: p.pid == self_pid,
        })
        .collect();
    out.sort_by_key(|e| e.process.pid);
    out
}

/// Inventário ao vivo (somente leitura).
pub fn observe_processes(
    ctx: &Context,
    managed: &HashMap<u32, (String, String)>,
    include_all: bool,
) -> HubResult<Vec<ProcessEntry>> {
    let ports = listening_ports()?;
    Ok(process_inventory(
        system::inventory(),
        &ports,
        ctx,
        managed,
        include_all,
        std::process::id(),
    ))
}
