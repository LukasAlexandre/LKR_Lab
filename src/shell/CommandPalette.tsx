import { useEffect, useMemo, useState } from "react";
import { Bot, Box, Command, FileText, FolderOpen, GitBranch, Network, RefreshCw, Search, Square, Terminal } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { routes } from "../app/routing";
import { api, desktop } from "../shared/api";
import { Modal } from "../shared/ui";
import { fuzzyScore } from "../shared/search";
import { useActiveProjectId, useResource, workspace } from "../state/workspace";
import { protectedProcesses } from "../features/portClassification";

interface PaletteCommand { id: string; label: string; detail: string; icon: LucideIcon; keywords: string; run: () => void }
export function CommandPalette({ initialQuery, close, navigate, context, report, confirmKill }: {
  initialQuery: string; close: () => void; navigate: (route: string) => void; context: () => void;
  report: (error: unknown) => void; confirmKill: (pid: number, startTime: number, name: string) => void;
}) {
  const id = useActiveProjectId();
  const { data: projects } = useResource(workspace.projects);
  const { data: ports } = useResource(workspace.ports);
  const { data: processes } = useResource(workspace.processes);
  const { data: prompts } = useResource(workspace.prompts);
  const { data: knowledge } = useResource(workspace.knowledge);
  const { data: providers } = useResource(workspace.agentProviders);
  const sources = workspace.forProject(id);
  const { data: trees } = useResource(sources.worktrees);
  const { data: git } = useResource(sources.git);
  const [query, setQuery] = useState(initialQuery);
  const [index, setIndex] = useState(0);
  useEffect(() => {
    if (!desktop) return;
    void workspace.knowledge.refresh(30_000);
    void workspace.agentProviders.refresh(30_000);
  }, []);
  const commands = useMemo(() => {
    const items: PaletteCommand[] = routes.map(route => ({ id: `route-${route.id}`, label: route.title, detail: "Navegação", icon: route.icon, keywords: `abrir página ${route.title}`, run: () => navigate(route.id) }));
    projects.forEach(project => items.push({ id: `project-${project.id}`, label: project.name, detail: project.localPath, icon: FolderOpen, keywords: `project projeto workspace repositório repo ${project.tags.join(" ")}`, run: () => { workspace.selectProject(project.id); navigate("projects"); } }));
    const selected = projects.find(project => project.id === id);
    if (selected) {
      for (const [action, label, icon] of [["terminal", "Abrir terminal", Terminal], ["vscode", "Abrir IDE", FolderOpen], ["folder", "Abrir pasta", FolderOpen], ["github", "Abrir GitHub", GitBranch]] as const) {
        if (action === "github" && !selected.repository) continue;
        items.push({ id: `launch-${action}`, label: `${label} · ${selected.name}`, detail: selected.localPath, icon, keywords: action, run: () => void api("launch_project", { id, action }).catch(report) });
      }
      items.push({ id: "git", label: `Git status · ${git?.branch ?? selected.name}`, detail: "Consultar arquivos e branch locais", icon: GitBranch, keywords: "branch alterações git status", run: () => { void sources.git.refresh(); navigate("git"); } });
      items.push({ id: "context", label: "Gerar contexto do projeto", detail: selected.name, icon: FileText, keywords: "context snapshot agent ia", run: context });
      items.push({ id: "create-worktree", label: "Gerenciar / criar worktree", detail: selected.name, icon: Box, keywords: "create branch worktree", run: () => navigate("worktrees") });
    }
    ports.forEach(port => items.push({ id: `port-${port.protocol}-${port.address}-${port.port}-${port.pid}`, label: `Porta ${port.port} · ${port.process}`, detail: `${port.protocol} · ${port.address} · PID ${port.pid ?? "?"}`, icon: Network, keywords: "porta ports socket", run: () => navigate("ports") }));
    processes.forEach(process => {
      items.push({ id: `process-${process.pid}`, label: `${process.name} · PID ${process.pid}`, detail: projects.find(project => project.id === process.projectId)?.name ?? "Processo local", icon: Terminal, keywords: "processo process inspect", run: () => navigate("processes") });
      if (process.projectId && process.pid > 4 && process.startTime && !protectedProcesses.has(process.name.toLowerCase())) items.push({ id: `stop-${process.pid}`, label: `Parar ${process.name} · PID ${process.pid}`, detail: "Exige confirmação", icon: Square, keywords: "stop serviço encerrar", run: () => confirmKill(process.pid, process.startTime, process.name) });
    });
    trees.forEach(tree => items.push({ id: `tree-${tree.path}`, label: `Worktree · ${tree.branch || "Detached HEAD"}`, detail: tree.path, icon: Box, keywords: "worktree branch terminal", run: () => navigate("worktrees") }));
    prompts.forEach(prompt => items.push({ id: `prompt-${prompt.id}`, label: prompt.title, detail: `Prompt · ${prompt.category}`, icon: FileText, keywords: "prompt template", run: () => navigate("prompts") }));
    knowledge.forEach(entry => items.push({ id: `knowledge-${entry.id}`, label: entry.title, detail: `Conhecimento · ${entry.kind} · ${entry.tags}`, icon: FileText, keywords: "knowledge conhecimento documentação nota", run: () => navigate("knowledge") }));
    providers.forEach(provider => items.push({ id: `provider-${provider.provider}`, label: provider.provider, detail: `Provider · ${provider.availability}`, icon: Bot, keywords: "agent agente ia provider", run: () => navigate("agents") }));
    items.push({ id: "refresh", label: "Atualizar ambiente local", detail: "Portas, processos e sistema em segundo plano", icon: RefreshCw, keywords: "refresh atualizar", run: () => void workspace.refreshEnvironment() });
    return items;
  }, [projects, ports, processes, prompts, knowledge, providers, trees, git, id, sources, navigate, context, report, confirmKill]);
  const results = useMemo(() => commands.map(command => ({ command, score: fuzzyScore(query, `${command.label} ${command.detail} ${command.keywords}`) })).filter(item => item.score >= 0).sort((a, b) => b.score - a.score).slice(0, 12).map(item => item.command), [commands, query]);
  const active = Math.min(index, Math.max(0, results.length - 1));
  const execute = (command: PaletteCommand) => { close(); command.run(); };
  return <Modal title="Command Center" close={close} wide>
    <div className="form-body"><div className="input-icon"><Search size={18} /><input autoFocus role="combobox" aria-label="Buscar ações e recursos" aria-expanded="true" aria-controls="command-results" aria-activedescendant={results[active] ? `command-option-${active}` : undefined} placeholder="Projeto, porta, processo, worktree, prompt ou ação…" value={query} onChange={event => { setQuery(event.target.value); setIndex(0); }} onKeyDown={event => {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setIndex(Math.max(0, Math.min(results.length - 1, active + (event.key === "ArrowDown" ? 1 : -1)))); }
      else if (event.key === "Enter" && results[active]) { event.preventDefault(); execute(results[active]); }
    }} /></div><div className="palette-results" id="command-results" role="listbox" aria-label="Resultados">
      {results.map((command, position) => { const Icon = command.icon; return <button id={`command-option-${position}`} key={command.id} tabIndex={-1} role="option" aria-selected={position === active} className={position === active ? "selected" : ""} onMouseEnter={() => setIndex(position)} onClick={() => execute(command)}><span className="palette-icon"><Icon size={17} /></span><span className="palette-copy"><strong>{command.label}</strong><small>{command.detail}</small></span><kbd>{position === active ? "↵" : ""}</kbd></button>; })}
      {!results.length && <div className="palette-empty">Nenhum resultado para “{query}”.</div>}
    </div></div><div className="palette-footer"><Command size={13} /> ↑↓ navegar · Enter executar · Esc fechar · Ctrl+P projetos</div>
  </Modal>;
}
