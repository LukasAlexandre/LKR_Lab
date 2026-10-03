import { describe, expect, it } from "vitest";
import "./lkr-portable.js";
import "./lkr-workspace.js";

const ws = globalThis.LKR.workspace;

const project = { id: "p1", name: "LKR_Lab", description: "", repository: "", stack: [], tags: [], ports: [], commands: [], createdAt: "", updatedAt: "" };
const session = (extra = {}) => ({
  id: "s1",
  projectId: "p1",
  number: 1,
  title: "Feature",
  objective: "Objetivo",
  status: "active",
  blocks: [{ id: "b1", title: "Bloco", status: "pending" }],
  decisions: [],
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-02T00:00:00Z",
  ...extra,
});
const state = (extra) => ({ version: 2, projects: [project], prompts: [], knowledge: [], preferences: {}, ddae: [session(extra)] });
const invalid = (input) => {
  const result = ws.validate(input);
  expect(result.ok).toBe(false);
  return result.error;
};

describe("workspace portátil: contexto da Session (DDAE)", () => {
  it("preserva resultado desejado, restrições, critérios, notas e referências", () => {
    const extra = {
      desiredOutcome: "Feature pronta",
      constraints: ["Sem LLM"],
      criteria: ["Testes verdes"],
      notes: ["Uma nota"],
      references: [
        { kind: "project_path", value: "docs/ddae/x.md" },
        { kind: "url", value: "https://github.com/org/repo" },
      ],
    };
    const result = ws.validate(state(extra));
    expect(result.ok).toBe(true);
    const { criteria, ...rest } = extra;
    expect(result.state.ddae[0]).toMatchObject(rest);
    // Critério em texto (formato antigo) vira objeto não concluído; o id é atribuído pelo hub-core.
    expect(result.state.ddae[0].criteria).toEqual([{ id: "", text: "Testes verdes", completed: false }]);
    expect(criteria).toEqual(["Testes verdes"]);
  });
  it("campos vazios não aparecem e itens vazios são descartados", () => {
    const result = ws.validate(state({ desiredOutcome: "  ", constraints: ["", "  a  ", "   "], criteria: [], notes: [] }));
    expect(result.ok).toBe(true);
    const s = result.state.ddae[0];
    expect("desiredOutcome" in s).toBe(false);
    expect(s.constraints).toEqual(["a"]);
    expect("criteria" in s).toBe(false);
    expect("references" in s).toBe(false);
  });
  it("referência de caminho precisa ser RELATIVA ao projeto", () => {
    for (const value of ["C:\\Users\\x\\f.md", "/home/x/f.md", "~/f.md", "../fora", "a/../b", "a//b", "a\\b", "C:/x", "./x"]) {
      expect(invalid(state({ references: [{ kind: "project_path", value }] }))).toMatch(/RELATIVO|caminho local|referências/i);
    }
  });
  it("URL só https sem credenciais; tipo desconhecido é recusado", () => {
    for (const value of ["http://x.com/a", "https://u:p@x.com/a", "ftp://x/y"]) {
      expect(invalid(state({ references: [{ kind: "url", value }] }))).toMatch(/https|credencial/);
    }
    expect(invalid(state({ references: [{ kind: "file", value: "x" }] }))).toMatch(/tipo desconhecido/);
  });
  it("sem caminho local, credencial ou excesso nos textos novos", () => {
    expect(invalid(state({ desiredOutcome: "C:\\Users\\x\\app" }))).toMatch(/caminho local/);
    expect(invalid(state({ notes: ["/home/x/app"] }))).toMatch(/caminho local/);
    expect(invalid(state({ constraints: ["token ghp_" + "a".repeat(36)] }))).toMatch(/credencial/);
    expect(invalid(state({ criteria: ["x".repeat(501)] }))).toMatch(/excede/);
    expect(invalid(state({ notes: Array.from({ length: 51 }, (_, i) => "n" + i) }))).toMatch(/máximo/);
  });
});

describe("workspace portátil v3: critérios marcáveis, eventos e blocos", () => {
  const criterion = (n, extra = {}) => ({ id: "c" + n, text: "Critério " + n, completed: false, ...extra });
  const event = (n, extra = {}) => ({ id: "e" + n, type: "BLOCK_STARTED", payload: { title: "Bloco" }, createdAt: "2026-01-0" + n + "T00:00:00Z", ...extra });
  it("preserva critérios marcáveis com id e estado", () => {
    const criteria = [criterion(1, { completed: true }), criterion(2)];
    expect(ws.validate(state({ criteria })).state.ddae[0].criteria).toEqual(criteria);
    expect(invalid(state({ criteria: [criterion(1), criterion(1)] }))).toMatch(/id duplicado/);
    expect(invalid(state({ criteria: [criterion(1, { completed: "sim" })] }))).toMatch(/verdadeiro ou falso/);
  });
  it("eventos: ordem canônica (createdAt, id), tipos fechados, sem caminho local", () => {
    const events = [event(3), event(1), event(2, { id: "a2", createdAt: "2026-01-02T00:00:00Z" })];
    const ordered = ws.validate(state({ events })).state.ddae[0].events.map((e) => e.id);
    expect(ordered).toEqual(["e1", "a2", "e3"]);
    expect(invalid(state({ events: [event(1, { type: "OUTRO" })] }))).toMatch(/tipo de evento/);
    expect(invalid(state({ events: [event(1, { payload: { path: "C:\\Users\\x" } })] }))).toMatch(/caminho local/);
    expect(invalid(state({ events: [event(1, { payload: { n: { a: 1 } } })] }))).toMatch(/só texto/);
    expect(invalid(state({ events: [event(1), event(1)] }))).toMatch(/id duplicado/);
  });
  it("evento sem payload e sem bloco não ganha campos vazios", () => {
    const e = ws.validate(state({ events: [{ id: "e1", type: "SESSION_RESUMED", createdAt: "2026-01-01T00:00:00Z" }] })).state.ddae[0].events[0];
    expect("payload" in e).toBe(false);
    expect("blockId" in e).toBe(false);
  });
  it("descrição de bloco, rótulo de referência e decisão ligada a um bloco", () => {
    const blocks = [{ id: "b1", title: "Bloco", description: "Detalhe", status: "pending" }];
    const decisions = [{ id: "d1", blockId: "b1", title: "D", body: "", createdAt: "" }];
    const references = [{ kind: "project_path", value: "docs/x.md", label: "Doc" }];
    const result = ws.validate(state({ blocks, decisions, references })).state.ddae[0];
    expect(result.blocks[0].description).toBe("Detalhe");
    expect(result.decisions[0].blockId).toBe("b1");
    expect(result.references[0].label).toBe("Doc");
    expect(invalid(state({ blocks, decisions: [{ ...decisions[0], blockId: "fantasma" }] }))).toMatch(/bloco que não existe/);
  });
  it("finalizada não revalida critérios (workspace antigo continua válido)", () => {
    const done = { status: "completed", blocks: [{ id: "b1", title: "B", status: "completed" }], criteria: ["Em texto"] };
    expect(ws.validate(state(done)).ok).toBe(true);
  });
});
