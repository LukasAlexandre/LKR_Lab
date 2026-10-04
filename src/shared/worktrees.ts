import { normalizeSearch } from "./search";
import type { GitSummary, WorktreeItem, WorktreeKind, WorktreeStatus } from "./types";

/*
 * Lógica pura da página Worktrees (Concept 08). O backend entrega cada item já classificado
 * (principal / gerenciado / não localizado / não gerenciado) e com o Git real; aqui só se filtra,
 * se rotula e se explica. O ESTADO OPERACIONAL é metadata do LKR LAB: nada aqui o deriva de
 * Git, de runtime ou da Session.
 */

export const STATUS_LABEL: Record<WorktreeStatus, string> = {
  active: "Ativo",
  frozen: "Congelado",
  stopped: "Parado",
  completed: "Finalizado",
};
/** ATIVO, CONGELADO, PARADO, FINALIZADO. */
export const statusBadge = (status: WorktreeStatus) => STATUS_LABEL[status].toUpperCase();

/** Rótulo do tipo de item (não é estado operacional). */
export const KIND_BADGE: Partial<Record<WorktreeKind, string>> = {
  primary: "PRINCIPAL",
  unmanaged: "NÃO GERENCIADO",
  managed_missing: "NÃO LOCALIZADO",
};

export type OperationalFilter = "all" | WorktreeStatus | "missing" | "unmanaged";
export type GitFilter = "all" | "clean" | "changes" | "ahead" | "behind" | "conflicts";

export const OPERATIONAL_FILTERS: { id: OperationalFilter; label: string }[] = [
  { id: "all", label: "Todos" },
  { id: "active", label: "Ativos" },
  { id: "frozen", label: "Congelados" },
  { id: "stopped", label: "Parados" },
  { id: "completed", label: "Finalizados" },
  { id: "missing", label: "Não localizados" },
  { id: "unmanaged", label: "Não gerenciados" },
];

export const GIT_FILTERS: { id: GitFilter; label: string }[] = [
  { id: "all", label: "Todos" },
  { id: "clean", label: "Clean" },
  { id: "changes", label: "Alterações" },
  { id: "ahead", label: "Ahead" },
  { id: "behind", label: "Behind" },
  { id: "conflicts", label: "Conflitos" },
];

/** O Git lido do path (somente quando o Git foi consultado e respondeu). */
export const gitFacts = (item: WorktreeItem): GitSummary | null =>
  item.gitSummary?.status === "available" ? item.gitSummary.data : null;

export const changeCount = (g: GitSummary) => g.changes || g.staged + g.unstaged + g.untracked;
export const isDirty = (g: GitSummary | null) => !!g && (changeCount(g) > 0 || g.conflicts > 0);

/** Nome exibido: o persistido, senão a branch, senão a pasta (nunca o path inteiro). */
export function itemName(item: WorktreeItem): string {
  if (item.managed?.displayName) return item.managed.displayName;
  if (item.git?.branch) return item.git.branch;
  if (item.git?.detached) return folderName(item.git.path) || "Detached HEAD";
  return folderName(item.git?.path ?? "") || "Worktree";
}

export const folderName = (path: string) => path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? "";

/** Branch real do Git; sem Git (não localizado) cai na dica portátil. */
export const itemBranch = (item: WorktreeItem): string => item.git?.branch || item.managed?.branchHint || "";

export function matchesGit(item: WorktreeItem, filter: GitFilter): boolean {
  if (filter === "all") return true;
  const g = gitFacts(item);
  if (!g) return false; // sem Git lido, nenhum filtro Git acusa o item
  switch (filter) {
    case "clean": return !isDirty(g);
    case "changes": return changeCount(g) > 0;
    case "ahead": return (g.ahead ?? 0) > 0;
    case "behind": return (g.behind ?? 0) > 0;
    case "conflicts": return g.conflicts > 0;
  }
}

export function matchesOperational(item: WorktreeItem, filter: OperationalFilter): boolean {
  if (filter === "all") return true;
  if (filter === "missing") return item.kind === "managed_missing";
  if (filter === "unmanaged") return item.kind === "unmanaged";
  return item.managed?.status === filter; // o principal não tem estado operacional
}

/** Busca por nome, branch, Session e Block (sem acento nem caixa). */
export function matchesQuery(item: WorktreeItem, query: string): boolean {
  const needle = normalizeSearch(query);
  if (!needle) return true;
  const fields = [
    itemName(item),
    itemBranch(item),
    item.managed?.description ?? "",
    item.managed?.session?.label ?? "",
    item.managed?.session?.title ?? "",
    item.managed?.block?.title ?? "",
  ];
  return fields.some((f) => normalizeSearch(f).includes(needle));
}

export interface Filters {
  query: string;
  operational: OperationalFilter;
  git: GitFilter;
}

/** Os itens do grid (o principal fica numa seção própria e fora do ciclo operacional). */
export function visibleItems(items: WorktreeItem[], f: Filters): WorktreeItem[] {
  return items.filter((i) => i.kind !== "primary" && matchesOperational(i, f.operational) && matchesGit(i, f.git) && matchesQuery(i, f.query));
}

/** O principal só some com filtro operacional específico; busca e Git também se aplicam a ele. */
export function showPrimary(item: WorktreeItem | undefined, f: Filters): boolean {
  return !!item && f.operational === "all" && matchesGit(item, f.git) && matchesQuery(item, f.query);
}

export type StateAction = "freeze" | "stop" | "resume" | "complete";

/** Transições permitidas (espelha o core): COMPLETED é terminal. */
export function stateActions(status: WorktreeStatus): StateAction[] {
  switch (status) {
    case "active": return ["freeze", "stop", "complete"];
    case "frozen":
    case "stopped": return ["resume", "complete"];
    default: return [];
  }
}

export interface GitWarning {
  level: "none" | "changes" | "conflicts";
  text: string;
}

/**
 * Aviso ao FINALIZAR (nunca bloqueia): Git e estado operacional são independentes e finalizar no
 * LKR LAB não faz commit, merge, push nem remoção.
 */
export function finalizeGitWarning(g: GitSummary | null): GitWarning {
  if (!g) return { level: "none", text: "" };
  if (g.conflicts > 0) {
    return { level: "conflicts", text: `Esta worktree possui conflitos Git (${g.conflicts}). Finalizar no LKR LAB não resolve conflitos nem fará commit, merge, push ou remoção.` };
  }
  if (changeCount(g) > 0 || (g.ahead ?? 0) > 0) {
    return { level: "changes", text: "Esta worktree possui alterações Git. Finalizar no LKR LAB não fará commit, merge, push ou remoção." };
  }
  return { level: "none", text: "" };
}

/** Aviso ao CONGELAR/PARAR com alterações: permitido, só informa. */
export const pauseGitWarning = (g: GitSummary | null) => (isDirty(g) ? "Há alterações Git nesta worktree; elas continuam como estão." : "");

export const WARNING_TEXT: Record<WorktreeItem["warnings"][number], string> = {
  session_completed_worktree_open: "A Session vinculada está finalizada, mas este worktree continua aberto.",
  worktree_completed_session_active: "Este worktree está finalizado, mas a Session vinculada continua ativa.",
};

export function gitText(item: WorktreeItem): { text: string; tone: "good" | "warn" | "neutral" } {
  if (item.kind === "managed_missing" || !item.gitSummary) return { text: "—", tone: "neutral" };
  const g = gitFacts(item);
  if (!g) return { text: item.gitSummary.status === "not_applicable" ? "—" : "Indisponível", tone: "neutral" };
  if (g.conflicts > 0) return { text: `Conflitos (${g.conflicts})`, tone: "warn" };
  const n = changeCount(g);
  if (n > 0) return { text: `${n} ${n === 1 ? "alteração" : "alterações"}`, tone: "warn" };
  return { text: "Clean", tone: "good" };
}

/** "↑2 ↓1" quando há upstream; vazio caso contrário. */
export function syncText(g: GitSummary | null): string {
  if (!g) return "";
  return [g.ahead ? `↑${g.ahead}` : "", g.behind ? `↓${g.behind}` : ""].filter(Boolean).join(" ");
}

/**
 * Pasta irmã sugerida para um novo worktree (LOCAL; o usuário confirma e nada disso é portátil).
 * `C:\Dev\LKR_Lab` + `feature/x` → `C:\Dev\LKR_Lab-feature-x`.
 */
export function suggestPath(projectPath: string, branch: string): string {
  const clean = projectPath.replace(/^\\\\\?\\/, "").replace(/[\\/]+$/, "");
  if (!clean || !branch.trim()) return "";
  const sep = clean.includes("\\") ? "\\" : "/";
  const slug = branch.trim().replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "");
  return slug ? `${clean}-${slug}` : clean + sep;
}

export interface CreateForm {
  mode: "new_branch" | "existing_branch";
  branch: string;
  baseRef: string;
  path: string;
  blockId: string;
  sessionId: string;
}

/** Checagem leve do formulário (o backend e o Git validam de verdade). */
export function createProblem(f: CreateForm): string | null {
  if (!f.branch.trim()) return "Informe a branch.";
  if (f.branch.trim().startsWith("-")) return "Nome de branch inválido.";
  if (f.mode === "new_branch" && !f.baseRef.trim()) return "Informe a base da nova branch (por padrão, HEAD).";
  if (!f.path.trim()) return "Informe o destino local.";
  if (!/^(?:[A-Za-z]:[\\/]|[\\/]{2}|\/)/.test(f.path.trim())) return "O destino precisa ser um caminho absoluto desta máquina.";
  if (f.blockId && !f.sessionId) return "Um bloco só pode ser vinculado junto com a Session.";
  return null;
}
