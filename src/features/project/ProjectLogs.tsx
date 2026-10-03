import { useEffect, useMemo, useRef, useState } from "react";
import { ScrollText } from "lucide-react";
import { Badge, Empty, Panel } from "../../shared/ui";
import type { LogLine, RunInfo } from "../../shared/types";
import { startRuntimeEvents, useRunLogs, watchRun } from "../../state/runtime";
import { useResource, workspace } from "../../state/workspace";

const LINE_LABEL: Record<NonNullable<LogLine["source"]>, string> = { cargo: "Cargo", vite: "Vite", tauri: "Tauri" };
const live = (run: RunInfo) => run.state === "starting" || run.state === "running" || run.state === "stopping";

/** Logs reais das execuções GERENCIADAS pelo LKR LAB para este projeto (nada é inventado nem lido de fora). */
export function ProjectLogs({ projectId }: { projectId: string }) {
  const source = workspace.forProject(projectId).runtime;
  const { data: runtime } = useResource(source);
  const [picked, setPicked] = useState<string | null>(null);
  const logRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    startRuntimeEvents();
    void source.refresh(3000);
  }, [source]);

  const runs = useMemo(() => runtime?.runs ?? [], [runtime]);
  const current = useMemo(
    () => runs.find((r) => r.id === picked) ?? runs.find((r) => live(r) && !r.observer) ?? runs.find(live) ?? runs[0] ?? null,
    [runs, picked],
  );
  useEffect(() => {
    watchRun(current?.id ?? null);
    return () => watchRun(null);
  }, [current?.id]);
  const log = useRunLogs(current?.id ?? null);
  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [log.lines.length]);

  return (
    <Panel title="Logs" icon={<ScrollText size={18} />} action={current ? <Badge>{current.state}</Badge> : undefined}>
      {!current ? (
        <Empty title="Nenhuma execução registrada">
          <p>Os logs aparecem aqui quando o LKR LAB inicia uma execução deste projeto (Runtime). Processos iniciados fora dele não têm logs capturados.</p>
        </Empty>
      ) : (
        <div className="runtime-logs">
          <div className="runtime-logs-head">
            <select aria-label="Execução" value={current.id} onChange={(event) => setPicked(event.target.value)}>
              {runs.map((run) => (
                <option key={run.id} value={run.id}>{run.command} · {run.state}</option>
              ))}
            </select>
            {log.truncated && <small>Linhas antigas descartadas (limite de 2000)</small>}
          </div>
          <pre ref={logRef} aria-label={`Logs de ${current.command}`}>
            {log.lines.length === 0 ? <span className="muted">Sem saída ainda.</span> : log.lines.map((line) => (
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
