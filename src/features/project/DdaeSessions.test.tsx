import { renderToStaticMarkup } from "react-dom/server";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { DdaeBlock, DdaeBlockStatus, DdaeOverview, DdaeSessionStatus, DdaeSessionView } from "../../shared/types";

// O componente só fala com o backend pelo `api`; aqui ele devolve um DDAE fixo e finge ser o desktop.
const responses = new Map<string, DdaeOverview>();
vi.mock("../../shared/api", () => ({
  desktop: true,
  errorText: (e: unknown) => String(e),
  api: async (_command: string, args?: { projectId?: string }) => responses.get(args?.projectId ?? ""),
}));

const { DdaeSessions } = await import("./DdaeSessions");
const { workspace } = await import("../../state/workspace");

const block = (id: string, title: string, status: DdaeBlockStatus): DdaeBlock => ({ id, title, status });
function view(number: number, status: DdaeSessionStatus, extra: Partial<DdaeSessionView> = {}): DdaeSessionView {
  const blocks = extra.blocks ?? [];
  const completed = blocks.filter((b) => b.status === "completed").length;
  return {
    id: `id-${number}`, projectId: "p", number, label: `SESSION-${String(number).padStart(3, "0")}`, title: `Sessão ${number}`, objective: "Objetivo real",
    status, blocks, decisions: [], createdAt: "2026-01-01T00:00:00.000Z", updatedAt: "2026-01-02T00:00:00.000Z",
    progress: { completed, total: blocks.length }, currentBlock: blocks.find((b) => b.status === "in_progress") ?? null,
    nextBlock: blocks.find((b) => b.status === "pending") ?? null, canComplete: blocks.length > 0 && completed === blocks.length, recentDecision: null, ...extra,
  };
}
function overview(projectId: string, sessions: DdaeSessionView[], activeId: string | null): DdaeOverview {
  const count = (s: DdaeSessionStatus) => sessions.filter((x) => x.status === s).length;
  return {
    projectId, sessions, activeSessionId: activeId, legacyImport: "not_applicable",
    counts: { total: sessions.length, active: count("active"), frozen: count("frozen"), stopped: count("stopped"), completed: count("completed") },
    blocksTotal: sessions.reduce((n, s) => n + s.blocks.length, 0),
  };
}

const REAL = "proj-real";
const EMPTY = "proj-empty";
const MIXED = "proj-mixed";

beforeAll(async () => {
  // O LKR_Lab real: somente a SESSION-001 (nada de SESSION-002/003/004 de exemplo).
  const blocks = [block("a", "Product Architecture", "completed"), block("b", "Concept 08 — Worktrees", "completed"), block("c", "Concept 09 — Planejamento", "in_progress")];
  responses.set(REAL, overview(REAL, [view(1, "active", { title: "Machine Context & Project Workspace Foundation", blocks })], "id-1"));
  responses.set(EMPTY, overview(EMPTY, [], null));
  responses.set(MIXED, overview(MIXED, [
    view(3, "stopped", { title: "Auto Update Foundation", pauseReason: "Sem previsão", blocks: [block("x", "Revisar arquitetura", "pending")] }),
    view(2, "frozen", { title: "External Process Adoption", pauseReason: "Aguardando o Runtime Manager" }),
    view(1, "completed", { title: "Portable Workspace", result: "Concluído", blocks: [block("y", "Fechar", "completed")] }),
  ], null));
  for (const id of [REAL, EMPTY, MIXED]) await workspace.forProject(id).ddae.refresh();
});

const render = (id: string) => renderToStaticMarkup(<DdaeSessions projectId={id} notify={() => {}} />);

describe("DDAE / Sessões (dados reais)", () => {
  it("mostra a SESSION-001 real com bloco atual, progresso e estado, e nenhuma sessão de exemplo", () => {
    const html = render(REAL);
    expect(html).toContain("DDAE / Sessões");
    expect(html).toContain("SESSION-001");
    expect(html).toContain("Machine Context &amp; Project Workspace Foundation");
    expect(html).toContain("ATIVA");
    expect(html).toContain("2 / 3 blocos");
    expect(html).toContain("Concept 09 — Planejamento");
    for (const fake of ["SESSION-002", "SESSION-003", "SESSION-004"]) expect(html).not.toContain(fake);
  });
  it("cards de resumo e filtros fecham com o total", () => {
    const html = render(REAL);
    expect(html).toContain("Sessões totais");
    expect(html).toContain("1 ativa");
    expect(html).toContain("Blocos (total)");
    expect(html).toContain("Próximo bloco");
    expect(html).toMatch(/Todas <span>1<\/span>/);
    expect(html).toMatch(/Ativa <span>1<\/span>/);
    expect(html).toMatch(/Congelada <span>0<\/span>/);
    expect(html).toMatch(/Parada <span>0<\/span>/);
    expect(html).toMatch(/Finalizada <span>0<\/span>/);
  });
  it("preview do master/detail e ações da sessão ativa; Abrir sessão aguarda o Concept 07", () => {
    const html = render(REAL);
    expect(html).toContain("Objetivo real");
    expect(html).toContain("Feature");
    expect(html).toContain("Concluir bloco");
    expect(html).toContain("Congelar");
    expect(html).toContain("Parar");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>(?:(?!<\/button>).)*Abrir sessão/s);
    expect(html).toContain("Próxima ação");
    expect(html).toContain("Continuar Concept 09 — Planejamento");
  });
  it("congelada/parada/finalizada mostram o motivo, o próximo passo e o resultado; só a ativa pulsa", () => {
    const html = render(MIXED);
    expect(html).toContain("CONGELADA");
    expect(html).toContain("PARADA");
    expect(html).toContain("FINALIZADA");
    expect(html).toContain("Aguardando o Runtime Manager");
    expect(html).toContain("Sem previsão · Próximo: Revisar arquitetura");
    expect(html).toContain("Resultado: Concluído");
    expect(html).toMatch(/Todas <span>3<\/span>/);
    expect(html).toMatch(/Parada <span>1<\/span>/);
    expect(html).not.toContain("is-active");
    expect(html).toContain("Nenhuma sessão ativa");
  });
  it("projeto sem sessões: estado vazio honesto", () => {
    const html = render(EMPTY);
    expect(html).toContain("Nenhuma sessão neste projeto");
    expect(html).toContain("Nova sessão");
    expect(html).not.toContain("SESSION-");
  });
});
