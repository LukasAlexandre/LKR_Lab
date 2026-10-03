import { describe, expect, it } from "vitest";
import {
  branchLabel, cardAction, displayPath, emptyKind, gitLabel, matchesSearch, relativeTime, runtimeLabel,
  summarize, syncLabel, visibleProjects, visibleStack,
} from "./projectOverview";
import type { GitSummary, ProjectOverview } from "./types";

const git = (over: Partial<GitSummary> = {}): GitSummary => ({
  isRepo: true, branch: "main", detached: false, upstream: null, ahead: null, behind: null,
  staged: 0, unstaged: 0, untracked: 0, conflicts: 0, changes: 0, clean: true, error: null, ...over,
});
function make(id: string, over: Partial<ProjectOverview> = {}): ProjectOverview {
  return {
    id, name: id, slug: id, description: "", localPath: `C:\\x\\${id}`, repository: "", stack: [], tags: [],
    ports: [], commands: [], createdAt: "", updatedAt: "", location: "available",
    git: { status: "available", data: git(), message: null },
    runtime: { status: "available", data: { running: false, managedRuns: 0, listeningPorts: [] }, message: null },
    stackSource: "detected", lastActivity: null, ...over,
  };
}
const running = (id: string, over: Partial<ProjectOverview> = {}) =>
  make(id, { runtime: { status: "available", data: { running: true, managedRuns: 1, listeningPorts: [3000] }, message: null }, ...over });
const dirty = (id: string, over: Partial<ProjectOverview> = {}) =>
  make(id, { git: { status: "available", data: git({ changes: 2, unstaged: 2, clean: false }), message: null }, ...over });
const absent = (id: string, location: "missing" | "unbound", over: Partial<ProjectOverview> = {}) =>
  make(id, {
    location, localPath: "", stackSource: "registered",
    git: { status: "not_applicable", data: null, message: "sem pasta" },
    runtime: { status: "not_applicable", data: null, message: "sem pasta" }, ...over,
  });

const fixtures = [
  running("alfa", { description: "Cockpit do desenvolvedor", stack: ["Tauri", "React"] }),
  dirty("beta", { stack: ["Node.js"], lastActivity: "2026-10-03 10:00:00" }),
  make("gama", { stack: ["Rust"], lastActivity: "2026-10-03 11:00:00" }),
  absent("delta", "missing", { stack: ["Astro"] }),
  absent("epsilon", "unbound"),
];

describe("summarize", () => {
  it("fecha com os cards", () => {
    const s = summarize(fixtures);
    expect(s).toEqual({ total: 5, available: 3, missing: 1, unbound: 1, running: 1, dirty: 1 });
    expect(s.available + s.missing + s.unbound).toBe(s.total);
  });
  it("projeto sem pasta nunca conta como em execução nem com alterações", () => {
    expect(summarize([absent("a", "missing"), absent("b", "unbound")])).toMatchObject({ running: 0, dirty: 0, available: 0 });
  });
  it("ahead/behind sozinho não é alteração local", () => {
    const p = make("x", { git: { status: "available", data: git({ ahead: 2, behind: 1 }), message: null } });
    expect(summarize([p]).dirty).toBe(0);
  });
  it("lista vazia", () => {
    expect(summarize([])).toEqual({ total: 0, available: 0, missing: 0, unbound: 0, running: 0, dirty: 0 });
  });
});

describe("busca", () => {
  it("por nome, descrição e stack, sem diferenciar caixa ou acento", () => {
    expect(visibleProjects(fixtures, "ALF", "all", "name").map((p) => p.id)).toEqual(["alfa"]);
    expect(visibleProjects(fixtures, "cockpit", "all", "name").map((p) => p.id)).toEqual(["alfa"]);
    expect(visibleProjects(fixtures, "rust", "all", "name").map((p) => p.id)).toEqual(["gama"]);
    expect(matchesSearch(make("a", { description: "Relatórios de produção" }), "relatorios")).toBe(true);
  });
  it("não busca no caminho local", () => {
    expect(visibleProjects(fixtures, "C:\\x", "all", "name")).toEqual([]);
  });
  it("vazio devolve tudo", () => {
    expect(visibleProjects(fixtures, "  ", "all", "name")).toHaveLength(5);
  });
});

describe("filtros", () => {
  const ids = (f: Parameters<typeof visibleProjects>[2]) => visibleProjects(fixtures, "", f, "name").map((p) => p.id);
  it("disponíveis", () => expect(ids("available")).toEqual(["alfa", "beta", "gama"]));
  it("não localizados (missing + unbound)", () => expect(ids("unlocated")).toEqual(["delta", "epsilon"]));
  it("em execução", () => expect(ids("running")).toEqual(["alfa"]));
  it("com alterações", () => expect(ids("dirty")).toEqual(["beta"]));
  it("busca e filtro combinados", () => {
    expect(visibleProjects(fixtures, "node", "dirty", "name").map((p) => p.id)).toEqual(["beta"]);
    expect(visibleProjects(fixtures, "node", "running", "name")).toEqual([]);
  });
});

describe("ordenação", () => {
  it("por nome", () => {
    expect(visibleProjects(fixtures, "", "all", "name").map((p) => p.id)).toEqual(["alfa", "beta", "delta", "epsilon", "gama"]);
  });
  it("por atividade recente: sem registro vai para o fim, empate por nome", () => {
    expect(visibleProjects(fixtures, "", "all", "recent").map((p) => p.id)).toEqual(["gama", "beta", "alfa", "delta", "epsilon"]);
  });
});

describe("estado vazio", () => {
  it("distingue sem projetos de nenhum resultado", () => {
    expect(emptyKind(0, 0)).toBe("no-projects");
    expect(emptyKind(5, 0)).toBe("no-results");
    expect(emptyKind(5, 2)).toBeNull();
  });
});

describe("rótulos do card", () => {
  it("disponível: Git, branch e runtime reais", () => {
    expect(gitLabel(make("a"))).toEqual({ text: "Clean", tone: "good" });
    expect(gitLabel(dirty("a")).text).toBe("2 alterações");
    expect(gitLabel(make("a", { git: { status: "available", data: git({ changes: 1, untracked: 1, clean: false }), message: null } })).text).toBe("1 alteração");
    expect(runtimeLabel(running("a"))).toEqual({ text: "Em execução", tone: "blue" });
    expect(runtimeLabel(make("a")).text).toBe("Parado");
    expect(branchLabel(make("a"))).toBe("main");
  });
  it("sem pasta: nada de Clean/Parado/branch inventados", () => {
    const p = absent("a", "unbound");
    expect(gitLabel(p).text).toBe("—");
    expect(runtimeLabel(p).text).toBe("Indisponível");
    expect(branchLabel(p)).toBe("—");
    expect(syncLabel(p)).toBe("");
  });
  it("erro de Git ou de runtime vira Indisponível sem derrubar o card", () => {
    const p = make("a", {
      git: { status: "error", data: null, message: "timeout" },
      runtime: { status: "error", data: null, message: "falhou" },
    });
    expect(gitLabel(p).text).toBe("Indisponível");
    expect(runtimeLabel(p).text).toBe("Indisponível");
  });
  it("conflitos têm destaque e ahead/behind aparece à parte", () => {
    const p = make("a", { git: { status: "available", data: git({ conflicts: 2, ahead: 1, behind: 3, clean: false }), message: null } });
    expect(gitLabel(p).text).toBe("Conflitos (2)");
    expect(syncLabel(p)).toBe("↑1 ↓3");
  });
  it("HEAD destacado", () => {
    expect(branchLabel(make("a", { git: { status: "available", data: git({ detached: true, branch: "(detached)" }), message: null } }))).toBe("HEAD destacado");
  });
});

describe("CTA e stack", () => {
  it("disponível abre; sem pasta localiza", () => {
    expect(cardAction(make("a"))).toBe("open");
    expect(cardAction(absent("a", "missing"))).toBe("locate");
    expect(cardAction(absent("a", "unbound"))).toBe("locate");
  });
  it("limita os chips com +N", () => {
    expect(visibleStack(["a", "b", "c", "d", "e"])).toEqual({ shown: ["a", "b", "c"], extra: 2 });
    expect(visibleStack(["a"])).toEqual({ shown: ["a"], extra: 0 });
    expect(visibleStack([])).toEqual({ shown: [], extra: 0 });
  });
});

describe("displayPath", () => {
  it("remove o prefixo verbatim do Windows", () => {
    expect(displayPath("\\\\?\\C:\\Dev\\app")).toBe("C:\\Dev\\app");
    expect(displayPath("C:\\Dev\\app")).toBe("C:\\Dev\\app");
  });
});

describe("última atividade", () => {
  const now = new Date("2026-10-03T12:00:00Z");
  it("sem registro mostra —", () => {
    expect(relativeTime(null, now)).toBe("—");
    expect(relativeTime("lixo", now)).toBe("—");
  });
  it("tempo relativo a partir do UTC do SQLite", () => {
    expect(relativeTime("2026-10-03 11:59:30", now)).toBe("Agora");
    expect(relativeTime("2026-10-03 11:15:00", now)).toBe("Há 45 min");
    expect(relativeTime("2026-10-03 09:00:00", now)).toBe("Há 3 h");
    expect(relativeTime("2026-10-02 08:00:00", now)).toBe("Ontem");
    expect(relativeTime("2026-09-28T12:00:00.000Z", now)).toBe("Há 5 dias");
  });
});

