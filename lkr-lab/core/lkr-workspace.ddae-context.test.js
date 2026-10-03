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
    expect(result.state.ddae[0]).toMatchObject(extra);
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
