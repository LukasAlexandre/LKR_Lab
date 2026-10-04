import type { DdaeBlock, DdaeCriterion, DdaeEvent, DdaeReference, DdaeSessionView } from "./types";

/*
 * Lógica pura do Detalhe da Session (Concept 07). Só projeta o que o backend já entrega
 * (SessionView + eventos); regras de verdade (invariantes, finalização) vivem no hub-core.
 */

export type DetailTab = "overview" | "blocks" | "plan" | "decisions" | "files" | "notes";

/** "Plano da sessão" (e não "Planejamento"): não colide com o Concept 09 (Planejamento do Project). */
export const DETAIL_TABS: { id: DetailTab; label: string }[] = [
  { id: "overview", label: "Visão geral" },
  { id: "blocks", label: "Blocos" },
  { id: "plan", label: "Plano da sessão" },
  { id: "decisions", label: "Decisões" },
  { id: "files", label: "Arquivos" },
  { id: "notes", label: "Anotações" },
];

const BLOCKER_TEXT: Record<DdaeSessionView["completionBlockers"][number], string> = {
  no_blocks: "A sessão ainda não tem blocos.",
  block_in_progress: "Há um bloco em andamento.",
  blocks_pending: "Ainda há blocos pendentes.",
  criteria_pending: "Há critérios de conclusão pendentes.",
};

/** Finalizar é SEMPRE ação explícita; aqui só se explica se é possível e o que falta. */
export function finalizeStatus(view: DdaeSessionView): { eligible: boolean; reasons: string[] } {
  if (view.status === "completed") return { eligible: false, reasons: ["A sessão já está finalizada."] };
  if (view.status !== "active") return { eligible: false, reasons: ["Retome a sessão para finalizá-la."] };
  return { eligible: view.canComplete, reasons: view.completionBlockers.map((b) => BLOCKER_TEXT[b]) };
}

export type LifecycleAction = "freeze" | "stop" | "resume" | "complete";

/** Ações de estado válidas: ACTIVE congela/para/finaliza; FROZEN/STOPPED retomam; COMPLETED não volta. */
export function lifecycleActions(view: DdaeSessionView): LifecycleAction[] {
  switch (view.status) {
    case "active": return ["freeze", "stop", "complete"];
    case "frozen":
    case "stopped": return ["resume"];
    default: return [];
  }
}

export interface BlockActions {
  start: boolean;
  complete: boolean;
  rename: boolean;
  remove: boolean;
}

/** O backend é a autoridade (e recusa o resto); isto só evita oferecer o que sempre falharia. */
export function blockActions(view: DdaeSessionView, block: DdaeBlock): BlockActions {
  const open = view.status !== "completed";
  const active = view.status === "active";
  return {
    start: active && block.status === "pending" && !view.currentBlock,
    complete: active && block.status === "in_progress",
    rename: open && block.status !== "completed",
    remove: open && block.status === "pending",
  };
}

/** Escopo da Session = projeção dos Blocks (nenhuma segunda representação persistida). */
export function scopeProjection(view: DdaeSessionView) {
  const count = (status: DdaeBlock["status"]) => view.blocks.filter((b) => b.status === status).length;
  return { blocks: view.blocks, completed: count("completed"), inProgress: count("in_progress"), pending: count("pending") };
}

export const criteriaSummary = (criteria: DdaeCriterion[] | undefined) => {
  const list = criteria ?? [];
  return { done: list.filter((c) => c.completed).length, total: list.length };
};

const text = (e: DdaeEvent, key: string) => (typeof e.payload?.[key] === "string" ? (e.payload[key] as string) : "");
const quoted = (value: string) => (value ? `: ${value}` : "");

/** Linha do histórico a partir do evento real (nada é inventado além do tipo e do payload). */
export function eventLabel(e: DdaeEvent): string {
  switch (e.type) {
    case "SESSION_CREATED": return "Sessão criada";
    case "SESSION_FROZEN": return `Sessão congelada${quoted(text(e, "reason"))}`;
    case "SESSION_STOPPED": return `Sessão parada${quoted(text(e, "reason"))}`;
    case "SESSION_RESUMED": return "Sessão retomada";
    case "SESSION_COMPLETED": return `Sessão finalizada${quoted(text(e, "result"))}`;
    case "LEGACY_IMPORTED": return "Sessão importada do histórico legado";
    case "BLOCK_ADDED": return `Bloco adicionado${quoted(text(e, "title"))}`;
    case "BLOCK_STARTED": return `Bloco iniciado${quoted(text(e, "title"))}`;
    case "BLOCK_COMPLETED": return `Bloco concluído${quoted(text(e, "title"))}`;
    case "BLOCK_RENAMED": return `Bloco renomeado${quoted([text(e, "from"), text(e, "to")].filter(Boolean).join(" → "))}`;
    case "BLOCK_REMOVED": return `Bloco removido${quoted(text(e, "title"))}`;
    case "CRITERION_ADDED": return `Critério adicionado${quoted(text(e, "text"))}`;
    case "CRITERION_COMPLETED": return `Critério concluído${quoted(text(e, "text"))}`;
    case "CRITERION_REOPENED": return `Critério reaberto${quoted(text(e, "text"))}`;
    case "CRITERION_REMOVED": return `Critério removido${quoted(text(e, "text"))}`;
    case "DECISION_ADDED": return `Decisão registrada${quoted(text(e, "title"))}`;
    case "NOTE_ADDED": return "Anotação adicionada";
    case "NOTE_REMOVED": return "Anotação removida";
    case "DETAILS_UPDATED": return "Detalhes atualizados";
    case "WORKTREE_LINKED": return `Worktree vinculado${quoted(text(e, "name"))}`;
    case "WORKTREE_UNLINKED": return `Worktree desvinculado${quoted(text(e, "name"))}`;
    case "WORKTREE_BLOCK_LINKED": return `Worktree vinculado a um bloco${quoted(text(e, "name"))}`;
    case "WORKTREE_BLOCK_UNLINKED": return `Worktree desvinculado de um bloco${quoted(text(e, "name"))}`;
    default: return "Evento";
  }
}

/** Mais recentes primeiro; a ordem canônica do backend é (createdAt, id) crescente. */
export function recentEvents(events: DdaeEvent[] | undefined, limit = 6): DdaeEvent[] {
  return [...(events ?? [])].reverse().slice(0, limit);
}

/** Referência exibida: rótulo (se houver) e o valor relativo/URL. */
export const referenceTitle = (r: DdaeReference) => r.label || r.value;

/** Checagem leve do que o usuário digita; o backend valida de verdade (e converte absoluto em relativo). */
export function referenceInputProblem(kind: DdaeReference["kind"], value: string): string | null {
  const v = value.trim();
  if (!v) return "Informe o caminho ou a URL.";
  if (kind === "url" && !/^https:\/\/[^\s/@]+\/\S+/.test(v)) return "Use uma URL https completa, sem credenciais.";
  return null;
}

/** O backend responde "Sessão não encontrada neste projeto." para inexistente OU de outro Project. */
export const isSessionNotFound = (message: string) => message.includes("não encontrada");

/**
 * O que a rota faz com o resultado da leitura do par (Project, Session):
 * achou → mostra; "não encontrada" → volta para a lista com aviso (nunca escolhe outra Session);
 * qualquer outro erro → mostra o erro com "tentar de novo".
 */
export function resolveSessionLoad(result: { ok: true } | { ok: false; message: string }): "show" | "redirect" | "error" {
  if (result.ok) return "show";
  return isSessionNotFound(result.message) ? "redirect" : "error";
}
