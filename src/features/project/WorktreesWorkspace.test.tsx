import { renderToStaticMarkup } from "react-dom/server";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { GitSummary, Worktree, WorktreeCounts, WorktreeItem, WorktreeManagedView, WorktreeOverview, WorktreeStatus } from "../../shared/types";
import type { MachineRegistry } from "../../state/machine";

// O componente só fala com o backend pelo `api`; aqui ele devolve um overview fixo e finge ser o desktop.
const responses = new Map<string, WorktreeOverview | null>();
vi.mock("../../shared/api", () => ({
  desktop: true,
  errorText: (e: unknown) => String(e),
  api: async (command: string, args?: { id?: string; projectId?: string }) => (command === "project_worktree_overview" ? responses.get(args?.id ?? "") : null),
}));

const { WorktreesWorkspace } = await import("./WorktreesWorkspace");
const { workspace } = await import("../../state/workspace");
const { MachineContext } = await import("../../state/machine");

const git = (over: Partial<GitSummary> = {}): GitSummary => ({
  isRepo: true, branch: "x", detached: false, upstream: null, ahead: null, behind: null,
  staged: 0, unstaged: 0, untracked: 0, conflicts: 0, changes: 0, clean: true, error: null, ...over,
});
const gw = (over: Partial<Worktree> = {}): Worktree => ({ path: "C:/dev/wt", head: "abcdef1234", branch: "feature/x", isPrimary: false, detached: false, bare: false, locked: false, prunable: false, ...over });
const summary = (over: Partial<GitSummary> = {}) => ({ status: "available" as const, data: git(over), message: null });
const managed = (status: WorktreeStatus, over: Partial<WorktreeManagedView> = {}): WorktreeManagedView => ({
  id: "w-" + status, projectId: "p", displayName: "Gerenciado " + status, status, createdAt: "", updatedAt: "",
  session: null, block: null, lastEventAt: null, ...over,
});
const item = (kind: WorktreeItem["kind"], over: Partial<WorktreeItem> = {}): WorktreeItem => ({ kind, git: gw(), managed: null, gitSummary: summary(), warnings: [], ...over });
const counts = (over: Partial<WorktreeCounts> = {}): WorktreeCounts => ({ gitTotal: 1, managed: 0, active: 0, frozen: 0, stopped: 0, completed: 0, missing: 0, unmanaged: 0, withChanges: 0, ...over });
const overview = (projectId: string, items: WorktreeItem[], c: WorktreeCounts, over: Partial<WorktreeOverview> = {}): WorktreeOverview => ({ projectId, projectAvailable: true, items, counts: c, gitError: null, ...over });

const primary = item("primary", { git: gw({ isPrimary: true, branch: "main", path: "C:/dev/LKR_Lab" }) });

const REAL = "p-real";
const MIXED = "p-mixed";
const OFFLINE = "p-offline";

beforeAll(async () => {
  // O LKR_Lab real hoje: só o checkout principal. Nada de cards de exemplo.
  responses.set(REAL, overview(REAL, [primary], counts()));
  responses.set(MIXED, overview(MIXED, [
    primary,
    item("managed_available", { git: gw({ branch: "feature/a" }), managed: managed("active", { session: { id: "s1", label: "SESSION-001", title: "Machine Context", status: "active" }, block: { id: "b9", title: "Concept 09 — Planejamento", status: "in_progress" }, lastEventAt: "2026-10-03T22:35:05Z" }) }),
    item("managed_available", { git: gw({ branch: "feature/b" }), managed: managed("frozen", { stateReason: "Aguardando revisão" }) }),
    item("managed_available", { git: gw({ branch: "feature/c" }), managed: managed("stopped"), gitSummary: summary({ unstaged: 3, changes: 3, clean: false }) }),
    item("managed_available", { git: gw({ branch: "feature/d" }), managed: managed("completed", { result: "Integrado na main" }), warnings: ["worktree_completed_session_active"] }),
    item("managed_missing", { git: null, gitSummary: null, managed: managed("active", { displayName: "Sumido", branchHint: "feature/sumido" }) }),
    item("unmanaged", { git: gw({ branch: "feature/solta", path: "C:/dev/solta" }) }),
  ], counts({ gitTotal: 6, managed: 5, active: 2, frozen: 1, stopped: 1, completed: 1, missing: 1, unmanaged: 1, withChanges: 1 })));
  responses.set(OFFLINE, overview(OFFLINE, [item("managed_missing", { git: null, gitSummary: null, managed: managed("frozen", { displayName: "Só portátil" }) })], counts({ gitTotal: 0, managed: 1, frozen: 1, missing: 1 }), { projectAvailable: false }));
  for (const id of [REAL, MIXED, OFFLINE]) await workspace.forProject(id).worktreeOverview.refresh();
});

const machine = { status: { machine: { name: "PC Casa" } } } as unknown as MachineRegistry;
const render = (id: string) => renderToStaticMarkup(<MachineContext.Provider value={machine}><WorktreesWorkspace projectId={id} notify={() => {}} /></MachineContext.Provider>);

describe("Worktrees (dados reais)", () => {
  it("LKR_Lab real: só o checkout PRINCIPAL, sem estado operacional e sem cards fictícios", () => {
    const html = render(REAL);
    expect(html).toContain("Worktrees");
    expect(html).toContain("PRINCIPAL");
    expect(html).toContain("main");
    expect(html).toContain("PC Casa");
    expect(html).not.toMatch(/wt-badge is-(active|frozen|stopped|completed)/);
    expect(html).not.toContain("Adotar");
    expect(html).not.toContain("external-process-adoption");
    expect(html).toContain("ainda não tem worktrees além do checkout principal");
    // contadores só de gerenciados: zero
    expect(html).toMatch(/<strong>0<\/strong><span>Gerenciados/);
  });
  it("o principal não participa do ciclo: sem menu de estado, sem Adotar, com nota explícita", () => {
    const html = render(REAL);
    const card = html.slice(html.indexOf('aria-label="Checkout principal"'));
    expect(card).toContain("fora do ciclo operacional");
    expect(card).not.toContain("wt-menu");
    expect(card).not.toContain("Congelar");
    expect(card).not.toContain("Finalizar");
  });
  it("estados operacionais reais, cada um com seu selo; PARADO é slate (classe própria)", () => {
    const html = render(MIXED);
    for (const label of ["ATIVO", "CONGELADO", "PARADO", "FINALIZADO"]) expect(html).toContain(label);
    expect(html).toContain("wt-badge is-stopped");
    expect(html).not.toMatch(/amber|âmbar/i);
  });
  it("contadores fecham: gerenciados = ativos + congelados + parados + finalizados", () => {
    const html = render(MIXED);
    expect(html).toMatch(/<strong>5<\/strong><span>Gerenciados/);
    expect(html).toMatch(/<strong>2<\/strong><span>Ativos/);
    expect(html).toMatch(/<strong>1<\/strong><span>Congelados/);
    expect(html).toMatch(/<strong>1<\/strong><span>Parados/);
    expect(html).toMatch(/<strong>1<\/strong><span>Finalizados/);
    expect(html).toMatch(/<strong>1<\/strong><span>com alterações Git/);
    expect(html).toContain("1 não localizado");
    expect(html).toContain("1 não gerenciado");
    const c = overview(MIXED, [], counts()).counts; // forma do contrato
    expect(c.managed).toBe(c.active + c.frozen + c.stopped + c.completed);
  });
  it("Session e Bloco só quando o vínculo é real; senão '—'", () => {
    const html = render(MIXED);
    expect(html).toContain("SESSION-001");
    expect(html).toContain('href="#project/p-mixed/ddae/s1"');
    expect(html).toContain("Concept 09 — Planejamento");
    expect(html).toContain("Bloco relacionado");
    // um worktree sem vínculo mostra '—' em DDAE e Bloco
    expect((html.match(/<dt>DDAE<\/dt><dd class="">—<\/dd>/g) ?? []).length).toBeGreaterThanOrEqual(3);
    expect((html.match(/<dt>Bloco relacionado<\/dt><dd class="">—<\/dd>/g) ?? []).length).toBeGreaterThanOrEqual(3);
  });
  it("não gerenciado: selo neutro e CTA Adotar; não localizado: sem Git e CTA Localizar", () => {
    const html = render(MIXED);
    expect(html).toContain("NÃO GERENCIADO");
    expect(html).toContain("Adotar");
    expect(html).toContain("NÃO LOCALIZADO");
    expect(html).toContain("Localizar");
    expect(html).toContain("Não localizado nesta máquina");
    const missing = html.slice(html.indexOf('aria-label="Sumido"'));
    expect(missing.slice(0, missing.indexOf("</article>"))).not.toContain("Clean");
  });
  it("runtime por worktree não é inventado; resultado e motivo aparecem quando reais", () => {
    const html = render(MIXED);
    expect(html).toContain("Não disponível");
    expect(html).toContain("Integrado na main");
    expect(html).toContain("Aguardando revisão");
  });
  it("Git sujo em worktree PARADA só avisa (não muda o estado)", () => {
    const html = render(MIXED);
    expect(html).toContain("Há alterações Git nesta worktree; elas continuam como estão.");
    expect(html).toContain("3 alterações");
  });
  it("incoerências Session×worktree são sinalizadas, não corrigidas", () => {
    expect(render(MIXED)).toContain("Este worktree está finalizado, mas a Session vinculada continua ativa.");
  });
  it("ações do menu: finalizado não tem retorno; remover do Git é um item próprio e separado", () => {
    const html = render(MIXED);
    expect(html).toContain("Remover worktree do Git…");
    expect(html).toContain("Congelar");
    const done = html.slice(html.indexOf('aria-label="Gerenciado completed"'));
    const card = done.slice(0, done.indexOf("</article>"));
    expect(card).not.toContain("Retomar");
    expect(card).not.toContain("Congelar");
  });
  it("busca e os dois grupos de filtro (operacional e Git) estão separados", () => {
    const html = render(MIXED);
    expect(html).toContain("Buscar worktrees");
    for (const l of ["Todos", "Ativos", "Congelados", "Parados", "Finalizados", "Não localizados", "Não gerenciados", "Clean", "Alterações", "Ahead", "Behind", "Conflitos"]) expect(html).toContain(l);
    expect(html).toContain('aria-label="Filtrar por estado operacional"');
    expect(html).toContain('aria-label="Filtrar por Git"');
  });
  it("projeto sem pasta nesta máquina: avisa e mostra só o gerenciado como não localizado", () => {
    const html = render(OFFLINE);
    expect(html).toContain("Projeto não localizado nesta máquina");
    expect(html).toContain("NÃO LOCALIZADO");
    expect(html).not.toContain("PRINCIPAL");
    expect(html).toMatch(/Novo worktree<\/button>/);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>(?:(?!<\/button>).)*Novo worktree/s);
  });
  it("rodapé: Session ativa e Próxima ação vêm só do DDAE real (aqui, nenhum)", () => {
    const html = render(REAL);
    expect(html).toContain("Nenhuma sessão ativa neste projeto.");
    expect(html).toContain("Nenhuma ação pendente na Session.");
  });
});
