import { describe, expect, it } from "vitest";
import {
  MACHINE_NAME_MAX,
  cpuDetail,
  detectionAge,
  formatBytes,
  formatMemory,
  osDetail,
  suggestedName,
  usageLabel,
  validateMachineName,
} from "./machine";
import type { MachineSnapshot } from "./types";

const GB = 1024 ** 3;
const snapshot = (patch: Partial<MachineSnapshot> = {}): MachineSnapshot => ({
  hostname: null, osName: null, osVersion: null, osBuild: null, cpuModel: null,
  cpuCores: null, cpuThreads: null, memoryTotal: null, gpus: [], storage: [],
  networkInterfaces: [], activeInterface: null, localIpv4: null, uptime: null,
  detectedAt: 0, ...patch,
});

describe("machine registry display rules", () => {
  it("formats installed memory and VRAM", () => {
    expect(formatMemory(25_542_582_272)).toBe("24 GB");
    expect(formatMemory(32 * GB)).toBe("32 GB");
    expect(formatBytes(GB)).toBe("1 GB");
    expect(formatBytes(1.5 * GB)).toBe("1,5 GB");
    expect(formatBytes(512 * 1024 ** 2)).toBe("512 MB");
    expect(formatMemory(null)).toBeNull();
    expect(formatBytes(0)).toBeNull();
  });
  it("describes OS and CPU only with what was detected", () => {
    expect(osDetail(snapshot({ osVersion: "25H2", osBuild: "26200.9457" }))).toBe("Versão 25H2 (Build 26200.9457)");
    expect(osDetail(snapshot({ osBuild: "26200" }))).toBe("Build 26200");
    expect(osDetail(snapshot())).toBeNull();
    expect(cpuDetail(snapshot({ cpuCores: 8, cpuThreads: 16 }))).toBe("8 núcleos / 16 threads");
    expect(cpuDetail(snapshot({ cpuThreads: 4 }))).toBe("4 threads");
    expect(cpuDetail(snapshot())).toBeNull();
  });
  it("shows detection age relative to now", () => {
    const now = 1_790_000_000_000;
    expect(detectionAge(now, now)).toBe("Agora");
    expect(detectionAge(now - 5 * 60_000, now)).toBe("Há 5 min");
    expect(detectionAge(now - 6 * 3_600_000, now)).toBe("Há 6 h");
    expect(detectionAge(now - 49 * 3_600_000, now)).toBe("Há 2 dias");
  });
  it("suggests the hostname but never invents a friendly name", () => {
    expect(suggestedName(snapshot({ hostname: "DESKTOP-LKR" }))).toBe("DESKTOP-LKR");
    expect(suggestedName(snapshot())).toBe("");
    expect(suggestedName(null)).toBe("");
    expect(suggestedName(snapshot({ hostname: "x".repeat(80) }))).toHaveLength(MACHINE_NAME_MAX);
  });
  it("validates the friendly name and maps usage labels", () => {
    expect(validateMachineName("PC Casa")).toBeNull();
    expect(validateMachineName("   ")).not.toBeNull();
    expect(validateMachineName("x".repeat(MACHINE_NAME_MAX + 1))).not.toBeNull();
    expect(usageLabel("home")).toBe("Casa");
    expect(usageLabel("work")).toBe("Trabalho");
    expect(usageLabel("garagem")).toBe("Outro");
  });
});
