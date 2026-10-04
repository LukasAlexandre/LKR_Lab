import type { WinHealth, WinSection, WinServiceView, WindowsHealthSnapshot } from "./types";

/* Dados de teste do contrato de Windows Health. Domínio sem dado é `unknown`/`null`, como no backend. */

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

export const service = (patch: Partial<WinServiceView> = {}): WinServiceView => ({
  id: "Schedule", label: "Agendador de Tarefas", state: "running", start: "automatic",
  expectation: "running", health: "healthy", reason: null, ...patch,
});

/** Uma máquina saudável (o estado real observado nesta máquina). */
export const healthySnapshot = (): WindowsHealthSnapshot => ({
  capturedAt: NOW,
  overall: { status: "healthy", reasons: [], evaluated: 6, rateable: 6 },
  system: section(
    { productName: "Windows 11 Pro", edition: "Professional", version: "25H2", build: "26200.9457", architecture: "x64", installedAt: 1_777_452_325_000, bootTime: 1_790_988_000, uptimeSecs: 133_273 },
    { status: "unknown", rated: false },
  ),
  restart: section({ pending: false, fileRenameOperations: true }, { ttlMs: 45_000 }),
  updates: section(
    { service: service({ id: "wuauserv", label: "Windows Update", state: "running", start: "manual", expectation: "on_demand" }), lastInstallSuccessAt: null, lastScanSuccessAt: null, failures7d: 0, pendingCount: null },
    { ttlMs: 600_000 },
  ),
  services: section({ items: [service(), service({ id: "wuauserv", label: "Windows Update", start: "manual", expectation: "on_demand" })] }, { ttlMs: 45_000 }),
  devices: section({ issues: [], disabled: 0, total: 192 }, { ttlMs: 600_000 }),
  events: section(
    {
      last24h: { critical: 0, error: 5, warning: 22 }, last7d: { critical: 0, error: 25, warning: null }, warningsCapped: false,
      signals: [{ kind: "application_crash", label: "Falha de aplicativo", count24h: 1, count7d: 1, lastAt: NOW - 3_600_000 }],
      recent: [], truncated: false,
    },
    { ttlMs: 180_000 },
  ),
  volumes: section({ items: [{ mount: "C:", filesystem: "NTFS", readOnly: false, dirty: false, status: "healthy", reasons: [] }] }, { ttlMs: 60_000 }),
  reliability: section({ appCrashes7d: 1, appHangs7d: 0 }, { status: "unknown", rated: false, ttlMs: 180_000 }),
  integrity: section(
    {
      signals: [{ id: "servicing_pending", label: "Reinício pendente de servicing/atualização", present: false }],
      onDemand: [
        { id: "sfc_verify", label: "Verificação de arquivos do sistema (SFC /verifynow)", available: false, requiresElevation: true, note: "Diagnóstico sob demanda: não é executado automaticamente e fica para o Block 09." },
      ],
    },
    { status: "unknown", rated: false, ttlMs: 0, reasons: ["Nenhum sinal passivo de problema. A integridade completa só é comprovada por verificação sob demanda (SFC/DISM), que não foi executada."] },
  ),
  capabilities: [],
});

export const withStatus = <K extends "restart" | "updates" | "services" | "devices" | "events" | "volumes">(
  snapshot: WindowsHealthSnapshot,
  key: K,
  status: WinHealth,
  reasons: string[],
  data: Partial<WindowsHealthSnapshot[K]> = {},
): WindowsHealthSnapshot => ({
  ...snapshot,
  [key]: { ...snapshot[key], ...data, status, reasons },
});
