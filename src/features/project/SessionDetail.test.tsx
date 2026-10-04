import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { DdaeBlock, DdaeSessionView } from "../../shared/types";

vi.mock("../../shared/api", () => ({
  desktop: true,
  errorText: (e: unknown) => String(e),
  api: async () => undefined,
}));

const { SessionDetail, SessionOverview } = await import("./SessionDetail");
const { BlocksTab, DecisionsTab, FilesTab, NotesTab, PlanTab } = await import("./SessionTabs");

const done = (i: number): DdaeBlock => ({ id: `b${i}`, title: `Concept ${String(i).padStart(2, "0")}`, status: "completed" });
// O estado REAL da SESSION-001: 10 blocos, 9 concluídos, Concept 09 em andamento, sem próximo.
const BLOCKS: DdaeBlock[] = [...[1, 2, 3, 4, 5, 6, 7, 8].map(done), { id: "b9", title: "Concept 09 — Planejamento", status: "in_progress" }];
const REAL_BLOCKS: DdaeBlock[] = [{ id: "b0", title: "Product Architecture", status: "completed" }, ...BLOCKS];

function session(extra: Partial<DdaeSessionView> = {}): DdaeSessionView {
  return {
    id: "9e5dfe47-963a-5766-85a2-80a5f4cf11c5", projectId: "p", number: 1, label: "SESSION-001",
    title: "Machine Context & Project Workspace Foundation",
    objective: "Transformar o LKR LAB em um assistente consciente da máquina e do projeto.",
    status: "active", blocks: REAL_BLOCKS, decisions: [],
    createdAt: "2026-10-03T22:35:05Z", updatedAt: "2026-10-03T22:35:05Z",
    progress: { completed: 9, total: 10 }, currentBlock: REAL_BLOCKS[9], nextBlock: null,
    canComplete: false, completionBlockers: ["block_in_progress"], recentDecision: null,
    readyForAi: { state: "incomplete", ready: false, missing: ["desired_outcome", "criteria"] },
    events: [{ id: "e1", type: "LEGACY_IMPORTED", createdAt: "2026-10-03T22:35:05Z" }],
    ...extra,
  };
}

const ops = (view: DdaeSessionView) => ({ view, busy: false, error: null, run: async () => true, setError: () => {} });
const overview = (view: DdaeSessionView) => renderToStaticMarkup(<SessionOverview view={view} setTab={() => {}} openContext={() => {}} />);

describe("Visão geral (SESSION-001 real)", () => {
  const html = overview(session());
  it("status operacional: bloco atual, progresso 9/10 e sem próximo bloco", () => {
    expect(html).toContain("Status operacional da sessão");
    expect(html).toContain("Concept 09 — Planejamento");
    expect(html).toContain("9 / 10");
    expect(html).toContain("9 concluídos");
    expect(html).toContain("1 em andamento");
    expect(html).toContain("0 pendentes");
    expect(html).toMatch(/Próximo bloco<\/small><strong>—<\/strong>/);
  });
  it("Escopo da Session é a projeção dos 10 Blocks reais (sem blocos fictícios)", () => {
    expect(html).toContain("Escopo da Session");
    expect((html.match(/<li class="is-/g) ?? []).length).toBeGreaterThanOrEqual(10);
    expect(html).toContain("Product Architecture");
    expect(html).not.toContain("Fechamento / validação");
  });
  it("Contexto incompleto com o que falta, sem botão de 'marcar pronto'", () => {
    expect(html).toContain("Contexto incompleto");
    expect(html).toContain("Falta: resultado desejado, critérios de conclusão.");
    expect(html).not.toMatch(/marcar pronto/i);
    expect(html).not.toContain('type="checkbox"');
  });
  it("campos ausentes aparecem como vazios honestos", () => {
    expect(html).toContain("Nenhum critério definido.");
    expect(html).toContain("Nenhuma restrição.");
    expect(html).toContain("Nenhuma decisão registrada.");
    expect(html).toContain("Nenhuma referência.");
    expect(html).toContain("Não informado.");
  });
  it("histórico mostra só o evento real (importação legada)", () => {
    expect(html).toContain("Sessão importada do histórico legado");
    expect(html).not.toContain("Bloco concluído");
  });
  it("próxima ação: continuar o bloco atual", () => {
    expect(html).toContain("Continuar Concept 09 — Planejamento");
  });
  it("estado pronto e finalizada mudam o Contexto IA", () => {
    expect(overview(session({ readyForAi: { state: "ready", ready: true, missing: [] } }))).toContain("Pronto");
    expect(overview(session({ readyForAi: { state: "available", ready: false, missing: [] } }))).toContain("Contexto disponível");
  });
});

describe("Blocos", () => {
  it("lista todos por posição; só o em andamento pode concluir; nada remove concluído/em andamento", () => {
    const html = renderToStaticMarkup(<BlocksTab ops={ops(session())} />);
    expect((html.match(/class="sd-block /g) ?? []).length).toBe(10);
    expect(html).toContain("Concluir");
    expect(html).not.toContain(">Iniciar<");
    expect(html).not.toContain("Remover Concept");
    expect(html).toContain("Renomear Concept 09");
    expect(html).not.toContain("Renomear Product Architecture");
    expect(html).toContain("Concluir o último bloco não finaliza a sessão");
  });
  it("com bloco pendente: Iniciar (desabilitado se há um em andamento) e Remover", () => {
    const pending: DdaeBlock = { id: "bp", title: "Novo", status: "pending" };
    const html = renderToStaticMarkup(<BlocksTab ops={ops(session({ blocks: [...REAL_BLOCKS, pending], nextBlock: pending }))} />);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*title="Conclua “Concept 09 — Planejamento” antes de iniciar outro\."[^>]*>Iniciar/);
    expect(html).toContain("Remover Novo");
    expect(html).toContain("Novo bloco");
  });
  it("sessão finalizada: sem formulário de novo bloco e sem ações de edição", () => {
    const html = renderToStaticMarkup(<BlocksTab ops={ops(session({ status: "completed" }))} />);
    expect(html).not.toContain("Novo bloco");
    expect(html).not.toContain("Renomear");
  });
});

describe("Plano da sessão", () => {
  it("objetivo, resultado desejado, restrições e critérios; Blocks não se repetem aqui", () => {
    const html = renderToStaticMarkup(<PlanTab ops={ops(session())} />);
    expect(html).toContain("Objetivo");
    expect(html).toContain("Resultado desejado");
    expect(html).toContain("Restrições");
    expect(html).toContain("Critérios de conclusão");
    expect(html).toContain("Nenhum critério definido.");
    expect(html).toContain("Contexto incompleto");
    expect(html).toContain("Falta: resultado desejado, critérios de conclusão.");
    expect(html).not.toContain("sd-block");
    expect(html).toContain("não são blocos nem progresso de execução");
  });
  it("critérios marcáveis: concluído vira botão de reabrir; progresso 1 / 2", () => {
    const html = renderToStaticMarkup(<PlanTab ops={ops(session({ criteria: [{ id: "c1", text: "Build verde", completed: true }, { id: "c2", text: "Testes", completed: false }] }))} />);
    expect(html).toContain("Reabrir: Build verde");
    expect(html).toContain("Concluir: Testes");
    expect(html).toContain("1 / 2");
  });
  it("sessão finalizada: campos desabilitados", () => {
    const html = renderToStaticMarkup(<PlanTab ops={ops(session({ status: "completed" }))} />);
    expect(html).toMatch(/<textarea[^>]*disabled=""/);
  });
});

describe("Decisões, Arquivos e Anotações", () => {
  it("decisões: lista com bloco associado e formulário só de adicionar (sem editar/apagar)", () => {
    const view = session({ decisions: [{ id: "d1", blockId: "b9", title: "Usar SQLite", body: "fonte de verdade", createdAt: "2026-10-03T22:35:05Z" }] });
    const html = renderToStaticMarkup(<DecisionsTab ops={ops(view)} />);
    expect(html).toContain("Usar SQLite");
    expect(html).toContain("Bloco: Concept 09 — Planejamento");
    expect(html).toContain("Registrar decisão");
    expect(html).toContain("não são editadas nem apagadas");
    expect(html).not.toContain("Remover decisão");
    expect(renderToStaticMarkup(<DecisionsTab ops={ops(session())} />)).toContain("Nenhuma decisão registrada.");
  });
  it("arquivos: tipo, rótulo e valor RELATIVO; aviso de conversão; sem caminho absoluto", () => {
    const view = session({ references: [{ kind: "project_path", value: "docs/ddae/x.md", label: "Plano" }, { kind: "url", value: "https://github.com/org/repo" }] });
    const html = renderToStaticMarkup(<FilesTab ops={ops(view)} />);
    expect(html).toContain("Plano");
    expect(html).toContain("docs/ddae/x.md");
    expect(html).toContain("URL");
    expect(html).toContain("RELATIVO ao projeto");
    expect(html).not.toMatch(/[A-Za-z]:\\/);
    expect(renderToStaticMarkup(<FilesTab ops={ops(session())} />)).toContain("Nenhuma referência.");
  });
  it("anotações: lista simples com remover; vazio honesto", () => {
    const html = renderToStaticMarkup(<NotesTab ops={ops(session({ notes: ["Lembrar de validar"] }))} />);
    expect(html).toContain("Lembrar de validar");
    expect(html).toContain("Remover anotação 1");
    expect(renderToStaticMarkup(<NotesTab ops={ops(session())} />)).toContain("Nenhuma anotação.");
  });
});

describe("casca do detalhe", () => {
  it("antes de ler o par (Project, Session) mostra carregando, nunca dados de outra Session", () => {
    const html = renderToStaticMarkup(<SessionDetail projectId="p" sessionId="s" notify={() => {}} />);
    expect(html).toContain("Carregando sessão…");
    expect(html).not.toContain("SESSION-");
  });
});

describe("Worktrees relacionados (Session Detail)", () => {
  it("sem vínculo real: nenhum worktree (nada é inventado para a SESSION-001)", () => {
    const html = overview(session());
    expect(html).toContain("Worktrees relacionados");
    expect(html).toContain("Nenhum worktree vinculado.");
    expect(html).toContain("Ver worktrees");
    expect(html).toContain("#project/p/worktrees");
  });
  it("mostra só os vínculos reais: nome, branch, estado e disponibilidade nesta máquina", () => {
    const html = renderToStaticMarkup(
      <SessionOverview view={session()} setTab={() => {}} openContext={() => {}} worktrees={[
        { id: "w1", displayName: "feature/a", status: "active", branchHint: "feature/a", blockId: null, available: true },
        { id: "w2", displayName: "Em outra máquina", status: "frozen", branchHint: null, blockId: null, available: false },
      ]} />,
    );
    expect(html).toContain("feature/a");
    expect(html).toContain("Ativo");
    expect(html).toContain("Congelado");
    expect(html).toContain("não localizado nesta máquina");
    expect(html).not.toContain("Nenhum worktree vinculado.");
  });
  it("o histórico da Session mostra a associação com worktree sem path", () => {
    const view = session({ events: [{ id: "e2", type: "WORKTREE_LINKED", payload: { worktreeId: "w1", name: "feature/a" }, createdAt: "2026-10-04T10:00:00Z" }] });
    const html = overview(view);
    expect(html).toContain("Worktree vinculado: feature/a");
    expect(html).not.toMatch(/[A-Za-z]:\\/);
  });
});
