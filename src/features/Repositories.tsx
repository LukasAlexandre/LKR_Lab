import { memo } from "react";
import { ExternalLink, FolderOpen, GitBranch, Terminal } from "lucide-react";
import { api } from "../shared/api";
import { changesLabel } from "../shared/logic";
import { Badge, Empty, Panel, Refresh } from "../shared/ui";
import { SourceStatus } from "../components/SourceStatus";
import { LocationNotice } from "../components/LocationNotice";
import { useActiveProjectId, useResource, workspace } from "../state/workspace";
import type { Project } from "../shared/types";

const RepositoryRow = memo(function RepositoryRow({ project, report }: { project: Project; report: (error: unknown) => void }) {
  const selectedId = useActiveProjectId();
  const source = workspace.forProject(project.id).git;
  const { data: git, loading } = useResource(source);
  const launch = (action: string) => void api("launch_project", { id: project.id, action }).catch(report);
  return <article className={`repository-row repository-rich ${selectedId === project.id ? "active" : ""}`}>
    <GitBranch size={20} />
    <div>
      <button className="text-button" onClick={() => workspace.selectProject(project.id)}>{project.name}</button>
      <small>{project.repository || "Origin não informado"}</small>
      {project.localPath && <small className="mono">{project.localPath}</small>}
      <LocationNotice project={project} report={report} />
      {project.location === "available" && <SourceStatus source={source} label="Git" />}
      {git?.commits[0] && <small>{git.commits[0].hash} · {git.commits[0].subject}</small>}
    </div>
    <div className="repo-state">
      {git ? <><span className="mono">{git.branch}</span><Badge tone={git.clean ? "good" : "warn"}>{git.clean ? "Clean" : changesLabel(git.staged + git.unstaged + git.untracked)}</Badge><small>{git.ahead ?? "—"} ahead · {git.behind ?? "—"} behind</small></> : <small>{loading ? "Consultando Git…" : "Git indisponível"}</small>}
      <Refresh onClick={() => void source.refresh()} busy={loading} />
    </div>
    <div className="row repository-actions">
      <button className="button subtle" onClick={() => launch("folder")}><FolderOpen size={14} /> Open</button>
      <button className="icon-button" title="Abrir terminal" aria-label={`Abrir terminal em ${project.name}`} onClick={() => launch("terminal")}><Terminal size={14} /></button>
      <button className="icon-button" title="Abrir GitHub" aria-label={`Abrir GitHub de ${project.name}`} disabled={!project.repository} onClick={() => launch("github")}><ExternalLink size={14} /></button>
    </div>
  </article>;
});

export function Repositories({ report }: { report: (error: unknown) => void }) {
  const { data: projects } = useResource(workspace.projects);
  return <Panel title="Repositórios cadastrados" action={<button className="button" onClick={() => void workspace.refreshRepositories(0)}>Atualizar repositórios</button>}>
    {projects.length ? projects.map(project => <RepositoryRow key={project.id} project={project} report={report} />) : <Empty title="Nenhum repositório cadastrado"><p>Adicione uma pasta de projeto para consultar seu Git local.</p></Empty>}
  </Panel>;
}
