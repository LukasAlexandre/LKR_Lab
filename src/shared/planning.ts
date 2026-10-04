import { normalizeSearch } from "./search";
import type { DdaeSessionStatus, PlanningMove, PlanningPhase, PlanningRow } from "./types";

/*
 * Lógica pura da página Planejamento (Concept 09). O backend entrega cada item já com a fase
 * DERIVADA (planejado / em execução / concluído / cancelado), a Session vinculada e o que pode ser
 * feito; aqui só se filtra, se busca e se rotula. Nada aqui grava, deriva estado nem inventa dados.
 */

export type PlanningFilter = "all" | PlanningPhase;

export const PLANNING_FILTERS: { id: PlanningFilter; label: string }[] = [
  { id: "all", label: "Todos" },
  { id: "planned", label: "Planejados" },
  { id: "executing", label: "Em execução" },
  { id: "completed", label: "Concluídos" },
  { id: "cancelled", label: "Cancelados" },
];

/** Selo principal da fase derivada. */
export const PHASE_BADGE: Record<PlanningPhase, string> = {
  planned: "Planejado",
  executing: "Em execução",
  completed: "Concluído",
  cancelled: "Cancelado",
};

/** Subselo com o estado REAL da Session (a fase "em execução" cobre ativa, congelada e parada). */
export const SESSION_SUBBADGE: Record<DdaeSessionStatus, string> = {
  active: "ATIVA",
  frozen: "CONGELADA",
  stopped: "PARADA",
  completed: "FINALIZADA",
};

/** Subselo exibido ao lado do selo principal; planejado e cancelado não têm Session. */
export function subBadge(row: PlanningRow): { label: string; status: DdaeSessionStatus } | null {
  if (!row.session) return null;
  if (row.phase !== "executing" && row.phase !== "completed") return null;
  return { label: SESSION_SUBBADGE[row.session.status], status: row.session.status };
}

/** Busca por título, descrição e rótulo da Session relacionada (sem acentos nem caixa). */
export function matchesQuery(row: PlanningRow, query: string): boolean {
  const needle = normalizeSearch(query);
  if (!needle) return true;
  const fields = [row.title, row.description ?? ""];
  if (row.session) {
    const padded = String(row.session.number).padStart(3, "0");
    fields.push(row.session.label, `session ${padded}`, padded);
  }
  return fields.some((field) => normalizeSearch(field).includes(needle));
}

/**
 * A ordem da fila (posição) é a do backend e nunca muda por filtro ou busca. "Todos" mostra tudo,
 * inclusive o cancelado (que só fica fora do total operacional do resumo).
 */
export function visibleRows(rows: PlanningRow[], filter: PlanningFilter, query: string): PlanningRow[] {
  return rows.filter((row) => (filter === "all" || row.phase === filter) && matchesQuery(row, query));
}

export const MOVE_LABEL: Record<PlanningMove, string> = {
  up: "Mover para cima",
  down: "Mover para baixo",
  top: "Mover ao topo",
};

/** Ações do menu "…" que fazem sentido para a linha (nunca mostra ação inválida). */
export type RowAction = "edit" | "up" | "down" | "top" | "cancel" | "open" | "restore";

export function rowActions(row: PlanningRow): RowAction[] {
  const actions: RowAction[] = [];
  if (row.phase === "planned") {
    if (row.canEdit) actions.push("edit");
    if (row.canMoveUp) actions.push("up");
    if (row.canMoveDown) actions.push("down");
    if (row.canMoveUp) actions.push("top");
    if (row.canCancel) actions.push("cancel");
  } else if (row.phase === "cancelled") {
    if (row.canRestore) actions.push("restore");
  } else if (row.session) {
    actions.push("open");
  }
  return actions;
}

/** Texto do progresso da Session vinculada ("6 / 9"). */
export const progressText = (row: PlanningRow) =>
  row.session && row.session.progress.total > 0 ? `${row.session.progress.completed} / ${row.session.progress.total}` : "";

/** Fração (0–1) do progresso da Session para a barra. */
export const progressFraction = (row: PlanningRow) =>
  row.session && row.session.progress.total > 0 ? row.session.progress.completed / row.session.progress.total : 0;

/** Origem discreta mostrada no DDAE: só quando a Session nasceu de um item (a SESSION-001 não mostra nada). */
export const originText = (view: { planningItem?: { title: string } | null }): string | null =>
  view.planningItem ? `Origem: Planejamento — ${view.planningItem.title}` : null;

export const HELPER_ACTIVE_SESSION = "Já existe uma Session ativa neste projeto.";
