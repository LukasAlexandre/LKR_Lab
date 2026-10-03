import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, FileText, FolderOpen, GitBranch, Play, RotateCw, ScrollText, Square, Terminal } from "lucide-react";
import { api, desktop, errorText } from "../shared/api";
import { changesLabel } from "../shared/logic";
import { Badge, Panel } from "../shared/ui";
import { useResource, workspace } from "../state/workspace";
import { restartRun, runCommand, startRuntimeEvents, stopRun, useRunLogs, watchRun } from "../state/runtime";
import type {
  ActionGroup,
  ComposeServiceRuntime,
  LogLine,
  Project,
  ProjectRuntime as Runtime,
  RunInfo,
  RunSource,
  RunState,
  RuntimeCommand,
  RuntimeStatus,
} from "../shared/types";

type Tone = "good" | "warn" | "blue" | "neutral";
const STATUS: Record<RuntimeStatus, { label: string; tone: Tone }> = {
  running: { label: "Running", tone: "good" },
  partial: { label: "Parcial", tone: "warn" },
  error: { label: "Erro", tone: "warn" },
  ready: { label: "Pronto", tone: "blue" },
  unbound: { label: "Não localizado", tone: "warn" },
  missing: { label: "Pasta ausente", tone: "warn" },
};
const RUN_STATE: Record<RunState, { label: string; tone: Tone }> = {
  starting: { label: "Iniciando…", tone: "blue" },
  running: { label: "Em execução", tone: "good" },
  stopping: { label: "Parando…", tone: "warn" },
  stopped: { label: "Parado", tone: "neutral" },
  failed: { label: "Falhou", tone: "warn" },
  completed: { label: "Concluído", tone: "neutral" },
};
const SOURCE_LABEL: Record<RunSource, string> = { node: "Node", cargo: "Cargo", tauri: "Tauri", compose: "Docker" };
const LINE_LABEL: Record<NonNullable<LogLine["source"]>, string> = { cargo: "Cargo", vite: "Vite", tauri: "Tauri" };
const GROUP_LABEL: Record<ActionGroup, string> = { run: "Execução", quality: "Qualidade", build: "Build", control: "Controle" };
const GROUPS: ActionGroup[] = ["run", "quality", "build", "control"];
const SERVICE_STATE: Record<string, { label: string; tone: Tone }> = {
  running: { label: "Running", tone: "good" },
  restarting: { label: "Reiniciando", tone: "warn" },
  paused: { label: "Pausado", tone: "neutral" },
  created: { label: "Criado", tone: "neutral" },
  absent: { label: "Parado", tone: "neutral" },
  dead: { label: "Falhou", tone: "warn" },
};

const live = (run: RunInfo) => run.state === "starting" || run.state === "running" || run.state === "stopping";

function headline(runtime: Runtime) {
  const status = STATUS[runtime.status];
  if (runtime.status === "running" && runtime.externalRunning && !runtime.runs.some((r) => live(r) && !r.observer)) {
    return { label: "Em execução externamente", tone: status.tone };
  }
  return status;
}

/** Tarefas mostram o resultado (exit code); serviços, o estado do processo. */
function runBadge(run: RunInfo): { label: string; tone: Tone } {
  if (run.kind === "task" && !run.observer) {
    if (run.state === "completed") return { label: `Concluído · exit ${run.exitCode ?? 0}`, tone: "good" };
    if (run.state === "failed") return { label: `Falhou · exit ${run.exitCode ?? "?"}`, tone: "warn" };
    if (run.state === "stopped") return { label: "Cancelado", tone: "neutral" };
  }
  return RUN_STATE[run.state];
}

function serviceBadge(service: ComposeServiceRuntime): { label: string; tone: Tone } {
  if (service.state === "exited") {
    return { label: `Saiu · exit ${service.exitCode ?? "?"}`, tone: service.exitCode ? "warn" : "neutral" };
  }
  const base = SERVICE_STATE[service.state] ?? { label: service.state, tone: "neutral" as const };
  if (service.state === "running" && service.health === "unhealthy") return { label: "Running · não saudável", tone: "warn" };
  if (service.state === "running" && service.health === "healthy") return { label: "Running · saudável", tone: "good" };
  return base;
}

const portsText = (service: ComposeServiceRuntime) =>
  service.ports.map((p) => (p.published ? `:${p.published}→${p.target}` : `${p.target}`)).join(" ");

/** Runtime do projeto selecionado: ações por intenção, tarefas, serviços, logs e contexto de IA. */
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

  // Logs ao vivo (Compose) são observadores: nunca sobrevivem à tela que os mostra.
  useEffect(() => {
    const id = project.id;
    return () => { void api("runtime_stop_observers", { id }).catch(() => undefined); };
  }, [project.id]);

  const runs = useMemo(() => runtime?.runs ?? [], [runtime]);
  const current = useMemo(
    () => runs.find((r) => r.id === picked) ?? runs.find((r) => live(r) && !r.observer) ?? runs.find(live) ?? runs[0] ?? null,
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
  const commands = runtime.commands;
  const byId = (id: string | null) => commands.find((c) => c.id === id);
  const primary = byId(runtime.primaryCommand);
  const running = (command: RuntimeCommand, selection?: string) =>
    runs.some((r) => r.commandId === command.id && r.selection === (selection ?? null) && live(r));
  const canRun = runtime.canRun && runnable && !busy;
  const compose = runtime.compose;
  const composeDown = byId("compose:down");
  const composeRestart = byId("compose:restart");

  // Parar: cancela/para a execução viva selecionada (logs ao vivo têm o próprio botão); sem ela,
  // os containers do Compose vão para `compose down`. Reiniciar: reexecuta a execução selecionada
  // ou, para o Compose, `compose restart`. Containers pertencem ao daemon, não a um PID do cliente.
  const liveCurrent = current && live(current) && !current.observer ? current : null;
  const stopLabel = liveCurrent?.kind === "task" ? "Cancelar" : "Parar";
  const stopViaCompose = !liveCurrent && composeDown?.available === true ? composeDown : null;
  const canStop = !busy && (liveCurrent ? liveCurrent.state !== "stopping" : stopViaCompose !== null);
  const restartViaRun = current && !current.observer && current.source !== "compose" ? current : null;
  const restartViaCompose = !restartViaRun && composeRestart?.available === true ? composeRestart : null;
  const canRestart = !busy && runnable && (restartViaRun ? restartViaRun.state !== "stopping" : restartViaCompose !== null);

  const stop = () => {
    if (liveCurrent) return act(() => stopRun(liveCurrent.id));
    if (stopViaCompose) return act(() => runCommand(project.id, stopViaCompose.id));
    return Promise.resolve();
  };
  const restart = () => {
    if (restartViaRun) return act(() => restartRun(project.id, restartViaRun.id));
    if (restartViaCompose) return act(() => runCommand(project.id, restartViaCompose.id));
    return Promise.resolve();
  };
  const start = (command: RuntimeCommand, selection?: string) =>
    act(async () => {
      const run = await runCommand(project.id, command.id, selection);
      if (command.observer) {
        setPicked(run.id);
        setLogsOpen(true);
      }
    });
  const toggleLogs = () => {
    const next = !logsOpen;
    if (!next) void api("runtime_stop_observers", { id: project.id }).catch(() => undefined);
    setLogsOpen(next);
  };

  // Processos que sustentam um serviço aparecem como serviço; o resto (shells, ferramentas) só é contado.
  const backing = new Set(runtime.services.map((s) => s.pid));
  const auxiliary = runtime.processes.filter((p) => !p.managed && !backing.has(p.pid));
  const git = runtime.git;
  const dirty = git?.isRepo ? git.changes : 0;
  const missingTools = runtime.tools.filter((t) => !t.available);

  const item = (command: RuntimeCommand, selection?: string, label?: string) => {
    const choice = selection ? command.choices.find((c) => c.id === selection) : undefined;
    const text = label ?? (choice ? `${command.label} · ${choice.label}` : command.label);
    const blocked = !command.available;
    return (
      <button
        key={`${command.id}|${selection ?? ""}`}
        type="button"
        role="menuitem"
        disabled={blocked || running(command, selection)}
        title={blocked ? command.unavailableReason ?? "Indisponível" : command.detail}
        onClick={() => void start(command, selection)}
      >
        <span>{text}</span>
        <small>{blocked ? "indisponível" : command.source === "node" ? (command.kind === "service" ? "serviço" : command.kind === "task" ? "tarefa" : "Node") : SOURCE_LABEL[command.source]}</small>
      </button>
    );
  };
  const menuItems = (command: RuntimeCommand) => {
    if (command.choices.length === 0) return item(command);
    return (
      <Fragment key={command.id}>
        {!command.selectionRequired && item(command, undefined, `${command.label} · todos`)}
        {command.choices.map((choice) => item(command, choice.id))}
      </Fragment>
    );
  };
  const menuGroups = GROUPS.map((group) => ({ group, items: commands.filter((c) => c.group === group) })).filter((g) => g.items.length > 0);
  const primaryText = primary ? (primary.source === "node" ? `Rodar ${primary.label}` : primary.label) : "Rodar";
  const primaryBlocked = primary && !primary.available;

  return (
    <Panel
      title="Runtime"
      icon={<Play size={18} />}
      className="runtime-panel"
      action={<Badge tone={status.tone}>{status.label}</Badge>}
    >
      <div className="runtime-summary">
        <span>
          <strong>{runtime.composition.headline || (runtime.stack.length ? runtime.stack.map((s) => s.label).join(" · ") : "Stack não identificada")}</strong>
        </span>
        {git?.isRepo && (
          <span className="runtime-git">
            <GitBranch size={13} /> {git.detached ? "HEAD destacado" : git.branch}
            {" • "}
            {dirty ? changesLabel(dirty) : "limpo"}
            {git.ahead || git.behind ? ` · ↑${git.ahead ?? 0} ↓${git.behind ?? 0}` : ""}
            {git.conflicts > 0 && <Badge tone="warn">{git.conflicts} conflito(s)</Badge>}
          </span>
        )}
        {git && !git.isRepo && runnable && <span className="muted">Sem repositório Git</span>}
      </div>
      {runtime.composition.parts.length > 1 && (
        <div className="runtime-composition" aria-label="Composição da stack">
          {runtime.composition.parts.map((part) => (
            <span key={part.role}><small>{part.role}</small> {part.label}</span>
          ))}
        </div>
      )}
      {runtime.statusDetail && <p className="muted runtime-note">{runtime.statusDetail}</p>}
      {runnable && missingTools.length > 0 && (
        <div className="runtime-tools" role="status">
          {missingTools.map((tool) => (
            <Badge key={tool.id} tone="warn">{tool.label} não disponível</Badge>
          ))}
        </div>
      )}
      {runnable && runtime.docker?.kind === "dockerfile" && (
        <p className="muted runtime-note">Dockerfile detectado, sem Compose: o LKR LAB não oferece ações de container para este projeto.</p>
      )}

      <div className="quick-actions runtime-actions">
        <button className="button" disabled={!runnable} onClick={() => open("folder")}>
          <FolderOpen size={15} /> Abrir
        </button>
        <div className="split-button">
          <button
            className="button primary"
            disabled={primary ? !canRun || !primary.available || running(primary) : !canRun}
            title={primary ? (primaryBlocked ? primary.unavailableReason ?? "" : primary.detail) : "Escolha o que rodar"}
            onClick={() => (primary ? void start(primary) : setMenu((value) => !value))}
          >
            <Play size={15} /> {primaryText}
          </button>
          <button
            className="button primary split-toggle"
            aria-label="Escolher ação"
            aria-haspopup="menu"
            aria-expanded={menu}
            disabled={!runnable || commands.length === 0 || busy}
            onClick={() => setMenu((value) => !value)}
          >
            <ChevronDown size={15} />
          </button>
          {menu && (
            <div className="split-menu" role="menu">
              {menuGroups.map((g) => (
                <Fragment key={g.group}>
                  <div className="split-menu-title" role="presentation">{GROUP_LABEL[g.group]}</div>
                  {g.items.map(menuItems)}
                </Fragment>
              ))}
            </div>
          )}
        </div>
        <button className="button" disabled={!canStop} onClick={() => void stop()}>
          <Square size={14} /> {stopViaCompose ? "Parar containers" : stopLabel}
        </button>
        <button className="button" disabled={!canRestart} onClick={() => void restart()}>
          <RotateCw size={14} /> {restartViaCompose ? "Reiniciar containers" : "Reiniciar"}
        </button>
        <button className="button" disabled={!runnable} onClick={() => open("terminal")}>
          <Terminal size={15} /> Terminal
        </button>
        <button className="button" disabled={!project.repository} title={project.repository ? "Abrir repositório" : "Projeto sem repositório cadastrado"} onClick={() => open("github")}>
          <GitBranch size={15} /> Git
        </button>
        <button className="button" disabled={!runs.length} aria-pressed={logsOpen} onClick={toggleLogs}>
          <ScrollText size={15} /> Logs
        </button>
        <button className="button" disabled={!runnable} onClick={context}>
          <FileText size={15} /> Contexto IA
        </button>
      </div>
      {runnable && primaryBlocked && primary.unavailableReason && <p className="muted runtime-note">{primary.unavailableReason}</p>}
      {!runtime.canRun && runnable && runtime.runBlockedReason && <p className="muted runtime-note">{runtime.runBlockedReason}</p>}
      {runtime.lastTask && (
        <p className="muted runtime-note">
          Última tarefa: <code>{runtime.lastTask.command}</code> —{" "}
          {runtime.lastTask.state === "completed"
            ? `concluída (exit ${runtime.lastTask.exitCode ?? 0})`
            : runtime.lastTask.state === "stopped"
              ? "cancelada"
              : `falhou (exit ${runtime.lastTask.exitCode ?? "?"})`}
        </p>
      )}

      {(runs.length > 0 || runtime.services.length > 0 || auxiliary.length > 0 || runtime.declaredPorts.length > 0 || compose) && (
        <div className="runtime-list">
          {runs.slice(0, 5).map((run) => {
            const badge = runBadge(run);
            return (
              <div
                key={run.id}
                className={`runtime-row ${current?.id === run.id ? "selected" : ""}`}
                onClick={() => setPicked(run.id)}
              >
                <span><strong>{run.command}</strong> <small>{SOURCE_LABEL[run.source]}{run.observer ? " · observando" : " · gerenciado"}</small></span>
                <span className="mono">{run.pid && live(run) ? `PID ${run.pid}` : ""}</span>
                <Badge tone={badge.tone}>{badge.label}</Badge>
              </div>
            );
          })}
          {compose && (
            <div className="runtime-row runtime-compose-head">
              <span>
                <strong>Compose</strong> <code>{compose.file}</code>
                {compose.startedHere && <small> · subido pelo LKR LAB</small>}
              </span>
              <span className="mono">{compose.expected ? `${compose.running}/${compose.expected} ativos` : ""}</span>
              <Badge tone={compose.running > 0 ? "good" : "neutral"}>{compose.containers} {compose.containers === 1 ? "container" : "containers"}</Badge>
            </div>
          )}
          {compose?.note && <p className="muted runtime-note">{compose.note}</p>}
          {compose?.error && <p className="runtime-note runtime-error" role="alert">{compose.error}</p>}
          {compose?.services.map((service) => {
            const badge = serviceBadge(service);
            return (
              <div className="runtime-row" key={`svc-${service.name}`}>
                <span>
                  {service.name} {portsText(service) && <code>{portsText(service)}</code>}
                  {service.profiles.length > 0 && <small> · profile {service.profiles.join(", ")}</small>}
                </span>
                <span />
                <Badge tone={badge.tone}>{badge.label}</Badge>
              </div>
            );
          })}
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
            <Badge>{SOURCE_LABEL[current.source]}</Badge>
            {log.truncated && <small>Linhas antigas descartadas (limite de 2000)</small>}
          </div>
          <pre ref={logRef} aria-label={`Logs de ${current.command}`}>
            {log.lines.length === 0 ? <span className="muted">Sem saída ainda.</span> : log.lines.map((line) => (
              // stderr em vermelho só no Node; Cargo e Docker escrevem progresso normal no stderr.
              <span key={line.seq} className={line.stream === "err" && current.source === "node" ? "log-err" : undefined}>
                {line.source && <i className={`log-src log-src-${line.source}`}>{LINE_LABEL[line.source]}</i>}
                {line.text}{"\n"}
              </span>
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
      <p className="muted">Disponível no aplicativo desktop: detectar stack, ferramentas, Git e ações (Node, Rust, Tauri e Docker Compose), e rodar, parar ou reiniciar o projeto.</p>
      <div className="quick-actions runtime-actions">
        {["Abrir", "Rodar", "Parar", "Reiniciar", "Terminal", "Logs"].map((label) => (
          <button className="button" key={label} disabled title="Disponível no aplicativo desktop">{label}</button>
        ))}
      </div>
    </Panel>
  );
}
