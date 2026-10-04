import { describe, expect, it } from "vitest";
import {
  UNKNOWN_TEXT,
  WIN_HEALTH_LABEL,
  checkedLabel,
  countOrDash,
  isStale,
  restartLabel,
  serviceCounts,
  showBadge,
  sourceText,
  summaryItems,
  unavailableSources,
  updateSummary,
} from "./windowsHealth";
import { NOW, healthySnapshot, service, withStatus } from "./windowsHealth.fixtures";

describe("rótulos de saúde", () => {
  it("quatro estados, e 'desconhecido' nunca se parece com saudável", () => {
    expect(WIN_HEALTH_LABEL).toEqual({ healthy: "Saudável", attention: "Atenção", critical: "Crítico", unknown: "Desconhecido" });
    expect(WIN_HEALTH_LABEL.unknown).not.toBe(WIN_HEALTH_LABEL.healthy);
  });
});

describe("reinício pendente", () => {
  it("sim, não e desconhecido (fonte que não respondeu nunca vira 'não')", () => {
    expect(restartLabel(true)).toBe("Sim");
    expect(restartLabel(false)).toBe("Não");
    expect(restartLabel(null)).toBe(UNKNOWN_TEXT);
  });
});

describe("Windows Update", () => {
  const base = healthySnapshot().updates;
  it("sem falhas, com falhas, serviço desabilitado e desconhecido", () => {
    expect(updateSummary(base)).toBe("Sem falhas recentes");
    expect(updateSummary({ ...base, status: "attention", failures7d: 1 })).toBe("1 falha em 7 dias");
    expect(updateSummary({ ...base, status: "attention", failures7d: 3 })).toBe("3 falhas em 7 dias");
    expect(updateSummary({ ...base, status: "attention", failures7d: 0, service: service({ start: "disabled" }) })).toBe("Serviço desabilitado");
    expect(updateSummary({ ...base, status: "unknown", failures7d: null })).toBe(UNKNOWN_TEXT);
  });
});

describe("serviços", () => {
  it("conta saudáveis, atenção/críticos e desconhecidos", () => {
    const counts = serviceCounts([
      service(), service(),
      service({ health: "attention" }), service({ health: "critical" }),
      service({ health: "unknown" }),
    ]);
    expect(counts).toEqual({ total: 5, healthy: 2, attention: 2, unknown: 1 });
    expect(serviceCounts([])).toEqual({ total: 0, healthy: 0, attention: 0, unknown: 0 });
  });
});

describe("resumo compacto", () => {
  it("máquina saudável: seis itens com valores reais", () => {
    const items = Object.fromEntries(summaryItems(healthySnapshot()).map((i) => [i.id, i]));
    expect(Object.keys(items)).toEqual(["windows", "update", "restart", "events", "devices", "services"]);
    expect(items.windows.value).toBe("Saudável");
    expect(items.restart.value).toBe("Não");
    expect(items.events.value).toBe("0");
    expect(items.devices.value).toBe("0");
    expect(items.services.value).toBe("2 de 2 saudáveis");
  });
  it("domínio desconhecido não vira zero nem 'ok'", () => {
    let s = withStatus(healthySnapshot(), "events", "unknown", ["Registro de Eventos do Windows: indisponível"]);
    s = withStatus(s, "devices", "unknown", ["sem dispositivos"]);
    const items = Object.fromEntries(summaryItems(s).map((i) => [i.id, i]));
    expect(items.events.value).toBe(UNKNOWN_TEXT);
    expect(items.devices.value).toBe(UNKNOWN_TEXT);
  });
  it("atenção e crítico aparecem com seus números", () => {
    let s = withStatus(healthySnapshot(), "devices", "attention", ["2 dispositivos reportam problema"], {
      issues: [
        { name: "A", class: null, manufacturer: null, problemCode: 43, problem: "código 43" },
        { name: "B", class: null, manufacturer: null, problemCode: 10, problem: "código 10" },
      ],
    });
    s = withStatus(s, "restart", "attention", ["Reinicialização pendente: o Windows Update exige reinício."], { pending: true });
    const items = Object.fromEntries(summaryItems(s).map((i) => [i.id, i]));
    expect(items.devices.value).toBe("2");
    expect(items.devices.status).toBe("attention");
    expect(items.restart.value).toBe("Sim");
  });
  it("serviços desconhecidos são mencionados", () => {
    const s = healthySnapshot();
    s.services.items = [service(), service({ health: "unknown", state: "unknown", start: "unknown" })];
    expect(summaryItems(s).find((i) => i.id === "services")?.value).toBe("1 de 2 saudáveis · 1 desconhecidos");
    s.services.items = [];
    expect(summaryItems(s).find((i) => i.id === "services")?.value).toBe(UNKNOWN_TEXT);
  });
});

describe("frescor das leituras", () => {
  it("passa de velho só além do dobro do TTL do próprio domínio", () => {
    const section = { checkedAt: NOW, ttlMs: 60_000 };
    expect(isStale(section, NOW + 119_000)).toBe(false);
    expect(isStale(section, NOW + 121_000)).toBe(true);
    expect(isStale({ checkedAt: NOW, ttlMs: 0 }, NOW + 9_999_999)).toBe(false);
  });
  it("rótulo de 'última leitura'", () => {
    expect(checkedLabel(NOW, NOW + 5_000)).toBe("agora");
    expect(checkedLabel(NOW, NOW + 3 * 60_000)).toBe("há 3 min");
    expect(checkedLabel(NOW, NOW + 2 * 3_600_000)).toBe("há 2 h");
    expect(checkedLabel(NOW, NOW - 10_000)).toBe("agora");
  });
});

describe("fontes indisponíveis e elevação", () => {
  const notes = [
    { id: "a", label: "Fonte A", state: "available" as const, reason: null },
    { id: "b", label: "Fonte B", state: "requires_elevation" as const, reason: "negado" },
    { id: "c", label: "Fonte C", state: "unavailable" as const, reason: "O Windows não mantém mais este registro" },
    { id: "d", label: "Fonte D", state: "partial" as const, reason: null },
  ];
  it("lista só o que não está disponível", () => {
    expect(unavailableSources(notes).map((n) => n.id)).toEqual(["b", "c", "d"]);
  });
  it("texto: elevação é dita sem pedir UAC; indisponível traz a razão", () => {
    expect(sourceText(notes[1])).toBe("Requer privilégio administrativo");
    expect(sourceText(notes[2])).toBe("O Windows não mantém mais este registro");
    expect(sourceText(notes[3])).toBe("Disponível em parte");
    expect(sourceText({ ...notes[2], reason: null })).toBe("Não disponível");
  });
});

describe("contagens e selos", () => {
  it("janela não medida é traço, nunca zero", () => {
    expect(countOrDash(null)).toBe("—");
    expect(countOrDash(undefined)).toBe("—");
    expect(countOrDash(0)).toBe("0");
    expect(countOrDash(12)).toBe("12");
  });
  it("seção informativa e desconhecida não mostra selo de saúde", () => {
    expect(showBadge({ rated: false, status: "unknown" })).toBe(false);
    expect(showBadge({ rated: true, status: "unknown" })).toBe(true);
    expect(showBadge({ rated: false, status: "attention" })).toBe(true);
  });
});
