import { projectHash } from "../app/projectRoute";
import type {
  AlertCta,
  AlertDomain,
  AlertRecord,
  AlertSeverity,
  AlertStatus,
  AlertSummary,
  DiagInfo,
  DiagResult,
  DiagRun,
  DiagnosticsView,
} from "./types";

/* Leitura do contrato de Alerts & Diagnostics para a tela. Só apresentação: severidade, confiança,
 * evidência e estado vêm do motor determinístico do backend (sem IA, sem pontuação). */

export const SEVERITY_LABEL: Record<AlertSeverity, string> = {
  critical: "Crítico",
  attention: "Atenção",
  info: "Informação",
};
export const SEVERITY_RANK: Record<AlertSeverity, number> = { critical: 3, attention: 2, info: 1 };
export const SEVERITY_TONE: Record<AlertSeverity, "warn" | "neutral" | "blue"> = {
  critical: "warn",
  attention: "warn",
  info: "blue",
};
export const STATUS_LABEL: Record<AlertStatus, string> = {
  active: "Ativo",
  acknowledged: "Reconhecido",
  resolved: "Resolvido",
};
export const CONFIDENCE_LABEL = { high: "Confiança alta", medium: "Confiança média", low: "Confiança baixa" } as const;
export const DOMAIN_LABEL: Record<AlertDomain, string> = {
  machine: "Máquina",
  windows: "Windows",
  security: "Segurança",
  network: "Rede",
  runtime: "Runtime",
};
export const RESULT_LABEL: Record<DiagResult, string> = {
  clean: "Sem problemas",
  problems_found: "Problemas encontrados",
  inconclusive: "Inconclusivo",
  failed: "Falhou",
  cancelled: "Cancelado",
};
export const SOURCE_STATE_LABEL = {
  evaluated: "Avaliada",
  stale: "Desatualizada",
  unavailable: "Sem dado",
} as const;

export type StatusFilter = "open" | "active" | "acknowledged" | "resolved" | "all";
export interface AlertFilter {
  status: StatusFilter;
  severity: "all" | AlertSeverity;
  domain: "all" | AlertDomain;
}
export const DEFAULT_ALERT_FILTER: AlertFilter = { status: "open", severity: "all", domain: "all" };
export const STATUS_FILTER_LABEL: Record<StatusFilter, string> = {
  open: "Abertos",
  active: "Ativos",
  acknowledged: "Reconhecidos",
  resolved: "Resolvidos",
  all: "Todos",
};

/** Abertos antes de resolvidos; Crítico > Atenção > Info; dentro disso o mais antigo primeiro e o
 * fingerprint desempata (mesma ordem do backend, para que filtrar nunca embaralhe). */
export function sortAlerts(alerts: AlertRecord[]): AlertRecord[] {
  return [...alerts].sort(
    (a, b) =>
      Number(a.status === "resolved") - Number(b.status === "resolved") ||
      SEVERITY_RANK[b.severity] - SEVERITY_RANK[a.severity] ||
      a.firstSeen - b.firstSeen ||
      a.fingerprint.localeCompare(b.fingerprint),
  );
}

export function filterAlerts(alerts: AlertRecord[], filter: AlertFilter): AlertRecord[] {
  return sortAlerts(
    alerts.filter((a) => {
      if (filter.status === "open" && a.status === "resolved") return false;
      if (["active", "acknowledged", "resolved"].includes(filter.status) && a.status !== filter.status) return false;
      if (filter.severity !== "all" && a.severity !== filter.severity) return false;
      if (filter.domain !== "all" && a.domain !== filter.domain) return false;
      return true;
    }),
  );
}

/** Domínios que aparecem nos alertas atuais (para o filtro). */
export const domainsOf = (alerts: AlertRecord[]): AlertDomain[] =>
  [...new Set(alerts.map((a) => a.domain))].sort((a, b) => DOMAIN_LABEL[a].localeCompare(DOMAIN_LABEL[b]));

/** "0 críticos · 2 atenção · 3 informações". */
export function summaryText(summary: AlertSummary): string {
  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  return [plural(summary.critical, "crítico", "críticos"), `${summary.attention} atenção`, plural(summary.info, "informação", "informações")].join(" · ");
}
export const openCount = (summary: AlertSummary) => summary.critical + summary.attention + summary.info;
/** Pior severidade aberta (`null` = nenhum alerta aberto). */
export const worstSeverity = (summary: AlertSummary): AlertSeverity | null =>
  summary.critical > 0 ? "critical" : summary.attention > 0 ? "attention" : summary.info > 0 ? "info" : null;

/** "agora", "há 3 min", "há 2 h", "há 4 d". */
export function ago(ms: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - ms) / 1000));
  if (seconds < 20) return "agora";
  if (seconds < 3600) return `há ${Math.max(1, Math.round(seconds / 60))} min`;
  if (seconds < 86_400) return `há ${Math.round(seconds / 3600)} h`;
  return `há ${Math.round(seconds / 86_400)} d`;
}
export const absolute = (ms: number | null | undefined) =>
  ms == null ? "—" : new Date(ms).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" });

/** Duração curta de um diagnóstico: "42 s", "3 min 05 s". */
export function duration(startedAt: number, finishedAt: number | null, now: number): string {
  const seconds = Math.max(0, Math.round(((finishedAt ?? now) - startedAt) / 1000));
  if (seconds < 60) return `${seconds} s`;
  return `${Math.floor(seconds / 60)} min ${String(seconds % 60).padStart(2, "0")} s`;
}

export type CtaAction =
  | { kind: "hash"; label: string; value: string }
  | { kind: "scroll"; label: string; value: string };

/** Para onde o CTA leva. Navegação apenas: nunca "corrigir". */
export function ctaAction(cta: AlertCta | null | undefined): CtaAction | null {
  if (!cta) return null;
  switch (cta.kind) {
    case "runtime":
      return cta.target ? { kind: "hash", label: "Abrir Runtime", value: projectHash(cta.target, "runtime") } : null;
    case "project":
      return cta.target ? { kind: "hash", label: "Abrir Project", value: projectHash(cta.target) } : null;
    case "windows_health":
      return { kind: "scroll", label: "Abrir Windows Health", value: "windows-health" };
    case "network_security":
      return { kind: "scroll", label: "Abrir Network & Security", value: "network-security" };
    case "machine":
      return { kind: "hash", label: "Ver processos", value: "#processes" };
    default:
      return null;
  }
}

export type DiagnosticStatus =
  | { state: "available" }
  | { state: "requires_elevation"; reason: string }
  | { state: "unavailable"; reason: string }
  | { state: "running" };

/** Situação de um diagnóstico do catálogo agora. */
export function diagnosticStatus(info: DiagInfo | undefined, current: DiagRun | null): DiagnosticStatus {
  if (!info) return { state: "unavailable", reason: "Diagnóstico fora do catálogo." };
  if (current?.running) return { state: "running" };
  if (!info.available) {
    return info.requiresElevation
      ? { state: "requires_elevation", reason: info.reason ?? "Requer administrador." }
      : { state: "unavailable", reason: info.reason ?? "Indisponível." };
  }
  return { state: "available" };
}

export const runLabel = (run: Pick<DiagRun, "label" | "target">) => (run.target ? `${run.label} · ${run.target}` : run.label);

/** Resultado em linguagem curta; nunca "sucesso" só porque terminou. */
export function runOutcome(run: DiagRun): string {
  if (run.running) return "Em execução";
  return run.result ? RESULT_LABEL[run.result] : "Inconclusivo";
}

export const diagnosticFor = (view: DiagnosticsView, id: string): DiagInfo | undefined => view.catalog.find((d) => d.id === id);

/** Diagnósticos que podem ser iniciados agora (nenhum se algum já roda). */
export const canStart = (view: DiagnosticsView, id: string) => diagnosticStatus(diagnosticFor(view, id), view.current).state === "available";
