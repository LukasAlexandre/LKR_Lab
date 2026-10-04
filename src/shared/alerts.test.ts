import { describe, expect, it } from "vitest";
import {
  DEFAULT_ALERT_FILTER,
  absolute,
  ago,
  canStart,
  ctaAction,
  diagnosticStatus,
  domainsOf,
  duration,
  filterAlerts,
  openCount,
  runLabel,
  runOutcome,
  sortAlerts,
  summaryText,
  worstSeverity,
} from "./alerts";
import { MIN, NOW, alert, attention, catalog, diagInfo, diagnostics, info, run, runtimeFailed, summaryOf, volumeProblem } from "./alerts.fixtures";

const acknowledged = () => attention({ alertId: "ack", fingerprint: "x@ack", status: "acknowledged", acknowledgedAt: NOW - MIN });
const resolved = () => alert({ alertId: "res", fingerprint: "x@res", status: "resolved", resolvedAt: NOW - 5 * MIN });

describe("ordenação previsível", () => {
  it("abertos antes de resolvidos; Crítico > Atenção > Info", () => {
    const sorted = sortAlerts([info(), resolved(), attention(), alert()]);
    expect(sorted.map((a) => a.alertId)).toEqual(["a1", "a2", "a3", "res"]);
  });
  it("na mesma severidade, o mais antigo primeiro; o fingerprint desempata", () => {
    const older = attention({ alertId: "old", fingerprint: "b", firstSeen: NOW - 60 * MIN });
    const newer = attention({ alertId: "new", fingerprint: "a", firstSeen: NOW - 5 * MIN });
    const tie = attention({ alertId: "tie", fingerprint: "a0", firstSeen: NOW - 5 * MIN });
    expect(sortAlerts([newer, tie, older]).map((a) => a.alertId)).toEqual(["old", "new", "tie"]);
  });
  it("não altera o array original e é determinística", () => {
    const input = [info(), alert(), attention()];
    const copy = [...input];
    const once = sortAlerts(input);
    expect(input).toEqual(copy);
    expect(sortAlerts(once)).toEqual(once);
  });
});

describe("filtros", () => {
  const all = [alert(), attention(), info(), acknowledged(), resolved(), runtimeFailed()];
  it("padrão: abertos (ativos + reconhecidos), sem resolvidos", () => {
    const ids = filterAlerts(all, DEFAULT_ALERT_FILTER).map((a) => a.alertId);
    expect(ids).not.toContain("res");
    expect(ids).toContain("ack");
    expect(ids).toHaveLength(5);
  });
  it("Ativos, Reconhecidos e Resolvidos", () => {
    expect(filterAlerts(all, { ...DEFAULT_ALERT_FILTER, status: "active" }).every((a) => a.status === "active")).toBe(true);
    expect(filterAlerts(all, { ...DEFAULT_ALERT_FILTER, status: "acknowledged" }).map((a) => a.alertId)).toEqual(["ack"]);
    expect(filterAlerts(all, { ...DEFAULT_ALERT_FILTER, status: "resolved" }).map((a) => a.alertId)).toEqual(["res"]);
    expect(filterAlerts(all, { ...DEFAULT_ALERT_FILTER, status: "all" })).toHaveLength(6);
  });
  it("Crítico, Atenção e Info", () => {
    expect(filterAlerts(all, { ...DEFAULT_ALERT_FILTER, severity: "critical" }).map((a) => a.alertId)).toEqual(["a1"]);
    expect(filterAlerts(all, { ...DEFAULT_ALERT_FILTER, severity: "info" }).map((a) => a.alertId)).toEqual(["a3"]);
    expect(filterAlerts(all, { ...DEFAULT_ALERT_FILTER, severity: "attention" }).every((a) => a.severity === "attention")).toBe(true);
  });
  it("por domínio, combinando com os demais filtros", () => {
    expect(filterAlerts(all, { ...DEFAULT_ALERT_FILTER, domain: "runtime" }).map((a) => a.alertId)).toEqual(["a4"]);
    expect(filterAlerts(all, { status: "resolved", severity: "critical", domain: "machine" }).map((a) => a.alertId)).toEqual(["res"]);
    expect(filterAlerts(all, { status: "active", severity: "info", domain: "machine" })).toEqual([]);
  });
  it("domínios disponíveis, ordenados e sem repetição", () => {
    expect(domainsOf(all)).toEqual(["machine", "network", "runtime", "windows"]);
    expect(domainsOf([])).toEqual([]);
  });
});

describe("resumo", () => {
  it("texto, total aberto e pior severidade", () => {
    const summary = summaryOf([alert(), attention(), attention({ alertId: "b" }), info(), info({ alertId: "c" }), info({ alertId: "d" })]);
    expect(summaryText(summary)).toBe("1 crítico · 2 atenção · 3 informações");
    expect(openCount(summary)).toBe(6);
    expect(worstSeverity(summary)).toBe("critical");
  });
  it("sem alertas: zero em tudo e nenhuma severidade", () => {
    const summary = summaryOf([]);
    expect(summaryText(summary)).toBe("0 críticos · 0 atenção · 0 informações");
    expect(worstSeverity(summary)).toBeNull();
  });
  it("resolvidos não contam como abertos, só como 'resolvidos recentemente'", () => {
    const summary = summaryOf([resolved(), info()]);
    expect(openCount(summary)).toBe(1);
    expect(summary.resolvedRecently).toBe(1);
    expect(worstSeverity(summaryOf([attention()]))).toBe("attention");
    expect(worstSeverity(summaryOf([info()]))).toBe("info");
  });
});

describe("tempo", () => {
  it("há quanto tempo", () => {
    expect(ago(NOW - 5_000, NOW)).toBe("agora");
    expect(ago(NOW - 3 * MIN, NOW)).toBe("há 3 min");
    expect(ago(NOW - 2 * 3_600_000, NOW)).toBe("há 2 h");
    expect(ago(NOW - 4 * 86_400_000, NOW)).toBe("há 4 d");
    expect(ago(NOW + 5_000, NOW)).toBe("agora");
    expect(absolute(null)).toBe("—");
    expect(absolute(NOW)).toMatch(/\d/);
  });
  it("duração de diagnóstico", () => {
    expect(duration(NOW - 42_000, null, NOW)).toBe("42 s");
    expect(duration(NOW - 185_000, null, NOW)).toBe("3 min 05 s");
    expect(duration(NOW - 120_000, NOW - 60_000, NOW)).toBe("1 min 00 s");
  });
});

describe("CTA contextual (só navegação, nunca corrigir)", () => {
  it("Runtime e Project levam à rota do Project", () => {
    expect(ctaAction({ kind: "runtime", target: "p1" })).toEqual({ kind: "hash", label: "Abrir Runtime", value: "#project/p1/runtime" });
    expect(ctaAction({ kind: "project", target: "p1" })).toEqual({ kind: "hash", label: "Abrir Project", value: "#project/p1/overview" });
  });
  it("Windows Health e Network & Security levam ao painel", () => {
    expect(ctaAction({ kind: "windows_health", target: null })).toEqual({ kind: "scroll", label: "Abrir Windows Health", value: "windows-health" });
    expect(ctaAction({ kind: "network_security", target: null })).toEqual({ kind: "scroll", label: "Abrir Network & Security", value: "network-security" });
    expect(ctaAction({ kind: "machine", target: null })?.value).toBe("#processes");
  });
  it("sem alvo, desconhecido ou ausente: nenhum botão", () => {
    expect(ctaAction({ kind: "runtime", target: null })).toBeNull();
    expect(ctaAction({ kind: "fix_automatically", target: null })).toBeNull();
    expect(ctaAction(null)).toBeNull();
  });
  it("nenhum rótulo propõe corrigir", () => {
    for (const kind of ["runtime", "project", "windows_health", "network_security", "machine"]) {
      expect(ctaAction({ kind, target: "p1" })?.label.toLowerCase() ?? "").not.toMatch(/corrig|repar|fix/);
    }
  });
});

describe("diagnósticos", () => {
  it("disponível, requer administrador, em execução e indisponível", () => {
    expect(diagnosticStatus(diagInfo({ available: true, reason: null }), null)).toEqual({ state: "available" });
    expect(diagnosticStatus(diagInfo(), null).state).toBe("requires_elevation");
    expect(diagnosticStatus(diagInfo({ available: true }), run({ running: true, result: null })).state).toBe("running");
    expect(diagnosticStatus(diagInfo({ requiresElevation: false, available: false, reason: "x" }), null)).toEqual({ state: "unavailable", reason: "x" });
    expect(diagnosticStatus(undefined, null).state).toBe("unavailable");
  });
  it("um diagnóstico já terminado não bloqueia o próximo", () => {
    expect(diagnosticStatus(diagInfo({ available: true }), run()).state).toBe("available");
  });
  it("canStart depende da elevação e de nada rodando", () => {
    expect(canStart(diagnostics({ catalog: catalog(true) }), "dism_checkhealth")).toBe(true);
    expect(canStart(diagnostics({ catalog: catalog(false) }), "dism_checkhealth")).toBe(false);
    expect(canStart(diagnostics({ catalog: catalog(true), current: run({ running: true, result: null }) }), "sfc_verifyonly")).toBe(false);
    expect(canStart(diagnostics({ catalog: catalog(true) }), "sfc_scannow")).toBe(false);
  });
  it("rótulos de execução e resultado", () => {
    expect(runLabel(run())).toContain("CHKDSK");
    expect(runLabel(run())).toContain("C:");
    expect(runLabel(run({ target: null }))).not.toContain("·");
    expect(runOutcome(run())).toBe("Sem problemas");
    expect(runOutcome(run({ result: "problems_found" }))).toBe("Problemas encontrados");
    expect(runOutcome(run({ result: "failed" }))).toBe("Falhou");
    expect(runOutcome(run({ result: "cancelled" }))).toBe("Cancelado");
    expect(runOutcome(run({ result: "inconclusive" }))).toBe("Inconclusivo");
    expect(runOutcome(run({ running: true, result: null }))).toBe("Em execução");
  });
  it("o catálogo da interface só tem diagnósticos de leitura", () => {
    const ids = catalog(true).map((d) => d.id);
    expect(ids).toEqual(["sfc_verifyonly", "dism_checkhealth", "dism_scanhealth", "chkdsk_scan"]);
    expect(JSON.stringify(volumeProblem().diagnosticAction)).not.toMatch(/scannow|restorehealth/i);
  });
});
