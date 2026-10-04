import { describe, expect, it } from "vitest";
import { PHASE_BADGE, PLANNING_FILTERS, matchesQuery, originText, progressFraction, progressText, rowActions, subBadge, visibleRows } from "./planning";
import { deriveProjectNextAction } from "./projectContext";
import type { DdaeSessionStatus, GitSummary, PlanningPhase, PlanningRow, PlanningSummary, ProjectOverview } from "./types";

function row(id: string, title: string, phase: PlanningPhase, over: Partial<PlanningRow> = {}): PlanningRow {
  return {
    id, projectId: "p", title, description: "", position: 1000, storedStatus: phase === "cancelled" ? "cancelled" : "open",
    createdAt: "", updatedAt: "", phase, session: null, canStart: false, disabledReason: null,
    canEdit: phase === "planned", canCancel: phase === "planned", canRestore: phase === "cancelled",
    canMoveUp: false, canMoveDown: false, lastActivityAt: "2026-10-03T20:00:00Z", ...over,
  };
}
const session = (number: number, status: DdaeSessionStatus, completed: number, total: number) => ({
  id: `s${number}`, number, label: `SESSION-${String(number).padStart(3, "0")}`, title: "t", status, progress: { completed, total },
});

const rows = [
  row("1", "Configuração de ambientes por projeto", "planned", { description: "Permitir presets locais", canMoveDown: true }),
  row("2", "Interface principal do LKR LAB", "executing", { session: session(5, "frozen", 6, 9) }),
  row("3", "Project Runtime Manager V2", "executing", { session: session(6, "stopped", 4, 7) }),
  row("4", "Storage & Sync (GitHub)", "completed", { session: session(4, "completed", 10, 10) }),
  row("5", "Segurança e Hardening", "cancelled"),
];

describe("filtros do Planejamento", () => {
  it("só os cinco filtros aprovados (sem 'Mais filtros', prioridade ou área)", () => {
    expect(PLANNING_FILTERS.map((f) => f.label)).toEqual(["Todos", "Planejados", "Em execução", "Concluídos", "Cancelados"]);
  });
  it("cada filtro seleciona a sua fase e Todos mostra tudo, inclusive o cancelado", () => {
    expect(visibleRows(rows, "all", "")).toHaveLength(5);
    expect(visibleRows(rows, "planned", "").map((r) => r.id)).toEqual(["1"]);
    expect(visibleRows(rows, "executing", "").map((r) => r.id)).toEqual(["2", "3"]);
    expect(visibleRows(rows, "completed", "").map((r) => r.id)).toEqual(["4"]);
    expect(visibleRows(rows, "cancelled", "").map((r) => r.id)).toEqual(["5"]);
  });
  it("filtrar ou buscar nunca reordena a fila", () => {
    expect(visibleRows(rows, "all", "o").map((r) => r.id)).toEqual(["1", "2", "3", "4", "5"].filter((id) => visibleRows(rows, "all", "o").some((r) => r.id === id)));
  });
});

describe("busca", () => {
  it("encontra por título, descrição e Session relacionada, sem acento nem caixa", () => {
    expect(matchesQuery(rows[0], "CONFIGURACAO")).toBe(true);
    expect(matchesQuery(rows[0], "presets")).toBe(true);
    expect(matchesQuery(rows[1], "session-005")).toBe(true);
    expect(matchesQuery(rows[1], "SESSION 005")).toBe(true);
    expect(matchesQuery(rows[1], "005")).toBe(true);
    expect(matchesQuery(rows[0], "session-005")).toBe(false);
    expect(visibleRows(rows, "all", "storage").map((r) => r.id)).toEqual(["4"]);
    expect(visibleRows(rows, "all", "session-006").map((r) => r.id)).toEqual(["3"]);
    expect(visibleRows(rows, "all", "   ")).toHaveLength(5);
  });
});

describe("selos e progresso", () => {
  it("fase principal e subselo com o estado REAL da Session", () => {
    expect(PHASE_BADGE.executing).toBe("Em execução");
    expect(subBadge(rows[1])?.label).toBe("CONGELADA");
    expect(subBadge(rows[2])?.label).toBe("PARADA");
    expect(subBadge(rows[3])?.label).toBe("FINALIZADA");
    expect(subBadge(row("a", "x", "executing", { session: session(1, "active", 9, 10) }))?.label).toBe("ATIVA");
    expect(subBadge(rows[0])).toBeNull();
    expect(subBadge(rows[4])).toBeNull();
  });
  it("progresso vem da Session; concluído mostra o total completo", () => {
    expect(progressText(rows[1])).toBe("6 / 9");
    expect(progressText(rows[3])).toBe("10 / 10");
    expect(progressFraction(rows[3])).toBe(1);
    expect(progressText(rows[0])).toBe("");
    expect(progressFraction(row("b", "x", "executing", { session: session(2, "active", 0, 0) }))).toBe(0);
  });
  it("origem no DDAE só existe quando há item", () => {
    expect(originText({ planningItem: { title: "Plugins" } })).toBe("Origem: Planejamento — Plugins");
    expect(originText({ planningItem: null })).toBeNull();
    expect(originText({})).toBeNull();
  });
});

describe("ações da linha (nunca mostra ação inválida)", () => {
  it("planejado: editar, mover só para onde há espaço, cancelar", () => {
    expect(rowActions(rows[0])).toEqual(["edit", "down", "cancel"]);
    expect(rowActions(row("m", "meio", "planned", { canMoveUp: true, canMoveDown: true }))).toEqual(["edit", "up", "down", "top", "cancel"]);
    expect(rowActions(row("u", "último", "planned", { canMoveUp: true }))).toEqual(["edit", "up", "top", "cancel"]);
  });
  it("com Session: só abrir sessão (sem cancelar nem editar)", () => {
    for (const r of rows.slice(1, 4)) expect(rowActions(r)).toEqual(["open"]);
  });
  it("cancelado: só restaurar", () => {
    expect(rowActions(rows[4])).toEqual(["restore"]);
  });
});

const git = (): GitSummary => ({
  isRepo: true, branch: "main", detached: false, upstream: null, ahead: null, behind: null,
  staged: 0, unstaged: 0, untracked: 0, conflicts: 0, changes: 0, clean: true, error: null,
});
const project: ProjectOverview = {
  id: "p", name: "p", slug: "p", description: "", localPath: "C:\\x", repository: "", stack: [], tags: [], ports: [], commands: [],
  createdAt: "", updatedAt: "", location: "available", git: { status: "available", data: git(), message: null },
  runtime: { status: "available", data: { running: false, managedRuns: 0, listeningPorts: [] }, message: null },
  stackSource: "detected", lastActivity: null,
};
const counts = { operational: 1, planned: 1, executing: 0, completed: 0, cancelled: 0 };
const next = { id: "i1", title: "Configuração de ambientes por projeto", description: "", canStart: true, disabledReason: null };

describe("Próxima ação × PRÓXIMO do Planejamento", () => {
  it("sem Session ativa e com item planejado: Iniciar <item>, depois dos problemas críticos", () => {
    const planning: PlanningSummary = { counts, next, activeSession: null };
    expect(deriveProjectNextAction(project, null, null, planning)).toMatchObject({
      id: "start-planning-item", title: "Iniciar Configuração de ambientes por projeto", target: { kind: "area", area: "planning" },
    });
    const dirty = { ...project, git: { status: "available" as const, data: { ...git(), changes: 2, unstaged: 2, clean: false }, message: null } };
    expect(deriveProjectNextAction(dirty, null, null, planning).id).toBe("review-changes");
  });
  it("com Session ativa o PRÓXIMO continua existindo mas NÃO vira a próxima ação", () => {
    const planning: PlanningSummary = { counts, next: { ...next, canStart: false, disabledReason: "Já existe uma Session ativa neste projeto." }, activeSession: { id: "s1", number: 1, label: "SESSION-001" } };
    expect(deriveProjectNextAction(project, null, null, planning).id).toBe("none");
  });
  it("a Session ativa com bloco em andamento continua ganhando do Planejamento", () => {
    const planning: PlanningSummary = { counts, next, activeSession: null };
    const ddae = {
      projectId: "p", activeSessionId: "s1", counts: { total: 1, active: 1, frozen: 0, stopped: 0, completed: 0 }, blocksTotal: 1, legacyImport: "not_applicable" as const,
      sessions: [{
        id: "s1", label: "SESSION-001", status: "active", currentBlock: { id: "b", title: "Concept 09 — Planejamento", status: "in_progress" }, nextBlock: null,
        progress: { completed: 9, total: 10 },
      }],
    } as unknown as Parameters<typeof deriveProjectNextAction>[2];
    expect(deriveProjectNextAction(project, null, ddae, planning).id).toBe("continue-block");
  });
  it("sem itens a regra não dispara", () => {
    expect(deriveProjectNextAction(project, null, null, { counts: { ...counts, planned: 0, operational: 0 }, next: null, activeSession: null }).id).toBe("none");
    expect(deriveProjectNextAction(project, null, null, null).id).toBe("none");
  });
});
