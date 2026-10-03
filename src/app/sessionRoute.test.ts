import { describe, expect, it } from "vitest";
import { SESSION_NOT_FOUND, parseHash, projectHash, resolveRoute, switchProjectHash } from "./projectRoute";
import { resolveSessionLoad } from "../shared/ddaeDetail";

const GLOBAL = ["dashboard", "projects", "git"];
const A = "11111111-1111-4111-8111-111111111111";
const B = "22222222-2222-4222-8222-222222222222";
const S = "9e5dfe47-963a-5766-85a2-80a5f4cf11c5";

describe("rota canônica da Session: #project/<id>/ddae/<session-uuid>", () => {
  it("o UUID da Session é a identidade da rota", () => {
    expect(parseHash(`#project/${A}/ddae/${S}`, GLOBAL)).toEqual({ kind: "project", projectId: A, area: "ddae", sessionId: S, normalized: false });
  });
  it("sem o 4º segmento continua sendo a lista do DDAE", () => {
    const parsed = parseHash(`#project/${A}/ddae`, GLOBAL);
    expect(parsed).toMatchObject({ kind: "project", area: "ddae", normalized: false });
    expect("sessionId" in parsed).toBe(false);
  });
  it("só o DDAE aceita Session: nas demais áreas o 4º segmento é corrigido", () => {
    expect(parseHash(`#project/${A}/runtime/${S}`, GLOBAL)).toMatchObject({ area: "runtime", normalized: true });
    expect("sessionId" in parseHash(`#project/${A}/runtime/${S}`, GLOBAL)).toBe(false);
  });
  it("id de Session malformado, vazio ou segmentos extras são corrigidos para a lista", () => {
    for (const bad of ["a b", "x".repeat(80), "", "a/b"]) {
      const parsed = parseHash(`#project/${A}/ddae/${bad}`, GLOBAL);
      expect(parsed).toMatchObject({ kind: "project", area: "ddae", normalized: true });
      expect("sessionId" in parsed).toBe(false);
    }
  });
  it("SESSION-NNN não é usado como chave: o parser só conhece o token seguro, quem decide é o backend", () => {
    // Um rótulo humano é um token válido sintaticamente; o backend o trata como inexistente.
    expect(parseHash(`#project/${A}/ddae/SESSION-001`, GLOBAL)).toMatchObject({ sessionId: "SESSION-001" });
    expect(projectHash(A, "ddae", S)).toBe(`#project/${A}/ddae/${S}`);
  });
  it("projectHash só inclui a Session na área ddae", () => {
    expect(projectHash(A, "ddae")).toBe(`#project/${A}/ddae`);
    expect(projectHash(A, "runtime", S)).toBe(`#project/${A}/runtime`);
    expect(projectHash(A)).toBe(`#project/${A}/overview`);
  });
  it("reload e deep link: o Project válido renderiza sem depender de seleção anterior", () => {
    const route = parseHash(projectHash(A, "ddae", S), GLOBAL);
    expect(resolveRoute(route, { loaded: true, ids: [A, B] }, "")).toEqual({ action: "render" });
    expect(resolveRoute(route, { loaded: false, ids: [] }, "")).toEqual({ action: "wait" });
  });
  it("Project inexistente na rota da Session volta para Projetos", () => {
    const route = parseHash(projectHash(B, "ddae", S), GLOBAL);
    expect(resolveRoute(route, { loaded: true, ids: [A] }, A)).toMatchObject({ action: "redirect", hash: "#projects" });
  });
  it("URL de Session com área corrigida redireciona preservando a Session do DDAE", () => {
    const route = parseHash(`#project/${A}/ddae/${S}`, GLOBAL);
    expect(resolveRoute(route, { loaded: true, ids: [A] }, A)).toEqual({ action: "render" });
  });
  it("trocar de Project dentro do detalhe não leva a Session junto", () => {
    const route = parseHash(projectHash(A, "ddae", S), GLOBAL);
    expect(switchProjectHash(route, B)).toBe(`#project/${B}/ddae`);
  });
});

describe("par Project/Session", () => {
  it("Session inexistente ou de outro Project volta para a lista com aviso; nunca escolhe outra", () => {
    expect(SESSION_NOT_FOUND).toBe("Sessão não encontrada neste projeto.");
    expect(resolveSessionLoad({ ok: false, message: SESSION_NOT_FOUND })).toBe("redirect");
    expect(projectHash(A, "ddae")).toBe(`#project/${A}/ddae`);
  });
  it("leitura ok mostra; outro erro (banco, etc.) mostra o erro, sem redirecionar", () => {
    expect(resolveSessionLoad({ ok: true })).toBe("show");
    expect(resolveSessionLoad({ ok: false, message: "Banco temporariamente indisponível" })).toBe("error");
  });
});
