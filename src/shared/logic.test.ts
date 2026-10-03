import { describe, expect, it } from "vitest";
import { parsePorts, renderPrompt, percent, changesLabel } from "./logic";
import type { Project } from "./types";
const project: Project = {
  id: "1",
  name: "LK Wallet",
  slug: "lk-wallet",
  localPath: "C:/Dev/Wallet",
  repository: "",
  description: "",
  stack: ["React", "Rust"],
  tags: [],
  ports: [],
  commands: [],
  createdAt: "",
  updatedAt: "",
};
describe("project ports", () => {
  it("parses named ports", () =>
    expect(parsePorts("web:3000, api:4000")).toEqual([
      { name: "web", port: 3000 },
      { name: "api", port: 4000 },
    ]));
  it.each([
    "web:0",
    "api:65536",
    "web:3000, api:3000",
    ":80",
    "web:12.5",
    "web:1e3",
    "web:abc",
    "web:80:90",
  ])("rejects invalid input %s", (value) =>
    expect(() => parsePorts(value)).toThrow(),
  );
  it("permits no declared ports", () => expect(parsePorts("")).toEqual([]));
});
describe("prompt rendering", () => {
  it("uses actual project context and labels missing Git", () =>
    expect(renderPrompt("{{project.name}} {{git.branch}}", project).text).toBe(
      "LK Wallet NOT VERIFIED",
    ));
  it("preserves and reports unknown variables", () =>
    expect(renderPrompt("{{unknown}}", project)).toEqual({
      text: "{{unknown}}",
      unresolved: ["unknown"],
    }));
  it("does not recursively substitute user strings", () =>
    expect(
      renderPrompt("{{project.name}}", { ...project, name: "{{git.head}}" })
        .text,
    ).toBe("{{git.head}}"));
});
it("does not manufacture metrics without a denominator", () =>
  expect(percent(10, 0)).toBeNull());

describe("changesLabel", () => {
  it("pluraliza corretamente", () => {
    expect(changesLabel(1)).toBe("1 alteração");
    expect(changesLabel(2)).toBe("2 alterações");
    expect(changesLabel(0)).toBe("0 alterações");
  });
});
