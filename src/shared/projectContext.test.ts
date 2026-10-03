import { describe, expect, it } from "vitest";
import { deriveProjectNextAction, runtimeHeadline, worktreeSummary } from "./projectContext";
import type { GitSummary, ProjectOverview } from "./types";

const git = (over: Partial<GitSummary> = {}): GitSummary => ({
  isRepo: true, branch: "main", detached: false, upstream: null, ahead: null, behind: null,
  staged: 0, unstaged: 0, untracked: 0, conflicts: 0, changes: 0, clean: true, error: null, ...over,
});
function make(over: Partial<ProjectOverview> = {}): ProjectOverview {
  return {
    id: "p", name: "p", slug: "p", description: "", localPath: "C:\\x", repository: "", stack: [], tags: [],
    ports: [], commands: [], createdAt: "", updatedAt: "", location: "available",
    git: { status: "available", data: git(), message: null },
    runtime: { status: "available", data: { running: false, managedRuns: 0, listeningPorts: [] }, message: null },
    stackSource: "detected", lastActivity: null, ...over,
  };
}
const absent = (location: "missing" | "unbound") =>
  make({
    location, localPath: "", stackSource: "registered",
    git: { status: "not_applicable", data: null, message: "sem pasta" },
    runtime: { status: "not_applicable", data: null, message: "sem pasta" },
  });

describe("deriveProjectNextAction", () => {
  it("Missing e Unbound: Localizar projeto, sem olhar Git/runtime", () => {
    for (const location of ["missing", "unbound"] as const) {
      const a = deriveProjectNextAction(absent(location), false);
      expect(a).toMatchObject({ id: "locate", cta: "Localizar", target: { kind: "locate" } });
    }
  });
  it("conflitos vencem alterações", () => {
    const p = make({ git: { status: "available", data: git({ conflicts: 2, changes: 5, clean: false }), message: null } });
    expect(deriveProjectNextAction(p, false)).toMatchObject({ id: "conflicts", target: { kind: "area", area: "git" } });
    expect(deriveProjectNextAction(p, false).description).toContain("2 arquivos em conflito");
  });
  it("alterações locais: revisar no Git, com plural correto", () => {
    const one = make({ git: { status: "available", data: git({ changes: 1, untracked: 1, clean: false }), message: null } });
    const many = make({ git: { status: "available", data: git({ changes: 3, unstaged: 3, clean: false }), message: null } });
    expect(deriveProjectNextAction(one, null)).toMatchObject({ id: "review-changes", target: { kind: "area", area: "git" } });
    expect(deriveProjectNextAction(one, null).description).toContain("1 alteração");
    expect(deriveProjectNextAction(many, null).description).toContain("3 alterações");
  });
  it("ahead/behind sozinho não é urgência", () => {
    const p = make({ git: { status: "available", data: git({ ahead: 3, behind: 1 }), message: null } });
    expect(deriveProjectNextAction(p, true).id).toBe("none");
  });
  it("problema real de runtime", () => {
    const p = make({ runtime: { status: "error", data: null, message: "Portas: sem acesso" } });
    expect(deriveProjectNextAction(p, true)).toMatchObject({ id: "review-runtime", target: { kind: "area", area: "runtime" } });
    expect(deriveProjectNextAction(p, true).description).toBe("Portas: sem acesso");
  });
  it("Git indisponível não inventa ação; runtime parado não é problema", () => {
    const p = make({ git: { status: "error", data: null, message: "timeout" } });
    expect(deriveProjectNextAction(p, true).id).toBe("none");
  });
  it("contexto nunca gerado sugere gerar; desconhecido não dispara", () => {
    expect(deriveProjectNextAction(make(), false)).toMatchObject({ id: "generate-context", target: { kind: "context" } });
    expect(deriveProjectNextAction(make(), null).id).toBe("none");
    expect(deriveProjectNextAction(make(), true).id).toBe("none");
  });
  it("nada urgente: sem CTA e sem DDAE/sessão inventada", () => {
    const a = deriveProjectNextAction(make(), true);
    expect(a).toMatchObject({ id: "none", cta: null, target: { kind: "none" } });
    expect(JSON.stringify(a)).not.toMatch(/SESSION|sessão/i);
  });
  it("é determinística", () => {
    const p = make();
    expect(deriveProjectNextAction(p, false)).toEqual(deriveProjectNextAction(p, false));
  });
});

describe("worktreeSummary", () => {
  it("só fatos do Git", () => {
    const s = worktreeSummary([
      { path: "a", head: "1", branch: "main", locked: false },
      { path: "b", head: "2", branch: "feat/x", locked: true },
      { path: "c", head: "3", branch: "", locked: false },
    ]);
    expect(s).toEqual({ count: 3, branches: ["main", "feat/x"], locked: 1 });
    expect(worktreeSummary([])).toEqual({ count: 0, branches: [], locked: 0 });
  });
});

describe("runtimeHeadline", () => {
  it("rótulos reais", () => {
    expect(runtimeHeadline(make())).toBe("Parado");
    expect(runtimeHeadline(make({ runtime: { status: "available", data: { running: true, managedRuns: 1, listeningPorts: [1420, 4317] }, message: null } }))).toBe("Em execução · 1 execução · portas 1420, 4317");
    expect(runtimeHeadline(absent("missing"))).toBe("Indisponível");
  });
});
