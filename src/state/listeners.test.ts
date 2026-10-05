import { beforeEach, describe, expect, it, vi } from "vitest";
import alerts from "./alerts.ts?raw";
import machine from "./machine.ts?raw";
import networkSecurity from "./networkSecurity.ts?raw";
import telemetry from "./telemetry.ts?raw";
import windowsHealth from "./windowsHealth.ts?raw";
import runtimeHub from "../features/project/RuntimeHub.tsx?raw";
import projectRuntime from "../features/ProjectRuntime.tsx?raw";

/* Hardening (Block 10): montar e desmontar telas repetidamente não pode duplicar listeners de
 * eventos do Tauri nem timers. O estado de execução e a telemetria têm UM listener por app. */
const listen = vi.hoisted(() => vi.fn(async (...args: [string, unknown]) => (args.length ? () => undefined : () => undefined)));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("../shared/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../shared/api")>()),
  desktop: true,
  api: vi.fn(async () => ({})),
}));

beforeEach(() => listen.mockClear());

describe("listeners de eventos do backend", () => {
  it("o listener de execuções é ligado uma única vez, por mais que as telas montem", async () => {
    const { startRuntimeEvents } = await import("./runtime");
    for (let mount = 0; mount < 25; mount++) startRuntimeEvents();
    const registered = listen.mock.calls.filter(([name]) => name === "runtime://event");
    expect(registered.length).toBeLessThanOrEqual(1);
    // ligar de novo depois (outras telas montando) nunca registra um segundo listener
    for (let mount = 0; mount < 25; mount++) startRuntimeEvents();
    expect(listen.mock.calls.filter(([name]) => name === "runtime://event").length).toBeLessThanOrEqual(1);
  });
});

describe("timers e listeners de tela (auditoria de código)", () => {
  const timed: Record<string, string> = { alerts, networkSecurity, windowsHealth, telemetry, machine, runtimeHub, projectRuntime };

  it("todo setInterval de estado e de tela tem o clearInterval correspondente", () => {
    for (const [name, text] of Object.entries(timed)) {
      const sets = (text.match(/setInterval\(/g) ?? []).length;
      const clears = (text.match(/clearInterval\(/g) ?? []).length;
      expect(sets, name).toBeGreaterThan(0);
      expect(clears, `${name}: setInterval sem clearInterval`).toBeGreaterThanOrEqual(sets);
    }
  });

  it("os polls de fundo só rodam com a janela visível", () => {
    for (const [name, text] of Object.entries({ alerts, networkSecurity, windowsHealth, runtimeHub, projectRuntime })) {
      expect(text, name).toContain('document.visibilityState === "visible"');
    }
  });

  it("listeners de documento e janela são removidos na limpeza", () => {
    for (const [name, text] of Object.entries({ machine, telemetry })) {
      const added = (text.match(/addEventListener\(/g) ?? []).length;
      const removed = (text.match(/removeEventListener\(/g) ?? []).length;
      expect(removed, `${name}: addEventListener sem removeEventListener`).toBeGreaterThanOrEqual(added);
    }
  });

  it("o console remonta por execução (sem estado herdado entre execuções)", () => {
    expect(runtimeHub).toContain("<RuntimeConsole key={opened.runId}");
  });
});
