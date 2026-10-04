import { describe, expect, it } from "vitest";
import "./lkr-portable.js";
import "./lkr-workspace.js";

const ws = globalThis.LKR.workspace;

const project = { id: "p1", name: "LKR_Lab", description: "", repository: "", stack: [], tags: [], ports: [], commands: [], createdAt: "", updatedAt: "" };
const session = (extra = {}) => ({
  id: "s1", projectId: "p1", number: 1, title: "Feature", objective: "", status: "active",
  blocks: [{ id: "b1", title: "Bloco", status: "pending" }], decisions: [], createdAt: "", updatedAt: "", ...extra,
});
const item = (extra = {}) => ({
  id: "i1", projectId: "p1", title: "Primeiro", position: 1000, storedStatus: "open", createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-02T00:00:00Z", ...extra,
});
const event = (n, extra = {}) => ({ id: "e" + n, itemId: "i1", type: "PLANNING_ITEM_CREATED", payload: { title: "Primeiro" }, createdAt: "2026-01-0" + n + "T00:00:00Z", ...extra });
const state = (items, extra = {}) => ({ version: 5, projects: [project], prompts: [], knowledge: [], preferences: {}, planningItems: items, ...extra });
const invalid = (input) => {
  const result = ws.validate(input);
  expect(result.ok).toBe(false);
  return result.error;
};

describe("workspace v5: planejamento", () => {
  it("preserva a fila na forma canônica, sem path e sem estado derivado", () => {
    const result = ws.validate(state([item({ id: "i2", position: 2000, title: "Segundo", description: "Algo" }), item()], { planningEvents: [event(2), event(1)] }));
    expect(result.ok).toBe(true);
    expect(result.state.version).toBe(5);
    expect(result.state.planningItems.map((i) => i.id)).toEqual(["i1", "i2"]);
    expect(result.state.planningEvents.map((e) => e.id)).toEqual(["e1", "e2"]);
    expect(JSON.stringify(result.state)).not.toMatch(/localPath|C:\\|\/home\/|"status":"planned"|inProgress/);
    expect(result.summary.planningItems).toBe(2);
    expect(ws.isEmpty(result.state)).toBe(false);
  });

  it("v1–v4 continuam legíveis sem a chave de planejamento e viram v5 com o mesmo hash", () => {
    const base = { projects: [project], prompts: [], knowledge: [], preferences: {} };
    const results = [1, 2, 3, 4, 5].map((version) => ws.validate({ version, ...base }));
    for (const r of results) {
      expect(r.ok).toBe(true);
      expect(r.state.version).toBe(5);
      expect("planningItems" in r.state).toBe(false);
      expect("planningEvents" in r.state).toBe(false);
    }
    expect(new Set(results.map((r) => r.hash)).size).toBe(1);
  });

  it("a Session sem vínculo mantém planningItemId ausente (SESSION-001 legada)", () => {
    const result = ws.validate(state([], { ddae: [session()] }));
    expect(result.ok).toBe(true);
    expect("planningItemId" in result.state.ddae[0]).toBe(false);
    expect("planningItems" in result.state).toBe(false);
  });

  it("aceita o vínculo 1 item : 0..1 Session do mesmo Project", () => {
    const result = ws.validate(state([item()], { ddae: [session({ planningItemId: "i1" })] }));
    expect(result.ok).toBe(true);
    expect(result.state.ddae[0].planningItemId).toBe("i1");
  });

  it("recusa planejamento em workspace anterior à v5", () => {
    expect(invalid(state([item()], { version: 4 }))).toMatch(/versão 5/);
    expect(invalid(state([], { version: 4, ddae: [session({ planningItemId: "i1" })] }))).toMatch(/versão 5/);
  });

  it("recusa itens inconsistentes", () => {
    expect(invalid(state([item({ projectId: "fantasma" })]))).toMatch(/projeto/);
    expect(invalid(state([item(), item({ position: 2000 })]))).toMatch(/duplicado/);
    expect(invalid(state([item(), item({ id: "i2" })]))).toMatch(/posição repetida/);
    expect(invalid(state([item({ position: 0 })]))).toMatch(/position/);
    expect(invalid(state([item({ title: "  " })]))).toMatch(/obrigatório/);
    expect(invalid(state([item({ title: "x".repeat(121) })]))).toMatch(/excede/);
    expect(invalid(state([item({ storedStatus: "planned" })]))).toMatch(/estado desconhecido/);
    expect(invalid(state([item({ storedStatus: "cancelled" })]))).toMatch(/cancelledAt/);
    expect(invalid(state([item({ cancelledAt: "2026-01-03T00:00:00Z" })]))).toMatch(/cancelledAt/);
    expect(invalid(state([item({ cancelReason: "x" })]))).toMatch(/motivo/);
    expect(invalid(state([item({ description: "C:\\Users\\x\\proj" })]))).toMatch(/caminho local/);
  });

  it("recusa vínculos inconsistentes com a Session", () => {
    const cancelled = item({ storedStatus: "cancelled", cancelledAt: "2026-01-03T00:00:00Z" });
    expect(invalid(state([cancelled], { ddae: [session({ planningItemId: "i1" })] }))).toMatch(/cancelado/);
    expect(invalid(state([item()], { ddae: [session({ planningItemId: "nada" })] }))).toMatch(/não está no workspace/);
    const two = [session({ planningItemId: "i1" }), session({ id: "s2", number: 2, status: "frozen", planningItemId: "i1", blocks: [{ id: "b2", title: "Bloco", status: "pending" }] })];
    expect(invalid(state([item()], { ddae: two }))).toMatch(/outra Session/);
    const p2 = { ...project, id: "p2" };
    expect(invalid(state([item({ projectId: "p2" })], { projects: [project, p2], ddae: [session({ planningItemId: "i1" })] }))).toMatch(/outro projeto/);
  });

  it("valida os eventos: item existente, tipo conhecido, sem path", () => {
    expect(ws.validate(state([item()], { planningEvents: [event(1)] })).ok).toBe(true);
    expect(invalid(state([item()], { planningEvents: [event(1, { itemId: "fantasma" })] }))).toMatch(/item inexistente/);
    expect(invalid(state([item()], { planningEvents: [event(1, { type: "PLANNING_ITEM_COMPLETED_DERIVED" })] }))).toMatch(/tipo de evento/);
    expect(invalid(state([item()], { planningEvents: [event(1, { payload: { title: "C:\\Users\\x" } })] }))).toMatch(/caminho local/);
    expect(invalid(state([item()], { planningEvents: [event(1), event(2, { id: "e1" })] }))).toMatch(/duplicado/);
  });

  it("a ordem de entrada e a posição são determinísticas", () => {
    const a = ws.validate(state([item({ id: "i2", position: 2000 }), item()]));
    const b = ws.validate(state([item(), item({ id: "i2", position: 2000 })]));
    expect(a.hash).toBe(b.hash);
    const moved = ws.validate(state([item({ id: "i2", position: 500 }), item()]));
    expect(moved.state.planningItems.map((i) => i.id)).toEqual(["i2", "i1"]);
    expect(moved.hash).not.toBe(a.hash);
  });
});
