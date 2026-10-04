import { useSyncExternalStore } from "react";
import { api, desktop } from "../shared/api";
import { preferences } from "../shared/preferences";
import type { ControlPlaneSnapshot, Activity, AgentContext, DdaeOverview, PlanningOverview, PlanningSummary, WorktreeCounts, WorktreeOverview, ProjectsOverview, ProjectRuntime, AgentProviderStatus, GitState, HostingState, PortInfo, ProcessInfo, Project, Prompt, SystemState, Worktree } from "../shared/types";
import { createResource, type Resource } from "./resource";
import type { KnowledgeEntry } from "../shared/types";
import { trackOperation } from "./operations";

export function useResource<T>(resource: Resource<T>) {
  return useSyncExternalStore(resource.subscribe, resource.getSnapshot, resource.getSnapshot);
}

const projects = createResource<Project[]>([], () => api("list_projects"));
// Página Projetos: UMA chamada agregada (disponibilidade + Git + runtime + stack), nunca N consultas.
const overviews = createResource<ProjectsOverview | null>(null, () => trackOperation("Consultando projetos", () => api("project_overviews")));
const ports = createResource<PortInfo[]>([], () => trackOperation("Consultando portas", () => api("list_ports")));
const processes = createResource<ProcessInfo[]>([], () => trackOperation("Consultando processos", () => api("list_processes")));
// Control Plane (SESSION-002): leitura LOCAL e passiva da máquina (processos, portas, runtimes).
const controlPlane = createResource<ControlPlaneSnapshot | null>(null, () => api("control_plane_snapshot"));
const environment = createResource<SystemState | null>(null, () => api("system_state"));
const activities = createResource<Activity[]>([], () => api("list_activities"));
const prompts = createResource<Prompt[]>([], () => api("list_prompts"));
const agentProviders = createResource<AgentProviderStatus[]>([], () => api("agent_providers"));
const knowledge = createResource<KnowledgeEntry[]>([], () => api("list_knowledge"));

function projectSources(id: string) {
  return {
    git: createResource<GitState | null>(null, () => trackOperation("Consultando Git", () => api("git_state", { id }), id)),
    hosting: createResource<HostingState | null>(null, () => trackOperation("Consultando GitHub", () => api("github_state", { id }), id)),
    agents: createResource<AgentContext | null>(null, () => api("agent_context", { id })),
    runtime: createResource<ProjectRuntime | null>(null, () => api("project_runtime", { id })),
    worktrees: createResource<Worktree[]>([], () => trackOperation("Consultando worktrees", () => api("list_worktrees", { id }), id)),
    // Worktrees (Concept 08): leitura agregada READ-ONLY (git worktree list + git status) e só as contagens.
    worktreeOverview: createResource<WorktreeOverview | null>(null, () => trackOperation("Consultando worktrees", () => api("project_worktree_overview", { id }), id)),
    worktreeSummary: createResource<WorktreeCounts | null>(null, () => api("worktree_summary", { id })),
    // DDAE: lista, derivados e importação idempotente da SESSION-001 histórica (backend).
    ddae: createResource<DdaeOverview | null>(null, () => api("ddae_overview", { projectId: id })),
    // Planejamento (Concept 09): leitura PASSIVA da fila (fase derivada no backend) e só o resumo.
    planning: createResource<PlanningOverview | null>(null, () => api("planning_overview", { projectId: id })),
    planningSummary: createResource<PlanningSummary | null>(null, () => api("planning_summary", { projectId: id })),
  };
}
const repositories = new Map<string, ReturnType<typeof projectSources>>();
function forProject(id: string) {
  let sources = repositories.get(id);
  if (!sources) { sources = projectSources(id); repositories.set(id, sources); }
  return sources;
}

// Projeto ativo: preferência desta máquina (o id vem do SQLite local).
const selectProject = (id: string) => preferences.set({ activeProjectId: id });
const activeId = () => preferences.get().activeProjectId;
export function useActiveProjectId() {
  return useSyncExternalStore(preferences.subscribe, activeId);
}

async function loadRegistry() {
  if (!desktop) return;
  await Promise.all([projects.refresh(), activities.refresh(), prompts.refresh()]);
  const snapshot = projects.getSnapshot();
  if (snapshot.status === "ready" && !snapshot.data.some((project) => project.id === activeId()))
    selectProject(snapshot.data[0]?.id ?? "");
}

// Two concurrent Git subprocess groups at most; never block project selection.
async function refreshRepositories(maxAgeMs = 30_000) {
  const queue = [...projects.getSnapshot().data];
  await Promise.all(Array.from({ length: 2 }, async () => {
    while (queue.length) {
      const project = queue.shift();
      if (project) await forProject(project.id).git.refresh(maxAgeMs);
    }
  }));
}

/** Relê o DDAE e o Planejamento de todos os Projects já carregados (depois de um sync que aplicou o workspace). */
async function refreshDdae() {
  const loaded = (resource: { getSnapshot: () => { lastUpdated: number | null }; refresh: () => Promise<unknown> }) =>
    resource.getSnapshot().lastUpdated === null ? Promise.resolve() : resource.refresh();
  await Promise.all([...repositories.values()].flatMap((sources) => [loaded(sources.ddae), loaded(sources.planning), loaded(sources.planningSummary)]));
}

export const workspace = {
  projects, overviews, ports, processes, controlPlane, environment, activities, prompts, agentProviders, knowledge, forProject,
  selectProject, loadRegistry, refreshRepositories, refreshDdae,
  async refreshEnvironment() {
    if (!desktop) return;
    const id = activeId();
    await Promise.all([
      ports.refresh(), processes.refresh(), environment.refresh(),
      // Atualizar também refaz o runtime (Git, scripts, processos, portas) do projeto ativo.
      id ? forProject(id).runtime.refresh() : Promise.resolve(),
    ]);
  },
};
