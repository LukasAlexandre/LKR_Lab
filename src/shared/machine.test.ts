import { describe, expect, it } from "vitest";
import {
  EDIT_IDLE,
  MACHINE_NAME_MAX,
  cpuDetail,
  detectionAge,
  draftChanged,
  draftError,
  draftInput,
  formatBytes,
  formatMemory,
  keepMetadata,
  machineEdit,
  osDetail,
  suggestedName,
  usageLabel,
  validateMachineName,
} from "./machine";
import type { Machine, MachineSnapshot, MachineStatus } from "./types";

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

const machine: Machine = {
  machineId: "id-fixo", name: "PC Casa", usage: "home", description: "",
  createdAt: "2026-01-01T00:00:00.000Z", updatedAt: "2026-01-01T00:00:00.000Z", lastDetectedAt: 1,
};
const status = (patch: Partial<Machine>, detected = 1): MachineStatus => ({
  registered: true, stale: false, ttlMs: 21_600_000,
  machine: { ...machine, ...patch, lastDetectedAt: detected },
  snapshot: snapshot({ detectedAt: detected }),
});

describe("machine metadata editing", () => {
  it("edits only name, usage and description, trimmed", () => {
    let state = machineEdit(EDIT_IDLE, { type: "edit", machine });
    expect(state.mode).toBe("editing");
    state = machineEdit(state, { type: "change", patch: { name: "  PC Casa Teste  ", usage: "work", description: " x " } });
    expect(draftInput(state.draft!)).toEqual({ name: "PC Casa Teste", usage: "work", description: "x" });
    expect(Object.keys(draftInput(state.draft!)).sort()).toEqual(["description", "name", "usage"]);
    expect(draftChanged(machine, state.draft!)).toBe(true);
    expect(draftChanged(machine, { name: " PC Casa ", usage: "home", description: "" })).toBe(false);
  });
  it("validates like the registration", () => {
    expect(draftError({ name: "PC", usage: "home", description: "" })).toBeNull();
    expect(draftError({ name: "  ", usage: "home", description: "" })).not.toBeNull();
    expect(draftError({ name: "PC", usage: "garagem" as never, description: "" })).not.toBeNull();
    expect(draftError({ name: "PC", usage: "home", description: "d".repeat(121) })).not.toBeNull();
    expect(draftError({ name: "PC", usage: "home", description: "d".repeat(120) })).toBeNull();
  });
  it("cancel discards the draft and nothing is left to save", () => {
    let state = machineEdit(EDIT_IDLE, { type: "edit", machine });
    state = machineEdit(state, { type: "change", patch: { name: "Outro" } });
    state = machineEdit(state, { type: "cancel" });
    expect(state).toEqual(EDIT_IDLE);
    // "save" sem rascunho não vira salvamento.
    expect(machineEdit(state, { type: "save" })).toEqual(EDIT_IDLE);
  });
  it("goes through saving, success and error states", () => {
    let state = machineEdit(EDIT_IDLE, { type: "edit", machine });
    state = machineEdit(state, { type: "save" });
    expect(state.mode).toBe("saving");
    expect(machineEdit(state, { type: "cancel" }).mode).toBe("saving");
    const failed = machineEdit(state, { type: "failed", error: "Informe um nome" });
    expect(failed).toMatchObject({ mode: "error", error: "Informe um nome" });
    expect(failed.draft).toEqual(state.draft);
    const saved = machineEdit(state, { type: "saved" });
    expect(saved.mode).toBe("success");
    expect(machineEdit(saved, { type: "settle" })).toEqual(EDIT_IDLE);
  });
  it("a refresh that started before a save keeps the saved name (topbar) and takes the new detection", () => {
    const afterSave = status({ name: "PC Casa Teste", updatedAt: "2026-02-01T00:00:00.000Z" });
    const lateRefresh = status({ name: "PC Casa" }, 99);
    const merged = keepMetadata(afterSave, lateRefresh);
    expect(merged.machine?.name).toBe("PC Casa Teste");
    expect(merged.machine?.updatedAt).toBe("2026-02-01T00:00:00.000Z");
    expect(merged.machine?.lastDetectedAt).toBe(99);
    expect(merged.machine?.machineId).toBe("id-fixo");
    expect(merged.snapshot?.detectedAt).toBe(99);
  });
});
