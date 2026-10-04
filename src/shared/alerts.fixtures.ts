import type {
  AlertRecord,
  AlertSourceStatus,
  AlertSummary,
  AlertsSnapshot,
  DiagInfo,
  DiagRun,
  DiagnosticsView,
} from "./types";

/* Dados de teste do contrato de Alerts & Diagnostics. */

export const NOW = 1_791_121_512_000;
export const MIN = 60_000;

export const alert = (patch: Partial<AlertRecord> = {}): AlertRecord => ({
  alertId: "a1",
  id: "machine.disk.low_space@C:",
  fingerprint: "machine.disk.low_space@C:",
  ruleId: "machine.disk.low_space",
  title: "Pouco espaço livre em C:",
  summary: "O volume C: tem 2.0% livre (4.0 GiB).",
  severity: "critical",
  confidence: "high",
  domain: "machine",
  source: "machine.disk",
  resource: "C:",
  evidence: [
    { label: "Volume", value: "C:", source: "machine.disk" },
    { label: "Livre", value: "2.0% (4.0 GiB)", source: "machine.disk" },
  ],
  reason: "Espaço livre abaixo do limite (menos de 5% e menos de 5.0 GiB livres).",
  recommendedNextStep: "Libere espaço no volume C: antes que o sistema fique sem capacidade.",
  diagnosticAction: null,
  cta: { kind: "machine", target: null },
  status: "active",
  firstSeen: NOW - 30 * MIN,
  lastSeen: NOW - 1 * MIN,
  acknowledgedAt: null,
  resolvedAt: null,
  occurrenceCount: 1,
  observations: 12,
  ...patch,
});

export const attention = (patch: Partial<AlertRecord> = {}) =>
  alert({
    alertId: "a2",
    id: "windows.reboot.pending@system",
    fingerprint: "windows.reboot.pending@system",
    ruleId: "windows.reboot.pending",
    title: "Reinício do Windows pendente",
    summary: "O Windows indica que um reinício é necessário.",
    severity: "attention",
    domain: "windows",
    source: "windows.restart",
    resource: "system",
    evidence: [{ label: "Reinício pendente", value: "Sim", source: "windows.restart" }],
    reason: "Uma fonte forte informa reinício pendente.",
    recommendedNextStep: "Salve o trabalho e reinicie quando for conveniente. Nada é reiniciado automaticamente.",
    cta: { kind: "windows_health", target: null },
    firstSeen: NOW - 10 * MIN,
    ...patch,
  });

export const info = (patch: Partial<AlertRecord> = {}) =>
  alert({
    alertId: "a3",
    id: "network.listener.all_interfaces@vite.exe",
    fingerprint: "network.listener.all_interfaces@vite.exe",
    ruleId: "network.listener.all_interfaces",
    title: "vite.exe escuta em todas as interfaces",
    summary: "O processo aceita conexões em todas as interfaces desta máquina (0.0.0.0 ou ::).",
    severity: "info",
    domain: "network",
    source: "network.exposure",
    resource: "vite.exe",
    evidence: [
      { label: "Processo", value: "vite.exe", source: "network.exposure" },
      { label: "Portas", value: "3000, 5173", source: "network.exposure" },
    ],
    reason: "É um fato, não um defeito. Isto não significa exposição à internet.",
    recommendedNextStep: "Se o acesso pela rede local não for necessário, configure o serviço para escutar só em 127.0.0.1.",
    cta: { kind: "network_security", target: null },
    firstSeen: NOW - 50 * MIN,
    ...patch,
  });

export const runtimeFailed = (patch: Partial<AlertRecord> = {}) =>
  alert({
    alertId: "a4",
    id: "runtime.managed.failed@p1:node:lab",
    fingerprint: "runtime.managed.failed@p1:node:lab",
    ruleId: "runtime.managed.failed",
    title: "Execução falhou: npm run lab",
    summary: "A execução falhou e a porta declarada 4317 está ocupada por outro processo.",
    severity: "attention",
    domain: "runtime",
    source: "runtime.runs",
    resource: "p1:node:lab",
    evidence: [
      { label: "Código de saída", value: "1", source: "runtime.runs" },
      { label: "Porta declarada ocupada", value: "4317", source: "runtime.runs" },
    ],
    cta: { kind: "runtime", target: "p1" },
    ...patch,
  });

export const volumeProblem = (patch: Partial<AlertRecord> = {}) =>
  alert({
    alertId: "a5",
    id: "windows.volume.problem@D:",
    fingerprint: "windows.volume.problem@D:",
    ruleId: "windows.volume.problem",
    title: "Problema confirmado no volume D:",
    severity: "attention",
    domain: "windows",
    source: "windows.volumes",
    resource: "D:",
    diagnosticAction: { id: "chkdsk_scan", target: "D:", label: "Examinar um volume (CHKDSK, somente leitura)" },
    cta: { kind: "windows_health", target: null },
    ...patch,
  });

export const diagInfo = (patch: Partial<DiagInfo> = {}): DiagInfo => ({
  id: "sfc_verifyonly",
  label: "Verificar arquivos do sistema (SFC, somente verificação)",
  description: "Compara os arquivos protegidos do Windows com a cópia original e só informa; não repara nada.",
  needsTarget: false,
  requiresElevation: true,
  available: false,
  reason: "Requer administrador: o LKR LAB não solicita elevação automaticamente.",
  ...patch,
});

export const catalog = (elevated: boolean): DiagInfo[] => [
  diagInfo({ available: elevated, reason: elevated ? null : diagInfo().reason }),
  diagInfo({ id: "dism_checkhealth", label: "Verificar o component store (DISM CheckHealth)", description: "Consulta se o component store já foi marcado como corrompido.", available: elevated, reason: elevated ? null : diagInfo().reason }),
  diagInfo({ id: "dism_scanhealth", label: "Examinar o component store (DISM ScanHealth)", description: "Examina o component store em busca de corrupção, sem reparar.", available: elevated, reason: elevated ? null : diagInfo().reason }),
  diagInfo({ id: "chkdsk_scan", label: "Examinar um volume (CHKDSK, somente leitura)", description: "Examina o sistema de arquivos de um volume online, sem corrigir nada (/scan).", needsTarget: true, available: elevated, reason: elevated ? null : diagInfo().reason }),
];

export const run = (patch: Partial<DiagRun> = {}): DiagRun => ({
  id: "run1",
  diagnostic: "chkdsk_scan",
  label: "Examinar um volume (CHKDSK, somente leitura)",
  target: "C:",
  startedAt: NOW - 3 * MIN,
  finishedAt: NOW - 2 * MIN,
  running: false,
  result: "clean",
  exitCode: 0,
  summary: "O volume foi examinado e nenhum problema foi encontrado.",
  outputTail: ["Windows has scanned the file system and found no problems."],
  ...patch,
});

export const diagnostics = (patch: Partial<DiagnosticsView> = {}): DiagnosticsView => ({
  elevated: false,
  catalog: catalog(false),
  targets: ["C:", "D:"],
  current: null,
  history: [],
  ...patch,
});

export const sources = (): AlertSourceStatus[] => [
  { id: "machine.disk", label: "Espaço dos volumes", state: "evaluated", reason: null },
  { id: "windows.restart", label: "Reinício pendente", state: "evaluated", reason: null },
  { id: "network.exposure", label: "Portas em escuta", state: "evaluated", reason: null },
];

export const summaryOf = (alerts: AlertRecord[], now = NOW): AlertSummary => {
  const open = alerts.filter((a) => a.status !== "resolved");
  return {
    critical: open.filter((a) => a.severity === "critical").length,
    attention: open.filter((a) => a.severity === "attention").length,
    info: open.filter((a) => a.severity === "info").length,
    acknowledged: open.filter((a) => a.status === "acknowledged").length,
    resolvedRecently: alerts.filter((a) => a.status === "resolved" && a.resolvedAt != null && now - a.resolvedAt <= 24 * 60 * MIN).length,
  };
};

export const snapshot = (alerts: AlertRecord[] = [], patch: Partial<AlertsSnapshot> = {}): AlertsSnapshot => ({
  capturedAt: NOW,
  summary: summaryOf(alerts),
  alerts,
  sources: sources(),
  diagnostics: diagnostics(),
  ...patch,
});
