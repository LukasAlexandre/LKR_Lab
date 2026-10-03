import { describe, expect, it } from "vitest";
import { FILTERS, breakdownText, contextLine, deriveDdaeNextAction, filterCounts, matchesQuery, progressFraction, progressText, statusBadge, summarize, visibleSessions } from "./ddae";
import type { DdaeBlock, DdaeBlockStatus, DdaeOverview, DdaeSessionStatus, DdaeSessionView } from "./types";

const block = (id: string, title: string, status: DdaeBlockStatus): DdaeBlock => ({ id, title, status });

function session(number: number, status: DdaeSessionStatus, extra: Partial<DdaeSessionView> = {}): DdaeSessionView {
  const blocks = extra.blocks ?? [];
  const completed = blocks.filter((b) => b.status === "completed").length;
  return {
    id: `id-${number}`,
    projectId: "p1",
    number,
    label: `SESSION-${String(number).padStart(3, "0")}`,
    title: `Sessão ${number}`,
    objective: "",
    status,
    blocks,
    decisions: [],
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-02T00:00:00.000Z",
    progress: { completed, total: blocks.length },
    currentBlock: blocks.find((b) => b.status === "in_progress") ?? null,
    nextBlock: blocks.find((b) => b.status === "pending") ?? null,
    canComplete: blocks.length > 0 && completed === blocks.length,
    recentDecision: null,
    ...extra,
  };
}

const sessions = [
  session(4, "completed", { title: "Portable Workspace & Machine Binding", result: "Workspace portátil concluído.", blocks: [block("a", "A", "completed")] }),
  session(3, "stopped", { title: "Auto Update Foundation", pauseReason: "Sem previsão", blocks: [block("b", "Revisar arquitetura", "pending")] }),
  session(2, "frozen", { title: "External Process Adoption", pauseReason: "Aguardando conclusão do Runtime Manager", objective: "Adotar processos externos" }),
  session(1, "active", { title: "Machine Context & Project Workspace Foundation", objective: "Estruturar identidade da máquina", blocks: [block("c", "Concept 05", "completed"), block("d", "Concept 06", "in_progress"), block("e", "Concept 07", "pending")] }),
];
const overview = (list: DdaeSessionView[], activeId: string | null = "id-1"): DdaeOverview => ({
  projectId: "p1",
  sessions: list,
  counts: { total: list.length, active: list.filter((s) => s.status === "active").length, frozen: list.filter((s) => s.status === "frozen").length, stopped: list.filter((s) => s.status === "stopped").length, completed: list.filter((s) => s.status === "completed").length },
  blocksTotal: list.reduce((n, s) => n + s.blocks.length, 0),
  activeSessionId: activeId,
  legacyImport: "not_applicable",
});

describe("filtros e contadores", () => {
  it("os contadores por estado fecham exatamente com o total", () => {
    const counts = filterCounts(sessions);
    expect(counts).toEqual({ all: 4, active: 1, frozen: 1, stopped: 1, completed: 1 });
    expect(counts.active + counts.frozen + counts.stopped + counts.completed).toBe(counts.all);
  });
  it("os filtros são Todas, Ativa, Congelada, Parada e Finalizada", () => {
    expect(FILTERS.map((f) => f.label)).toEqual(["Todas", "Ativa", "Congelada", "Parada", "Finalizada"]);
  });
  it("filtra por estado e a busca não altera os contadores", () => {
    expect(visibleSessions(sessions, "frozen", "").map((s) => s.label)).toEqual(["SESSION-002"]);
    expect(visibleSessions(sessions, "all", "")).toHaveLength(4);
    expect(visibleSessions(sessions, "active", "auto update")).toHaveLength(0);
    expect(filterCounts(sessions).all).toBe(4);
  });
  it("estado sem sessões: tudo zero", () => {
    expect(filterCounts([])).toEqual({ all: 0, active: 0, frozen: 0, stopped: 0, completed: 0 });
  });
});

describe("busca", () => {
  it("encontra por SESSION-NNN, número, título e objetivo (sem acento nem caixa)", () => {
    expect(matchesQuery(sessions[3], "SESSION-001")).toBe(true);
    expect(matchesQuery(sessions[3], "session-001")).toBe(true);
    expect(matchesQuery(sessions[3], "001")).toBe(true);
    expect(matchesQuery(sessions[3], "session 001")).toBe(true);
    expect(matchesQuery(sessions[3], "MÁQUINA")).toBe(true);
    expect(matchesQuery(sessions[3], "maquina")).toBe(true);
    expect(matchesQuery(sessions[2], "adotar processos")).toBe(true);
    expect(matchesQuery(sessions[3], "SESSION-002")).toBe(false);
    expect(matchesQuery(sessions[3], "   ")).toBe(true);
  });
  it("combina busca e filtro", () => {
    expect(visibleSessions(sessions, "all", "workspace").map((s) => s.label)).toEqual(["SESSION-004", "SESSION-001"]);
    expect(visibleSessions(sessions, "completed", "workspace").map((s) => s.label)).toEqual(["SESSION-004"]);
  });
});

describe("rótulos derivados", () => {
  it("progresso é completed / total, sem percentual guardado", () => {
    expect(progressText(sessions[3])).toBe("1 / 3 blocos");
    expect(progressFraction(sessions[3])).toBeCloseTo(1 / 3);
    expect(progressText(sessions[2])).toBe("Sem blocos");
    expect(progressFraction(sessions[2])).toBe(0);
  });
  it("badges em caixa alta", () => {
    expect(statusBadge("active")).toBe("ATIVA");
    expect(statusBadge("frozen")).toBe("CONGELADA");
    expect(statusBadge("stopped")).toBe("PARADA");
    expect(statusBadge("completed")).toBe("FINALIZADA");
  });
  it("a linha de contexto explica por que a sessão está no estado atual", () => {
    expect(contextLine(sessions[2])).toBe("Aguardando conclusão do Runtime Manager");
    expect(contextLine(sessions[1])).toBe("Sem previsão · Próximo: Revisar arquitetura");
    expect(contextLine(sessions[0])).toBe("Resultado: Workspace portátil concluído.");
    expect(contextLine(sessions[3])).toBeNull();
    expect(contextLine(session(9, "completed"))).toBeNull();
  });
  it("breakdown só cita os estados que existem", () => {
    expect(breakdownText(overview(sessions).counts)).toBe("1 ativa · 1 congelada · 1 parada · 1 finalizada");
    expect(breakdownText(overview([sessions[3], session(5, "stopped"), session(6, "stopped")], "id-1").counts)).toBe("1 ativa · 2 paradas");
    expect(breakdownText(overview([]).counts)).toBe("Nenhuma sessão");
  });
});

describe("cards de resumo", () => {
  it("sessão ativa, blocos e próximo bloco vêm da sessão ativa real", () => {
    const summary = summarize(overview(sessions));
    expect(summary.total).toBe(4);
    expect(summary.active?.label).toBe("SESSION-001");
    expect(summary.blocksTotal).toBe(5);
    expect(summary.blocksInActive).toBe("1 / 3 na ativa");
    expect(summary.nextBlock).toBe("Concept 07");
  });
  it("sem sessão ativa não inventa nada", () => {
    const summary = summarize(overview([sessions[2]], null));
    expect(summary.active).toBeNull();
    expect(summary.blocksInActive).toBeNull();
    expect(summary.nextBlock).toBeNull();
  });
});

describe("próxima ação do DDAE", () => {
  it("sessão ativa com bloco em andamento → Continuar <bloco>", () => {
    const next = deriveDdaeNextAction(overview(sessions));
    expect(next).toMatchObject({ kind: "continue", sessionId: "id-1", title: "Continuar Concept 06", blockId: "d" });
    expect(next?.description).toBe("Concluir Concept 06 e avançar para: Concept 07");
  });
  it("sessão ativa sem bloco atual e com próximo → Iniciar <bloco>", () => {
    const idle = session(1, "active", { blocks: [block("c", "Concept 05", "completed"), block("d", "Concept 06", "pending")] });
    expect(deriveDdaeNextAction(overview([idle]))).toMatchObject({ kind: "start", title: "Iniciar Concept 06", blockId: "d" });
  });
  it("sem sessão ativa, sem blocos ou sem overview → nenhuma ação (sem inventar)", () => {
    expect(deriveDdaeNextAction(overview([sessions[2]], null))).toBeNull();
    expect(deriveDdaeNextAction(overview([session(1, "active")]))).toBeNull();
    expect(deriveDdaeNextAction(overview([session(1, "active", { blocks: [block("c", "X", "completed")] })]))).toBeNull();
    expect(deriveDdaeNextAction(null)).toBeNull();
    expect(deriveDdaeNextAction(undefined)).toBeNull();
  });
  it("uma sessão congelada com bloco em andamento não gera ação (só a ativa)", () => {
    const frozen = session(1, "frozen", { blocks: [block("d", "X", "in_progress")] });
    expect(deriveDdaeNextAction(overview([frozen], null))).toBeNull();
  });
});
