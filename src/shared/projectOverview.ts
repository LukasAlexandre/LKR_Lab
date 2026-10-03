import { changesLabel } from "./logic";
import type { OverviewTotals, ProjectOverview } from "./types";

/*
 * Lógica pura da página Projetos (Concept 03). O backend entrega cada projeto com
 * disponibilidade, Git, runtime e stack; aqui só se resume, filtra e rotula o que veio.
 * Projeto sem pasta nunca ganha Git/runtime "inventados": as dimensões vêm sem dado.
 */
export type ProjectFilter = "all" | "available" | "unlocated" | "running" | "dirty";
export type ProjectSort = "name" | "recent";

export const FILTERS: { id: ProjectFilter; label: string }[] = [
  { id: "all", label: "Todos" },
  { id: "available", label: "Disponíveis" },
  { id: "unlocated", label: "Não localizados" },
  { id: "running", label: "Em execução" },
  { id: "dirty", label: "Com alterações" },
];

export const isAvailable = (p: ProjectOverview) => p.location === "available";
/** Missing e Unbound aparecem juntos como "Não localizado nesta máquina". */
export const isUnlocated = (p: ProjectOverview) => p.location !== "available";
export const isRunning = (p: ProjectOverview) => p.runtime.status === "available" && p.runtime.data?.running === true;
/** Alterações locais (working tree sujo ou conflitos). Ahead/behind sozinho não conta. */
export const isDirty = (p: ProjectOverview) => {
  const git = p.git.status === "available" ? p.git.data : null;
  return !!git && (git.changes > 0 || git.staged + git.unstaged + git.untracked + git.conflicts > 0);
};

/** Mesma conta do backend, derivada da lista: os números sempre fecham com os cards. */
export function summarize(list: ProjectOverview[]): OverviewTotals {
  return {
    total: list.length,
    available: list.filter(isAvailable).length,
    missing: list.filter((p) => p.location === "missing").length,
    unbound: list.filter((p) => p.location === "unbound").length,
    running: list.filter(isRunning).length,
    dirty: list.filter(isDirty).length,
  };
}

const fold = (text: string) => text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** Busca por nome, descrição e stack (sem caminho local). */
export function matchesSearch(p: ProjectOverview, query: string): boolean {
  const q = fold(query.trim());
  if (!q) return true;
  return fold([p.name, p.description, ...p.stack].join(" ")).includes(q);
}

export function matchesFilter(p: ProjectOverview, filter: ProjectFilter): boolean {
  switch (filter) {
    case "available": return isAvailable(p);
    case "unlocated": return isUnlocated(p);
    case "running": return isRunning(p);
    case "dirty": return isDirty(p);
    default: return true;
  }
}

const activityTime = (p: ProjectOverview) => parseUtc(p.lastActivity)?.getTime() ?? Number.NEGATIVE_INFINITY;

export function sortProjects(list: ProjectOverview[], sort: ProjectSort): ProjectOverview[] {
  const byName = (a: ProjectOverview, b: ProjectOverview) => a.name.localeCompare(b.name, "pt-BR", { sensitivity: "base" });
  return [...list].sort(sort === "recent" ? (a, b) => activityTime(b) - activityTime(a) || byName(a, b) : byName);
}

export function visibleProjects(list: ProjectOverview[], query: string, filter: ProjectFilter, sort: ProjectSort) {
  return sortProjects(list.filter((p) => matchesSearch(p, query) && matchesFilter(p, filter)), sort);
}

/** "sem projetos" (convite a cadastrar) × "nenhum resultado" (busca/filtro sem correspondência). */
export function emptyKind(total: number, shown: number): "no-projects" | "no-results" | null {
  if (total === 0) return "no-projects";
  return shown === 0 ? "no-results" : null;
}

export type Tone = "good" | "warn" | "blue" | "neutral";
export interface Label { text: string; tone: Tone }

export function gitLabel(p: ProjectOverview): Label {
  if (p.git.status === "not_applicable") return { text: "—", tone: "neutral" };
  const git = p.git.data;
  if (p.git.status === "error" || !git) return { text: "Indisponível", tone: "neutral" };
  if (git.conflicts > 0) return { text: `Conflitos (${git.conflicts})`, tone: "warn" };
  if (isDirty(p)) return { text: changesLabel(git.changes || git.staged + git.unstaged + git.untracked), tone: "warn" };
  return { text: "Clean", tone: "good" };
}

/** Só para quem tem repositório consultado: "↑2 ↓1" (vazio se em dia ou sem upstream). */
export function syncLabel(p: ProjectOverview): string {
  const git = p.git.status === "available" ? p.git.data : null;
  if (!git) return "";
  return [git.ahead ? `↑${git.ahead}` : "", git.behind ? `↓${git.behind}` : ""].filter(Boolean).join(" ");
}

export function branchLabel(p: ProjectOverview): string {
  const git = p.git.status === "available" ? p.git.data : null;
  if (!git) return "—";
  return git.detached ? "HEAD destacado" : git.branch || "—";
}

export function runtimeLabel(p: ProjectOverview): Label {
  if (p.runtime.status !== "available" || !p.runtime.data) return { text: "Indisponível", tone: "neutral" };
  return p.runtime.data.running ? { text: "Em execução", tone: "blue" } : { text: "Parado", tone: "neutral" };
}

/** CTA principal do card: disponível abre, o resto localiza. */
export const cardAction = (p: ProjectOverview): "open" | "locate" => (isAvailable(p) ? "open" : "locate");

export function visibleStack(stack: string[], max = 3) {
  return { shown: stack.slice(0, max), extra: Math.max(0, stack.length - max) };
}

/** `created_at` do SQLite chega como "AAAA-MM-DD HH:MM:SS" em UTC (ou ISO com Z). */
export function parseUtc(value: string | null | undefined): Date | null {
  if (!value) return null;
  const iso = /[zZ]|[+-]\d\d:?\d\d$/.test(value) ? value : `${value.replace(" ", "T")}Z`;
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? null : date;
}

/** Tempo relativo real; sem registro ou data inválida = "—" (nunca um "há 2h" inventado). */
export function relativeTime(value: string | null | undefined, now: Date = new Date()): string {
  const date = parseUtc(value);
  if (!date) return "—";
  const seconds = Math.max(0, Math.floor((now.getTime() - date.getTime()) / 1000));
  if (seconds < 60) return "Agora";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `Há ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `Há ${hours} h`;
  const days = Math.floor(hours / 24);
  if (days === 1) return "Ontem";
  if (days < 30) return `Há ${days} dias`;
  return date.toLocaleDateString("pt-BR");
}

/** Caminho para exibição: sem o prefixo verbatim `\?\` que o Windows adiciona ao canonicalizar. */
export const displayPath = (path: string) => path.startsWith("\\\\?\\") ? path.slice(4) : path;
