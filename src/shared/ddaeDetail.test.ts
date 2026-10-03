import { describe, expect, it } from "vitest";
import {
  DETAIL_TABS,
  blockActions,
  criteriaSummary,
  eventLabel,
  finalizeStatus,
  lifecycleActions,
  recentEvents,
  referenceInputProblem,
  referenceTitle,
  scopeProjection,
} from "./ddaeDetail";
import type { DdaeBlock, DdaeBlockStatus, DdaeEvent, DdaeSessionStatus, DdaeSessionView } from "./types";

const block = (id: string, title: string, status: DdaeBlockStatus): DdaeBlock => ({ id, title, status });

export function view(status: DdaeSessionStatus, blocks: DdaeBlock[], extra: Partial<DdaeSessionView> = {}): DdaeSessionView {
  const completed = blocks.filter((b) => b.status === "completed").length;
  const criteria = extra.criteria ?? [];
  const blockers: DdaeSessionView["completionBlockers"] = [];
  if (!blocks.length) blockers.push("no_blocks");
  if (blocks.some((b) => b.status === "in_progress")) blockers.push("block_in_progress");
  if (blocks.some((b) => b.status === "pending")) blockers.push("blocks_pending");
  if (criteria.some((c) => !c.completed)) blockers.push("criteria_pending");
  return {
    id: "s1", projectId: "p", number: 1, label: "SESSION-001", title: "Feature", objective: "", status, blocks, decisions: [],
    createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-02T00:00:00Z",
    progress: { completed, total: blocks.length },
    currentBlock: blocks.find((b) => b.status === "in_progress") ?? null,
    nextBlock: blocks.find((b) => b.status === "pending") ?? null,
    canComplete: blockers.length === 0, completionBlockers: blockers, recentDecision: null,
    readyForAi: { state: "incomplete", ready: false, missing: ["desired_outcome", "criteria"] },
    ...extra,
  };
}

const evt = (type: DdaeEvent["type"], payload?: DdaeEvent["payload"], createdAt = "2026-01-01T00:00:00Z", id: string = type): DdaeEvent => ({ id, type, payload, createdAt });

describe("abas do Concept 07", () => {
  it("são seis e a terceira é 'Plano da sessão' (nunca 'Planejamento')", () => {
    expect(DETAIL_TABS.map((t) => t.label)).toEqual(["Visão geral", "Blocos", "Plano da sessão", "Decisões", "Arquivos", "Anotações"]);
    expect(DETAIL_TABS.some((t) => t.label === "Planejamento")).toBe(false);
  });
});

describe("elegibilidade de finalização (explícita, nunca automática)", () => {
  it("todos os blocos concluídos e sem critérios → elegível", () => {
    expect(finalizeStatus(view("active", [block("a", "A", "completed")]))).toEqual({ eligible: true, reasons: [] });
  });
  it("critério pendente bloqueia mesmo com todos os blocos concluídos", () => {
    const v = view("active", [block("a", "A", "completed")], { criteria: [{ id: "c", text: "Build", completed: false }] });
    const f = finalizeStatus(v);
    expect(f.eligible).toBe(false);
    expect(f.reasons).toEqual(["Há critérios de conclusão pendentes."]);
  });
  it("todos os critérios concluídos → elegível", () => {
    const v = view("active", [block("a", "A", "completed")], { criteria: [{ id: "c", text: "Build", completed: true }] });
    expect(finalizeStatus(v).eligible).toBe(true);
  });
  it("explica o que falta: sem blocos, em andamento, pendentes", () => {
    expect(finalizeStatus(view("active", [])).reasons).toEqual(["A sessão ainda não tem blocos."]);
    const v = view("active", [block("a", "A", "in_progress"), block("b", "B", "pending")]);
    expect(finalizeStatus(v).reasons).toEqual(["Há um bloco em andamento.", "Ainda há blocos pendentes."]);
  });
  it("só a ativa finaliza; congelada pede retomar; finalizada é terminal", () => {
    expect(finalizeStatus(view("frozen", [block("a", "A", "completed")])).eligible).toBe(false);
    expect(finalizeStatus(view("frozen", [])).reasons).toEqual(["Retome a sessão para finalizá-la."]);
    expect(finalizeStatus(view("completed", [block("a", "A", "completed")])).reasons).toEqual(["A sessão já está finalizada."]);
  });
  it("SESSION-001 real (9/10, Concept 09 em andamento): ainda não elegível; concluir o último bloco só a deixa elegível", () => {
    const blocks = [...Array.from({ length: 9 }, (_, i) => block(`b${i}`, `Bloco ${i}`, "completed")), block("c9", "Concept 09 — Planejamento", "in_progress")];
    expect(finalizeStatus(view("active", blocks)).eligible).toBe(false);
    const after = [...blocks.slice(0, 9), block("c9", "Concept 09 — Planejamento", "completed")];
    const v = view("active", after);
    expect(v.status).toBe("active");
    expect(finalizeStatus(v).eligible).toBe(true);
  });
});

describe("ações de ciclo de vida", () => {
  it("ACTIVE congela, para e finaliza; FROZEN/STOPPED só retomam; COMPLETED nada", () => {
    expect(lifecycleActions(view("active", []))).toEqual(["freeze", "stop", "complete"]);
    expect(lifecycleActions(view("frozen", []))).toEqual(["resume"]);
    expect(lifecycleActions(view("stopped", []))).toEqual(["resume"]);
    expect(lifecycleActions(view("completed", []))).toEqual([]);
  });
});

describe("ações de bloco", () => {
  const blocks = [block("a", "A", "completed"), block("b", "B", "in_progress"), block("c", "C", "pending")];
  it("iniciar só pendente, com sessão ativa e nenhum outro em andamento", () => {
    const v = view("active", blocks);
    expect(blockActions(v, blocks[2]).start).toBe(false);
    expect(blockActions(view("active", [blocks[0], blocks[2]]), blocks[2]).start).toBe(true);
    expect(blockActions(view("frozen", [blocks[2]]), blocks[2]).start).toBe(false);
  });
  it("concluir só o em andamento", () => {
    const v = view("active", blocks);
    expect(blockActions(v, blocks[1]).complete).toBe(true);
    expect(blockActions(v, blocks[0]).complete).toBe(false);
    expect(blockActions(v, blocks[2]).complete).toBe(false);
  });
  it("renomear pendente/em andamento, nunca concluído; remover SOMENTE pendente", () => {
    const v = view("active", blocks);
    expect(blockActions(v, blocks[0])).toMatchObject({ rename: false, remove: false });
    expect(blockActions(v, blocks[1])).toMatchObject({ rename: true, remove: false });
    expect(blockActions(v, blocks[2])).toMatchObject({ rename: true, remove: true });
  });
  it("sessão finalizada não oferece nenhuma edição de bloco", () => {
    const v = view("completed", [blocks[0]]);
    expect(blockActions(v, blocks[0])).toEqual({ start: false, complete: false, rename: false, remove: false });
  });
});

describe("projeções", () => {
  it("escopo da Session é a projeção dos Blocks, com contagens", () => {
    const scope = scopeProjection(view("active", [block("a", "A", "completed"), block("b", "B", "in_progress"), block("c", "C", "pending"), block("d", "D", "pending")]));
    expect(scope).toMatchObject({ completed: 1, inProgress: 1, pending: 2 });
    expect(scope.blocks.map((b) => b.title)).toEqual(["A", "B", "C", "D"]);
  });
  it("resumo de critérios", () => {
    expect(criteriaSummary(undefined)).toEqual({ done: 0, total: 0 });
    expect(criteriaSummary([{ id: "1", text: "a", completed: true }, { id: "2", text: "b", completed: false }])).toEqual({ done: 1, total: 2 });
  });
});

describe("histórico (ddae_events)", () => {
  it("rótulos vêm do tipo e do payload real", () => {
    expect(eventLabel(evt("SESSION_CREATED"))).toBe("Sessão criada");
    expect(eventLabel(evt("BLOCK_STARTED", { title: "Concept 09" }))).toBe("Bloco iniciado: Concept 09");
    expect(eventLabel(evt("BLOCK_RENAMED", { from: "A", to: "B" }))).toBe("Bloco renomeado: A → B");
    expect(eventLabel(evt("SESSION_FROZEN", { reason: "Aguardando" }))).toBe("Sessão congelada: Aguardando");
    expect(eventLabel(evt("SESSION_FROZEN"))).toBe("Sessão congelada");
    expect(eventLabel(evt("CRITERION_COMPLETED", { text: "Build verde" }))).toBe("Critério concluído: Build verde");
    expect(eventLabel(evt("LEGACY_IMPORTED"))).toBe("Sessão importada do histórico legado");
    expect(eventLabel(evt("NOTE_ADDED", { text: "segredo da nota" }))).toBe("Anotação adicionada");
  });
  it("recentes: mais novos primeiro, limitados; vazio é vazio", () => {
    const events = [1, 2, 3, 4, 5, 6, 7, 8].map((n) => evt("BLOCK_ADDED", { title: `B${n}` }, `2026-01-0${n}T00:00:00Z`, `e${n}`));
    expect(recentEvents(events).map((e) => e.id)).toEqual(["e8", "e7", "e6", "e5", "e4", "e3"]);
    expect(recentEvents(events, 2)).toHaveLength(2);
    expect(recentEvents(undefined)).toEqual([]);
  });
  it("a SESSION-001 legada mostra só a importação (nenhum BLOCK_COMPLETED inventado)", () => {
    const legacy = [evt("LEGACY_IMPORTED")];
    expect(recentEvents(legacy).map(eventLabel)).toEqual(["Sessão importada do histórico legado"]);
  });
});

describe("referências", () => {
  it("título usa o rótulo quando existe", () => {
    expect(referenceTitle({ kind: "project_path", value: "docs/x.md", label: "Plano" })).toBe("Plano");
    expect(referenceTitle({ kind: "project_path", value: "docs/x.md" })).toBe("docs/x.md");
  });
  it("checagem leve do que o usuário digita (o backend valida de verdade)", () => {
    expect(referenceInputProblem("url", "")).toBeTruthy();
    expect(referenceInputProblem("url", "http://x.com/a")).toBeTruthy();
    expect(referenceInputProblem("url", "https://u:p@x.com/a")).toBeTruthy();
    expect(referenceInputProblem("url", "https://github.com/org/repo")).toBeNull();
    expect(referenceInputProblem("project_path", "docs/ddae/x.md")).toBeNull();
    expect(referenceInputProblem("project_path", "  ")).toBeTruthy();
  });
});
