import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { WindowsHealthPanel } from "./WindowsHealthPanel";
import { NOW, healthySnapshot, service, withStatus } from "../shared/windowsHealth.fixtures";
import type { WindowsHealthSnapshot } from "../shared/types";

const ready = (snapshot: WindowsHealthSnapshot | null, error: string | null = null) =>
  renderToStaticMarkup(<WindowsHealthPanel state={{ snapshot, error, loading: false }} now={NOW + 5_000} />);

describe("Windows Health — máquina saudável (não inventa problema)", () => {
  const html = ready(healthySnapshot());
  it("mostra Saudável, sem motivos de alerta", () => {
    expect(html).toContain("Windows Health");
    expect(html).toContain("Saudável");
    expect(html).toContain("6 de 6 domínios avaliados");
    expect(html).not.toContain('role="alert"');
    expect(html).not.toMatch(/mh-win-reasons (attention|critical)/);
  });
  it("sistema: edição, versão, build, arquitetura e uptime da máquina", () => {
    expect(html).toContain("Windows 11 Pro");
    expect(html).toContain("versão 25H2");
    expect(html).toContain("build 26200.9457");
    expect(html).toContain("x64");
    expect(html).toContain("ligado há 1d 13h");
  });
  it("resumo com os seis indicadores", () => {
    const summary = html.slice(html.indexOf('aria-label="Resumo"'), html.indexOf("mh-win-system"));
    for (const label of ["Windows", "Windows Update", "Reinício pendente", "Eventos críticos (24 h)", "Dispositivos com problema", "Serviços essenciais"]) {
      expect(summary).toContain(label);
    }
    expect(summary).toContain("2 de 2 saudáveis");
  });
  it("reinício não pendente e ruído de renomeação explicado como informativo", () => {
    expect(html).toContain("Nenhuma fonte indica reinício pendente.");
    expect(html).toContain("renomeações de arquivo pendentes");
    expect(html).toContain("(não é um alerta)");
  });
  it("update sem falhas; pendentes e datas ausentes ficam 'Desconhecido'", () => {
    const updates = html.slice(html.indexOf('aria-label="Windows Update"'), html.indexOf('aria-label="Eventos do sistema"'));
    expect(updates).toContain("Sem falhas recentes");
    expect(updates).toMatch(/Atualizações pendentes<\/dt><dd[^>]*>Desconhecido/);
    expect(updates).toMatch(/Última instalação bem-sucedida<\/dt><dd>Desconhecido/);
    expect(updates).toContain("Falhas (7 dias)</dt><dd>0");
  });
  it("eventos: contagens, 7 dias sem aviso medido como traço, e a falha de app só informativa", () => {
    const events = html.slice(html.indexOf('aria-label="Eventos do sistema"'), html.indexOf('aria-label="Serviços essenciais"'));
    expect(events).toContain("0 críticos · 5 erros · 22 avisos");
    expect(events).toContain("avisos —");
    expect(events).toContain("Falha de aplicativo");
    expect(events).toContain("Erros e avisos comuns são ruído");
  });
  it("dispositivos: nenhum problema; volumes: limpo", () => {
    expect(html).toContain("Nenhum dispositivo presente reporta problema.");
    expect(html).toContain("192 presentes");
    expect(html).toContain("Não está sujo");
    expect(html).toContain("Leitura e escrita");
  });
  it("tudo recolhido por padrão e sem ação de reparo", () => {
    expect(html).not.toMatch(/<details[^>]* open/);
    expect((html.match(/<details/g) ?? []).length).toBeGreaterThanOrEqual(8);
    for (const forbidden of ["Reparar", "Corrigir", "Instalar", "Reiniciar agora", "Resolver"]) expect(html).not.toContain(forbidden);
    expect(html).toContain("Somente observação");
    expect(html).toContain("não repara, reinicia, instala nem altera nada");
  });
  it("integridade passiva sem sinais é 'Desconhecida' e diz por quê; SFC/DISM não executados", () => {
    const integrity = html.slice(html.indexOf('aria-label="Integridade do sistema (passiva)"'));
    expect(integrity).toContain("verificação sob demanda");
    expect(integrity).toContain("não são executados");
    expect(integrity).toContain("exigem privilégio administrativo");
  });
  it("cada domínio informa a última leitura", () => {
    expect(html).toContain("Última leitura agora.");
  });
});

describe("Windows Health — atenção e crítico sempre explicam o motivo", () => {
  it("reinício pendente após atualização", () => {
    const s = withStatus(healthySnapshot(), "restart", "attention", ["Reinicialização pendente: O Windows Update exige reinício."], { pending: true });
    s.overall = { status: "attention", evaluated: 6, rateable: 6, reasons: [{ domain: "restart", text: "Reinicialização pendente: O Windows Update exige reinício." }] };
    const html = ready(s);
    expect(html).toContain('role="alert"');
    expect(html).toContain("Atenção");
    expect(html).toContain("Reinício:");
    expect(html).toContain("O Windows Update exige reinício");
    expect(html).toContain("O Windows pede um reinício. Nada é reiniciado automaticamente.");
  });
  it("dispositivos com problema listam nome, classe, fabricante e o código", () => {
    const s = withStatus(healthySnapshot(), "devices", "attention", ["2 dispositivos reportam problema no Gerenciador de Dispositivos."], {
      issues: [
        { name: "Placa Wi-Fi X", class: "Net", manufacturer: "Acme", problemCode: 43, problem: "código 43: o Windows parou o dispositivo por ter reportado problemas" },
        { name: "Controlador Y", class: "System", manufacturer: null, problemCode: 28, problem: "código 28: drivers não instalados" },
      ],
    });
    s.overall = { status: "attention", evaluated: 6, rateable: 6, reasons: [{ domain: "devices", text: "2 dispositivos reportam problema no Gerenciador de Dispositivos." }] };
    const html = ready(s);
    expect(html).toContain("2 dispositivos reportam problema");
    expect(html).toContain("Placa Wi-Fi X");
    expect(html).toContain("código 43");
    expect(html).toContain("Acme");
    expect(html).toContain("código 28");
  });
  it("evento crítico (bugcheck) vira Crítico com o motivo e o sinal", () => {
    const s = withStatus(healthySnapshot(), "events", "critical", ["Tela azul (bugcheck) registrada nas últimas 24 horas."], {
      last24h: { critical: 1, error: 7, warning: 30 },
      signals: [{ kind: "bugcheck", label: "Tela azul (bugcheck)", count24h: 1, count7d: 1, lastAt: NOW - 7_200_000 }],
      recent: [{ at: NOW - 7_200_000, provider: "Microsoft-Windows-WER-SystemErrorReporting", id: 1001, kind: "bugcheck" }],
    });
    s.overall = { status: "critical", evaluated: 6, rateable: 6, reasons: [{ domain: "events", text: "Tela azul (bugcheck) registrada nas últimas 24 horas." }] };
    const html = ready(s);
    expect(html).toContain("Crítico");
    expect(html).toContain("Eventos:");
    expect(html).toContain("Tela azul (bugcheck)");
    expect(html).toContain("ID 1001");
    expect(html).toContain("1 críticos");
  });
  it("falha de atualização e serviço parado automático trazem a causa", () => {
    let s = withStatus(healthySnapshot(), "updates", "attention", ["2 falha(s) de instalação ou download do Windows Update nos últimos 7 dias."], { failures7d: 2 });
    s = withStatus(s, "services", "attention", ["Agendador de Tarefas está parado, mas é iniciado automaticamente."], {
      items: [service({ state: "stopped", health: "attention", reason: "Agendador de Tarefas está parado, mas é iniciado automaticamente." })],
    });
    const html = ready(s);
    expect(html).toContain("2 falhas em 7 dias");
    expect(html).toContain("está parado, mas é iniciado automaticamente");
    expect(html).toContain("Serviço parado só é problema quando o início é automático");
  });
  it("serviço sob demanda parado e saudável não gera alerta", () => {
    const s = healthySnapshot();
    s.services.items = [service({ id: "BITS", label: "BITS", state: "stopped", start: "manual", expectation: "on_demand" })];
    const html = ready(s);
    expect(html).toContain("Parado · Manual / sob demanda");
    expect(html).not.toContain('role="alert"');
  });
  it("volume sujo ou somente leitura explica o motivo", () => {
    const s = withStatus(healthySnapshot(), "volumes", "attention", ["O volume C: está marcado como sujo."], {
      items: [{ mount: "C:", filesystem: "NTFS", readOnly: false, dirty: true, status: "attention", reasons: ["O volume C: está marcado como sujo: o Windows verificará o disco na próxima inicialização."] }],
    });
    const html = ready(s);
    expect(html).toContain("Marcado como sujo");
    expect(html).toContain("o Windows verificará o disco");
  });
});

describe("Windows Health — desconhecido, dado parcial e elevação", () => {
  it("nada avaliado: estado Desconhecido, sem tratar como saudável", () => {
    const s = healthySnapshot();
    s.overall = { status: "unknown", reasons: [], evaluated: 0, rateable: 6 };
    const html = ready(s);
    expect(html).toContain("Desconhecido");
    expect(html).toContain("0 de 6 domínios avaliados");
    expect(html).toContain("nada é tratado como saudável");
  });
  it("volume cujo bit 'sujo' exige elevação mostra isso, sem UAC e sem 'limpo'", () => {
    const s = withStatus(healthySnapshot(), "volumes", "unknown", ["Estado de integridade do volume C: indisponível: requer privilégio administrativo."], {
      items: [{ mount: "C:", filesystem: "NTFS", readOnly: false, dirty: null, status: "unknown", reasons: ["Estado de integridade do volume C: indisponível: requer privilégio administrativo."] }],
    });
    s.volumes.sources = [{ id: "volumes", label: "Volumes e sistema de arquivos", state: "partial", reason: "O bit de volume sujo requer privilégio administrativo" }];
    s.capabilities = s.volumes.sources;
    const html = ready(s);
    expect(html).toContain("Sujo: requer privilégio administrativo");
    expect(html).not.toContain("Não está sujo");
    expect(html).toContain("Fontes não disponíveis (1)");
    expect(html).toContain("O bit de volume sujo requer privilégio administrativo");
  });
  it("fontes indisponíveis e que exigem elevação são listadas com a razão", () => {
    const s = healthySnapshot();
    s.capabilities = [
      { id: "update_install_time", label: "Última instalação bem-sucedida", state: "unavailable", reason: "O Windows não mantém mais este registro nesta versão" },
      { id: "windows_update", label: "Windows Update", state: "requires_elevation", reason: "Requer privilégio administrativo" },
    ];
    const html = ready(s);
    expect(html).toContain("Fontes não disponíveis (2)");
    expect(html).toContain("O Windows não mantém mais este registro nesta versão");
    expect(html).toContain("Requer privilégio administrativo");
  });
  it("reinício que não pôde ser confirmado é 'Desconhecido', nunca 'Não'", () => {
    const s = withStatus(healthySnapshot(), "restart", "unknown", ["Não foi possível confirmar se há reinício pendente: fonte indisponível (Windows Update)."], { pending: null });
    const html = ready(s);
    expect(html).toContain("Não foi possível confirmar; uma fonte não respondeu.");
    const summary = html.slice(html.indexOf('aria-label="Resumo"'), html.indexOf("mh-win-system"));
    expect(summary).toMatch(/Reinício pendente<\/small><strong>Desconhecido/);
  });
  it("seção desatualizada é marcada", () => {
    const s = healthySnapshot();
    s.restart.checkedAt = NOW - 600_000;
    expect(ready(s)).toContain("desatualizado");
  });
  it("serviço que não pôde ser consultado aparece como Desconhecido, não como ok", () => {
    const s = healthySnapshot();
    s.services.items = [service({ state: "unknown", start: "unknown", health: "unknown", reason: "O serviço não pôde ser consultado." })];
    s.services.status = "unknown";
    const html = ready(s);
    expect(html).toContain("Desconhecido");
    expect(html).toContain("O serviço não pôde ser consultado.");
  });
});

describe("Windows Health — carregando e falha", () => {
  it("sem snapshot: estado de espera honesto, sem números", () => {
    const html = ready(null);
    expect(html).toContain("Lendo o estado do Windows…");
    expect(html).not.toContain("domínios avaliados");
  });
  it("falha sem snapshot mostra o erro", () => {
    expect(ready(null, "falha de leitura")).toContain("falha de leitura");
  });
  it("falha depois de já haver leitura mantém a anterior e avisa", () => {
    const html = ready(healthySnapshot(), "timeout");
    expect(html).toContain("A última atualização falhou (timeout); mostrando a leitura anterior.");
    expect(html).toContain("Windows 11 Pro");
  });
  it("volume somente leitura é mostrado como tal", () => {
    const s = withStatus(healthySnapshot(), "volumes", "attention", ["O volume D: está montado como somente leitura."], {
      items: [{ mount: "D:", filesystem: "NTFS", readOnly: true, dirty: false, status: "attention", reasons: ["O volume D: está montado como somente leitura."] }],
    });
    expect(ready(s)).toContain("Somente leitura");
  });
});
