import { describe, expect, it } from "vitest";
import "./lkr-portable.js";
import "./lkr-workspace.js";

const ws = globalThis.LKR.workspace;

const project = { id: "p1", name: "LKR_Lab", description: "", repository: "", stack: [], tags: [], ports: [], commands: [], createdAt: "", updatedAt: "" };
const session = (extra = {}) => ({
  id: "s1", projectId: "p1", number: 1, title: "Feature", objective: "", status: "active",
  blocks: [{ id: "b1", title: "Bloco", status: "pending" }], decisions: [], createdAt: "", updatedAt: "", ...extra,
});
const worktree = (extra = {}) => ({
  id: "w1", projectId: "p1", displayName: "feature/x", status: "active", createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-02T00:00:00Z", ...extra,
});
const state = (worktrees, extra = {}) => ({ version: 4, projects: [project], prompts: [], knowledge: [], preferences: {}, ddae: [session()], managedWorktrees: worktrees, ...extra });
const invalid = (input) => {
  const result = ws.validate(input);
  expect(result.ok).toBe(false);
  return result.error;
};
const event = (n, extra = {}) => ({ id: "e" + n, type: "WORKTREE_STATE_CHANGED", payload: { from: "active", to: "frozen" }, createdAt: "2026-01-0" + n + "T00:00:00Z", ...extra });

describe("workspace v4: worktrees gerenciados", () => {
  it("preserva a metadata portátil e nunca inclui path", () => {
    const w = worktree({ description: "Experimento", branchHint: "feature/x", repositoryLocator: { remote: "github.com/org/lkr", path: "" }, sessionId: "s1", blockId: "b1", events: [event(1)] });
    const result = ws.validate(state([w]));
    expect(result.ok).toBe(true);
    expect(result.state.version).toBe(4);
    expect(result.state.managedWorktrees[0]).toMatchObject({ id: "w1", displayName: "feature/x", sessionId: "s1", blockId: "b1", branchHint: "feature/x" });
    expect(JSON.stringify(result.state)).not.toMatch(/localPath|C:\\|\/home\//);
    expect(result.summary.worktrees).toBe(1);
    expect(ws.isEmpty(result.state)).toBe(false);
  });
  it("sem worktrees a chave não aparece e v1–v3 continuam válidos (viram v4 com o mesmo hash)", () => {
    const base = { projects: [project], prompts: [], knowledge: [], preferences: {} };
    const hashes = [1, 2, 3, 4].map((version) => ws.validate({ version, ...base }));
    for (const r of hashes) {
      expect(r.ok).toBe(true);
      expect(r.state.version).toBe(4);
      expect("managedWorktrees" in r.state).toBe(false);
    }
    expect(new Set(hashes.map((r) => r.hash)).size).toBe(1);
  });
  it("exige a versão 4", () => {
    expect(invalid(state([worktree()], { version: 3 }))).toMatch(/versão 4/);
  });
  it("finalizado exige completedAt; os demais não podem ter", () => {
    expect(invalid(state([worktree({ status: "completed" })]))).toMatch(/completedAt/);
    expect(invalid(state([worktree({ status: "active", completedAt: "2026-01-03T00:00:00Z" })]))).toMatch(/completedAt/);
    expect(ws.validate(state([worktree({ status: "completed", completedAt: "2026-01-03T00:00:00Z", result: "ok" })])).ok).toBe(true);
    expect(invalid(state([worktree({ status: "ativo" })]))).toMatch(/estado desconhecido/);
  });
  it("vários ativos são válidos (sem regra de uma ativa)", () => {
    expect(ws.validate(state([worktree({ id: "w1" }), worktree({ id: "w2", sessionId: "s1" }), worktree({ id: "w3", sessionId: "s1" })])).ok).toBe(true);
  });
  it("relação: bloco exige Session; Session do mesmo projeto; bloco da mesma Session", () => {
    expect(invalid(state([worktree({ blockId: "b1" })]))).toMatch(/bloco exige Session/);
    expect(invalid(state([worktree({ sessionId: "fantasma" })]))).toMatch(/Session que não está/);
    const otherProject = { ...project, id: "p2" };
    const input = { ...state([worktree({ sessionId: "s2" })]), projects: [project, otherProject], ddae: [session(), session({ id: "s2", projectId: "p2", number: 1, blocks: [] })] };
    expect(invalid(input)).toMatch(/outro projeto/);
    expect(invalid(state([worktree({ sessionId: "s1", blockId: "b-outro" })]))).toMatch(/bloco não pertence/);
    expect(invalid(state([worktree({ projectId: "fantasma" })]))).toMatch(/projeto que não está/);
  });
  it("eventos: ordem canônica, tipos fechados, ids únicos, sem path", () => {
    const events = [event(3), event(1), event(2)];
    expect(ws.validate(state([worktree({ events })])).state.managedWorktrees[0].events.map((e) => e.id)).toEqual(["e1", "e2", "e3"]);
    expect(invalid(state([worktree({ events: [event(1, { type: "OUTRO" })] })]))).toMatch(/tipo de evento/);
    expect(invalid(state([worktree({ events: [event(1), event(1)] })]))).toMatch(/id duplicado/);
    expect(invalid(state([worktree({ events: [event(1, { payload: { path: "C:\\Users\\x\\wt" } })] })]))).toMatch(/caminho local/);
  });
  it("nenhum path absoluto em nenhum campo", () => {
    for (const bad of ["C:\\Users\\x\\wt", "D:/dev/wt", "/home/x/wt", "~/wt", "\\\\srv\\share"]) {
      expect(invalid(state([worktree({ displayName: bad })]))).toMatch(/caminho local/);
      expect(invalid(state([worktree({ description: bad })]))).toMatch(/caminho local/);
      expect(invalid(state([worktree({ branchHint: bad })]))).toMatch(/caminho local/);
      expect(invalid(state([worktree({ stateReason: bad })]))).toMatch(/caminho local/);
    }
    expect(invalid(state([worktree({ repositoryLocator: { remote: "C:\\Users\\x\\repo", path: "" } })]))).toMatch(/locator|remote/i);
    expect(ws.validate(state([worktree({ displayName: "feature/ddae-sessions" })])).ok).toBe(true);
  });
  it("eventos do DDAE de worktree são tipos válidos da Session", () => {
    const e = { id: "se1", type: "WORKTREE_LINKED", payload: { worktreeId: "w1", name: "feature/x" }, createdAt: "2026-01-01T00:00:00Z" };
    expect(ws.validate(state([worktree()], { ddae: [session({ events: [e] })] })).ok).toBe(true);
  });
});
