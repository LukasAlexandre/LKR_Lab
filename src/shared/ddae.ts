import { normalizeSearch } from "./search";
import type { DdaeCounts, DdaeOverview, DdaeReadyForAi, DdaeSessionStatus, DdaeSessionView } from "./types";

/*
 * Lógica pura do DDAE / Sessões (Concept 06). Só o que o backend já entrega: o estado da Session
 * e o que dele deriva (progresso, bloco atual, próximo). Nada de percentual gravado nem dado inventado.
 */

export type DdaeFilter = "all" | DdaeSessionStatus;

export const STATUS_LABEL: Record<DdaeSessionStatus, string> = {
  active: "Ativa",
  frozen: "Congelada",
  stopped: "Parada",
  completed: "Finalizada",
};
/** Rótulo do status em caixa alta, como nos badges do Concept 06. */
export const statusBadge = (status: DdaeSessionStatus) => STATUS_LABEL[status].toUpperCase();

export const FILTERS: { id: DdaeFilter; label: string }[] = [
  { id: "all", label: "Todas" },
  { id: "active", label: "Ativa" },
  { id: "frozen", label: "Congelada" },
  { id: "stopped", label: "Parada" },
  { id: "completed", label: "Finalizada" },
];

/** Contadores sobre TODAS as sessões (a busca não os altera): a soma por estado fecha com o total. */
export function filterCounts(sessions: DdaeSessionView[]): Record<DdaeFilter, number> {
  const counts: Record<DdaeFilter, number> = { all: sessions.length, active: 0, frozen: 0, stopped: 0, completed: 0 };
  for (const session of sessions) counts[session.status] += 1;
  return counts;
}

/** Busca por SESSION-NNN, título e objetivo (sem acentos nem caixa). */
export function matchesQuery(session: DdaeSessionView, query: string): boolean {
  const needle = normalizeSearch(query);
  if (!needle) return true;
  const padded = String(session.number).padStart(3, "0");
  return [session.label, `session ${padded}`, padded, session.title, session.objective].some((field) => normalizeSearch(field).includes(needle));
}

export function visibleSessions(sessions: DdaeSessionView[], filter: DdaeFilter, query: string): DdaeSessionView[] {
  return sessions.filter((s) => (filter === "all" || s.status === filter) && matchesQuery(s, query));
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "1 ativa · 1 congelada · 1 parada · 1 finalizada" (só os estados que existem). */
export function breakdownText(counts: DdaeCounts): string {
  const parts = [
    counts.active ? plural(counts.active, "ativa", "ativas") : "",
    counts.frozen ? plural(counts.frozen, "congelada", "congeladas") : "",
    counts.stopped ? plural(counts.stopped, "parada", "paradas") : "",
    counts.completed ? plural(counts.completed, "finalizada", "finalizadas") : "",
  ].filter(Boolean);
  return parts.join(" · ") || "Nenhuma sessão";
}

export const progressText = (s: DdaeSessionView) => (s.progress.total ? `${s.progress.completed} / ${s.progress.total} blocos` : "Sem blocos");
/** Só para o anel/barra: fração derivada (completed / total), nunca guardada. */
export const progressFraction = (s: DdaeSessionView) => (s.progress.total ? s.progress.completed / s.progress.total : 0);

/**
 * A linha que explica POR QUE a sessão está no estado atual (regra 3 do Concept 06):
 * motivo do congelamento/parada, próximo passo da parada ou resultado da finalizada.
 */
export function contextLine(s: DdaeSessionView): string | null {
  if (s.status === "frozen") return s.pauseReason ?? null;
  if (s.status === "stopped") {
    const parts = [s.pauseReason, s.nextBlock ? `Próximo: ${s.nextBlock.title}` : ""].filter(Boolean);
    return parts.join(" · ") || null;
  }
  if (s.status === "completed") return s.result ? `Resultado: ${s.result}` : null;
  return null;
}

export interface DdaeSummary {
  total: number;
  breakdown: string;
  active: DdaeSessionView | null;
  blocksTotal: number;
  /** "x / y na ativa" */
  blocksInActive: string | null;
  nextBlock: string | null;
}

export function summarize(overview: DdaeOverview): DdaeSummary {
  const active = overview.sessions.find((s) => s.id === overview.activeSessionId) ?? null;
  return {
    total: overview.counts.total,
    breakdown: breakdownText(overview.counts),
    active,
    blocksTotal: overview.blocksTotal,
    blocksInActive: active ? `${active.progress.completed} / ${active.progress.total} na ativa` : null,
    nextBlock: active?.nextBlock?.title ?? null,
  };
}

export interface DdaeNextAction {
  kind: "continue" | "start";
  sessionId: string;
  label: string;
  title: string;
  description: string;
  blockId: string;
}

/**
 * Próxima ação do DDAE (sem IA, sem inferência por chat):
 *  - sessão ativa + bloco em andamento → Continuar <bloco atual>
 *  - sessão ativa sem bloco atual + próximo pendente → Iniciar <próximo bloco>
 */
export function deriveDdaeNextAction(overview: DdaeOverview | null | undefined): DdaeNextAction | null {
  const active = overview?.sessions.find((s) => s.id === overview.activeSessionId);
  if (!active) return null;
  if (active.currentBlock) {
    return {
      kind: "continue",
      sessionId: active.id,
      label: active.label,
      title: `Continuar ${active.currentBlock.title}`,
      description: active.nextBlock
        ? `Concluir ${active.currentBlock.title} e avançar para: ${active.nextBlock.title}`
        : `Concluir ${active.currentBlock.title}; não há blocos pendentes depois dele.`,
      blockId: active.currentBlock.id,
    };
  }
  if (active.nextBlock) {
    return {
      kind: "start",
      sessionId: active.id,
      label: active.label,
      title: `Iniciar ${active.nextBlock.title}`,
      description: `${active.label} não tem bloco em andamento. O próximo bloco pendente é ${active.nextBlock.title}.`,
      blockId: active.nextBlock.id,
    };
  }
  return null;
}

const MISSING_LABEL: Record<DdaeReadyForAi["missing"][number], string> = {
  objective: "objetivo",
  desired_outcome: "resultado desejado",
  blocks: "blocos",
  criteria: "critérios de conclusão",
  actionable_block: "bloco atual ou pendente",
};

/** Texto discreto do Contexto IA no preview: derivado do backend, sem checkbox manual. */
export function contextStatus(ready: DdaeReadyForAi): { label: string; tone: "ready" | "incomplete" | "available"; detail: string } {
  if (ready.state === "ready") return { label: "Pronto", tone: "ready", detail: "A sessão tem o necessário para um agente continuá-la." };
  if (ready.state === "available") return { label: "Contexto disponível", tone: "available", detail: "Sessão finalizada: o contexto pode ser gerado." };
  return {
    label: "Contexto incompleto",
    tone: "incomplete",
    detail: ready.missing.length ? `Falta: ${ready.missing.map((m) => MISSING_LABEL[m]).join(", ")}.` : "Faltam informações.",
  };
}
