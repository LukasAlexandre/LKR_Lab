import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ProjectContextNav } from "./ProjectContextNav";
import { ProjectOverviewArea } from "./ProjectOverviewArea";
import { MachineContext } from "../../state/machine";
import type { MachineRegistry } from "../../state/machine";
import type { GitSummary, ProjectOverview } from "../../shared/types";

const ID = "11111111-1111-4111-8111-111111111111";

describe("PROJETO ATUAL (sidebar contextual)", () => {
  const html = renderToStaticMarkup(<ProjectContextNav projectId={ID} name="Meu Projeto" area="runtime" compact={false} />);
  it("lista as 8 áreas, todas com o ID do Project no link", () => {
    expect(html).toContain("PROJETO ATUAL");
    for (const title of ["Visão geral", "DDAE / Sessões", "Worktrees", "Planejamento", "Runtime", "Git", "Logs", "Contexto IA"]) {
      expect(html).toContain(title);
    }
    expect((html.match(/href="#project\//g) ?? []).length).toBe(8);
    expect(html).toContain(`href="#project/${ID}/runtime"`);
    expect(html).toContain(`href="#project/${ID}/overview"`);
  });
  it("marca somente a área atual", () => {
    expect((html.match(/aria-current="page"/g) ?? []).length).toBe(1);
    expect(html).toMatch(/class="nav-item active"[^>]*aria-current="page"[^>]*>(?:(?!<\/a>).)*Runtime/s);
  });
});

const git = (over: Partial<GitSummary> = {}): GitSummary => ({
  isRepo: true, branch: "main", detached: false, upstream: null, ahead: null, behind: null,
  staged: 0, unstaged: 0, untracked: 0, conflicts: 0, changes: 0, clean: true, error: null, ...over,
});
const base: ProjectOverview = {
  id: ID, name: "Meu Projeto", slug: "p", description: "", localPath: "C:\\Dev\\app", repository: "", stack: ["Node.js"], tags: [],
  ports: [], commands: [], createdAt: "", updatedAt: "", location: "available",
  git: { status: "available", data: git(), message: null },
  runtime: { status: "available", data: { running: false, managedRuns: 0, listeningPorts: [] }, message: null },
  stackSource: "detected", lastActivity: null,
};
const machine = { status: { machine: { name: "PC Teste" } } } as unknown as MachineRegistry;
const render = (p: ProjectOverview) =>
  renderToStaticMarkup(
    <MachineContext.Provider value={machine}>
      <ProjectOverviewArea project={p} go={() => {}} locate={() => {}} generateContext={() => {}} refreshKey={0} />
    </MachineContext.Provider>,
  );

describe("Visão geral", () => {
  it("DDAE vem do backend (sem sessão de exemplo) e só Planejamento segue como placeholder", () => {
    const html = render(base);
    expect(html).toContain("DDAE / Sessões");
    expect(html).toContain("Planejamento");
    // Fora do aplicativo desktop não há banco: o card diz isso em vez de inventar sessões.
    expect(html).toContain("Disponível apenas no aplicativo desktop");
    expect((html.match(/Ainda não disponível/g) ?? []).length).toBe(1);
    expect(html).not.toMatch(/SESSION-|ATIVA|\d+ ?\/ ?10/);
  });
  it("projeto disponível: Git, branch, Runtime e worktrees reais", () => {
    const html = render(base);
    expect(html).toContain("Clean");
    expect(html).toContain("main");
    expect(html).toContain("Parado");
    expect(html).toContain("Worktrees");
    expect(html).toContain("PC Teste");
    expect(html).toContain("C:\\Dev\\app");
  });
  it("sem atividade carregada não inventa eventos", () => {
    const html = render(base);
    expect(html).not.toContain("pcc-activity");
  });
  it("Missing: sem Git/Runtime/branch/path falsos e com Localizar", () => {
    const html = render({
      ...base, location: "missing", localPath: "C:\\apagada", stackSource: "registered",
      git: { status: "not_applicable", data: null, message: "x" },
      runtime: { status: "not_applicable", data: null, message: "x" },
    });
    expect(html).toContain("Não localizado nesta máquina");
    expect(html).toContain("Localizar");
    expect(html).not.toContain("Clean");
    expect(html).not.toContain("Parado");
    expect(html).not.toContain("C:\\apagada");
    expect(html).not.toContain("Abrir Git");
    expect(html).not.toContain("Abrir Runtime");
    expect(html).toContain("Próxima ação");
  });
});
