import { describe, expect, it } from "vitest";
import { parseHash, projectHash, resolveRoute, switchProjectHash } from "./projectRoute";

const GLOBAL = ["dashboard", "projects", "git", "worktrees"];
const A = "11111111-1111-4111-8111-111111111111";
const B = "22222222-2222-4222-8222-222222222222";
const loaded = (...ids: string[]) => ({ loaded: true, ids });

describe("parseHash", () => {
  it("rota canônica de Project (overview)", () => {
    expect(parseHash(`#project/${A}/overview`, GLOBAL)).toEqual({ kind: "project", projectId: A, area: "overview", normalized: false });
  });
  it("cada área válida", () => {
    for (const area of ["ddae", "worktrees", "planning", "runtime", "git", "logs", "context"] as const) {
      expect(parseHash(`#project/${A}/${area}`, GLOBAL)).toMatchObject({ kind: "project", area, normalized: false });
    }
  });
  it("área inválida ou ausente cai em overview e pede correção da URL", () => {
    expect(parseHash(`#project/${A}/nada`, GLOBAL)).toEqual({ kind: "project", projectId: A, area: "overview", normalized: true });
    expect(parseHash(`#project/${A}`, GLOBAL)).toMatchObject({ area: "overview", normalized: true });
    expect(parseHash(`#project/${A}/runtime/extra`, GLOBAL)).toMatchObject({ area: "runtime", normalized: true });
  });
  it("id ausente ou malformado é inválido", () => {
    expect(parseHash("#project", GLOBAL)).toEqual({ kind: "invalid-project" });
    expect(parseHash("#project//overview", GLOBAL)).toEqual({ kind: "invalid-project" });
    expect(parseHash("#project/a b/overview", GLOBAL)).toEqual({ kind: "invalid-project" });
    expect(parseHash(`#project/${"x".repeat(80)}/overview`, GLOBAL)).toEqual({ kind: "invalid-project" });
  });
  it("rotas de projetos e legado", () => {
    expect(parseHash("#projects", GLOBAL)).toEqual({ kind: "global", route: "projects" });
    expect(parseHash("#projects/new", GLOBAL)).toEqual({ kind: "new-project" });
    expect(parseHash("#projects/open", GLOBAL)).toEqual({ kind: "legacy-open" });
  });
  it("rotas globais não são afetadas; desconhecida vira dashboard", () => {
    expect(parseHash("#git", GLOBAL)).toEqual({ kind: "global", route: "git" });
    expect(parseHash("#worktrees", GLOBAL)).toEqual({ kind: "global", route: "worktrees" });
    expect(parseHash("", GLOBAL)).toEqual({ kind: "global", route: "dashboard" });
    expect(parseHash("#zzz", GLOBAL)).toEqual({ kind: "global", route: "dashboard" });
  });
});

describe("resolveRoute", () => {
  it("Project válido renderiza (reload com deep link)", () => {
    expect(resolveRoute(parseHash(projectHash(A, "runtime"), GLOBAL), loaded(A, B), "")).toEqual({ action: "render" });
  });
  it("não decide antes de o registro carregar", () => {
    expect(resolveRoute(parseHash(projectHash(A), GLOBAL), { loaded: false, ids: [] }, A)).toEqual({ action: "wait" });
    expect(resolveRoute({ kind: "legacy-open" }, { loaded: false, ids: [] }, A)).toEqual({ action: "wait" });
  });
  it("id inexistente volta para Projetos com aviso, sem eleger outro Project", () => {
    expect(resolveRoute(parseHash(projectHash(B), GLOBAL), loaded(A), A)).toEqual({ action: "redirect", hash: "#projects", notice: "Projeto não encontrado." });
    expect(resolveRoute({ kind: "invalid-project" }, loaded(A), A)).toMatchObject({ hash: "#projects", notice: "Projeto não encontrado." });
  });
  it("Project removido enquanto aberto sai do contexto", () => {
    const route = parseHash(projectHash(A, "git"), GLOBAL);
    expect(resolveRoute(route, loaded(A, B), A)).toEqual({ action: "render" });
    expect(resolveRoute(route, loaded(B), A)).toMatchObject({ action: "redirect", hash: "#projects" });
  });
  it("legado #projects/open usa o Project ativo válido", () => {
    expect(resolveRoute({ kind: "legacy-open" }, loaded(A, B), B)).toEqual({ action: "redirect", hash: projectHash(B) });
  });
  it("legado sem Project ativo válido vai para Projetos", () => {
    expect(resolveRoute({ kind: "legacy-open" }, loaded(A), "")).toEqual({ action: "redirect", hash: "#projects" });
    expect(resolveRoute({ kind: "legacy-open" }, loaded(A), B)).toEqual({ action: "redirect", hash: "#projects" });
  });
  it("área inválida é normalizada para a rota canônica", () => {
    expect(resolveRoute(parseHash(`#project/${A}/xx`, GLOBAL), loaded(A), "")).toEqual({ action: "redirect", hash: projectHash(A) });
  });
  it("rotas globais e cadastro passam direto, mesmo sem registro", () => {
    expect(resolveRoute({ kind: "global", route: "git" }, { loaded: false, ids: [] }, "")).toEqual({ action: "render" });
    expect(resolveRoute({ kind: "new-project" }, { loaded: false, ids: [] }, "")).toEqual({ action: "render" });
  });
});

describe("switchProjectHash", () => {
  it("troca de Project preservando a área", () => {
    expect(switchProjectHash(parseHash(projectHash(A, "runtime"), GLOBAL), B)).toBe(projectHash(B, "runtime"));
    expect(switchProjectHash(parseHash(projectHash(A, "git"), GLOBAL), B)).toBe(`#project/${B}/git`);
  });
  it("fora do contexto não há rota para trocar", () => {
    expect(switchProjectHash({ kind: "global", route: "git" }, B)).toBeNull();
    expect(switchProjectHash({ kind: "global", route: "projects" }, B)).toBeNull();
  });
});
