import { useCallback, useState } from "react";
import { Box, Code2, FolderOpen, Plus, Terminal } from "lucide-react";
import { api, errorText } from "../shared/api";
import { Empty, Modal, Panel, Refresh } from "../shared/ui";
import { SourceStatus } from "../components/SourceStatus";
import { useActiveProjectId, useResource, workspace } from "../state/workspace";
import type { Worktree } from "../shared/types";
import { trackOperation } from "../state/operations";

export function Worktrees() {
  const id = useActiveProjectId();
  const { data: projects } = useResource(workspace.projects);
  const project = projects.find(item => item.id === id);
  const source = workspace.forProject(id).worktrees;
  const { data: trees, loading } = useResource(source);
  const [creating, setCreating] = useState(false);
  const [removing, setRemoving] = useState<Worktree | null>(null);
  const [branch, setBranch] = useState("");
  const [path, setPath] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const close = useCallback(() => { if (!pending) { setCreating(false); setRemoving(null); } }, [pending]);
  async function mutate(kind: "create" | "remove") {
    const originId = id;
    setPending(true); setError(""); setNotice("");
    try {
      if (kind === "create") await trackOperation("Criando worktree", () => api("create_worktree", { id: originId, path, branch }), originId);
      else if (removing) await trackOperation("Removendo worktree", () => api("remove_worktree", { id: originId, path: removing.path, confirmed: true }), originId);
      setCreating(false); setRemoving(null);
      setNotice(kind === "create" ? "Worktree criada." : "Worktree removida; branch preservada.");
    } catch (error) { setError(errorText(error)); }
    finally {
      setPending(false);
      void workspace.forProject(originId).worktrees.refresh();
      void workspace.activities.refresh();
    }
  }
  const launch = (tree: Worktree, action: string) => void api("launch_worktree", { id, path: tree.path, action }).catch(error => setError(errorText(error)));
  return <Panel title={project ? `Worktrees · ${project.name}` : "Worktrees"} action={<div className="row"><Refresh busy={loading} onClick={() => { if (id) void source.refresh(); }} /><button className="button primary" disabled={!project || pending} onClick={() => { setError(""); setCreating(true); }}><Plus size={14} /> Criar worktree</button></div>}>
    <SourceStatus source={source} label="Worktrees" />
    {error && <p className="error" role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {trees.length ? trees.map((tree, index) => <article className="repository-row" key={tree.path}>
      <Box size={18} /><div><strong>{tree.branch || "Detached HEAD"}</strong><small className="mono">{tree.path}</small><small>{tree.head.slice(0,12)}{index === 0 ? " · Principal" : ""}{tree.locked ? " · Bloqueada" : ""} · Agente não associado</small></div>
      <div className="row">
        <button className="icon-button" title="Abrir pasta" aria-label={`Abrir pasta ${tree.branch}`} onClick={() => launch(tree, "folder")}><FolderOpen size={15} /></button>
        <button className="icon-button" title="Terminal" aria-label={`Terminal ${tree.branch}`} onClick={() => launch(tree, "terminal")}><Terminal size={15} /></button>
        <button className="icon-button" title="VS Code" aria-label={`VS Code ${tree.branch}`} onClick={() => launch(tree, "vscode")}><Code2 size={15} /></button>
        <button className="button subtle" disabled={index === 0 || tree.locked || pending} onClick={() => { setError(""); setRemoving(tree); }}>Remover…</button>
      </div>
    </article>) : <Empty title={project ? "Nenhuma worktree consultada" : "Selecione um projeto"}><p>Use worktrees para trabalhar em branches simultaneamente, em diretórios separados.</p></Empty>}
    {creating && <Modal title="Criar worktree" close={close}>
      <form onSubmit={event => { event.preventDefault(); void mutate("create"); }}>
        <div className="form-body"><p>Uma nova branch será criada a partir do HEAD atual de {project?.name}.</p><label>Nova branch<input required value={branch} onChange={event => setBranch(event.target.value)} placeholder="feature/minha-tarefa" disabled={pending} /></label><label>Novo diretório absoluto<input required value={path} onChange={event => setPath(event.target.value)} placeholder="C:\\Dev\\projeto-tarefa" disabled={pending} /></label>{error && <p role="alert" className="error">{error}</p>}</div>
        <div className="modal-footer"><button type="button" className="button" onClick={close} disabled={pending}>Cancelar</button><button className="button primary" disabled={pending}>{pending ? "Criando…" : "Criar worktree"}</button></div>
      </form>
    </Modal>}
    {removing && <Modal title="Remover worktree?" close={close}><div className="form-body"><p>A pasta {removing.path} será removida. A branch continuará no repositório. A operação será recusada se existirem arquivos modificados, não rastreados ou ignorados.</p>{error && <p role="alert" className="error">{error}</p>}</div><div className="modal-footer"><button className="button" disabled={pending} onClick={close}>Cancelar</button><button className="button danger" disabled={pending} onClick={() => void mutate("remove")}>{pending ? "Removendo…" : "Remover worktree"}</button></div></Modal>}
  </Panel>;
}
