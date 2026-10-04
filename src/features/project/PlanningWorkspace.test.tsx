import { renderToStaticMarkup } from "react-dom/server";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { DdaeOverview, DdaeSessionStatus, PlanningOverview, PlanningPhase, PlanningRow, ProjectsOverview } from "../../shared/types";

// O componente só fala com o backend pelo `api`; aqui ele devolve estados fixos e finge ser o desktop.
const planning = new Map<string, PlanningOverview>();
const ddae = new Map<string, DdaeOverview>();
let projects: ProjectsOverview = { projects: [], totals: {} as ProjectsOverview["totals"] };
vi.mock("../../shared/api", () => ({
  desktop: true,
  errorText: (e: unknown) => String(e),
  api: async (command: string, args?: { projectId?: string }) => {
    if (command === "planning_overview") return planning.get(args?.projectId ?? "");
    if (command === "ddae_overview") return ddae.get(args?.projectId ?? "");
    if (command === "project_overviews") return projects;
    return null;
  },
}));

const { PlanningWorkspace } = await import("./PlanningWorkspace");
const { workspace } = await import("../../state/workspace");

const session = (number: number, status: DdaeSessionStatus, completed: number, total: number) => ({
  id: `s${number}`, number, label: `SESSION-${String(number).padStart(3, "0")}`, title: `Sessão ${number}`, status, progress: { completed, total },
});
function row(n: number, title: string, phase: PlanningPhase, over: Partial<PlanningRow> = {}): PlanningRow {
  return {
    id: `i${n}`, projectId: "p", title, description: `Descrição de ${title}`, position: n * 1000, storedStatus: phase === "cancelled" ? "cancelled" : "open",
    createdAt: "", updatedAt: "", phase, session: null, canStart: false, disabledReason: null,
    canEdit: phase === "planned", canCancel: phase === "planned", canRestore: phase === "cancelled",
    canMoveUp: false, canMoveDown: false, lastActivityAt: "2026-10-03T20:00:00Z", ...over,
  };
}
function build(projectId: string, rows: PlanningRow[], activeSession: PlanningOverview["activeSession"]): PlanningOverview {
  const count = (phase: PlanningPhase) => rows.filter((r) => r.phase === phase).length;
  const firstPlanned = rows.find((r) => r.phase === "planned");
  return {
    projectId, items: rows, activeSession,
    counts: { operational: rows.length - count("cancelled"), planned: count("planned"), executing: count("executing"), completed: count("completed"), cancelled: count("cancelled") },
    next: firstPlanned ? { id: firstPlanned.id, title: firstPlanned.title, description: firstPlanned.description ?? "", canStart: firstPlanned.canStart, disabledReason: firstPlanned.disabledReason } : null,
  };
}

const REASON = "Já existe uma Session ativa neste projeto.";
const EMPTY = "p-empty";
const BLOCKED = "p-blocked";
const FREE = "p-free";

const queue = (blocked: boolean): PlanningRow[] => {
  const start = { canStart: !blocked, disabledReason: blocked ? REASON : null };
  return [
    row(1, "Configuração de ambientes por projeto", "planned", { ...start, canMoveDown: true }),
    row(2, "Interface principal do LKR LAB", "executing", { session: session(5, "frozen", 6, 9) }),
    row(3, "Project Runtime Manager V2", "executing", { session: session(6, "stopped", 4, 7) }),
    row(4, "Storage & Sync (GitHub)", "completed", { session: session(4, "completed", 10, 10) }),
    row(5, "Templates de projetos", "planned", { ...start, canMoveUp: true, canMoveDown: true }),
    row(6, "Segurança e Hardening", "cancelled", { cancelReason: "Fora do escopo agora." }),
    row(7, "Temas e Personalização", "planned", { ...start, canMoveUp: true, canMoveDown: true }),
    row(8, "Sistema de Plugins", "planned", { ...start, canMoveUp: true }),
  ];
};

beforeAll(async () => {
  planning.set(EMPTY, build(EMPTY, [], { id: "s1", number: 1, label: "SESSION-001" }));
  planning.set(BLOCKED, build(BLOCKED, queue(true), { id: "s1", number: 1, label: "SESSION-001" }));
  const activeRow = row(9, "Plugins ativo", "executing", { session: session(7, "active", 2, 5) });
  planning.set(FREE, build(FREE, [row(1, "Primeiro item", "planned", { canStart: true }), activeRow].slice(0, 1), null));
  const active = {
    id: "s1", number: 1, label: "SESSION-001", title: "Machine Context & Workspace Foundation", status: "active", progress: { completed: 9, total: 10 },
    currentBlock: { id: "b9", title: "Concept 09 — Planejamento", status: "in_progress" }, nextBlock: null,
  };
  const overview = (sessions: unknown[], activeSessionId: string | null) =>
    ({ projectId: "", sessions, counts: { total: sessions.length, active: sessions.length, frozen: 0, stopped: 0, completed: 0 }, blocksTotal: 10, activeSessionId, legacyImport: "not_applicable" }) as unknown as DdaeOverview;
  ddae.set(EMPTY, overview([active], "s1"));
  ddae.set(BLOCKED, overview([active], "s1"));
  ddae.set(FREE, overview([], null));
  const git = { isRepo: true, branch: "main", detached: false, upstream: null, ahead: null, behind: null, staged: 0, unstaged: 0, untracked: 0, conflicts: 0, changes: 0, clean: true, error: null };
  const project = (id: string) => ({
    id, name: id, slug: id, description: "", localPath: "C:\\x", repository: "", stack: [], tags: [], ports: [], commands: [], createdAt: "", updatedAt: "", location: "available",
    git: { status: "available", data: git, message: null }, runtime: { status: "available", data: { running: false, managedRuns: 0, listeningPorts: [] }, message: null },
    stackSource: "detected", lastActivity: null,
  });
  projects = { projects: [project(EMPTY), project(BLOCKED), project(FREE)], totals: {} as ProjectsOverview["totals"] } as unknown as ProjectsOverview;
  await workspace.overviews.refresh();
  for (const id of [EMPTY, BLOCKED, FREE]) {
    await workspace.forProject(id).planning.refresh();
    await workspace.forProject(id).ddae.refresh();
  }
});

const render = (id: string) => renderToStaticMarkup(<PlanningWorkspace projectId={id} notify={() => {}} />);

describe("Planejamento sem itens (estado real da SESSION-001)", () => {
  let html = "";
  beforeAll(() => { html = render(EMPTY); });
  it("mostra o empty state honesto, preservando o layout do módulo", () => {
    expect(html).toContain("Nenhum item no planejamento.");
    expect(html).toContain("Novo item");
    expect(html).toContain("Planejamento");
    expect(html).toContain("PRÓXIMO");
    expect(html).toContain("Nenhum item planejado");
    expect(html).toContain("Itens operacionais");
    expect(html).not.toContain("<table");
  });
  it("não fabrica o mock: nenhum item, nenhuma Session de exemplo", () => {
    expect(html).not.toMatch(/Templates de projetos|Sistema de Plugins|SESSION-00[2-9]|Configuração de ambientes/);
  });
  it("a SESSION-001 aparece só no bloco inferior, com o progresso real e a Próxima ação separada", () => {
    const footer = html.slice(html.indexOf("pl-footer"));
    expect(footer).toContain("SESSION-001");
    expect(footer).toContain("9 / 10");
    expect(footer).toContain("Concept 09 — Planejamento");
    expect(footer).toContain("Próxima ação");
    expect(footer).toContain("Continuar SESSION-001");
    expect(html.slice(0, html.indexOf("pl-footer"))).not.toContain("SESSION-001");
  });
});

describe("Planejamento com Session ativa (versão aprovada)", () => {
  let html = "";
  beforeAll(() => { html = render(BLOCKED); });
  it("cabeçalho: Atualizar e Novo item sem dropdown", () => {
    expect(html).toContain("Organize o que vem a seguir e transforme planos em execução.");
    expect(html).toContain("Atualizar");
    expect(html).toMatch(/Novo item<\/button>/);
    expect(html).not.toMatch(/Novo item[^<]*<svg[^>]*chevron/i);
  });
  it("resumo: 7 operacionais = 4 planejados + 2 em execução + 1 concluído; cancelado separado", () => {
    expect(html).toMatch(/<strong>7<\/strong><span>Itens operacionais/);
    expect(html).toMatch(/<strong>4<\/strong><span>Planejados/);
    expect(html).toMatch(/<strong>2<\/strong><span>Em execução/);
    expect(html).toMatch(/<strong>1<\/strong><span>Concluídos/);
    expect(html).toMatch(/<strong>1<\/strong><span>Cancelado</);
    expect(html).toContain("pl-summary is-cancelled");
  });
  it("PRÓXIMO é o primeiro planejado, com Iniciar desabilitado e o motivo", () => {
    const next = html.slice(html.indexOf('aria-label="Próximo"'), html.indexOf("pl-filters"));
    expect(next).toContain("Configuração de ambientes por projeto");
    expect(next).toMatch(/<button[^>]*disabled=""[^>]*>(?:(?!<\/button>).)*Iniciar/s);
    expect(next).toContain(REASON);
  });
  it("todos os Iniciar ficam desabilitados enquanto a SESSION-001 está ativa", () => {
    const buttons = [...html.matchAll(/<button[^>]*aria-label="Iniciar [^"]*"[^>]*>/g)].map((m) => m[0]);
    expect(buttons).toHaveLength(4);
    for (const button of buttons) expect(button).toContain('disabled=""');
  });
  it("tabela: colunas aprovadas, sem drag handles, prioridade, responsável ou 'Mais filtros'", () => {
    for (const column of ["#", "Item / Descrição", "Estado", "Session DDAE", "Última atualização", "Ações"]) expect(html).toContain(column);
    expect(html).not.toMatch(/drag|grip|Prioridade|Responsável|Mais filtros|Backlog|Alta|Média|Baixa/i);
    expect(html).not.toMatch(/Área|Worktree<\/th>/);
  });
  it("filtros: somente os cinco aprovados", () => {
    const group = html.slice(html.indexOf('aria-label="Filtrar por estado"'));
    for (const label of ["Todos", "Planejados", "Em execução", "Concluídos", "Cancelados"]) expect(group).toContain(label);
    expect((html.match(/class="ddae-filter(?: is-on)? ?"/g) ?? []).length).toBe(5);
  });
  it("estados derivados com subbadge real e a Session vinculada com progresso", () => {
    for (const label of ["Planejado", "Em execução", "Concluído", "Cancelado", "CONGELADA", "PARADA", "FINALIZADA"]) expect(html).toContain(label);
    expect(html).toContain("pl-sub is-stopped"); // PARADA usa slate
    expect(html).toContain("6 / 9");
    expect(html).toContain("4 / 7");
    expect(html).toContain("10 / 10");
    expect(html).toContain('href="#project/p-blocked/ddae/s5"');
    expect(html).toMatch(/SESSION-005/);
  });
  it("CTAs: Abrir sessão com Session, Restaurar no cancelado; a numeração segue a fila", () => {
    expect((html.match(/>Abrir sessão</g) ?? []).length).toBeGreaterThanOrEqual(3);
    expect(html).toContain('aria-label="Restaurar Segurança e Hardening"');
    expect(html).toContain(">01<");
    expect(html).toContain(">08<");
  });
  it("menu '…': só ações válidas por linha", () => {
    const first = html.slice(html.indexOf("Mais ações de Configuração de ambientes por projeto"));
    const menu = first.slice(0, first.indexOf("</details>"));
    expect(menu).toContain("Editar");
    expect(menu).toContain("Mover para baixo");
    expect(menu).toContain("Cancelar");
    expect(menu).not.toContain("Mover para cima");
    expect(menu).not.toContain("Mover ao topo");
    const last = html.slice(html.indexOf("Mais ações de Sistema de Plugins"));
    const lastMenu = last.slice(0, last.indexOf("</details>"));
    expect(lastMenu).toContain("Mover para cima");
    expect(lastMenu).toContain("Mover ao topo");
    expect(lastMenu).not.toContain("Mover para baixo");
    const executing = html.slice(html.indexOf("Mais ações de Interface principal do LKR LAB"));
    const executingMenu = executing.slice(0, executing.indexOf("</details>"));
    expect(executingMenu).toContain("Abrir sessão");
    expect(executingMenu).not.toMatch(/Cancelar|Editar|Mover/);
  });
  it("a SESSION-001 não aparece na tabela (não nasceu do Planejamento), só no rodapé", () => {
    const table = html.slice(html.indexOf("<table"), html.indexOf("</table>"));
    expect(table).not.toContain("SESSION-001");
    expect(html.slice(html.indexOf("pl-footer"))).toContain("SESSION-001");
  });
});

describe("Planejamento sem Session ativa", () => {
  let html = "";
  beforeAll(() => { html = render(FREE); });
  it("Iniciar fica habilitado, sem motivo, e não há Session ativa no rodapé", () => {
    const next = html.slice(html.indexOf('aria-label="Próximo"'), html.indexOf("pl-filters"));
    expect(next).not.toMatch(/<button[^>]*disabled=""[^>]*>(?:(?!<\/button>).)*Iniciar/s);
    expect(next).not.toContain(REASON);
    expect(html).toContain("Nenhuma sessão ativa neste projeto.");
  });
  it("a Próxima ação passa a ser Iniciar o item (e não Continuar)", () => {
    const footer = html.slice(html.indexOf("pl-footer"));
    expect(footer).toContain("Iniciar Primeiro item");
    expect(footer).not.toContain("Continuar SESSION");
  });
});
