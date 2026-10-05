import { beforeEach, describe, expect, it, vi } from "vitest";

/* Hardening (Block 10): montar e desmontar telas repetidamente não pode duplicar listeners de
 * eventos do Tauri nem timers. O estado de execução e a telemetria têm UM listener por app. */
const listen = vi.fn(async () => () => undefined);
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
    const runtimeListeners = listen.mock.calls.filter(([name]) => name === "runtime://event");
    expect(runtimeListeners).toHaveLength(1);
  });

  it("religar depois de desmontar tudo continua sem duplicar", async () => {
    const { startRuntimeEvents } = await import("./runtime");
    startRuntimeEvents();
    startRuntimeEvents();
    expect(listen.mock.calls.filter(([name]) => name === "runtime://event")).toHaveLength(0);
  });
});

describe("timers e listeners de tela (auditoria de código)", () => {
  it("todo setInterval de estado e de tela tem o clearInterval correspondente", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const files = [
      "state/alerts.ts", "state/networkSecurity.ts", "state/windowsHealth.ts", "state/telemetry.ts",
      "state/machine.ts", "features/project/RuntimeHub.tsx", "features/ProjectRuntime.tsx",
    ];
    for (const file of files) {
      const text = fs.readFileSync(path.join(process.cwd(), "src", file), "utf8");
      const sets = (text.match(/setInterval\(/g) ?? []).length;
      const clears = (text.match(/clearInterval\(/g) ?? []).length;
      expect(sets, file).toBeGreaterThan(0);
      expect(clears, `${file}: setInterval sem clearInterval`).toBeGreaterThanOrEqual(sets);
    }
  });

  it("os polls de fundo só rodam com a janela visível", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    for (const file of ["state/alerts.ts", "state/networkSecurity.ts", "state/windowsHealth.ts", "features/project/RuntimeHub.tsx", "features/ProjectRuntime.tsx"]) {
      const text = fs.readFileSync(path.join(process.cwd(), "src", file), "utf8");
      expect(text, file).toContain('document.visibilityState === "visible"');
    }
  });

  it("o console remonta por execução (sem estado herdado entre execuções)", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const text = fs.readFileSync(path.join(process.cwd(), "src/features/project/RuntimeHub.tsx"), "utf8");
    expect(text).toContain("<RuntimeConsole key={opened.runId}");
  });
});
