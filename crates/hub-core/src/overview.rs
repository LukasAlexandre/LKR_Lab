//! Visão agregada dos projetos para a página Projetos (Concept 03).
//!
//! UMA operação de backend monta tudo: o frontend nunca consulta Git/runtime projeto a projeto.
//! Somente leitura: `git status` (sem fetch/pull/checkout), leitura de manifests e a foto de
//! processos/portas. Nada é executado e nada é gravado.
//!
//! Regras:
//! * disponibilidade (`Location`) é recalculada a cada chamada e nunca persistida;
//! * `Missing`/`Unbound` NÃO tocam Git, stack nem runtime (não há pasta para consultar) e saem
//!   com dimensões `not_applicable`, nunca com um "Clean"/"Parado" inventado;
//! * cada dimensão (Git, runtime, stack) falha sozinha: um projeto com erro vira diagnóstico
//!   localizado e a lista continua inteira;
//! * a coleta de Git roda com paralelismo limitado (`MAX_WORKERS`) para não abrir dezenas de
//!   subprocessos de uma vez;
//! * "em execução" = execução gerenciada ativa (não-tarefa, não-observador) OU porta TCP em escuta
//!   atribuída ao projeto. Um shell aberto na pasta NÃO conta, nem o Compose (que exigiria Docker).
use crate::{
    git::{self, GitSummary},
    models::{Location, Project, ProjectEntry},
    ports::{self, PortInfo},
    projects, runtime,
    supervisor::RunInfo,
    system,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Teto de projetos enriquecidos ao mesmo tempo (cada um pode abrir um `git status`).
pub const MAX_WORKERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionStatus {
    /// Consultado com sucesso.
    Available,
    /// Não existe para este projeto aqui (sem pasta, sem Git).
    NotApplicable,
    /// A consulta falhou; `message` explica. Não derruba a lista.
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimension<T> {
    pub status: DimensionStatus,
    pub data: Option<T>,
    pub message: Option<String>,
}

impl<T> Dimension<T> {
    pub fn available(data: T) -> Self {
        Self {
            status: DimensionStatus::Available,
            data: Some(data),
            message: None,
        }
    }
    pub fn not_applicable(message: impl Into<String>) -> Self {
        Self {
            status: DimensionStatus::NotApplicable,
            data: None,
            message: Some(message.into()),
        }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            status: DimensionStatus::Error,
            data: None,
            message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSummary {
    pub running: bool,
    /// Execuções iniciadas pelo LKR LAB ainda ativas (não conta tarefas nem observadores).
    pub managed_runs: u32,
    /// Portas TCP em escuta atribuídas ao projeto.
    pub listening_ports: Vec<u16>,
}

/// De onde vem a stack exibida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StackSource {
    /// Detecção canônica (`runtime::detect`) da pasta desta máquina.
    Detected,
    /// Stack gravada no cadastro portátil (projeto sem pasta aqui): informação do cadastro.
    Registered,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverview {
    #[serde(flatten)]
    pub entry: ProjectEntry,
    pub git: Dimension<GitSummary>,
    pub runtime: Dimension<RuntimeSummary>,
    pub stack: Vec<String>,
    pub stack_source: StackSource,
    /// Última ação registrada pelo LKR LAB para o projeto (tabela `activities`, UTC).
    pub last_activity: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewTotals {
    pub total: usize,
    pub available: usize,
    pub missing: usize,
    pub unbound: usize,
    pub running: usize,
    /// Alterações locais: working tree sujo (staged/unstaged/untracked) ou conflitos.
    /// Ahead/behind sozinho NÃO conta.
    pub dirty: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsOverview {
    pub projects: Vec<ProjectOverview>,
    pub totals: OverviewTotals,
}

pub fn is_dirty(git: &GitSummary) -> bool {
    git.changes > 0 || git.staged + git.unstaged + git.untracked + git.conflicts > 0
}

pub fn totals(projects: &[ProjectOverview]) -> OverviewTotals {
    let mut t = OverviewTotals {
        total: projects.len(),
        ..Default::default()
    };
    for p in projects {
        match p.entry.location {
            Location::Available => t.available += 1,
            Location::Missing => t.missing += 1,
            Location::Unbound => t.unbound += 1,
        }
        if p.runtime.data.as_ref().is_some_and(|r| r.running) {
            t.running += 1;
        }
        if p.git.data.as_ref().is_some_and(is_dirty) {
            t.dirty += 1;
        }
    }
    t
}

/// Fotografia global de processos/portas, tirada UMA vez para todos os projetos.
#[derive(Debug, Clone, Default)]
pub struct LiveFacts {
    /// project_id → runtime. Projeto ausente = nada em execução.
    pub by_project: HashMap<String, RuntimeSummary>,
}

/// Tudo que a montagem consulta, injetável para testar sem Git/processos reais.
pub struct Sources<'a> {
    pub git: &'a (dyn Fn(&Path) -> Dimension<GitSummary> + Sync),
    pub stack: &'a (dyn Fn(&Path) -> Vec<String> + Sync),
    /// `Err` = a foto de processos/portas falhou (todos os projetos disponíveis ficam com
    /// runtime `error`, o resto da lista continua).
    pub live: Result<LiveFacts, String>,
    pub last_activity: HashMap<String, String>,
}

fn enrich(project: Project, sources: &Sources<'_>, root: Option<&Path>) -> ProjectOverview {
    let last_activity = sources.last_activity.get(&project.id).cloned();
    let entry = projects::entry(project);
    let Some(root) = root else {
        let why = match entry.location {
            Location::Missing => "A pasta vinculada não existe mais nesta máquina.",
            _ => "Projeto ainda não localizado nesta máquina.",
        };
        return ProjectOverview {
            git: Dimension::not_applicable(why),
            runtime: Dimension::not_applicable(why),
            stack: entry.project.stack.clone(),
            stack_source: StackSource::Registered,
            last_activity,
            entry,
        };
    };
    let git = guarded("Git", || (sources.git)(root));
    let stack = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (sources.stack)(root)))
        .unwrap_or_default();
    let runtime = match &sources.live {
        Ok(live) => Dimension::available(
            live.by_project
                .get(&entry.project.id)
                .cloned()
                .unwrap_or_default(),
        ),
        Err(error) => Dimension::error(error.clone()),
    };
    ProjectOverview {
        git,
        runtime,
        stack,
        stack_source: StackSource::Detected,
        last_activity,
        entry,
    }
}

/// Um pânico numa dimensão vira `error` daquela dimensão, nunca derruba a lista.
fn guarded<T>(label: &str, f: impl FnOnce() -> Dimension<T>) -> Dimension<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Dimension::error(format!("{label}: falha inesperada na consulta.")))
}

/// Monta a visão. A ordem da lista de entrada é preservada.
pub fn build(projects: Vec<Project>, sources: &Sources<'_>, workers: usize) -> ProjectsOverview {
    let slots: Vec<(Project, Option<std::path::PathBuf>)> = projects
        .into_iter()
        .map(|p| {
            let root = (projects::location(&p) == Location::Available)
                .then(|| std::path::PathBuf::from(&p.local_path));
            (p, root)
        })
        .collect();
    let workers = workers.clamp(1, MAX_WORKERS).min(slots.len().max(1));
    let next = AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<ProjectOverview>>> =
        slots.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((project, root)) = slots.get(i) else {
                    break;
                };
                let overview = enrich(project.clone(), sources, root.as_deref());
                if let Ok(mut slot) = results[i].lock() {
                    *slot = Some(overview);
                }
            });
        }
    });
    let projects: Vec<ProjectOverview> = results
        .into_iter()
        .zip(slots)
        .map(|(slot, (project, _))| {
            slot.into_inner().ok().flatten().unwrap_or_else(|| {
                // Só acontece se a thread morreu antes de gravar: degrada, não derruba.
                let message = "Falha inesperada ao consultar este projeto.";
                ProjectOverview {
                    git: Dimension::error(message),
                    runtime: Dimension::error(message),
                    stack: project.stack.clone(),
                    stack_source: StackSource::Registered,
                    last_activity: None,
                    entry: projects::entry(project),
                }
            })
        })
        .collect();
    ProjectsOverview {
        totals: totals(&projects),
        projects,
    }
}

/// Há `.git` (pasta ou arquivo de worktree) na pasta ou acima dela?
pub fn inside_git_repo(path: &Path) -> bool {
    path.ancestors().any(|dir| dir.join(".git").exists())
}

pub fn real_git(path: &Path) -> Dimension<GitSummary> {
    if !inside_git_repo(path) {
        return Dimension::not_applicable("A pasta não é um repositório Git.");
    }
    let summary = git::summary(path);
    match summary.error.clone() {
        Some(error) => Dimension::error(error),
        None => Dimension::available(summary),
    }
}

pub fn real_stack(path: &Path) -> Vec<String> {
    runtime::detect(path)
        .stack
        .iter()
        .map(|s| s.label.to_string())
        .collect()
}

/// Deriva o runtime de cada projeto da foto global de processos e portas.
pub fn live_facts(
    all: &[Project],
    runs: &HashMap<String, Vec<RunInfo>>,
    managed: &dyn Fn() -> HashMap<u32, String>,
) -> Result<LiveFacts, String> {
    let processes = system::processes_managed_with(all, managed);
    let ports: Vec<PortInfo> =
        ports::inspect_with_processes(all, &processes).map_err(|e| format!("Portas: {e}"))?;
    Ok(facts_from(all, runs, &ports))
}

/// Parte pura de `live_facts` (testável sem sockets).
pub fn facts_from(
    all: &[Project],
    runs: &HashMap<String, Vec<RunInfo>>,
    ports: &[PortInfo],
) -> LiveFacts {
    let mut by_project = HashMap::new();
    for project in all {
        let managed_runs = runs
            .get(&project.id)
            .map(|list| {
                list.iter()
                    .filter(|r| {
                        r.state.is_active() && r.kind != runtime::ScriptKind::Task && !r.observer
                    })
                    .count() as u32
            })
            .unwrap_or(0);
        let mut listening: Vec<u16> = ports
            .iter()
            .filter(|p| p.protocol == "TCP" && p.project_id.as_deref() == Some(project.id.as_str()))
            .map(|p| p.port)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        listening.sort_unstable();
        by_project.insert(
            project.id.clone(),
            RuntimeSummary {
                running: managed_runs > 0 || !listening.is_empty(),
                managed_runs,
                listening_ports: listening,
            },
        );
    }
    LiveFacts { by_project }
}

/// Operação completa com as fontes reais.
pub fn collect(
    all: Vec<Project>,
    last_activity: HashMap<String, String>,
    runs: HashMap<String, Vec<RunInfo>>,
    managed: &dyn Fn() -> HashMap<u32, String>,
) -> ProjectsOverview {
    // A foto de processos só é necessária se algum projeto tem pasta aqui.
    let any_local = all
        .iter()
        .any(|p| projects::location(p) == Location::Available);
    let live = if any_local {
        live_facts(&all, &runs, managed)
    } else {
        Ok(LiveFacts::default())
    };
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    build(
        all.clone(),
        &Sources {
            git: &real_git,
            stack: &real_stack,
            live,
            last_activity,
        },
        workers,
    )
}
