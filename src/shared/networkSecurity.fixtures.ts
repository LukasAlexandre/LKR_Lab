import type {
  NetConnection,
  NetListener,
  NetworkSecuritySnapshot,
  WinHealth,
  WinSection,
  WinSourceNote,
} from "./types";

/* Dados de teste do contrato de Network & Security. Domínio sem dado é `unknown`/`null`, como no backend. */

export const NOW = 1_791_121_512_000;

const section = <T extends object>(data: T, patch: Partial<WinSection<object>> = {}): WinSection<T> => ({
  status: "healthy",
  rated: true,
  reasons: [],
  checkedAt: NOW,
  ttlMs: 60_000,
  sources: [],
  ...patch,
  ...data,
});

export const listener = (patch: Partial<NetListener> = {}): NetListener => ({
  port: 4317,
  address: "127.0.0.1",
  ipVersion: "v4",
  scope: "loopback",
  pid: 100,
  processName: "node.exe",
  executable: "C:\\Dev\\Lab\\node.exe",
  projectId: "p1",
  projectName: "LKR_Lab",
  confidence: "high",
  system: false,
  note: "Somente esta máquina (loopback).",
  ...patch,
});

export const connection = (patch: Partial<NetConnection> = {}): NetConnection => ({
  localAddress: "192.168.1.42",
  localPort: 50_000,
  remoteAddress: "140.82.112.3",
  remotePort: 443,
  scope: "remote",
  pid: 200,
  processName: "git-remote-https.exe",
  projectName: null,
  ...patch,
});

export const note = (patch: Partial<WinSourceNote> = {}): WinSourceNote => ({
  id: "bitlocker", label: "BitLocker por volume", state: "requires_elevation",
  reason: "O estado do BitLocker por volume requer privilégio administrativo", ...patch,
});

/** Uma máquina saudável, como a observada no desktop (BitLocker e categoria da rede exigem administrador). */
export const healthySnapshot = (): NetworkSecuritySnapshot => ({
  capturedAt: NOW,
  overall: { status: "healthy", reasons: [], evaluated: 4, rateable: 5 },
  network: section(
    {
      interfaces: [
        {
          name: "Wi-Fi", description: "Wi-Fi adapter", kind: "wifi", up: true, active: true,
          ipv4: ["192.168.1.42"], ipv6: ["fe80::1"], prefix: 24, gateways: ["192.168.1.1"],
          dns: ["1.1.1.1", "8.8.8.8"], dhcp: true, mac: "AA:BB:CC:DD:EE:FF", linkSpeedBps: 574_000_000, profile: null,
        },
        {
          name: "Ethernet", description: "Ethernet adapter", kind: "ethernet", up: false, active: false,
          ipv4: [], ipv6: [], prefix: null, gateways: [], dns: [], dhcp: false, mac: null, linkSpeedBps: null, profile: null,
        },
      ],
      activeInterface: "Wi-Fi", localIpv4: "192.168.1.42", gateway: "192.168.1.1", dns: ["1.1.1.1", "8.8.8.8"], profile: null,
      publicIp: { queried: false, note: "Não consultado: o LKR LAB não faz requisições externas para descobrir o IP público." },
    },
    { status: "unknown", rated: false, ttlMs: 15_000 },
  ),
  exposure: section(
    {
      listeners: [
        listener({ port: 3000, address: "0.0.0.0", scope: "all_interfaces", processName: "vite.exe", pid: 11, projectName: null, note: "Escuta em todas as interfaces. Não significa exposição à internet: depende do firewall e do roteador, que o app não testa." }),
        listener({ port: 5432, address: "192.168.1.42", scope: "specific", processName: "postgres.exe", pid: 12, projectName: null, note: "Escuta em um endereço específico de uma interface." }),
        listener(),
      ],
      counts: { total: 3, loopback: 1, specific: 1, allInterfaces: 1, unidentified: 0 },
      notes: ["Somente portas TCP em escuta; UDP não é listado."],
    },
    { status: "unknown", rated: false, ttlMs: 15_000 },
  ),
  connections: section(
    { total: 2, loopback: 1, local: 0, remote: 1, items: [connection(), connection({ scope: "loopback", remoteAddress: "127.0.0.1", remotePort: 4317, processName: "node.exe", projectName: "LKR_Lab" })], truncated: false, notes: ["Sem resolução reversa de DNS: endereços aparecem como o sistema os informa."] },
    { status: "unknown", rated: false, ttlMs: 10_000 },
  ),
  firewall: section({
    profiles: [
      { kind: "domain", label: "Domínio", enabled: true, defaultInbound: null, defaultOutbound: null, active: false },
      { kind: "private", label: "Privado", enabled: true, defaultInbound: null, defaultOutbound: null, active: false },
      { kind: "public", label: "Público", enabled: true, defaultInbound: null, defaultOutbound: null, active: false },
    ],
    activeProfile: null, securityCenter: "good", notes: [],
  }),
  antivirus: section({
    provider: "defender", thirdPartyCount: 0, securityCenter: "good",
    defender: {
      state: "active", serviceRunning: true, realtimeProtection: null, engineVersion: "1.1.26080.3",
      signatureVersion: "1.459.546.0", signaturesUpdatedAt: NOW - 3_600_000, signatureAgeDays: 0, activeThreats: null,
    },
    notes: ["Ameaças ativas: não consultado (a leitura passiva não pergunta ao Defender)."],
  }),
  encryption: section({ volumes: [] }, {
    status: "unknown",
    reasons: ["BitLocker por volume: O estado do BitLocker por volume requer privilégio administrativo"],
    ttlMs: 300_000,
    sources: [note()],
  }),
  secureBoot: section({ state: "enabled", uefi: true }, { ttlMs: 600_000 }),
  tpm: section({ present: true, version: "2.0" }, { ttlMs: 600_000 }),
  capabilities: [note()],
});

export const withStatus = <K extends "firewall" | "antivirus" | "encryption" | "secureBoot" | "tpm">(
  snapshot: NetworkSecuritySnapshot,
  key: K,
  status: WinHealth,
  reasons: string[],
  data: Partial<NetworkSecuritySnapshot[K]> = {},
): NetworkSecuritySnapshot => ({
  ...snapshot,
  [key]: { ...snapshot[key], ...data, status, reasons },
});

export const withOverall = (
  snapshot: NetworkSecuritySnapshot,
  status: WinHealth,
  reasons: { domain: string; text: string }[],
  evaluated = 5,
): NetworkSecuritySnapshot => ({ ...snapshot, overall: { status, reasons, evaluated, rateable: 5 } });
