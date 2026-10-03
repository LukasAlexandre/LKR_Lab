import { describe, expect, it } from "vitest";
import {
  DEFAULT_PROCESS_TAB,
  HEALTH_LABEL,
  LEVEL_LABEL,
  PROCESS_TABS,
  SEVERITY_ORDER,
  UNAVAILABLE,
  available,
  formatBits,
  formatCelsius,
  formatClock,
  formatRate,
  formatSize,
  formatUptime,
  gpuMemory,
  metricValue,
  pct,
  processMemory,
  processRows,
  share,
  diskLabel,
  sparkline,
  volumeUsage,
} from "./telemetry";
import { appendPoint } from "../state/telemetry";
import type { ProcessEntry, Telemetry, TelemetryCapabilities } from "./types";

const caps = (patch: Partial<TelemetryCapabilities> = {}): TelemetryCapabilities => ({
  cpuUsage: "available", cpuClock: "available", memoryUsage: "available", gpuUsage: "available",
  gpuMemory: "available", gpuProcessUsage: "available", cpuPackageTemperature: "unavailable",
  gpuTemperature: "unavailable", storageTemperature: "unavailable", thermalZoneTemperature: "unavailable",
  motherboardTemperature: "unavailable", diskIo: "available", diskActivity: "available",
  networkRate: "available", processDiskIo: "available", storagePhysicalHealth: "unavailable", ...patch,
});
const row = (pid: number, patch: Partial<ProcessEntry> = {}): ProcessEntry => ({
  pid, name: `p${pid}.exe`, cpu: 0, memory: 0, gpu: 0, diskRead: 0, diskWrite: 0, ...patch,
});
const telemetry = (patch: Partial<Telemetry> = {}): Telemetry => ({
  timestamp: 1000, active: true,
  cpu: { usage: 10, clockMhz: null },
  memory: { total: 100, used: 40, available: 60, percent: 40, swapTotal: 0, swapUsed: 0 },
  gpus: [], diskIo: { readPerSec: 0, writePerSec: 0, activity: null, busiestDisk: null }, volumes: [],
  network: { interface: null, ipv4: null, downloadBps: 0, uploadBps: 0 },
  temperatures: [], processes: { cpu: [], memory: [], gpu: [], disk: [], total: 0 },
  uptime: 0, bootTime: 0, capabilities: caps(),
  health: { status: "healthy", alerts: [], checks: [] }, ...patch,
});

describe("telemetry formatting", () => {
  it("formats sizes, rates and network bits", () => {
    expect(formatSize(1.2 * 1024 ** 3)).toBe("1,2 GB");
    expect(formatSize(842 * 1024 ** 2)).toBe("842 MB");
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(null)).toBeNull();
    expect(formatSize(-1)).toBeNull();
    expect(formatRate(2 * 1024 ** 2)).toBe("2 MB/s");
    expect(formatRate(0)).toBe("0 B/s");
    expect(formatBits(12_400_000)).toBe("12 Mbps");
    expect(formatBits(3_100_000)).toBe("3,1 Mbps");
    expect(formatBits(7200)).toBe("7,2 Kbps");
    expect(formatBits(500)).toBe("500 bps");
    expect(formatBits(Number.NaN)).toBeNull();
  });
  it("formats percentages, temperatures, clock and uptime; missing data stays missing", () => {
    expect(pct(18.4)).toBe("18%");
    expect(pct(null)).toBeNull();
    expect(pct(Number.POSITIVE_INFINITY)).toBeNull();
    expect(formatCelsius(52.4)).toBe("52 °C");
    expect(formatCelsius(null)).toBeNull();
    expect(formatClock(3301.9)).toBe("3,3 GHz");
    expect(formatClock(null)).toBeNull();
    expect(formatUptime(3 * 3600 + 47 * 60)).toBe("3h 47min");
    expect(formatUptime(2 * 86400 + 5 * 3600)).toBe("2d 5h");
    expect(formatUptime(0)).toBeNull();
    expect(UNAVAILABLE).toBe("Não disponível");
    expect(LEVEL_LABEL.unavailable).toBe(UNAVAILABLE);
  });
  it("maps health and severity labels", () => {
    expect(HEALTH_LABEL).toEqual({ healthy: "Saudável", attention: "Atenção", critical: "Crítico" });
    expect(SEVERITY_ORDER.critical).toBeGreaterThan(SEVERITY_ORDER.attention);
    expect(available("available")).toBe(true);
    expect(available("unavailable")).toBe(false);
    expect(available(undefined)).toBe(false);
  });
  it("computes volume usage", () => {
    expect(volumeUsage(1000, 250)).toEqual({ used: 750, percent: 75 });
    expect(volumeUsage(0, 0).percent).toBe(0);
  });
  it("draws sparklines only with data", () => {
    expect(sparkline([], 120, 28)).toBe("");
    expect(sparkline([null, 5], 120, 28)).toBe("");
    expect(sparkline([0, 100], 100, 20)).toBe("M0.0 20.0 L100.0 0.0");
    // max=0: escala pelo maior valor (rede).
    expect(sparkline([0, 50], 100, 10, 0)).toBe("M0.0 10.0 L100.0 0.0");
  });
});

describe("process tabs", () => {
  it("defaults to memory and has the four concept tabs", () => {
    expect(DEFAULT_PROCESS_TAB).toBe("memory");
    expect(PROCESS_TABS.map((t) => t.label)).toEqual(["CPU", "Memória", "GPU", "Disco"]);
  });
  it("shows the backend ranking for the chosen tab", () => {
    const t = telemetry({
      processes: { cpu: [row(1, { cpu: 50 })], memory: [row(2, { memory: 9 }), row(3, { memory: 4 })], gpu: [], disk: [], total: 3 },
    });
    expect(processRows(t, "memory").rows.map((r) => r.pid)).toEqual([2, 3]);
    expect(processRows(t, "cpu").rows[0].pid).toBe(1);
    expect(metricValue(row(1, { diskRead: 3, diskWrite: 4 }), "disk")).toBe(7);
    expect(metricValue(row(1, { gpu: null }), "gpu")).toBe(0);
  });
  it("never invents a GPU ranking", () => {
    const t = telemetry({ capabilities: caps({ gpuProcessUsage: "unavailable" }), processes: { cpu: [], memory: [], gpu: [row(1, { gpu: 30 })], disk: [], total: 1 } });
    expect(processRows(t, "gpu")).toEqual({ rows: [], empty: "Telemetria de GPU por processo indisponível nesta máquina." });
    const idle = telemetry({ processes: { cpu: [], memory: [], gpu: [row(1, { gpu: 0 })], disk: [], total: 1 } });
    expect(processRows(idle, "gpu").empty).toBe("Nenhum processo usando a GPU agora.");
    const mixed = telemetry({ processes: { cpu: [], memory: [], gpu: [row(1, { gpu: 5 }), row(4, { gpu: 0 })], disk: [], total: 2 } });
    expect(processRows(mixed, "gpu").rows.map((r) => r.pid)).toEqual([1]);
  });
  it("does not show protected processes as using 0 B of memory", () => {
    expect(processMemory(0)).toBeNull();
    expect(processMemory(512 * 1024 ** 2)).toBe("512 MB");
  });
  it("handles empty lists, no disk activity and the idle mode", () => {
    expect(processRows(telemetry(), "memory").empty).toBe("Nenhum processo para mostrar.");
    const quiet = telemetry({ processes: { cpu: [], memory: [], gpu: [], disk: [row(1)], total: 1 } });
    expect(processRows(quiet, "disk").empty).toBe("Nenhuma E/S de disco relevante agora.");
    expect(processRows(telemetry({ processes: null }), "cpu").rows).toEqual([]);
    expect(processRows(null, "cpu").empty).not.toBeNull();
  });
});

describe("telemetry history", () => {
  it("keeps a bounded window and ignores repeated samples; no GPU stays null", () => {
    let history = appendPoint([], telemetry({ timestamp: 1 }));
    expect(history[0].gpu).toBeNull();
    history = appendPoint(history, telemetry({ timestamp: 1 }));
    expect(history).toHaveLength(1);
    for (let i = 2; i < 200; i++) history = appendPoint(history, telemetry({ timestamp: i }));
    expect(history).toHaveLength(120);
    expect(history[119].at).toBe(199);
    const gpu = appendPoint([], telemetry({ gpus: [gpuOf({ name: "a", usage: 3 }), gpuOf({ name: "b", usage: 40 })] }));
    expect(gpu[0].gpu).toBe(40);
  });
});

const GB = 1024 ** 3;
const gpuOf = (patch: Partial<Telemetry["gpus"][number]> = {}): Telemetry["gpus"][number] => ({
  id: "pci:0:2.0", name: "gpu", usage: null, dedicatedUsed: null, dedicatedTotal: null, sharedUsed: null, sharedTotal: null,
  temperature: null,
  capabilities: { usage: "available", dedicatedMemory: "available", sharedMemory: "available", temperature: "unavailable" },
  ...patch,
});

describe("GPU semantics", () => {
  it("keeps dedicated and shared memory apart (integrated GPU)", () => {
    const iris = gpuOf({ dedicatedUsed: 0, dedicatedTotal: 128 * 1024 ** 2, sharedUsed: 1.2 * GB, sharedTotal: 11.9 * GB });
    expect(gpuMemory(iris)).toEqual({ dedicated: "0 B / 128 MB", shared: "1,2 GB / 12 GB" });
  });
  it("shows the discrete GPU dedicated memory and never invents what is unavailable", () => {
    const gtx = gpuOf({ dedicatedUsed: 144 * 1024 ** 2, dedicatedTotal: 3.8 * GB, sharedUsed: 0, sharedTotal: null });
    expect(gpuMemory(gtx)).toEqual({ dedicated: "144 MB / 3,8 GB", shared: "0 B" });
    const blind = gpuOf({ dedicatedUsed: 5, capabilities: { usage: "unavailable", dedicatedMemory: "unavailable", sharedMemory: "unavailable", temperature: "unavailable" } });
    expect(gpuMemory(blind)).toEqual({ dedicated: null, shared: null });
  });
  it("labels unrated sensors without a status", () => {
    expect(LEVEL_LABEL.unrated).toBe("Sem limite");
  });
});

describe("process percentages and disk labels", () => {
  it("never shows a real but tiny value as 0%", () => {
    expect(share(0.02)).toBe("<0,1%");
    expect(share(0)).toBe("0%");
    expect(share(12.34)).toBe("12,3%");
    expect(share(null)).toBe("—");
  });
  it("names the busiest physical disk", () => {
    expect(diskLabel("0 C:")).toBe("Disco 0 (C:)");
    expect(diskLabel("1 D: E:")).toBe("Disco 1 (D: E:)");
    expect(diskLabel("2")).toBe("Disco 2");
    expect(diskLabel(null)).toBeNull();
  });
});
