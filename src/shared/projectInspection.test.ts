import { describe, expect, it } from "vitest";
import {
  applyInspection,
  canRegister,
  counter,
  DESCRIPTION_MAX,
  editDescription,
  editName,
  initialView,
  locateTarget,
  NAME_MAX,
  phaseOf,
  primaryAction,
  startAnalysis,
  validateDescription,
  validateName,
} from "./projectInspection";
import type { MatchedProject, ProjectInspection, RegistrationStatus } from "./types";

const match = (over: Partial<MatchedProject> = {}): MatchedProject => ({
  id: "p1",
  name: "LKR LAB",
  location: "unbound",
  boundPath: null,
  canLocate: true,
  reason: null,
  ...over,
});
const inspection = (status: RegistrationStatus = "new", over: Partial<ProjectInspection> = {}): ProjectInspection => ({
  folder: "C:/work/demo",
  valid: status !== "invalid",
  error: status === "invalid" ? "Pasta não encontrada ou sem acesso." : null,
  suggestedName: "demo",
  git: null,
  locator: null,
  locatorNote: null,
  repository: "",
  stack: [],
  packageManager: null,
  packageManagerNote: null,
  scripts: [],
  importantFiles: [],
  structure: [],
  registration: { status, matches: status === "known" ? [match()] : [], message: "" },
  warnings: [],
  ...over,
});
const ready = () => applyInspection(startAnalysis(initialView, "C:/work/demo"), inspection());

describe("validação de nome e descrição", () => {
  it("nome obrigatório e até 50 caracteres", () => {
    expect(validateName("   ")).not.toBeNull();
    expect(validateName("x".repeat(NAME_MAX))).toBeNull();
    expect(validateName("x".repeat(NAME_MAX + 1))).not.toBeNull();
  });
  it("descrição opcional e até 200 caracteres, com contador", () => {
    expect(validateDescription("")).toBeNull();
    expect(validateDescription("d".repeat(DESCRIPTION_MAX))).toBeNull();
    expect(validateDescription("d".repeat(DESCRIPTION_MAX + 1))).not.toBeNull();
    expect(counter("abc", NAME_MAX)).toBe("3/50");
  });
});

describe("fases da tela", () => {
  it("vazia, analisando, pronta", () => {
    expect(phaseOf(initialView)).toBe("empty");
    expect(phaseOf(startAnalysis(initialView, "C:/work/demo"))).toBe("analyzing");
    expect(phaseOf(ready())).toBe("ready");
  });
  it("pasta inválida nunca cadastra", () => {
    const view = applyInspection(startAnalysis(initialView, "C:/work/demo"), inspection("invalid"));
    expect(phaseOf(view)).toBe("invalid");
    expect(canRegister({ ...view, name: "ok" })).toBe(false);
    expect(primaryAction(view)).toBe("none");
  });
  it("pasta válida sem stack é permitida", () => {
    const view = ready();
    expect(view.inspection?.stack).toEqual([]);
    expect(canRegister(view)).toBe(true);
  });
  it("projeto já conhecido oferece Localizar e nunca Cadastrar", () => {
    const view = applyInspection(startAnalysis(initialView, "C:/work/demo"), inspection("known"));
    expect(phaseOf(view)).toBe("known");
    expect(canRegister(view)).toBe(false);
    expect(primaryAction(view)).toBe("locate");
    expect(locateTarget(view)?.id).toBe("p1");
  });
  it("conhecido com vínculo válido em outra pasta não oferece associar", () => {
    const known = inspection("known", {
      registration: { status: "known", matches: [match({ canLocate: false, location: "available" })], message: "" },
    });
    const view = applyInspection(startAnalysis(initialView, "C:/work/demo"), known);
    expect(primaryAction(view)).toBe("none");
    expect(locateTarget(view)).toBeNull();
  });
  it("ambíguo e já cadastrado aqui não decidem sozinhos", () => {
    for (const status of ["ambiguous", "already_here"] as const) {
      const view = applyInspection(startAnalysis(initialView, "C:/work/demo"), inspection(status));
      expect(phaseOf(view)).toBe(status);
      expect(primaryAction(view)).toBe("none");
      expect(canRegister(view)).toBe(false);
    }
  });
  it("registrando e erro têm prioridade e bloqueiam o cadastro", () => {
    expect(phaseOf({ ...ready(), registering: true })).toBe("registering");
    expect(canRegister({ ...ready(), registering: true })).toBe(false);
    const failed = { ...ready(), error: "falha" };
    expect(phaseOf(failed)).toBe("error");
    expect(canRegister(failed)).toBe(false);
  });
  it("cadastrar exige nome e descrição válidos", () => {
    const view = ready();
    expect(canRegister(editName(view, ""))).toBe(false);
    expect(canRegister(editName(view, "x".repeat(51)))).toBe(false);
    expect(canRegister(editDescription(view, "d".repeat(201)))).toBe(false);
    expect(canRegister(editName(view, "Meu projeto"))).toBe(true);
  });
});

describe("Reanalisar preserva o que o usuário digitou", () => {
  it("sugere o nome só quando o usuário não mexeu", () => {
    expect(ready().name).toBe("demo");
  });
  it("nome e descrição editados sobrevivem a uma nova análise", () => {
    let view = editDescription(editName(ready(), "Nome meu"), "minha descrição");
    view = applyInspection(startAnalysis(view, view.folder), inspection("new", { suggestedName: "outro" }));
    expect(view.name).toBe("Nome meu");
    expect(view.description).toBe("minha descrição");
  });
  it("nome apagado de propósito não volta sozinho", () => {
    const cleared = editName(ready(), "");
    const again = applyInspection(startAnalysis(cleared, cleared.folder), inspection());
    expect(again.nameTouched).toBe(true);
    expect(again.name).toBe("");
  });
  it("sugestão respeita o limite de 50 caracteres", () => {
    const view = applyInspection(startAnalysis(initialView, "C:/work/demo"), inspection("new", { suggestedName: "n".repeat(80) }));
    expect(view.name.length).toBe(NAME_MAX);
  });
  it("análise nova limpa o erro anterior", () => {
    const view = applyInspection({ ...ready(), error: "falha" }, inspection());
    expect(view.error).toBe("");
  });
});
