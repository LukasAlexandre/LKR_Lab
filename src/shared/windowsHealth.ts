import type {
  WinHealth,
  WinServiceState,
  WinServiceView,
  WinSection,
  WinSourceNote,
  WinStartType,
  WindowsHealthSnapshot,
} from "./types";

/* Leitura do contrato de Windows Health para a tela. Tudo aqui é só apresentação: o estado de
 * cada domínio e o motivo vêm do backend; "desconhecido" nunca é mostrado como saudável. */

export const UNKNOWN_TEXT = "Desconhecido";

export const WIN_HEALTH_LABEL: Record<WinHealth, string> = {
  healthy: "Saudável",
  attention: "Atenção",
  critical: "Crítico",
  unknown: UNKNOWN_TEXT,
};
export const WIN_HEALTH_TONE: Record<WinHealth, "good" | "warn" | "danger" | "neutral"> = {
  healthy: "good",
  attention: "warn",
  critical: "danger",
  unknown: "neutral",
};

export const DOMAIN_LABEL: Record<string, string> = {
  restart: "Reinício",
  updates: "Windows Update",
  services: "Serviços",
  devices: "Dispositivos",
  events: "Eventos",
  volumes: "Volumes",
};

export const SERVICE_STATE_LABEL: Record<WinServiceState, string> = {
  running: "Em execução",
  stopped: "Parado",
  start_pending: "Iniciando",
  stop_pending: "Parando",
  paused: "Pausado",
  unknown: UNKNOWN_TEXT,
};
export const START_TYPE_LABEL: Record<WinStartType, string> = {
  automatic: "Automático",
  automatic_delayed: "Automático (atrasado)",
  manual: "Manual / sob demanda",
  disabled: "Desabilitado",
  unknown: UNKNOWN_TEXT,
};

/** Dado velho: passou do dobro do TTL do próprio domínio (domínios derivados têm TTL 0 e nunca ficam velhos). */
export function isStale(section: Pick<WinSection<object>, "checkedAt" | "ttlMs">, now: number): boolean {
  return section.ttlMs > 0 && now - section.checkedAt > section.ttlMs * 2;
}

/** "agora", "há 3 min", "há 2 h". */
export function checkedLabel(checkedAt: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - checkedAt) / 1000));
  if (seconds < 20) return "agora";
  if (seconds < 3600) return `há ${Math.max(1, Math.round(seconds / 60))} min`;
  return `há ${Math.round(seconds / 3600)} h`;
}

export const formatDateTime = (ms: number | null | undefined) =>
  ms == null ? null : new Date(ms).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" });

/** Reinício pendente: sim/não/desconhecido (nunca "não" quando a fonte não respondeu). */
export function restartLabel(pending: boolean | null): string {
  return pending == null ? UNKNOWN_TEXT : pending ? "Sim" : "Não";
}

/** Texto curto e honesto da linha de Windows Update. */
export function updateSummary(updates: WindowsHealthSnapshot["updates"]): string {
  if (updates.status === "unknown") return UNKNOWN_TEXT;
  if (updates.failures7d != null && updates.failures7d > 0) {
    return `${updates.failures7d} falha${updates.failures7d === 1 ? "" : "s"} em 7 dias`;
  }
  if (updates.service?.start === "disabled") return "Serviço desabilitado";
  return "Sem falhas recentes";
}

export interface ServiceCounts { total: number; healthy: number; attention: number; unknown: number }
export function serviceCounts(items: WinServiceView[]): ServiceCounts {
  return {
    total: items.length,
    healthy: items.filter((i) => i.health === "healthy").length,
    attention: items.filter((i) => i.health === "attention" || i.health === "critical").length,
    unknown: items.filter((i) => i.health === "unknown").length,
  };
}

export interface SummaryItem { id: string; label: string; value: string; status: WinHealth | null }
/** Resumo compacto: Windows, Update, Restart, eventos críticos, dispositivos e serviços. */
export function summaryItems(s: WindowsHealthSnapshot): SummaryItem[] {
  const counts = serviceCounts(s.services.items);
  const services = counts.total === 0
    ? UNKNOWN_TEXT
    : `${counts.healthy} de ${counts.total} saudáveis${counts.unknown ? ` · ${counts.unknown} desconhecidos` : ""}`;
  return [
    { id: "windows", label: "Windows", value: WIN_HEALTH_LABEL[s.overall.status], status: s.overall.status },
    { id: "update", label: "Windows Update", value: updateSummary(s.updates), status: s.updates.status },
    { id: "restart", label: "Reinício pendente", value: restartLabel(s.restart.pending), status: s.restart.status },
    {
      id: "events",
      label: "Eventos críticos (24 h)",
      value: s.events.status === "unknown" ? UNKNOWN_TEXT : String(s.events.last24h.critical),
      status: s.events.status,
    },
    {
      id: "devices",
      label: "Dispositivos com problema",
      value: s.devices.status === "unknown" ? UNKNOWN_TEXT : String(s.devices.issues.length),
      status: s.devices.status,
    },
    { id: "services", label: "Serviços essenciais", value: services, status: s.services.status },
  ];
}

/** Fontes que NÃO estão disponíveis, para dizer isso sem chamar de falha. */
export function unavailableSources(capabilities: WinSourceNote[]): WinSourceNote[] {
  return capabilities.filter((c) => c.state !== "available");
}

export function sourceText(note: WinSourceNote): string {
  if (note.state === "requires_elevation") return "Requer privilégio administrativo";
  if (note.state === "partial") return note.reason ?? "Disponível em parte";
  return note.reason ?? "Não disponível";
}

/** Contagem de nível com "—" para janela em que o nível não é medido. */
export const countOrDash = (value: number | null | undefined) => (value == null ? "—" : String(value));

/** Classe do selo da seção: informativos não têm selo de saúde. */
export const showBadge = (section: Pick<WinSection<object>, "rated" | "status">) => section.rated || section.status !== "unknown";
