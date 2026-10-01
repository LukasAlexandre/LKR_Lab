import { useMemo, useState } from "react";
import { AlertTriangle, ExternalLink, Filter, FolderOpen, Square } from "lucide-react";
import { api } from "../shared/api";
import { Badge, Empty, Refresh } from "../shared/ui";
import type { PortInfo, ProcessInfo, Project } from "../shared/types";
import { VirtualList } from "../components/VirtualList";
import { portKind, protectedProcesses as systemProcesses, type PortKind } from "./portClassification";
import { usePreference, type PortProtocol } from "../shared/preferences";

export function Ports({ ports, projects, refresh, busy, confirmKill, report, associate, compact = false }: {
  ports: PortInfo[];
  projects: Project[];
  refresh: () => void;
  busy: boolean;
  confirmKill: (pid: number, startTime: number, name: string) => void;
  report: (e: unknown) => void;
  associate?: (projectId: string, port: number) => void;
  compact?: boolean;
}) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = usePreference("portsFilter");
  const [protocol, setProtocol] = usePreference("portsProtocol");
  const counts = useMemo(() => ports.reduce<Record<PortKind, number>>((result, port) => {
    result[portKind(port)] += 1;
    return result;
  }, { project: 0, expected: 0, unexpected: 0, unknown: 0, system: 0 }), [ports]);
  const filtered = useMemo(() => ports.filter((port) => {
    const kind = portKind(port);
    const matchesFilter = filter === "all"
      || (filter === "projects" && ["project", "expected", "unexpected"].includes(kind))
      || (filter === "expected" && port.expectedBy.length > 0)
      || (filter === "unexpected" && kind === "unexpected")
      || (filter === "unknown" && kind === "unknown")
      || (filter === "conflicts" && port.conflict)
      || (filter === "system" && kind === "system");
    const project = projects.find((item) => item.id === port.projectId)?.name ?? "";
    return matchesFilter && (protocol === "all" || protocol === port.protocol) && `${port.port} ${port.process} ${port.pid ?? ""} ${project}`.toLowerCase().includes(query.toLowerCase());
  }), [filter, ports, projects, query, protocol]);

  return <>
    {!compact && <div className="resource-toolbar">
      <div className="filter-tabs" role="tablist" aria-label="Classificação de portas">
        {([
          ["projects", "Projetos", counts.project + counts.expected + counts.unexpected],
          ["expected", "Esperadas", ports.filter(port => port.expectedBy.length > 0).length],
          ["unexpected", "Inesperadas", counts.unexpected],
          ["conflicts", "Conflitos", ports.filter(port => port.conflict).length],
          ["unknown", "Não identificadas", counts.unknown],
          ["system", "Sistema", counts.system],
          ["all", "Todas", ports.length],
        ] as const).map(([id, label, count]) => <button key={id} role="tab" aria-selected={filter === id} className={filter === id ? "active" : ""} onClick={() => setFilter(id)}>{label}<span>{count}</span></button>)}
      </div>
      <div className="toolbar-search"><Filter size={14} /><input aria-label="Filtrar portas" placeholder="Porta, processo, PID ou projeto…" value={query} onChange={(event) => setQuery(event.target.value)} /></div>
      <Refresh onClick={refresh} busy={busy} />
      <select aria-label="Protocolo" value={protocol} onChange={event => setProtocol(event.target.value as PortProtocol)}><option value="all">TCP + UDP</option><option>TCP</option><option>UDP</option></select>
    </div>}
    {!filtered.length ? <Empty title={ports.length ? "Nenhuma porta neste filtro" : "Nenhuma porta carregada"}>
      <p>{ports.length ? "Troque o filtro ou ajuste a busca." : "Atualize para consultar sockets locais."}</p>
      {!ports.length && <button className="button primary" onClick={refresh}>Atualizar portas</button>}
    </Empty> : <div className="port-grid">
      <div className="port-heading" aria-hidden="true"><span>Endpoint</span><span>Processo</span><span>Contexto</span><span>Estado</span><span>Ações</span></div>
      <VirtualList items={compact ? filtered.slice(0, 5) : filtered} rowHeight={compact ? 64 : 88} height={compact ? 320 : 528} label="Portas locais" itemKey={port => `${port.protocol}-${port.address}-${port.port}-${port.pid}`}>
        {(port) => {
        const project = projects.find((item) => item.id === port.projectId);
        const expected = port.expectedBy.map((id) => projects.find((item) => item.id === id)?.name).filter(Boolean);
        const kind = portKind(port);
        return <div className="port-row">
          <div><strong className="mono endpoint">localhost:{port.port}</strong><small>{port.protocol} · {port.address}</small></div>
          <div title={port.executable ?? ""}><strong>{port.process}</strong><small>PID {port.pid ?? "indisponível"}</small></div>
          <div>
            <strong>{project?.name ?? (expected.join(", ") || (kind === "system" ? "Windows / sistema" : "Sem projeto"))}</strong>
            <small>{project ? "CWD confirmado" : expected.length ? "Porta esperada" : "Origem não identificada"}</small>
            {!compact && associate && !project && kind !== "system" && <select aria-label={`Associar porta ${port.port}`} value="" onChange={(event) => event.target.value && associate(event.target.value, port.port)}><option value="">Associar ao projeto…</option>{projects.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select>}
          </div>
          <div><Badge tone={port.conflict || kind === "unexpected" ? "warn" : kind === "project" ? "good" : kind === "expected" ? "blue" : "neutral"}>{port.conflict ? "Conflict" : kind === "project" ? "Project" : kind === "expected" ? "Expected" : kind === "unexpected" ? "Unexpected" : kind === "system" ? "System" : "Unknown"}</Badge></div>
          <div>{!compact && <div className="row row-actions"><button className="icon-button" title="Abrir localhost" aria-label={`Abrir localhost na porta ${port.port}`} disabled={port.protocol !== "TCP"} onClick={() => void api("open_localhost", { port: port.port }).catch(report)}><ExternalLink size={14} /></button><button className="icon-button danger-text" title="Encerrar processo" aria-label={`Encerrar ${port.process}`} disabled={!port.pid || !port.startTime || kind === "system"} onClick={() => port.pid && port.startTime && confirmKill(port.pid, port.startTime, port.process)}><Square size={14} /></button></div>}</div>
        </div>;

        }}
      </VirtualList>
    </div>}
    {!compact && <p className="footnote"><AlertTriangle size={14} /> Portas do sistema ficam ocultas por padrão. “Expected” é uma declaração; “Project” exige vínculo observado por diretório.</p>}
  </>;
}

export function Processes({ processes, projects, refresh, busy, confirmKill }: {
  processes: ProcessInfo[];
  projects: Project[];
  refresh: () => void;
  busy: boolean;
  confirmKill: (pid: number, startTime: number, name: string) => void;
}) {
  const [filter, setFilter] = useState("");
  const [showAll, setShowAll] = useState(false);
  const visible = processes.filter((process) => {
    const devProcess = process.projectId || /node|cargo|rust|python|java|docker|vite|deno|bun/i.test(process.name);
    return (showAll || devProcess) && `${process.name} ${process.pid}`.toLowerCase().includes(filter.toLowerCase());
  });
  const grouped = projects.map((project) => ({ project, items: visible.filter((process) => process.projectId === project.id) })).filter((group) => group.items.length);
  const ungrouped = visible.filter((process) => !process.projectId);
  const processRows = (items: ProcessInfo[]) => <VirtualList items={items} rowHeight={64} label="Processos" itemKey={process => process.pid}>{(process) => <div className="process-row" key={process.pid}>
    <span className="process-icon"><FolderOpen size={15} /></span>
    <div><strong>{process.name}</strong><small className="mono">PID {process.pid} · {Math.round(process.memory / 1024 ** 2)} MB</small></div>
    <span className="process-link">{process.confidence === "cwd" ? "Project CWD" : "Development process"}</span>
    <button className="button subtle" onClick={() => confirmKill(process.pid, process.startTime, process.name)} disabled={process.pid <= 4 || !process.startTime || systemProcesses.has(process.name.toLowerCase())}>Stop…</button>
  </div>}</VirtualList>;

  return <>
    <div className="resource-toolbar"><div className="toolbar-search"><Filter size={14} /><input aria-label="Filtrar processos" placeholder="Processo ou PID…" value={filter} onChange={(event) => setFilter(event.target.value)} /></div><label className="check-control"><input type="checkbox" checked={showAll} onChange={(event) => setShowAll(event.target.checked)} /> Mostrar sistema</label><Refresh onClick={refresh} busy={busy} /></div>
    {!visible.length ? <Empty title={processes.length ? "Nenhum processo de desenvolvimento" : "Atualize para carregar processos"}><p>Processos do sistema permanecem ocultos para reduzir ruído.</p></Empty> : <div className="process-groups">
      {grouped.map(({ project, items }) => <section key={project.id} className="process-group"><header><div><span className="status-orb healthy" /><strong>{project.name}</strong></div><span>{items.length} processo{items.length === 1 ? "" : "s"}</span></header>{processRows(items)}</section>)}
      {!!ungrouped.length && <section className="process-group"><header><div><span className="status-orb idle" /><strong>Não associados</strong></div><span>{ungrouped.length} processos de desenvolvimento</span></header>{processRows(ungrouped)}</section>}
    </div>}
  </>;
}
