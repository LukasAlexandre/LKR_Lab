import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ProjectOverviewCard } from "./ProjectsOverview";
import type { ProjectOverview } from "../shared/types";

const base: ProjectOverview = {
  id: "p", name: "Hub Troca de Turno", slug: "p", description: "Relatórios de produção", localPath: "",
  repository: "", stack: ["Google Apps Script", "HTML", "JavaScript", "CSS"], tags: [], ports: [], commands: [],
  createdAt: "", updatedAt: "", location: "unbound",
  git: { status: "not_applicable", data: null, message: "sem pasta" },
  runtime: { status: "not_applicable", data: null, message: "sem pasta" },
  stackSource: "registered", lastActivity: null,
};
const render = (p: ProjectOverview) =>
  renderToStaticMarkup(
    <ProjectOverviewCard project={p} active={false} locating={false} open={() => {}} locate={() => {}} edit={() => {}} remove={() => {}} />,
  );

describe("card de projeto sem pasta", () => {
  it("mostra o aviso e Localizar, sem Git/runtime/branch inventados", () => {
    const html = render(base);
    expect(html).toContain("Não localizado nesta máquina");
    expect(html).toContain("Localizar");
    expect(html).not.toContain("Abrir projeto");
    expect(html).not.toContain("Clean");
    expect(html).not.toContain("Parado");
    expect(html).not.toContain("overview-path");
    expect(html).toContain("Indisponível");
    expect(html).toContain("+1");
  });
  it("binding Missing explica que a pasta sumiu e também localiza", () => {
    const html = render({ ...base, location: "missing", localPath: "C:\\apagada" });
    expect(html).toContain("não existe mais aqui");
    expect(html).not.toContain("C:\\apagada");
    expect(html).toContain("Localizar");
  });
});

describe("card de projeto disponível", () => {
  it("abre o projeto e mostra o caminho local de forma secundária", () => {
    const html = render({
      ...base, location: "available", localPath: "\\\\?\\C:\\Dev\\app", stackSource: "detected",
      git: { status: "available", message: null, data: { isRepo: true, branch: "main", detached: false, upstream: null, ahead: 1, behind: null, staged: 0, unstaged: 0, untracked: 1, conflicts: 0, changes: 1, clean: false, error: null } },
      runtime: { status: "available", message: null, data: { running: true, managedRuns: 0, listeningPorts: [3000] } },
    });
    expect(html).toContain("Abrir projeto");
    expect(html).not.toContain("Não localizado");
    expect(html).toContain("1 alteração");
    expect(html).toContain("Em execução");
    expect(html).toContain("C:\\Dev\\app");
    expect(html).not.toContain("\\\\?\\");
    expect(html).toContain("↑1");
  });
});
