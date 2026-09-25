import { describe, expect, it } from "vitest";
import { fuzzyScore, normalizeSearch } from "./search";

describe("fuzzy search", () => {
  it("ignores accents and case", () => {
    expect(normalizeSearch("Repositórios")).toBe("repositorios");
  });

  it("ranks exact matches above scattered matches", () => {
    expect(fuzzyScore("git", "Git / PRs")).toBeGreaterThan(
      fuzzyScore("git", "Gerar contexto inteligente"),
    );
  });

  it("rejects missing characters", () => {
    expect(fuzzyScore("worktree", "Portas")).toBe(-1);
  });
  it("keeps exact matches in long metadata searchable", () => {
    expect(fuzzyScore("worktree", `${"x".repeat(2000)} worktree`)).toBeGreaterThan(0);
  });
});
