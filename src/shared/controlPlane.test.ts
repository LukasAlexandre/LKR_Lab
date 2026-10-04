import { describe, expect, it } from "vitest";
import { associationRows, cpuLabel, distinctPorts, groupRuntimes, uptimeLabel } from "./controlPlane";
import type { Association, ControlPlaneSnapshot, RuntimeObservation } from "./types";

const none: Association = {
  confidence: "unknown", projectId: null, projectName: null, worktreeId: null, worktreeName: null,
  sessionId: null, sessionLabel: null, blockId: null, blockTitle: null, evidence: [],
};
const owned = (over: Partial<Association> = {}): Association => ({
  ...none, confidence: "high", projectId: "lab", projectName: "LKR LAB", ...over,
});
const runtime = (over: Partial<RuntimeObservation> = {}): RuntimeObservation => ({
  id: "r", origin: "discovered", category: "dev", label: "Vite", technology: "Vite", state: "running", rootPid: 10, pids: [10],
  ports: [{ port: 1420, address: "127.0.0.1", protocol: "TCP", ipVersion: "v4", pid: 10 }], tree: [], command: "vite", cwd: null,
  startedAt: 1_000, cpu: 1, memory: 1024, association: none, console: { available: false, runId: null, reason: "Console não disponível — processo iniciado fora do LKR LAB." },
  execution: null, isSelf: false, ...over,
});
const snapshot = (runtimes: RuntimeObservation[]): ControlPlaneSnapshot => ({ takenAt: 1, scope: "local", processTotal: 0, listeningTotal: 0, runtimes, signals: [], limitations: [] });

describe("agrupamento de runtimes do Control Plane", () => {
  const managed = runtime({ id: "m", origin: "managed", association: owned({ confidence: "exact" }), console: { available: true, runId: "m", reason: null } });
  const detected = runtime({ id: "d", association: owned() });
  const unknown = runtime({ id: "u", association: none });
  const otherProject = runtime({ id: "o", association: owned({ projectId: "other", projectName: "Outro" }) });
  const system = runtime({ id: "s", category: "system" });

  it("separa Managed, Detected deste Project e o resto da máquina", () => {
    const groups = groupRuntimes(snapshot([managed, detected, unknown, otherProject, system]), "lab");
    expect(groups.managed.map((r) => r.id)).toEqual(["m"]);
    expect(groups.detected.map((r) => r.id)).toEqual(["d"]);
    expect(groups.elsewhere.map((r) => r.id)).toEqual(["u", "o"]);
    expect(groups.system).toBe(1);
  });
  it("runtime sem atribuição nunca vira do Project aberto", () => {
    const groups = groupRuntimes(snapshot([unknown]), "lab");
    expect(groups.managed).toEqual([]);
    expect(groups.detected).toEqual([]);
    expect(groups.elsewhere).toHaveLength(1);
  });
  it("observadores (logs do Compose) não são runtimes gerenciados", () => {
    const observer = runtime({ id: "ob", origin: "managed", association: owned({ confidence: "exact" }), execution: { runId: "ob", commandId: "compose:logs", command: "docker compose logs", projectId: "lab", exitCode: null, observer: true } });
    expect(groupRuntimes(snapshot([observer]), "lab").managed).toEqual([]);
  });
  it("sem snapshot, tudo vazio", () => {
    expect(groupRuntimes(null, "lab")).toEqual({ managed: [], detected: [], elsewhere: [], system: 0 });
  });
});

describe("atribuição na tela", () => {
  it("Unknown mostra — em Project, Worktree, Session e Block", () => {
    expect(associationRows(none).map((r) => [r.label, r.value, r.known])).toEqual([
      ["Project", "—", false], ["Worktree", "—", false], ["Session", "—", false], ["Block", "—", false],
    ]);
  });
  it("Project/Worktree/Session/Block aparecem quando há relação real", () => {
    const rows = associationRows(owned({ worktreeName: "feature-x", sessionLabel: "SESSION-002", blockTitle: "04 — Managed Runtime Supervisor" }));
    expect(rows.map((r) => r.value)).toEqual(["LKR LAB", "feature-x", "SESSION-002", "04 — Managed Runtime Supervisor"]);
    expect(rows.every((r) => r.known)).toBe(true);
  });
  it("Project sem Worktree não inventa Worktree nem Session", () => {
    const rows = associationRows(owned());
    expect(rows.map((r) => r.value)).toEqual(["LKR LAB", "—", "—", "—"]);
  });
});

describe("rótulos", () => {
  it("uptime", () => {
    expect(uptimeLabel(null, 5000)).toBeNull();
    expect(uptimeLabel(10_000, 5_000)).toBeNull();
    expect(uptimeLabel(1_000, 31_000)).toBe("30 s");
    expect(uptimeLabel(0 + 1, 3 * 60_000 + 1)).toBe("3 min");
    expect(uptimeLabel(1, 2 * 3_600_000 + 5 * 60_000 + 1)).toBe("2 h 05 min");
    expect(uptimeLabel(1, 28 * 3_600_000 + 1)).toBe("1 d 4 h");
  });
  it("cpu e portas distintas (v4 + v6 viram uma)", () => {
    expect(cpuLabel(0.34)).toBe("0.3%");
    expect(cpuLabel(42.4)).toBe("42%");
    const dual = runtime({ ports: [
      { port: 1420, address: "127.0.0.1", protocol: "TCP", ipVersion: "v4", pid: 1 },
      { port: 1420, address: "::1", protocol: "TCP", ipVersion: "v6", pid: 1 },
      { port: 80, address: "0.0.0.0", protocol: "TCP", ipVersion: "v4", pid: 1 },
    ] });
    expect(distinctPorts(dual)).toEqual([80, 1420]);
  });
});
