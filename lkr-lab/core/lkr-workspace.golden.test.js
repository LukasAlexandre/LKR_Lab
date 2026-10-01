import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import "./lkr-portable.js";
import "./lkr-workspace.js";

// O mesmo arquivo é conferido pelo Rust (crates/hub-core/tests/sync.rs): as duas
// formas canônicas precisam concordar, senão o desktop veria divergências fantasma.
const golden = JSON.parse(readFileSync(new URL("../../crates/hub-core/tests/fixtures/workspace-golden.json", import.meta.url), "utf8"));

describe("workspace: forma canônica compartilhada com o Rust", () => {
  it("o estado do Rust é ponto fixo do canonical() do JS", () => {
    const result = globalThis.LKR.workspace.validate(golden);
    expect(result.ok).toBe(true);
    expect(result.state).toEqual(golden);
  });
  it("entrada fora de ordem e com espaços converge para a mesma forma", () => {
    const messy = structuredClone(golden);
    messy.projects.reverse();
    messy.prompts.reverse();
    messy.projects[1].name = "  Alpha ";
    messy.projects[1].stack = [" Rust ", "", "React"];
    messy.preferences.promptFavorites = ["mine", "audit", "audit"];
    expect(globalThis.LKR.workspace.validate(messy).state).toEqual(golden);
  });
});
