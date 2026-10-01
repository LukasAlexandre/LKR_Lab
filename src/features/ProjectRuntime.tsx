import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, FileText, FolderOpen, GitBranch, Play, RotateCw, ScrollText, Square, Terminal } from "lucide-react";
import { api, desktop, errorText } from "../shared/api";
import { Badge, Panel } from "../shared/ui";
import { useResource, workspace } from "../state/workspace";
import { restartRun, runScript, startRuntimeEvents, stopRun, useRunLogs, watchRun } from "../state/runtime";
import type { Project, ProjectRuntime as Runtime, RunInfo, RunState, RuntimeStatus } from "../shared/types";

const STATUS: Record<RuntimeStatus, { label: string; tone: "good" | "warn" | "blue" | "neutral" }> = {
  running: { label: "Running", tone: "good" },
  partial: { label: "Parcial", tone: "warn" },
  error: { label: "Erro", tone: "warn" },
  ready: { label: "Pronto", tone: "blue" },
  unbound: { label: "Não localizado", tone: "warn" },
  missing: { label: "Pasta ausente", tone: "warn" },
};
const RUN_STATE: Record<RunState, { label: string; tone: "good" | "warn" | "blue" | "neutral" }> = {
  starting: { label: "Iniciando…", tone: "blue" },
  running: { label: "Em execução", tone: "good" },
  stopping: { label: "Parando…", tone: "warn" },
  stopped: { label: "Parado", tone: "neutral" },
  failed: { label: "Falhou", tone: "warn" },
  completed: { label: "Concluído", tone: "neutral" },
};
const live = (run: RunInfo) => run.state === "starting" || run.state === "running" || run.state === "stopping";

function headline(runtime: Runtime) {
  const status = STATUS[runtime.status];
  if (runtime.status === "running" && runtime.externalRunning && !runtime.runs.some(live)) {
    return { label: "Em execução externamente", tone: status.tone };
  }
  return status;
}

/** Abrir, Rodar, Parar, Reiniciar, Terminal, Git, Logs e Contexto IA do projeto selecionado. */
export function ProjectRuntime({ project, context, report }: { project: Project; context: () => void; report: (error: unknown) => void }) {
  const source = workspace.forProject(project.id).runtime;
  const { data: runtime, loading } = useResource(source);
  const [menu, setMenu] = useState(false);
  const [busy, setBusy] = useState(false);
  const [logsOpen, setLogsOpen] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);

  useEffect(() => {
    startRuntimeEvents();
    void source.refresh(3000);
    // Processos externos não geram evento: consulta leve e só com a janela visível.
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void source.refresh(4000);
    }, 5000);
    return () => clearInterval(timer);
  }, [source]);

  const runs = useMemo(() => runtime?.runs ?? [], [runtime]);
  const current = useMemo(
    () => runs.find((r) => r.id === picked) ?? runs.find(live) ?? runs[0] ?? null,
    [runs, picked],
  );
  useEffect(() => {
    watchRun(logsOpen && current ? current.id : null);
    return () => watchRun(null);
  }, [logsOpen, current]);
  const log = useRunLogs(logsOpen && current ? current.id : null);
  const logRef = useRef<HTMLPreElement>(null);
  useEffect(() => {
    const el = logRef.current;
    if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 80) el.scrollTop = el.scrollHeight;
  }, [log.lines.length]);

  async function act(task: () => Promise<unknown>) {
    setBusy(true);
    setMenu(false);
    try {
      await task();
      await source.refresh();
    } catch (error) {
      report(errorText(error));
    } finally {
      setBusy(false);
    }
  }
  const open = (action: string) => void api("launch_project", { id: project.id, action }).catch(report);

  if (!runtime) {
    return (
      <Panel title="Runtime" icon={<Play size={18} />} className="runtime-panel">
        <div className="runtime-skeleton" aria-busy="true" aria-label="Lendo o estado do projeto">
          <span /><span /><span />
        </div>
        {!loading && <p className="muted">Estado indisponível.</p>}
      </Panel>
    );
  }

  const status = headline(runtime);
  const runnable = runtime.status !== "unbound" && runtime.status !== "missing";
  const running = (script: string) => runs.some((r) => r.script === script && live(r));
  const primary = runtime.scripts.find((s) => s.kind === "service") ?? runtime.scripts[0];
  const canRun = runtime.canRun && runnable && !busy;
  const services = runtime.scripts.filter((s) => s.kind !== "task");
  const tasks = runtime.scripts.filter((s) => s.kind === "task");
  // Processos que sustentam um serviço aparecem como serviço; o resto (shells, ferramentas) só é contado.
  const backing = new Set(runtime.services.map((s) => s.pid));
  const auxiliary = runtime.processes.filter((p) => !p.managed && !backing.has(p.pid));
  const git = runtime.git;
  const dirty = git?.isRepo ? git.changes : 0;

  const scriptItem = (name: string, kind: string) => (
    <button key={name} type="button" role="menuitem" disabled={running(name)} onClick={() => void act(() => runScript(project.id, name))}>
      <span>{name}</span>
      <small>{kind === "service" ? "serviço" : kind === "task" ? "tarefa" : ""}</small>
    </button>
  );

  return (
    <Panel
      title="Runtime"
      icon={<Play size={18} />}
      className="runtime-panel"
      action={<Badge tone={status.tone}>{status.label}</Badge>}
    >
      <div className="runtime-summary">
        <span>
          {runtime.stack.length ? runtime.stack.map((s) => s.label).join(" · ") : "Stack não identificada"}
          {runtime.packageManager ? ` · ${runtime.packageManager.name}` : ""}
        </span>
        {git?.isRepo && (
          <span className="runtime-git">
            <GitBranch size={13} /> {git.detached ? "HEAD destacado" : git.branch}
            {" • "}
            {dirty ? `${dirty} ${dirty === 1 ? "alteração" : "alterações"}` : "limpo"}
            {git.ahead || git.behind ? ` · ↑${git.ahead ?? 0} ↓${git.behind ?? 0}` : ""}
            {git.conflicts > 0 && <Badge tone="warn">{git.conflicts} conflito(s)</Badge>}
          </span>
        )}
        {git && !git.isRepo && runnable && <span className="muted">Sem repositório Git</span>}
      </div>
      {runtime.statusDetail && <p className="muted runtime-note">{runtime.statusDetail}</p>}

      <div className="quick-actions runtime-actions">
        <button className="button" disabled={!runnable} onClick={() => open("folder")}>
          <FolderOpen size={15} /> Abrir
        </button>
        <div className="split-button">
          <button
            className="button primary"
            disabled={!canRun || !primary || running(primary.name)}
            title={!runtime.canRun ? runtime.runBlockedReason ?? "" : primary ? `${runtime.packageManager?.name ?? ""} run ${primary.name}` : ""}
            onClick={() => primary && void act(() => runScript(project.id, primary.name))}
          >
            <Play size={15} /> Rodar{primary ? ` ${primary.name}` : ""}
          </button>
          <button
            className="button primary split-toggle"
            aria-label="Escolher script"
            aria-haspopup="menu"
            aria-expanded={menu}
            disabled={!canRun || runtime.scripts.length === 0}
            onClick={() => setMenu((value) => !value)}
          >
            <ChevronDown size={15} />
          </button>
          {menu && (
            <div className="split-menu" role="menu">
              {services.map((s) => scriptItem(s.name, s.kind))}
              {services.length > 0 && tasks.length > 0 && <hr />}
              {tasks.map((s) => scriptItem(s.name, s.kind))}
            </div>
          )}
        </div>
        <button className="button" disabled={!current || !live(current) || current.state === "stopping" || busy} onClick={() => current && void act(() => stopRun(current.id))}>
          <Square size={14} /> Parar
        </button>
        <button className="button" disabled={!current || current.state === "stopping" || busy || !runnable} onClick={() => current && void act(() => restartRun(project.id, current.id))}>
          <RotateCw size={14} /> Reiniciar
        </button>
        <button className="button" disabled={!runnable} onClick={() => open("terminal")}>
          <Terminal size={15} /> Terminal
        </button>
        <button className="button" disabled={!project.repository} title={project.repository ? "Abrir repositório" : "Projeto sem repositório cadastrado"} onClick={() => open("github")}>
          <GitBranch size={15} /> Git
        </button>
        <button className="button" disabled={!runs.length} aria-pressed={logsOpen} onClick={() => setLogsOpen((value) => !value)}>
          <ScrollText size={15} /> Logs
        </button>
        <button className="button" disabled={!runnable} onClick={context}>
          <FileText size={15} /> Contexto IA
        </button>
      </div>
      {!runtime.canRun && runnable && runtime.runBlockedReason && <p className="muted runtime-note">{runtime.runBlockedReason}</p>}

      {(runs.length > 0 || runtime.services.length > 0 || auxiliary.length > 0 || runtime.declaredPorts.length > 0) && (
        <div className="runtime-list">
          {runs.slice(0, 5).map((run) => (
            <div
              key={run.id}
              className={`runtime-row ${current?.id === run.id ? "selected" : ""}`}
              onClick={() => setPicked(run.id)}
            >
              <span><strong>{run.command}</strong> <small>gerenciado</small></span>
              <span className="mono">{run.pid && live(run) ? `PID ${run.pid}` : run.exitCode !== null ? `código ${run.exitCode}` : ""}</span>
              <Badge tone={RUN_STATE[run.state].tone}>{RUN_STATE[run.state].label}</Badge>
            </div>
          ))}
          {runtime.services.map((s) => (
            <div className="runtime-row" key={`${s.pid}-${s.port}`}>
              <span>{s.label}{s.managed ? "" : " (externo)"} <small>PID {s.pid}</small></span>
              <span className="mono">{s.port ? `:${s.port}` : ""}</span>
              {s.port && (
                <button className="button subtle" onClick={() => void api("open_localhost", { port: s.port }).catch(report)}>
                  Abrir no navegador
                </button>
              )}
            </div>
          ))}
          {auxiliary.length > 0 && (
            <div className="runtime-row">
              <span className="muted">
                {auxiliary.length} {auxiliary.length === 1 ? "processo auxiliar externo" : "processos auxiliares externos"} (shells, ferramentas) com a pasta do projeto como diretório
              </span>
              <span />
              <Badge>Externo · não iniciado pelo LKR LAB</Badge>
            </div>
          )}
          {runtime.declaredPorts.map((p) => (
            <div className="runtime-row" key={`d${p.port}`}>
              <span>{p.name} <code>:{p.port}</code> <small>declarada</small></span>
              <span />
              <Badge tone={p.state === "listening" ? "good" : p.state === "free" ? "neutral" : "warn"}>
                {p.state === "listening" ? "Ativa · dono verificado" : p.state === "free" ? "Livre" : "Ocupada · dono não verificado"}
              </Badge>
            </div>
          ))}
        </div>
      )}

      {logsOpen && current && (
        <div className="runtime-logs">
          <div className="runtime-logs-head">
            <strong>Logs · {current.command}</strong>
            {log.truncated && <small>Linhas antigas descartadas (limite de 2000)</small>}
          </div>
          <pre ref={logRef} aria-label={`Logs de ${current.command}`}>
            {log.lines.length === 0 ? <span className="muted">Sem saída ainda.</span> : log.lines.map((line) => (
              <span key={line.seq} className={line.stream === "err" ? "log-err" : undefined}>{line.text}{"\n"}</span>
            ))}
          </pre>
        </div>
      )}
    </Panel>
  );
}

/** Web: o controle de runtime é exclusivo do app desktop; nada é simulado. */
export function RuntimeDesktopOnly() {
  if (desktop) return null;
  return (
    <Panel title="Runtime" icon={<Play size={18} />} className="runtime-panel" action={<Badge>Somente desktop</Badge>}>
      <p className="muted">Disponível no aplicativo desktop: detectar stack, Git e scripts, e rodar, parar ou reiniciar o projeto.</p>
      <div className="quick-actions runtime-actions">
        {["Abrir", "Rodar", "Parar", "Reiniciar", "Terminal", "Logs"].map((label) => (
          <button className="button" key={label} disabled title="Disponível no aplicativo desktop">{label}</button>
        ))}
      </div>
    </Panel>
  );
}
