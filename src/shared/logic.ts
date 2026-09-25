import type { GitState, Project, ProjectPort } from "./types";
export function parsePorts(text: string): ProjectPort[] {
  if (!text.trim()) return [];
  const seen = new Set<number>();
  return text.split(",").map((value) => {
    const parts = value.trim().split(":");
    const port = Number(parts.at(-1));
    if (
      parts.length !== 2 ||
      !parts[0].trim() ||
      !/^\d+$/.test(parts[1].trim()) ||
      !Number.isInteger(port) ||
      port < 1 ||
      port > 65535 ||
      seen.has(port)
    )
      throw new Error(
        "Use nome:porta, sem repetições. Ex.: frontend:3000, backend:4000.",
      );
    seen.add(port);
    return { name: parts[0].trim(), port };
  });
}
export function renderPrompt(
  template: string,
  project: Project,
  git?: GitState | null,
  pr?: string,
  goal?: string,
): { text: string; unresolved: string[] } {
  const values: Record<string, string> = {
    project: project.name,
    path: project.localPath,
    branch: git?.branch ?? "NOT VERIFIED",
    ...(goal ? { goal } : {}),
    "project.name": project.name,
    "project.path": project.localPath,
    "project.stack": project.stack.join(", "),
    "git.branch": git?.branch ?? "NOT VERIFIED",
    "git.head": git?.head ?? "NOT VERIFIED",
    "github.pullRequest": pr ?? "NOT VERIFIED",
    "project.instructions":
      "Leia CLAUDE.md e AGENTS.md no repositório; conteúdo não incorporado.",
  };
  const unresolved: string[] = [];
  const text = template.replace(
    /\{\{\s*([\w.]+)\s*\}\}/g,
    (match: string, key: string) => {
      if (Object.hasOwn(values, key)) return values[key];
      unresolved.push(key);
      return match;
    },
  );
  return { text, unresolved };
}
export const bytes = (n: number) => `${(n / 1024 ** 3).toFixed(1)} GB`;
export const percent = (used: number, total: number) =>
  total > 0 ? Math.round((used / total) * 100) : null;
