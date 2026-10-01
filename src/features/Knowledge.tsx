import { useEffect, useMemo, useState } from "react";
import { Copy, Plus, Save, Search } from "lucide-react";
import { api, desktop, errorText } from "../shared/api";
import { usePreference } from "../shared/preferences";
import { Empty, Refresh } from "../shared/ui";
import { SourceStatus } from "../components/SourceStatus";
import { VirtualList } from "../components/VirtualList";
import { useActiveProjectId, useResource, workspace } from "../state/workspace";
import type { KnowledgeEntry } from "../shared/types";

const kinds = { note: "Nota", decision: "Decisão", architecture: "Arquitetura", bug: "Bug", documentation: "Documentação" };
export function Knowledge() {
  const activeId = useActiveProjectId();
  const { data: projects } = useResource(workspace.projects);
  const { data: entries, loading } = useResource(workspace.knowledge);
  const [draft, setDraft] = usePreference("knowledgeDraft");
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState("all");
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [replaceWith, setReplaceWith] = useState<KnowledgeEntry | null>(null);
  useEffect(() => { if (desktop) void workspace.knowledge.refresh(30_000); }, []);
  const saved = entries.find(entry => entry.id === draft?.id);
  const dirty = !!draft && (!saved || JSON.stringify(saved) !== JSON.stringify(draft));
  const visible = useMemo(() => {
    const terms = query.toLocaleLowerCase().split(/\s+/).filter(Boolean);
    return entries.filter(entry => (scope === "all" || entry.projectId === activeId) && terms.every(term => `${entry.title} ${entry.tags} ${entry.body}`.toLocaleLowerCase().includes(term)));
  }, [entries, query, scope, activeId]);
  function open(entry: KnowledgeEntry) {
    if (dirty) { setReplaceWith(entry); return; }
    setDraft(entry); setMessage(""); setError("");
  }
  async function save() {
    if (!draft) return;
    setPending(true); setError(""); setMessage("");
    try {
      const id = await api<string>("save_knowledge", { entry: draft });
      await workspace.knowledge.refresh();
      setDraft(workspace.knowledge.getSnapshot().data.find(entry => entry.id === id) ?? { ...draft, id });
      setMessage("Documento salvo localmente.");
    } catch (cause) { setError(errorText(cause)); }
    finally { setPending(false); }
  }
  return <div className="knowledge-layout">
    <aside className="knowledge-list">
      <div className="row spread"><button className="button primary" disabled={pending} onClick={() => open({ id: "", projectId: activeId || null, title: "", kind: "note", body: "", tags: "", updatedAt: "" })}><Plus size={14} /> Novo documento</button><Refresh busy={loading} onClick={() => void workspace.knowledge.refresh()} /></div>
      <div className="toolbar-search"><Search size={14} /><input aria-label="Buscar conhecimento" placeholder="Título, tags ou conteúdo" value={query} onChange={event => setQuery(event.target.value)} /></div>
      <select aria-label="Escopo da biblioteca" value={scope} onChange={event => setScope(event.target.value)}><option value="all">Todos os projetos</option><option value="active" disabled={!activeId}>Projeto ativo</option></select>
      <SourceStatus source={workspace.knowledge} label="Biblioteca local" />
      {visible.length ? <VirtualList items={visible} rowHeight={76} height={532} label="Documentos" itemKey={entry => entry.id}>{entry => <button disabled={pending} className={`knowledge-item ${draft?.id === entry.id ? "selected" : ""}`} onClick={() => open(entry)}><strong>{entry.title}</strong><small>{kinds[entry.kind]} · {projects.find(project => project.id === entry.projectId)?.name ?? "Global"}</small><small>{entry.tags || "Sem tags"}</small></button>}</VirtualList> : <Empty title={entries.length ? "Nenhum resultado" : "Sua biblioteca começa aqui"}><p>Registre decisões, arquitetura e soluções para reutilizar no próximo trabalho.</p></Empty>}
    </aside>
    <section className="panel knowledge-editor">
      {error && <p className="error" role="alert">{error}</p>}{message && <p role="status">{message}</p>}
      {replaceWith && <div className="draft-warning" role="alert"><p>Há um rascunho não salvo. Salve antes de trocar ou descarte apenas este rascunho.</p><div className="row"><button className="button" onClick={() => setReplaceWith(null)}>Continuar editando</button><button className="button danger" onClick={() => { setDraft(replaceWith); setReplaceWith(null); }}>Descartar rascunho e abrir</button></div></div>}
      {!draft ? <Empty title="Conhecimento do seu workspace"><p>Abra um documento para editar. O conteúdo fica neste computador e funciona offline.</p></Empty> : <form onSubmit={event => { event.preventDefault(); void save(); }}>
        <fieldset disabled={pending}>
          <label>Título<input required maxLength={240} value={draft.title} onChange={event => setDraft({ ...draft, title: event.target.value })} /></label>
          <div className="form-grid"><label>Tipo<select value={draft.kind} onChange={event => setDraft({ ...draft, kind: event.target.value as KnowledgeEntry["kind"] })}>{Object.entries(kinds).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label><label>Projeto<select value={draft.projectId ?? ""} onChange={event => setDraft({ ...draft, projectId: event.target.value || null })}><option value="">Global</option>{projects.map(project => <option key={project.id} value={project.id}>{project.name}</option>)}</select></label></div>
          <label>Tags<input maxLength={2000} placeholder="arquitetura, autenticação, ADR" value={draft.tags} onChange={event => setDraft({ ...draft, tags: event.target.value })} /></label>
          <label>Conteúdo<textarea className="mono" required rows={17} value={draft.body} onChange={event => setDraft({ ...draft, body: event.target.value })} /></label>
          <div className="row"><button className="button primary" disabled={!desktop || pending || !dirty}><Save size={14} /> {pending ? "Salvando…" : "Salvar"}</button><button type="button" className="button" onClick={() => void navigator.clipboard.writeText(draft.body).then(() => setMessage("Conteúdo copiado.")).catch(cause => setError(errorText(cause)))}><Copy size={14} /> Copiar</button><small className="muted">{dirty ? "Rascunho preservado neste computador" : "Salvo"}</small></div>
        </fieldset>
      </form>}
    </section>
  </div>;
}
