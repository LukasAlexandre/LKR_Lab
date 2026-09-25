import { useEffect, useRef, useState } from "react";
import { Activity, RefreshCw } from "lucide-react";
import { useOperations } from "../state/operations";
import { useResource, workspace } from "../state/workspace";

export function ActivityCenter() {
  const [open, setOpen] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const operations = useOperations();
  const { data: events } = useResource(workspace.activities);
  const { data: projects } = useResource(workspace.projects);
  const running = operations.filter(operation => operation.status === "running");
  useEffect(() => {
    if (!open) return;
    const pointer = (event: PointerEvent) => { if (event.target instanceof Node && !container.current?.contains(event.target)) setOpen(false); };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") { setOpen(false); container.current?.querySelector("button")?.focus(); } };
    document.addEventListener("pointerdown", pointer); document.addEventListener("keydown", key);
    return () => { document.removeEventListener("pointerdown", pointer); document.removeEventListener("keydown", key); };
  }, [open]);
  return <div className="activity-center" ref={container}>
    <button className="icon-button" aria-label={`Central de atividade${running.length ? `: ${running.length} em andamento` : ""}`} aria-expanded={open} aria-controls="activity-panel" onClick={() => setOpen(value => !value)} title="Central de atividade"><Activity size={16} />{!!running.length && <span className="activity-count">{running.length}</span>}</button>
    {open && <section id="activity-panel" className="activity-popover" aria-label="Central de atividade"><header><strong>Activity Center</strong><small>{running.length ? `${running.length} operações em andamento` : "Atividade local"}</small></header>
      <div className="activity-feed">{[...running, ...operations.filter(operation => operation.status !== "running").slice(0, 12)].map(operation => <div className="activity-item" key={operation.id}>{operation.status === "running" ? <RefreshCw size={14} className="spin" /> : <span className={`status-orb ${operation.status === "error" ? "warning" : "healthy"}`} />}<div><strong>{operation.label}</strong><small>{projects.find(project => project.id === operation.projectId)?.name ?? "Workspace"} · {operation.finishedAt ? `${((operation.finishedAt - operation.startedAt) / 1000).toFixed(1)} s · ${operation.status === "error" ? "Falhou" : "Concluído"}` : "Em andamento"}</small>{operation.error && <small className="warn-text">{operation.error}</small>}</div></div>)}
      {events.slice(0, 5).map(event => <div className="activity-item" key={`event-${event.id}`}><span className="status-orb idle" /><div><strong>{event.action}</strong><small>{new Date(event.createdAt).toLocaleString("pt-BR")}</small></div></div>)}
      {!operations.length && !events.length && <div className="activity-empty">Operações e eventos recentes aparecerão aqui.</div>}</div>
    </section>}
  </div>;
}
