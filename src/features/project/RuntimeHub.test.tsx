import { renderToStaticMarkup } from "react-dom/server";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { Association, ControlPlaneSnapshot, LogChunk, LogLine, RuntimeObservation } from "../../shared/types";
import { initialConsoleView, pause, clearView } from "../../shared/runtimeConsole";

// O componente só fala com o backend pelo `api`; aqui ele devolve snapshot/logs fixos e finge ser o desktop.
let snapshot: ControlPlaneSnapshot;
let chunk: LogChunk;
vi.mock("../../shared/api", () => ({
  desktop: true,
  errorText: (e: unknown) => String(e),
  api: async (command: string) => (command === "control_plane_snapshot" ? snapshot : command === "runtime_logs" ? chunk : null),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));

const { RuntimeHub, RuntimeConsole, ConsoleBody } = await import("./RuntimeHub");
const { workspace } = await import("../../state/workspace");
const { pullLogs } = await import("../../state/runtime");

const none: Association = { confidence: "unknown", projectId: null, projectName: null, worktreeId: null, worktreeName: null, sessionId: null, sessionLabel: null, blockId: null, blockTitle: null, evidence: [] };
const lab = (over: Partial<Association> = {}): Association => ({ ...none, confidence: "high", projectId: "lab", projectName: "LKR LAB", ...over });
const EXTERNAL = "Console não disponível — processo iniciado fora do LKR LAB.";
const runtime = (over: Partial<RuntimeObservation> = {}): RuntimeObservation => ({
  id: "r", origin: "discovered", category: "dev", label: "Vite", technology: "Vite", state: "running", rootPid: 1234,
  pids: [1234], ports: [{ port: 1420, address: "127.0.0.1", protocol: "TCP", ipVersion: "v4", pid: 1234 }], tree: [], command: "node vite", cwd: null,
  startedAt: 1_000_000 - 5 * 60_000, cpu: 2.5, memory: 512 * 1024 ** 2, association: none,
  console: { available: false, runId: null, reason: EXTERNAL }, execution: null, isSelf: false, ...over,
});
const managed = (over: Partial<RuntimeObservation> = {}) => runtime({
  id: "run-1", origin: "managed", label: "npm run dev", rootPid: 4567, pids: [4567, 4568],
  ports: [{ port: 4317, address: "0.0.0.0", protocol: "TCP", ipVersion: "v4", pid: 4568 }, { port: 4317, address: "::", protocol: "TCP", ipVersion: "v6", pid: 4568 }],
  tree: [{ pid: 4567, parentPid: null, name: "npm.exe", depth: 0, cpu: 0, memory: 1 }, { pid: 4568, parentPid: 4567, name: "node.exe", depth: 1, cpu: 1, memory: 2 }],
  association: lab({ confidence: "exact", worktreeName: "feature-x", sessionLabel: "SESSION-002", blockTitle: "05 — Live Console Hub", evidence: [{ kind: "managed_execution", detail: "grupo da execução run-1" }] }),
  console: { available: true, runId: "run-1", reason: null },
  execution: { runId: "run-1", commandId: "node:dev", command: "npm run dev", projectId: "lab", exitCode: null, observer: false }, ...over,
});
const snap = (runtimes: RuntimeObservation[], extra: Partial<ControlPlaneSnapshot> = {}): ControlPlaneSnapshot => ({ takenAt: 1_000_000, scope: "local", processTotal: 321, listeningTotal: 17, runtimes, signals: [], limitations: [], ...extra });
const line = (seq: number, stream: "out" | "err", text: string): LogLine => ({ seq, ts: 1, stream, text, source: null });

async function render(runtimes: RuntimeObservation[], extra: Partial<ControlPlaneSnapshot> = {}) {
  snapshot = snap(runtimes, extra);
  await workspace.controlPlane.refresh();
  return renderToStaticMarkup(<RuntimeHub projectId="lab" />);
}

describe("Control Plane — runtimes reais", () => {
  beforeAll(() => { snapshot = snap([]); });

  it("só mostra o que existe: sem runtimes, estados vazios honestos e nada de exemplo", async () => {
    const html = await render([]);
    expect(html).toContain("Managed Runtimes");
    expect(html).toContain("Detected Services");
    expect(html).toContain("Nenhum runtime iniciado pelo LKR LAB para este Project");
    expect(html).toContain("Nenhum serviço externo relacionado a este Project");
    expect(html).not.toContain("cp-row");
    expect(html).toContain("321 processos");
    expect(html).toContain("17 portas em escuta");
  });

  it("Managed: selo, estado, PID, portas sem duplicar v4/v6, uptime, CPU e memória", async () => {
    const html = await render([managed()]);
    const section = html.slice(html.indexOf('aria-label="Managed Runtimes"'), html.indexOf('aria-label="Detected Services"'));
    expect(section).toContain("npm run dev");
    expect(section).toContain("MANAGED");
    expect(section).toContain("RUNNING");
    expect(section).toContain(">4567<");
    expect((section.match(/:4317/g) ?? []).length).toBe(1);
    expect(section).toContain("5 min");
    expect(section).toContain("2.5%");
    expect(section).toContain("512 MB");
  });

  it("Managed com relação real mostra Project, Worktree, Session, Block e confiança Exata", async () => {
    const html = await render([managed()]);
    for (const text of ["LKR LAB", "feature-x", "SESSION-002", "05 — Live Console Hub", "Exata"]) expect(html).toContain(text);
    expect(html).toContain("grupo da execução run-1");
    expect(html).toContain("npm.exe");
    expect(html).toContain("node.exe");
  });

  it("Managed com console: CTA 'Abrir console' habilitado", async () => {
    const html = await render([managed()]);
    expect(html).toContain("Abrir console");
    expect(html).not.toContain("Console não disponível");
  });

  it("Discovered do Project: selo DISCOVERED e CTA desabilitado com o motivo", async () => {
    const html = await render([runtime({ association: lab() })]);
    const section = html.slice(html.indexOf('aria-label="Detected Services"'));
    expect(section).toContain("DISCOVERED");
    expect(section).toContain("Console não disponível");
    expect(section).toMatch(/<button[^>]*disabled[^>]*>.{0,800}Console não disponível/s);
    expect(section).toContain(EXTERNAL);
    expect(section).not.toContain("Abrir console");
    expect(section).toContain(":1420");
    expect(section).toContain(">1234<");
  });

  it("sem atribuição: Project, Worktree, Session e Block são '—' e não vão para a lista do Project", async () => {
    const html = await render([runtime({ label: "Serviço externo", ports: [{ port: 8080, address: "0.0.0.0", protocol: "TCP", ipVersion: "v4", pid: 1 }] })]);
    const detected = html.slice(html.indexOf('aria-label="Detected Services"'), html.indexOf("cp-elsewhere"));
    expect(detected).not.toContain("Serviço externo");
    const elsewhere = html.slice(html.indexOf("cp-elsewhere"));
    expect(elsewhere).toContain("Serviço externo");
    expect(elsewhere).toContain("Desconhecida");
    expect((elsewhere.match(/<dd class="muted">—<\/dd>/g) ?? []).length).toBe(4);
    expect(elsewhere).toContain(":8080");
  });

  it("estados STOPPED e FAILED (com exit code) e processo órfão de porta sem PID", async () => {
    const html = await render([
      managed({ id: "a", label: "parou", state: "stopped", pids: [], rootPid: null, ports: [] }),
      managed({ id: "b", label: "quebrou", state: "failed", execution: { runId: "b", commandId: "node:dev", command: "npm run dev", projectId: "lab", exitCode: 3, observer: false } }),
    ]);
    expect(html).toContain("STOPPED");
    expect(html).toContain("FAILED");
    expect(html).toContain("exit 3");
  });

  it("serviços do sistema ficam ocultos e contados; limitações e sinais aparecem", async () => {
    const html = await render([runtime({ category: "system" })], {
      limitations: [{ id: "protected_processes", detail: "12 processos protegidos não expõem caminho.", requiresElevation: true }],
      signals: [{ id: "x", domain: "runtime", severity: "warning", message: 'A execução "npm run dev" falhou (código 3).' }],
    });
    expect(html).toContain("1 serviço do sistema oculto");
    expect(html).not.toContain("cp-row");
    expect(html).toContain("12 processos protegidos");
    expect(html).toContain("falhou (código 3)");
  });
});

describe("Console Hub", () => {
  const lines = [line(0, "out", "VITE ready in 300 ms"), line(1, "err", "Warning: deprecated"), line(2, "out", "GET /index"), line(3, "err", "Error: boom")];
  const body = (view = initialConsoleView, source = lines) => renderToStaticMarkup(<ConsoleBody lines={source} view={view} nextSeq={4} truncated={false} />);

  it("mostra stdout e stderr, marcando o stderr", () => {
    const html = body();
    expect(html).toContain("VITE ready in 300 ms");
    expect(html).toContain('data-stream="out"');
    expect(html).toContain('data-stream="err"');
    expect(html).toContain("console-err");
  });
  it("cada linha mostra a hora local em que foi recebida", () => {
    const html = body();
    expect((html.match(/<time class="console-ts" dateTime="[^"]+">\d{2}:\d{2}:\d{2}<\/time>/g) ?? []).length).toBe(4);
  });
  it("filtros ALL, STDOUT e STDERR", () => {
    expect(body({ ...initialConsoleView, filter: "out" })).not.toContain("Warning: deprecated");
    expect(body({ ...initialConsoleView, filter: "out" })).toContain("GET /index");
    expect(body({ ...initialConsoleView, filter: "err" })).not.toContain("GET /index");
    expect(body({ ...initialConsoleView, filter: "err" })).toContain("Error: boom");
  });
  it("busca", () => {
    const html = body({ ...initialConsoleView, query: "boom" });
    expect(html).toContain("Error: boom");
    expect(html).not.toContain("GET /index");
    expect(body({ ...initialConsoleView, query: "zzz" })).toContain("Nenhuma linha corresponde ao filtro.");
  });
  it("pausa: congela a vista e avisa quantas linhas aguardam", () => {
    const html = body(pause(initialConsoleView, 2));
    expect(html).toContain("Warning: deprecated");
    expect(html).not.toContain("GET /index");
    expect(html).toContain("Pausado — 2 linhas novas aguardando.");
  });
  it("limpar vista e console vazio", () => {
    expect(body(clearView(initialConsoleView, 4))).toContain("Vista limpa — aguardando novas linhas.");
    expect(body(initialConsoleView, [])).toContain("Sem saída ainda.");
  });
  it("avisa quando o buffer descartou linhas antigas", () => {
    const html = renderToStaticMarkup(<ConsoleBody lines={lines} view={initialConsoleView} nextSeq={4} truncated />);
    expect(html).toContain("Linhas antigas descartadas pelo limite do buffer local.");
  });

  it("console de um runtime gerenciado: controles, aviso de privacidade e linhas vindas do backend", async () => {
    chunk = { lines, nextSeq: 4, truncated: false };
    await pullLogs("run-1");
    const html = renderToStaticMarkup(<RuntimeConsole runId="run-1" title="npm run dev" state="running" close={() => undefined} />);
    for (const label of ["ALL", "STDOUT", "STDERR", "Pausar", "Limpar vista", "Copiar", "Autoscroll ligado", "Buscar no console"]) expect(html).toContain(label);
    expect(html).toContain("não é sincronizado nem entra no workspace portátil");
    expect(html).toContain("VITE ready in 300 ms");
    expect(html).toContain("Error: boom");
    expect(html).toContain("RUNNING");
  });
});
