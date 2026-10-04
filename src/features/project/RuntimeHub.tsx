import { useEffect, useMemo, useRef, useState } from "react";
import { Activity, Copy, Eraser, Pause, Play, Terminal, X } from "lucide-react";
import { Badge, Panel } from "../../shared/ui";
import { formatBytes } from "../../shared/machine";
import {
  CONFIDENCE_LABEL,
  associationRows,
  cpuLabel,
  distinctPorts,
  groupRuntimes,
  uptimeLabel,
} from "../../shared/controlPlane";
import {
  clearView,
  copyText,
  initialConsoleView,
  pause,
  resume,
  shouldAutoscroll,
  visibleConsole,
  type ConsoleView as ConsoleViewState,
  type StreamFilter,
} from "../../shared/runtimeConsole";
import type { LogLine, RunState, RuntimeObservation } from "../../shared/types";
import { startRuntimeEvents, useRunLogs, watchRun } from "../../state/runtime";
import { useResource, workspace } from "../../state/workspace";

type Tone = "good" | "warn" | "blue" | "neutral";
const STATE: Record<RunState, { label: string; tone: Tone }> = {
  starting: { label: "STARTING", tone: "blue" },
  running: { label: "RUNNING", tone: "good" },
  stopping: { label: "STOPPING", tone: "warn" },
  stopped: { label: "STOPPED", tone: "neutral" },
  failed: { label: "FAILED", tone: "warn" },
  completed: { label: "COMPLETED", tone: "neutral" },
};
const FILTERS: { id: StreamFilter; label: string }[] = [
  { id: "all", label: "ALL" },
  { id: "out", label: "STDOUT" },
  { id: "err", label: "STDERR" },
];
/** Atualização do snapshot enquanto a página está visível (não é monitoramento contínuo). */
const REFRESH_MS = 4000;

/* ------------------------------------------------------------------ console (apresentação pura) */

export function ConsoleBody({ lines, view, nextSeq, truncated }: { lines: LogLine[]; view: ConsoleViewState; nextSeq: number; truncated: boolean }) {
  const shown = visibleConsole(lines, view);
  const filtering = view.filter !== "all" || view.query.trim() !== "";
  let empty: string | null = null;
  if (shown.lines.length === 0) {
    if (lines.length === 0) empty = "Sem saída ainda.";
    else if (view.clearedBeforeSeq >= nextSeq && !filtering) empty = "Vista limpa — aguardando novas linhas.";
    else empty = "Nenhuma linha corresponde ao filtro.";
  }
  return (
    <>
      {(truncated || shown.omitted > 0) && (
        <small className="console-note">
          {truncated ? "Linhas antigas descartadas pelo limite do buffer local. " : ""}
          {shown.omitted > 0 ? `Mostrando as ${shown.lines.length} mais recentes (${shown.omitted} acima).` : ""}
        </small>
      )}
      <pre className="console-body" aria-label="Saída do runtime">
        {empty ? <span className="muted">{empty}</span> : shown.lines.map((line) => (
          <span key={line.seq} className={line.stream === "err" ? "console-line console-err" : "console-line"} data-stream={line.stream}>
            {line.source && <i className={`log-src log-src-${line.source}`}>{line.source}</i>}
            {line.text}{"\n"}
          </span>
        ))}
      </pre>
      {shown.pending > 0 && <small className="console-note" role="status">Pausado — {shown.pending} {shown.pending === 1 ? "linha nova" : "linhas novas"} aguardando.</small>}
    </>
  );
}

/* ------------------------------------------------------------------ console (com estado) */

export function RuntimeConsole({ runId, title, state, close }: { runId: string; title: string; state: RunState; close: () => void }) {
  const log = useRunLogs(runId);
  const [view, setView] = useState<ConsoleViewState>(initialConsoleView);
  const [follow, setFollow] = useState(true);
  const bodyRef = useRef<HTMLDivElement>(null);
  const paused = view.pausedAtSeq !== null;

  // Só esta execução é acompanhada pelos eventos de saída; o dono evita brigar com o painel de logs.
  useEffect(() => {
    watchRun(runId, "console-hub");
    return () => watchRun(null, "console-hub");
  }, [runId]);

  const shown = useMemo(() => visibleConsole(log.lines, view), [log.lines, view]);
  useEffect(() => {
    const pre = bodyRef.current?.querySelector("pre");
    if (pre && follow && !paused) pre.scrollTop = pre.scrollHeight;
  }, [shown.lines.length, follow, paused]);

  const copy = () => void navigator.clipboard?.writeText(copyText(shown.lines)).catch(() => undefined);
  return (
    <section className="console" aria-label={`Console de ${title}`}>
      <header className="console-head">
        <strong>{title}</strong>
        <Badge tone={STATE[state].tone}>{STATE[state].label}</Badge>
        <small className="muted">Console local — não é sincronizado nem entra no workspace portátil.</small>
        <button type="button" className="button subtle console-close" aria-label="Fechar console" onClick={close}><X size={14} /></button>
      </header>
      <div className="console-tools" role="toolbar" aria-label="Ferramentas do console">
        <div className="console-filters" role="group" aria-label="Filtrar saída">
          {FILTERS.map((f) => (
            <button key={f.id} type="button" className="button subtle" aria-pressed={view.filter === f.id} onClick={() => setView({ ...view, filter: f.id })}>{f.label}</button>
          ))}
        </div>
        <input
          type="search"
          className="console-search"
          placeholder="Buscar no console"
          aria-label="Buscar no console"
          value={view.query}
          onChange={(event) => setView({ ...view, query: event.target.value })}
        />
        <button type="button" className="button subtle" aria-pressed={paused} onClick={() => setView(paused ? resume(view) : pause(view, log.nextSeq))}>
          {paused ? <><Play size={14} /> Retomar</> : <><Pause size={14} /> Pausar</>}
        </button>
        <button type="button" className="button subtle" aria-pressed={follow} onClick={() => setFollow(!follow)}>Autoscroll {follow ? "ligado" : "desligado"}</button>
        <button type="button" className="button subtle" title="Limpa só a visualização; o processo e o buffer local seguem intactos" onClick={() => setView(clearView(view, log.nextSeq))}><Eraser size={14} /> Limpar vista</button>
        <button type="button" className="button subtle" onClick={copy}><Copy size={14} /> Copiar</button>
      </div>
      <div
        ref={bodyRef}
        onScroll={(event) => {
          const el = event.currentTarget.querySelector("pre");
          if (el) setFollow(shouldAutoscroll(el.scrollHeight, el.scrollTop, el.clientHeight));
        }}
      >
        <ConsoleBody lines={log.lines} view={view} nextSeq={log.nextSeq} truncated={log.truncated} />
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ runtimes */

export function RuntimeRow({ runtime, now, onOpenConsole, active }: { runtime: RuntimeObservation; now: number; onOpenConsole: (runtime: RuntimeObservation) => void; active: boolean }) {
  const state = STATE[runtime.state];
  const ports = distinctPorts(runtime);
  const uptime = runtime.state === "running" || runtime.state === "starting" ? uptimeLabel(runtime.startedAt, now) : null;
  const rows = associationRows(runtime.association);
  const managed = runtime.origin === "managed";
  const memory = formatBytes(runtime.memory);
  return (
    <article className={`cp-row ${active ? "selected" : ""}`} aria-label={`${runtime.label}${managed ? " gerenciado" : " descoberto"}`}>
      <div className="cp-main">
        <strong>{runtime.label}</strong>
        <Badge tone={managed ? "blue" : "neutral"}>{managed ? "MANAGED" : "DISCOVERED"}</Badge>
        <Badge tone={state.tone}>{state.label}</Badge>
        {runtime.isSelf && <Badge>LKR LAB</Badge>}
        {runtime.execution && runtime.state === "failed" && <Badge tone="warn">exit {runtime.execution.exitCode ?? "?"}</Badge>}
      </div>
      <dl className="cp-facts">
        {runtime.rootPid !== null && <div><dt>PID</dt><dd className="mono">{runtime.rootPid}</dd></div>}
        <div><dt>Portas</dt><dd className="mono">{ports.length ? ports.map((p) => `:${p}`).join(" ") : "—"}</dd></div>
        {uptime && <div><dt>Uptime</dt><dd>{uptime}</dd></div>}
        {runtime.pids.length > 0 && <div><dt>CPU</dt><dd>{cpuLabel(runtime.cpu)}</dd></div>}
        {memory && <div><dt>Memória</dt><dd>{memory}</dd></div>}
        {rows.map((row) => <div key={row.label}><dt>{row.label}</dt><dd className={row.known ? "" : "muted"}>{row.value}</dd></div>)}
        <div><dt>Atribuição</dt><dd>{CONFIDENCE_LABEL[runtime.association.confidence]}</dd></div>
      </dl>
      <div className="cp-actions">
        {runtime.console.available && runtime.console.runId ? (
          <button type="button" className="button" onClick={() => onOpenConsole(runtime)}><Terminal size={14} /> Abrir console</button>
        ) : (
          <>
            <button type="button" className="button" disabled title={runtime.console.reason ?? undefined}><Terminal size={14} /> Console não disponível</button>
            <small className="muted">{runtime.console.reason ?? "Processo iniciado fora do LKR LAB."}</small>
          </>
        )}
      </div>
      <details className="cp-details">
        <summary>Processo e árvore</summary>
        {runtime.command && <p><small>Linha de comando</small><code className="cp-command">{runtime.command}</code></p>}
        {runtime.cwd && <p><small>Pasta de trabalho</small><code>{runtime.cwd}</code></p>}
        {runtime.tree.length > 0 && (
          <ul className="cp-tree" aria-label="Árvore de processos">
            {runtime.tree.map((node) => (
              <li key={node.pid} style={{ paddingLeft: `${node.depth * 14}px` }}><span className="mono">{node.pid}</span> {node.name}</li>
            ))}
          </ul>
        )}
        {runtime.association.evidence.length > 0 && (
          <ul className="cp-evidence" aria-label="Evidência da atribuição">
            {runtime.association.evidence.map((e) => <li key={`${e.kind}:${e.detail}`}>{e.detail}</li>)}
          </ul>
        )}
      </details>
    </article>
  );
}

function Section({ title, hint, runtimes, empty, now, open, current }: { title: string; hint?: string; runtimes: RuntimeObservation[]; empty: string; now: number; open: (r: RuntimeObservation) => void; current: string | null }) {
  return (
    <section className="cp-section" aria-label={title}>
      <h3>{title} <small>{runtimes.length}</small></h3>
      {hint && <p className="muted cp-hint">{hint}</p>}
      {runtimes.length === 0 ? <p className="muted cp-empty">{empty}</p> : runtimes.map((r) => <RuntimeRow key={r.id} runtime={r} now={now} onOpenConsole={open} active={current === r.id} />)}
    </section>
  );
}

/** Control Plane do Project: runtimes REAIS da máquina (gerenciados e detectados) e seus consoles. */
export function RuntimeHub({ projectId }: { projectId: string }) {
  const { data: snapshot, error } = useResource(workspace.controlPlane);
  const [opened, setOpened] = useState<{ runId: string; title: string; id: string } | null>(null);

  useEffect(() => {
    startRuntimeEvents();
    void workspace.controlPlane.refresh(2000);
    // Reduz o ritmo a zero com a janela oculta: sem monitoramento de fundo.
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void workspace.controlPlane.refresh(REFRESH_MS - 500);
    }, REFRESH_MS);
    return () => clearInterval(timer);
  }, []);

  const groups = useMemo(() => groupRuntimes(snapshot, projectId), [snapshot, projectId]);
  const now = snapshot?.takenAt ?? 0;
  const current = useMemo(() => (opened ? groups.managed.find((r) => r.id === opened.id) : null) ?? null, [groups, opened]);
  const open = (runtime: RuntimeObservation) => {
    if (runtime.console.runId) setOpened({ runId: runtime.console.runId, title: runtime.label, id: runtime.id });
  };

  return (
    <Panel
      title="Control Plane"
      icon={<Activity size={18} />}
      className="runtime-hub"
      action={<Badge>Somente local</Badge>}
    >
      {!snapshot ? (
        <p className="muted" role="status">{error ?? "Lendo processos e portas da máquina…"}</p>
      ) : (
        <>
          <p className="muted cp-summary">
            {snapshot.processTotal} processos · {snapshot.listeningTotal} portas em escuta · leitura passiva: nada é encerrado, fechado ou alterado.
          </p>
          <Section title="Managed Runtimes" hint="Iniciados pelo LKR LAB: console capturado desde o start." runtimes={groups.managed} now={now} open={open} current={opened?.id ?? null} empty="Nenhum runtime iniciado pelo LKR LAB para este Project. Use Rodar em Runtime." />
          <Section title="Detected Services" hint="Descobertos na máquina e relacionados a este Project por evidência (pasta, executável ou ancestral)." runtimes={groups.detected} now={now} open={open} current={null} empty="Nenhum serviço externo relacionado a este Project." />
          {groups.elsewhere.length > 0 && (
            <details className="cp-elsewhere">
              <summary>Outros serviços na máquina ({groups.elsewhere.length}) · sem relação com este Project</summary>
              {groups.elsewhere.map((r) => <RuntimeRow key={r.id} runtime={r} now={now} onOpenConsole={open} active={false} />)}
            </details>
          )}
          {groups.system > 0 && <p className="muted cp-hint">{groups.system} {groups.system === 1 ? "serviço do sistema oculto" : "serviços do sistema ocultos"}.</p>}
          {snapshot.limitations.map((l) => <p key={l.id} className="muted cp-hint">{l.detail}</p>)}
          {snapshot.signals.map((s) => <p key={s.id} className="cp-signal" role="status">{s.message}</p>)}
          {opened && current && current.console.runId && (
            <RuntimeConsole key={opened.runId} runId={opened.runId} title={opened.title} state={current.state} close={() => setOpened(null)} />
          )}
        </>
      )}
    </Panel>
  );
}
