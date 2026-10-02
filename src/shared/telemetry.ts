import type {
  Availability,
  HealthStatus,
  ProcessEntry,
  ProcessMetric,
  SensorLevel,
  Telemetry,
} from "./types";

/** Texto padrão para métrica sem fonte confiável nesta máquina. */
export const UNAVAILABLE = "Não disponível";

export const available = (value: Availability | undefined) => value === "available";

export const HEALTH_LABEL: Record<HealthStatus, string> = {
  healthy: "Saudável",
  attention: "Atenção",
  critical: "Crítico",
};
export const HEALTH_DETAIL: Record<HealthStatus, string> = {
  healthy: "Nenhuma condição de atenção nos sinais observáveis.",
  attention: "Há sinais que pedem atenção.",
  critical: "Há sinais críticos agora.",
};
/** `unrated`: leitura real de sensor sem limite conhecido (ex.: ACPI) — sem classificação. */
export const LEVEL_LABEL: Record<SensorLevel, string> = {
  normal: "Normal",
  attention: "Atenção",
  critical: "Crítico",
  unrated: "Sem limite",
  unavailable: UNAVAILABLE,
};

/** Memória de uma GPU com a semântica correta: dedicada e compartilhada nunca somadas. */
export function gpuMemory(gpu: Telemetry["gpus"][number]): { dedicated: string | null; shared: string | null } {
  const of = (used: number | null, total: number | null) =>
    used == null ? null : total ? `${formatSize(used)} / ${formatSize(total)}` : formatSize(used);
  return {
    dedicated: gpu.capabilities.dedicatedMemory === "available" ? of(gpu.dedicatedUsed, gpu.dedicatedTotal) : null,
    shared: gpu.capabilities.sharedMemory === "available" ? of(gpu.sharedUsed, gpu.sharedTotal) : null,
  };
}

/** Percentual de processo: valor pequeno porém real aparece como "<0,1%", nunca "0%". */
export function share(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "—";
  if (value > 0 && value < 0.1) return "<0,1%";
  return `${value.toLocaleString("pt-BR", { maximumFractionDigits: 1 })}%`;
}

/** Instância PDH "0 C:" → "Disco 0 (C:)". */
export function diskLabel(instance: string | null | undefined): string | null {
  if (!instance) return null;
  const [index, ...volumes] = instance.trim().split(/\s+/);
  return volumes.length ? `Disco ${index} (${volumes.join(" ")})` : `Disco ${index}`;
}

export const pct = (value: number | null | undefined) =>
  value == null || !Number.isFinite(value) ? null : `${Math.round(value)}%`;

const UNITS = ["B", "KB", "MB", "GB", "TB"];
/** Bytes em unidades binárias, 1 casa abaixo de 10 (ex.: 1,2 GB, 842 MB). */
export function formatSize(bytes: number | null | undefined): string | null {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return null;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value < 10 && unit > 0 ? 1 : 0;
  return `${value.toLocaleString("pt-BR", { maximumFractionDigits: digits })} ${UNITS[unit]}`;
}

/** E/S de disco em bytes/s. */
export const formatRate = (bytesPerSec: number | null | undefined) => {
  const size = formatSize(bytesPerSec);
  return size === null ? null : `${size}/s`;
};

/** Rede em bits/s (Kbps, Mbps, Gbps), como provedores e o Gerenciador de Tarefas. */
export function formatBits(bitsPerSec: number | null | undefined): string | null {
  if (bitsPerSec == null || !Number.isFinite(bitsPerSec) || bitsPerSec < 0) return null;
  const steps: [number, string][] = [[1e9, "Gbps"], [1e6, "Mbps"], [1e3, "Kbps"]];
  for (const [size, unit] of steps)
    if (bitsPerSec >= size) {
      const value = bitsPerSec / size;
      return `${value.toLocaleString("pt-BR", { maximumFractionDigits: value < 10 ? 1 : 0 })} ${unit}`;
    }
  return `${Math.round(bitsPerSec)} bps`;
}

export function formatUptime(seconds: number | null | undefined): string | null {
  if (!seconds || seconds <= 0) return null;
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days) return `${days}d ${hours}h`;
  if (hours) return `${hours}h ${minutes}min`;
  return `${minutes}min`;
}

export const formatCelsius = (celsius: number | null | undefined) =>
  celsius == null ? null : `${Math.round(celsius)} °C`;

export const formatClock = (mhz: number | null | undefined) =>
  mhz == null || mhz <= 0 ? null : `${(mhz / 1000).toLocaleString("pt-BR", { maximumFractionDigits: 2 })} GHz`;

/** Tabs de "Processos em destaque"; Memória é a padrão, como no Concept 02. */
export const PROCESS_TABS: { id: ProcessMetric; label: string }[] = [
  { id: "cpu", label: "CPU" },
  { id: "memory", label: "Memória" },
  { id: "gpu", label: "GPU" },
  { id: "disk", label: "Disco" },
];
export const DEFAULT_PROCESS_TAB: ProcessMetric = "memory";

/** Linhas da tab escolhida. A ordem já vem do backend pela métrica da tab. */
export function processRows(telemetry: Telemetry | null, tab: ProcessMetric): {
  rows: ProcessEntry[];
  empty: string | null;
} {
  if (!telemetry?.processes) return { rows: [], empty: "Ranking disponível com o Dashboard aberto." };
  if (tab === "gpu" && !available(telemetry.capabilities.gpuProcessUsage))
    return { rows: [], empty: "Telemetria de GPU por processo indisponível nesta máquina." };
  if (!telemetry.processes[tab].length) return { rows: [], empty: "Nenhum processo para mostrar." };
  // Em GPU e Disco, processo parado não é "destaque": só entra quem está consumindo.
  const rows = tab === "gpu" || tab === "disk"
    ? telemetry.processes[tab].filter((row) => metricValue(row, tab) > 0)
    : telemetry.processes[tab];
  if (!rows.length)
    return { rows, empty: tab === "gpu" ? "Nenhum processo usando a GPU agora." : "Nenhuma E/S de disco relevante agora." };
  return { rows, empty: null };
}

/** Memória 0: o Windows não deixa ler processos protegidos (ex.: dwm.exe). Não é "0 B". */
export const processMemory = (bytes: number) => (bytes > 0 ? formatSize(bytes) : null);

/** Valor da coluna principal da tab, para a barra proporcional. */
export function metricValue(row: ProcessEntry, tab: ProcessMetric): number {
  if (tab === "cpu") return row.cpu;
  if (tab === "memory") return row.memory;
  if (tab === "gpu") return row.gpu ?? 0;
  return row.diskRead + row.diskWrite;
}

/** Caminho SVG de uma sparkline (0..max) em `width`×`height`. Menos de 2 pontos: vazio. */
export function sparkline(values: (number | null)[], width: number, height: number, max = 100): string {
  const points = values.filter((v): v is number => v != null && Number.isFinite(v));
  if (points.length < 2) return "";
  const top = Math.max(max, ...points, 1e-9);
  const step = width / (points.length - 1);
  return points
    .map((v, i) => `${i ? "L" : "M"}${(i * step).toFixed(1)} ${(height - (Math.max(v, 0) / top) * height).toFixed(1)}`)
    .join(" ");
}

/** Ordem de gravidade para ordenar/realçar. */
export const SEVERITY_ORDER: Record<HealthStatus, number> = { healthy: 0, attention: 1, critical: 2 };

/** Volume: % usado e rótulo de capacidade. */
export function volumeUsage(total: number, available: number) {
  const used = Math.max(total - available, 0);
  return { used, percent: total > 0 ? (used / total) * 100 : 0 };
}
