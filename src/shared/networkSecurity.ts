import { UNKNOWN_TEXT } from "./windowsHealth";
import type {
  NetAvProvider,
  NetBitlockerState,
  NetConnection,
  NetDefenderState,
  NetFirewallAction,
  NetListener,
  NetListenerScope,
  NetProfileKind,
  NetRemoteScope,
  NetSecureBootState,
  NetWscHealth,
  NetworkSecuritySnapshot,
  WinHealth,
  WinSourceNote,
} from "./types";

/* Leitura do contrato de Network & Security para a tela. Só apresentação: estado e motivo vêm do
 * backend. "Desconhecido" nunca é saudável; escutar em todas as interfaces nunca é "exposto à internet". */

export const NET_DOMAIN_LABEL: Record<string, string> = {
  firewall: "Firewall",
  antivirus: "Antivírus",
  encryption: "Criptografia",
  secure_boot: "Secure Boot",
  tpm: "TPM",
};

export const PROFILE_LABEL: Record<NetProfileKind, string> = { domain: "Domínio", private: "Privado", public: "Público" };
export const LISTENER_SCOPE_LABEL: Record<NetListenerScope, string> = {
  loopback: "Somente esta máquina",
  specific: "Interface específica",
  all_interfaces: "Todas as interfaces",
};
export const REMOTE_SCOPE_LABEL: Record<NetRemoteScope, string> = { loopback: "Local (loopback)", local: "Rede local", remote: "Remoto" };
export const ACTION_LABEL: Record<NetFirewallAction, string> = { allow: "Permitir", block: "Bloquear" };
export const WSC_LABEL: Record<NetWscHealth, string> = {
  good: "Bom",
  not_monitored: "Não monitorado",
  poor: "Em risco",
  snooze: "Adiado",
};
export const DEFENDER_STATE_LABEL: Record<NetDefenderState, string> = {
  active: "Ativo",
  passive: "Passivo (outro antivírus protege)",
  disabled: "Desativado",
  unknown: UNKNOWN_TEXT,
};
export const AV_PROVIDER_LABEL: Record<NetAvProvider, string> = {
  defender: "Microsoft Defender",
  third_party: "Antivírus de terceiros",
  none: "Nenhum antivírus ativo",
  unknown: UNKNOWN_TEXT,
};
export const BITLOCKER_LABEL: Record<NetBitlockerState, string> = {
  protected: "Protegido",
  suspended: "Suspenso",
  off: "Desligado",
  unknown: UNKNOWN_TEXT,
};
export const SECURE_BOOT_LABEL: Record<NetSecureBootState, string> = {
  enabled: "Ativado",
  disabled: "Desativado",
  unsupported: "Não suportado (BIOS legado)",
  unavailable: UNKNOWN_TEXT,
};
export const INTERFACE_KIND_LABEL: Record<string, string> = {
  ethernet: "Ethernet",
  wifi: "Wi-Fi",
  tunnel: "Túnel / VPN",
  other: "Outra",
};

export const profileLabel = (profile: NetProfileKind | null | undefined) =>
  profile ? PROFILE_LABEL[profile] : UNKNOWN_TEXT;

export function formatLinkSpeed(bps: number | null | undefined): string {
  if (!bps) return UNKNOWN_TEXT;
  if (bps >= 1_000_000_000) return `${(bps / 1_000_000_000).toFixed(bps % 1_000_000_000 === 0 ? 0 : 1)} Gbps`;
  return `${Math.round(bps / 1_000_000)} Mbps`;
}

export const listenerAddress = (l: Pick<NetListener, "address" | "port" | "ipVersion">) =>
  l.ipVersion === "v6" ? `[${l.address}]:${l.port}` : `${l.address}:${l.port}`;

/** Quem escuta a porta; "não identificado" quando o SO não informou o dono. */
export const listenerOwner = (l: Pick<NetListener, "processName" | "pid">) =>
  l.processName ? `${l.processName} (PID ${l.pid})` : l.pid != null ? `PID ${l.pid}` : "Processo não identificado";

export type ScopeFilter = "all" | NetListenerScope;
export interface ExposureFilter { scope: ScopeFilter; query: string }
export const DEFAULT_EXPOSURE_FILTER: ExposureFilter = { scope: "all", query: "" };

/** Filtro de escopo + busca (porta, endereço, processo, PID, Project). Não altera a ordem. */
export function filterListeners(listeners: NetListener[], filter: ExposureFilter): NetListener[] {
  const q = filter.query.trim().toLowerCase();
  return listeners.filter((l) => {
    if (filter.scope !== "all" && l.scope !== filter.scope) return false;
    if (!q) return true;
    return [String(l.port), l.address, l.processName, l.projectName, l.pid != null ? String(l.pid) : null]
      .some((field) => field != null && field.toLowerCase().includes(q));
  });
}

export type ConnectionFilter = "all" | "remote" | "local";
/** "remote" = só o que sai da máquina; "local" = rede local e loopback. */
export function filterConnections(items: NetConnection[], filter: ConnectionFilter, query = ""): NetConnection[] {
  const q = query.trim().toLowerCase();
  return items.filter((c) => {
    if (filter === "remote" && c.scope !== "remote") return false;
    if (filter === "local" && c.scope === "remote") return false;
    if (!q) return true;
    return [c.remoteAddress, String(c.remotePort), c.processName, c.projectName]
      .some((field) => field != null && field.toLowerCase().includes(q));
  });
}

/** "49 portas · 23 em todas as interfaces" (descritivo; nunca "exposto"). */
export function exposureSummary(s: NetworkSecuritySnapshot): string {
  if (s.exposure.status === "unknown" && s.exposure.sources.some((n) => n.state !== "available")) return UNKNOWN_TEXT;
  const c = s.exposure.counts;
  return `${c.total} portas · ${c.allInterfaces} em todas as interfaces`;
}

export interface NetSummaryItem { id: string; label: string; value: string; status: WinHealth | null }
/** Resumo compacto: Rede, Firewall, Antivírus, BitLocker, Secure Boot, TPM e portas em escuta. */
export function summaryItems(s: NetworkSecuritySnapshot): NetSummaryItem[] {
  const net = s.network;
  const network = net.activeInterface
    ? `${net.activeInterface}${net.localIpv4 ? ` · ${net.localIpv4}` : ""}`
    : "Sem conexão ativa";
  const fw = s.firewall;
  const firewall = fw.status === "unknown"
    ? UNKNOWN_TEXT
    : fw.activeProfile
      ? `Perfil ${PROFILE_LABEL[fw.activeProfile]}`
      : `${fw.profiles.filter((p) => p.enabled).length} de ${fw.profiles.length} perfis ativos`;
  const av = s.antivirus;
  const antivirus = AV_PROVIDER_LABEL[av.provider];
  const systemVolumes = s.encryption.volumes.filter((v) => v.system);
  const encryption = systemVolumes.length === 0
    ? UNKNOWN_TEXT
    : systemVolumes.map((v) => `${v.mount} ${BITLOCKER_LABEL[v.state]}`).join(" · ");
  const tpm = s.tpm.present == null
    ? UNKNOWN_TEXT
    : s.tpm.present ? `Presente${s.tpm.version ? ` ${s.tpm.version}` : ""}` : "Não detectado";
  return [
    { id: "network", label: "Rede", value: network, status: null },
    { id: "firewall", label: "Firewall", value: firewall, status: s.firewall.status },
    { id: "antivirus", label: "Antivírus", value: antivirus, status: s.antivirus.status },
    { id: "encryption", label: "BitLocker", value: encryption, status: s.encryption.status },
    { id: "secure_boot", label: "Secure Boot", value: SECURE_BOOT_LABEL[s.secureBoot.state], status: s.secureBoot.status },
    { id: "tpm", label: "TPM", value: tpm, status: s.tpm.status },
    { id: "exposure", label: "Portas em escuta", value: exposureSummary(s), status: null },
  ];
}

/** Fontes que NÃO estão disponíveis, para dizer isso sem chamar de falha. */
export const unavailableNetSources = (s: NetworkSecuritySnapshot): WinSourceNote[] =>
  s.capabilities.filter((c) => c.state !== "available");
