import { describe, expect, it } from "vitest";
import "./lkr-portable.js";
import "./lkr-workspace.js";

const ws = globalThis.LKR.workspace;

const project = (extra = {}) => ({
  id: "p1",
  name: "LKR_Lab",
  description: "d",
  repository: "https://github.com/org/lkr",
  stack: ["Rust"],
  tags: ["x"],
  ports: [{ name: "web", port: 3000 }],
  commands: [{ name: "dev", program: "npm", args: ["run", "dev"] }],
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-02T00:00:00Z",
  ...extra,
});
const state = (extra = {}) => ({ version: 1, projects: [project()], prompts: [], knowledge: [], preferences: {}, ...extra });
const invalid = (input) => {
  const result = ws.validate(input);
  expect(result.ok).toBe(false);
  return result.error;
};

describe("workspace portátil: formato", () => {
  it("aceita estado puro e envelope, com o mesmo hash", () => {
    const pure = ws.validate(state());
    const envelope = ws.validate({ source: "lkr-lab", module: "workspace", schemaVersion: 1, state: state() });
    expect(pure.ok).toBe(true);
    expect(envelope.hash).toBe(pure.hash);
    expect(pure.summary).toEqual({ projects: 1, prompts: 0, knowledge: 0 });
  });
  it("não carrega caminho local: campos desconhecidos (localPath) são descartados", () => {
    const result = ws.validate(state({ projects: [project({ localPath: "C:\\Users\\x\\Dev\\p", pathAvailable: true, location: "available" })] }));
    expect(result.ok).toBe(true);
    const json = JSON.stringify(result.state);
    expect(json).not.toMatch(/localPath|Users|pathAvailable|location/);
  });
  it("ordem determinística: listas por id, mesmo hash para qualquer ordem de entrada", () => {
    const a = ws.validate(state({ projects: [project({ id: "b" }), project({ id: "a" })] }));
    const b = ws.validate(state({ projects: [project({ id: "a" }), project({ id: "b" })] }));
    expect(a.state.projects.map((p) => p.id)).toEqual(["a", "b"]);
    expect(a.hash).toBe(b.hash);
    expect(ws.validate(state({ projects: [project({ name: "Outro" })] })).hash).not.toBe(a.hash);
  });
  it("preferências: só campos portáteis e conhecidos", () => {
    const result = ws.validate(state({ preferences: { sidebarCompact: true, density: "compact", promptFavorites: ["b", "a", "a", "../x"], editorPath: "C:\\tools\\code.exe", modalOpen: true } }));
    expect(result.state.preferences).toEqual({ sidebarCompact: true, density: "compact", promptFavorites: ["a", "b"] });
  });
  it("workspace vazio e máquina nova", () => {
    const result = ws.validate({ version: 1 });
    expect(result.ok).toBe(true);
    expect(ws.isEmpty(result.state)).toBe(true);
    expect(ws.isEmpty(ws.validate(state()).state)).toBe(false);
  });
});

describe("workspace portátil: validação", () => {
  it("JSON inválido, versão ausente ou futura", () => {
    expect(invalid(null)).toMatch(/objeto/);
    expect(invalid([])).toMatch(/objeto/);
    expect(invalid({ projects: [] })).toMatch(/version/);
    expect(invalid({ version: 99 })).toMatch(/mais nova/);
    expect(invalid({ source: "lkr-lab", module: "workspace", schemaVersion: 99, state: state() })).toMatch(/mais nova/);
    expect(invalid({ source: "outro", module: "workspace", schemaVersion: 1, state: state() })).toMatch(/não é um workspace/);
    expect(invalid({ source: "lkr-lab", module: "lab-setup", schemaVersion: 1, state: state() })).toMatch(/não é um workspace/);
  });
  it("ids precisam ser seguros e únicos (sem path traversal)", () => {
    for (const bad of ["../x", "a/b", "a b", "", ".hidden", "x".repeat(65)]) {
      expect(invalid(state({ projects: [project({ id: bad })] }))).toMatch(/id inválido/);
    }
    expect(invalid(state({ projects: [project(), project()] }))).toMatch(/duplicado/);
  });
  it("repositório precisa ser HTTPS sem credenciais", () => {
    for (const repo of ["http://github.com/a/b", "git@github.com:a/b.git", "https://u:p@github.com/a/b", "https://github.com/a/b?x=1", "file:///etc/passwd"]) {
      expect(invalid(state({ projects: [project({ repository: repo })] }))).toMatch(/repository/);
    }
  });
  it("comandos não podem carregar caminhos absolutos de outra máquina", () => {
    const withCommand = (command) => state({ projects: [project({ commands: [command] })] });
    expect(invalid(withCommand({ name: "x", program: "C:\\Windows\\cmd.exe", args: [] }))).toMatch(/absoluto/);
    expect(invalid(withCommand({ name: "x", program: "/usr/bin/sh", args: [] }))).toMatch(/absoluto/);
    expect(invalid(withCommand({ name: "x", program: "node", args: ["\\\\srv\\share"] }))).toMatch(/absoluto/);
    expect(invalid(withCommand({ name: "x", program: "node", args: ["~/x"] }))).toMatch(/absoluto/);
  });
  it("secrets nunca são aceitos (prompts, knowledge, descrição)", () => {
    const token = "ghp_" + "a".repeat(36);
    const prompt = { id: "q", title: "T", category: "", projectId: null, body: "use " + token };
    expect(invalid(state({ prompts: [prompt] }))).toMatch(/credencial/);
    const note = { id: "k", projectId: null, title: "T", kind: "note", body: "-----BEGIN RSA PRIVATE KEY-----", tags: "", updatedAt: "" };
    expect(invalid(state({ knowledge: [note] }))).toMatch(/credencial/);
    expect(invalid(state({ projects: [project({ description: "https://user:senha@host.com/x" })] }))).toMatch(/credencial/);
  });
  it("referências precisam existir e tipos de knowledge são fechados", () => {
    const prompt = { id: "q", title: "T", category: "", projectId: "fantasma", body: "b" };
    expect(invalid(state({ prompts: [prompt] }))).toMatch(/projeto que não está/);
    const note = { id: "k", projectId: null, title: "T", kind: "malware", body: "b", tags: "", updatedAt: "" };
    expect(invalid(state({ knowledge: [note] }))).toMatch(/tipo desconhecido/);
  });
  it("prompts e knowledge válidos preservam conteúdo e associação", () => {
    const prompt = { id: "q", title: "T", category: "Dev", projectId: "p1", body: "corpo {{project.name}}" };
    const note = { id: "k", projectId: "p1", title: "N", kind: "decision", body: "b", tags: "a,b", updatedAt: "2026-01-01T00:00:00Z" };
    const result = ws.validate(state({ prompts: [prompt], knowledge: [note] }));
    expect(result.state.prompts).toEqual([prompt]);
    expect(result.state.knowledge).toEqual([note]);
  });
});
