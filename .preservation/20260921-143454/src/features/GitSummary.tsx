import { GitBranch } from "lucide-react";
import { Badge, Empty } from "../shared/ui";
import { useActiveProjectId, useResource, workspace } from "../state/workspace";
import { SourceStatus } from "../components/SourceStatus";
import { VirtualList } from "../components/VirtualList";
export function GitSummary() {
 const id = useActiveProjectId();
 const { data: projects } = useResource(workspace.projects);
 const selected = projects.find(project => project.id === id);
 const source = workspace.forProject(id).git;
 const { data: git, error: gitError } = useResource(source);
 return <><SourceStatus source={source} label="Git local" /><>
      {git ? (
        <>
          <div className="row spread">
            <span className="mono branch">
              <GitBranch size={15} />
              {git.branch}
            </span>
            <Badge tone={git.clean ? "good" : "warn"}>
              {git.clean ? "CLEAN" : "CHANGES"}
            </Badge>
          </div>
          <dl className="facts">
            <div>
              <dt>HEAD</dt>
              <dd className="mono">{git.head.slice(0, 12)}</dd>
            </div>
            <div>
              <dt>Ahead / behind</dt>
              <dd>
                {git.ahead ?? "—"} / {git.behind ?? "—"}
              </dd>
            </div>
            <div>
              <dt>Staged / unstaged</dt>
              <dd>
                {git.staged} / {git.unstaged}
              </dd>
            </div>
            <div>
              <dt>Untracked</dt>
              <dd>{git.untracked}</dd>
            </div>
            <div><dt>Stashes</dt><dd>{git.stashes}</dd></div>
            <div><dt>Origin</dt><dd className="mono" title={git.remote ?? ""}>{git.remote ?? "Não informado"}</dd></div>
          </dl>
          {!!git.files.length && <details><summary>Arquivos alterados ({git.files.length})</summary><VirtualList items={git.files} rowHeight={40} height={240} label="Arquivos alterados" itemKey={file => file.path}>{file => <div className="git-file-row"><code>{file.status}</code><span className="mono" title={file.original ? `${file.original} → ${file.path}` : file.path}>{file.path}</span></div>}</VirtualList></details>}
        </>
      ) : (
        <Empty
          title={selected ? "Git não disponível" : "Nenhum projeto selecionado"}
        >
          <p>
            {gitError ||
              "Cadastre um repositório para consultar branch e alterações."}
          </p>
        </Empty>
      )}
    </>
  </>;
}
