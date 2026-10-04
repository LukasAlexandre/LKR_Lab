import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AlertsPanel, AlertsSummaryStrip } from "./AlertsPanel";
import { MIN, NOW, alert, attention, catalog, diagnostics, info, run, runtimeFailed, snapshot, volumeProblem } from "../shared/alerts.fixtures";
import type { AlertsSnapshot } from "../shared/types";
import type { AlertActions, AlertsState } from "../state/alerts";

const noop: AlertActions = { acknowledge: () => undefined, start: () => undefined, cancel: () => undefined };
const state = (snap: AlertsSnapshot | null, error: string | null = null, actionError: string | null = null): AlertsState => ({ snapshot: snap, error, loading: false, actionError });
const render = (snap: AlertsSnapshot | null, error: string | null = null, actionError: string | null = null) =>
  renderToStaticMarkup(<AlertsPanel state={state(snap, error, actionError)} now={NOW} actions={noop} />);

const section = (html: string, label: string) => {
  const start = html.indexOf(`aria-label="${label}"`);
  expect(start, `seção ${label}`).toBeGreaterThan(-1);
  return html.slice(start, start + 6000);
};

describe("Alertas — estado vazio (máquina saudável, nenhum problema inventado)", () => {
  const html = render(snapshot([]));
  it("diz que não há alertas abertos, sem afirmar que está tudo bem", () => {
    expect(html).toContain("Alertas e diagnósticos");
    expect(html).toContain("Nenhum alerta aberto");
    expect(html).toContain("0 críticos · 0 atenção · 0 informações");
    expect(html).toContain("3 de 3 fontes avaliadas");
    expect(html).not.toContain('aria-label="Alertas"');
  });
  it("sem fonte avaliada, diz isso em vez de 'sem alertas'", () => {
    const none = render(snapshot([], { sources: [] }));
    expect(none).toContain("Nenhuma fonte foi avaliada ainda.");
  });
  it("sem snapshot: avaliando; com erro: mostra o erro", () => {
    expect(render(null)).toContain("Avaliando a máquina");
    expect(render(null, "falhou")).toContain("falhou");
  });
  it("erro com leitura anterior: mantém o conteúdo e avisa", () => {
    const html = render(snapshot([alert()]), "sem resposta");
    expect(html).toContain("sem resposta");
    expect(html).toContain("mostrando a leitura anterior");
    expect(html).toContain("Pouco espaço livre em C:");
  });
  it("erro de ação do usuário aparece como alerta de interface", () => {
    expect(render(snapshot([]), null, "Requer administrador")).toContain('role="alert"');
  });
});

describe("Alertas — severidades", () => {
  it("crítico: severidade, título, motivo, evidência, recurso, datas, estado, passo e confiança", () => {
    const html = render(snapshot([alert()]));
    const card = section(html, "Pouco espaço livre em C:");
    expect(card).toContain("Crítico");
    expect(card).toContain("Pouco espaço livre em C:");
    expect(card).toContain("O volume C: tem 2.0% livre (4.0 GiB).");
    expect(card).toContain("Por quê:");
    expect(card).toContain("menos de 5% e menos de 5.0 GiB livres");
    expect(card).toContain('aria-label="Evidência"');
    expect(card).toContain("2.0% (4.0 GiB)");
    expect(card).toContain("C:</span>");
    expect(card).toContain("primeira vez");
    expect(card).toContain("há 30 min");
    expect(card).toContain("última vez");
    expect(card).toContain("há 1 min");
    expect(card).toContain("Ativo");
    expect(card).toContain("O que verificar:");
    expect(card).toContain("Libere espaço no volume C:");
    expect(card).toContain("Confiança alta");
    expect(html).toContain("Crítico · 1 aberto");
    expect(html).toContain("1 crítico · 0 atenção · 0 informações");
  });
  it("atenção", () => {
    const html = render(snapshot([attention()]));
    const card = section(html, "Reinício do Windows pendente");
    expect(card).toContain("Atenção");
    expect(card).toContain("Windows");
    expect(card).toContain("Nada é reiniciado automaticamente");
    expect(html).toContain("Atenção · 1 aberto");
  });
  it("informação: fato, nunca vulnerabilidade nem exposição à internet", () => {
    const html = render(snapshot([info()]));
    const card = section(html, "vite.exe escuta em todas as interfaces");
    expect(card).toContain("Informação");
    expect(card).toContain("3000, 5173");
    expect(card).toContain("não significa exposição à internet");
    expect(card.toLowerCase()).not.toContain("vulner");
    expect(html).toContain("Informação · 1 aberto");
  });
  it("vários alertas aparecem na ordem de prioridade", () => {
    const html = render(snapshot([info(), attention(), alert()]));
    const order = ["Pouco espaço livre em C:", "Reinício do Windows pendente", "vite.exe escuta"].map((t) => html.indexOf(t));
    expect(order).toEqual([...order].sort((a, b) => a - b));
    expect(order.every((i) => i > -1)).toBe(true);
  });
  it("ocorrência repetida é mostrada; a primeira não", () => {
    expect(render(snapshot([alert({ occurrenceCount: 3 })]))).toContain("ocorrência 3");
    expect(render(snapshot([alert()]))).not.toContain("ocorrência 1");
  });
});

describe("Alertas — ciclo de vida", () => {
  it("ativo tem 'Reconhecer'; a dica diz que não altera a máquina", () => {
    const html = render(snapshot([alert()]));
    expect(html).toContain("Reconhecer");
    expect(html).toContain("Não altera a máquina nem resolve a condição");
  });
  it("reconhecido: mostra o estado e não oferece reconhecer de novo", () => {
    const html = render(snapshot([attention({ status: "acknowledged", acknowledgedAt: NOW - 2 * MIN })]));
    expect(html).toContain("Reconhecido");
    expect(html).toContain("reconhecido há 2 min");
    expect(html).not.toContain(">Reconhecer<");
    expect(html).toContain("1 reconhecido");
  });
  it("resolvido sai da lista padrão e conta como resolvido recentemente", () => {
    const html = render(snapshot([alert({ status: "resolved", resolvedAt: NOW - 5 * MIN })]));
    expect(html).not.toContain('aria-label="Alertas"');
    expect(html).toContain("Nenhum alerta corresponde ao filtro.");
    expect(html).toContain("Resolvidos recentemente");
  });
});

describe("Alertas — CTA contextual (nunca corrigir automaticamente)", () => {
  it("Runtime, Windows Health e Network & Security", () => {
    expect(render(snapshot([runtimeFailed()]))).toContain("Abrir Runtime");
    expect(render(snapshot([attention()]))).toContain("Abrir Windows Health");
    expect(render(snapshot([info()]))).toContain("Abrir Network &amp; Security");
  });
  it("alerta resolvido não oferece ações", () => {
    const html = render(snapshot([attention({ status: "resolved", resolvedAt: NOW - MIN })], {}), null);
    expect(html).not.toContain("Abrir Windows Health</button>");
  });
  it("nenhum botão corrige, repara, mata processo ou ativa algo", () => {
    const html = render(snapshot([alert(), attention(), info(), runtimeFailed(), volumeProblem()], { diagnostics: diagnostics({ elevated: true, catalog: catalog(true) }) }));
    expect(html).not.toMatch(/<button[^>]*>[^<]*(Corrigir|Reparar|Consertar|Encerrar|Matar|Ativar|Remover|Bloquear|Limpar)/i);
    expect(html).toContain("não corrige nada automaticamente");
  });
});

describe("Diagnósticos", () => {
  it("catálogo: quatro diagnósticos de leitura e a explicação de que reparos não existem", () => {
    const html = render(snapshot([]));
    const diag = section(html, "Catálogo de diagnósticos");
    for (const text of ["SFC", "DISM CheckHealth", "DISM ScanHealth", "CHKDSK"]) expect(diag).toContain(text);
    expect(html).toContain("Reparos (como sfc /scannow, DISM /RestoreHealth ou chkdsk /f) não existem neste catálogo");
    expect(html).toContain("Nada é executado automaticamente");
  });
  it("sem elevação: 'Requer administrador', sem botão Executar e sem pedir UAC", () => {
    const html = render(snapshot([]));
    expect(html).toContain("Requer administrador");
    expect(html).toContain("não solicita elevação automaticamente");
    expect(html).not.toContain(">Executar<");
    expect(html).not.toMatch(/UAC|runas/i);
  });
  it("elevado: disponível com Executar; CHKDSK pede o volume", () => {
    const html = render(snapshot([], { diagnostics: diagnostics({ elevated: true, catalog: catalog(true) }) }));
    expect(html.match(/Executar<\/button>/g)).toHaveLength(4);
    expect(html).toContain('aria-label="Volume"');
    expect(html).toContain("<option value=\"C:\"");
    expect(html).toContain("<option value=\"D:\"");
    expect(html).not.toContain("Os diagnósticos exigem administrador");
  });
  it("em execução: aberto, andamento, saída parcial e Cancelar", () => {
    const html = render(snapshot([], {
      diagnostics: diagnostics({
        elevated: true,
        catalog: catalog(true),
        current: run({ running: true, result: null, finishedAt: null, exitCode: null, summary: "Em execução…", outputTail: ["Verification 40% complete."] }),
      }),
    }));
    const diag = section(html, "Diagnósticos");
    expect(diag).toContain(" open");
    expect(diag).toContain("Em execução");
    expect(diag).toContain("Saída parcial");
    expect(diag).toContain("Verification 40% complete.");
    expect(diag).toContain("Cancelar diagnóstico");
    expect(diag).toContain("Outro em execução");
    expect(diag).not.toContain(">Executar<");
  });
  it("concluído sem problemas", () => {
    const html = render(snapshot([], { diagnostics: diagnostics({ elevated: true, catalog: catalog(true), current: run() }) }));
    const diag = section(html, "Execução: Examinar um volume (CHKDSK, somente leitura) · C:");
    expect(diag).toContain("Sem problemas");
    expect(diag).toContain("O volume foi examinado e nenhum problema foi encontrado.");
    expect(diag).toContain("código 0");
    expect(diag).toContain("Últimas linhas da saída");
    expect(html).not.toContain("Cancelar diagnóstico");
  });
  it("problemas encontrados e falha são distintos do sucesso", () => {
    const problems = render(snapshot([], { diagnostics: diagnostics({ current: run({ result: "problems_found", summary: "O CHKDSK encontrou problemas no volume. Nada foi corrigido." }) }) }));
    expect(problems).toContain("Problemas encontrados");
    expect(problems).toContain("Nada foi corrigido");
    const failed = render(snapshot([], { diagnostics: diagnostics({ current: run({ result: "failed", exitCode: 740, summary: "O DISM exige administrador." }) }) }));
    expect(failed).toContain("Falhou");
    expect(failed).toContain("código 740");
    expect(failed).toContain("O DISM exige administrador.");
    const cancelled = render(snapshot([], { diagnostics: diagnostics({ current: run({ result: "cancelled", summary: "Cancelado pelo usuário.", exitCode: null }) }) }));
    expect(cancelled).toContain("Cancelado");
  });
  it("histórico: vazio e com execuções", () => {
    expect(render(snapshot([]))).toContain("Nenhum diagnóstico foi executado nesta máquina.");
    const html = render(snapshot([], { diagnostics: diagnostics({ history: [run(), run({ id: "run2", result: "problems_found", summary: "Achou algo." })] }) }));
    const history = section(html, "Histórico de diagnósticos");
    expect(history).toContain("Sem problemas");
    expect(history).toContain("Problemas encontrados");
    expect(history).toContain("Achou algo.");
  });
  it("a saída é tratada como dado local", () => {
    const html = render(snapshot([], { diagnostics: diagnostics({ current: run() }) }));
    expect(html).toContain("A saída é local desta máquina");
  });
});

describe("Diagnóstico contextual a partir de um alerta", () => {
  const withAction = (view: ReturnType<typeof diagnostics>) => render(snapshot([volumeProblem()], { diagnostics: view }));
  it("indisponível sem administrador: explica em vez de oferecer um botão", () => {
    const html = withAction(diagnostics());
    expect(html).toContain("requer administrador");
    expect(html).not.toContain("Executar diagnóstico");
  });
  it("disponível: oferece executar o diagnóstico do volume", () => {
    const html = withAction(diagnostics({ elevated: true, catalog: catalog(true) }));
    expect(html).toContain("Executar diagnóstico (D:)");
  });
  it("em execução: não oferece outro", () => {
    const html = withAction(diagnostics({ elevated: true, catalog: catalog(true), current: run({ running: true, result: null }) }));
    expect(html).toContain("Um diagnóstico está em execução");
    expect(html).not.toContain("Executar diagnóstico (D:)");
  });
});

describe("Fontes e dado velho", () => {
  it("fontes sem dado, velhas ou que exigem administrador não são alerta", () => {
    const html = render(snapshot([], {
      sources: [
        { id: "machine.disk", label: "Espaço dos volumes", state: "evaluated", reason: null },
        { id: "windows.volumes", label: "Integridade dos volumes", state: "unavailable", reason: "A integridade dos volumes não pôde ser consultada." },
        { id: "windows.events", label: "Eventos do sistema", state: "stale", reason: "A leitura passou do dobro da validade; nenhum alerta novo é gerado a partir dela." },
      ],
    }));
    const missing = section(html, "Fontes não avaliadas");
    expect(html).toContain("Fontes não avaliadas (2)");
    expect(missing).toContain("Sem dado");
    expect(missing).toContain("Desatualizada");
    expect(html).toContain("ausência de informação não é problema");
    expect(html).toContain("1 de 3 fontes avaliadas");
    expect(html).not.toContain('aria-label="Alertas"');
  });
  it("tudo avaliado: sem a lista de fontes não avaliadas", () => {
    expect(render(snapshot([]))).not.toContain("Fontes não avaliadas");
  });
});

describe("Faixa do Dashboard", () => {
  const strip = (snap: AlertsSnapshot | null) => renderToStaticMarkup(<AlertsSummaryStrip state={state(snap)} />);
  it("contagens e o caminho para os diagnósticos", () => {
    const html = strip(snapshot([attention(), attention({ alertId: "b" }), info(), info({ alertId: "c" }), info({ alertId: "d" })]));
    expect(html).toContain("0 críticos");
    expect(html).toContain("2 atenção");
    expect(html).toContain("3 informações");
    expect(html).toContain("Ver diagnósticos");
    expect(html).toContain("attention");
  });
  it("crítico destaca a faixa; sem alertas fica neutra; singular correto", () => {
    expect(strip(snapshot([alert()]))).toContain("1 crítico<");
    expect(strip(snapshot([alert()]))).toContain("mh-alerts-strip critical");
    expect(strip(snapshot([]))).toContain("mh-alerts-strip clear");
    expect(strip(snapshot([info()]))).toContain("1 informação<");
  });
  it("sem snapshot não renderiza nada", () => {
    expect(strip(null)).toBe("");
  });
});
