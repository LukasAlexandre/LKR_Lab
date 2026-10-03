import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { CircleCheck, Circle, Pencil, Plus, Trash2 } from "lucide-react";
import { api } from "../../shared/api";
import { contextStatus } from "../../shared/ddae";
import { blockActions, criteriaSummary, referenceInputProblem, referenceTitle } from "../../shared/ddaeDetail";
import { relativeTime } from "../../shared/projectOverview";
import { Modal } from "../../shared/ui";
import type { DdaeBlock, DdaeCriterion, DdaeReference, DdaeSessionView } from "../../shared/types";
import { ConfirmDialog } from "./DdaeSessions";

/** O que as abas precisam do Detalhe: o estado lido do backend e como executar mudanças nele. */
export interface SessionOps {
  view: DdaeSessionView;
  busy: boolean;
  error: string | null;
  run: (action: () => Promise<unknown>, message: string) => Promise<boolean>;
  setError: (error: string | null) => void;
}

/** Campos editáveis de `ddae_update_details`; os não informados mantêm o valor LIDO do backend. */
interface DetailsPatch {
  objective?: string;
  desiredOutcome?: string;
  constraints?: string[];
  criteria?: DdaeCriterion[];
  notes?: string[];
  references?: DdaeReference[];
}

function saveDetails(ops: SessionOps, patch: DetailsPatch, message: string) {
  const v = ops.view;
  const details = {
    objective: patch.objective ?? v.objective,
    desiredOutcome: patch.desiredOutcome ?? v.desiredOutcome ?? "",
    constraints: patch.constraints ?? v.constraints ?? [],
    criteria: patch.criteria ?? v.criteria ?? [],
    notes: patch.notes ?? v.notes ?? [],
    references: patch.references ?? v.references ?? [],
  };
  return ops.run(() => api("ddae_update_details", { sessionId: v.id, details }), message);
}

const Banner = ({ ops }: { ops: SessionOps }) => (ops.error ? <p className="ddae-error sd-banner" role="alert">{ops.error}</p> : null);
const locked = (v: DdaeSessionView) => v.status === "completed";

// ------------------------------------------------------------------ Blocos

function RenameDialog({ block, ops, close }: { block: DdaeBlock; ops: SessionOps; close: () => void }) {
  const [title, setTitle] = useState(block.title);
  async function submit(e: FormEvent) {
    e.preventDefault();
    if (await ops.run(() => api("ddae_rename_block", { sessionId: ops.view.id, blockId: block.id, title }), "Bloco renomeado.")) close();
  }
  return (
    <Modal title="Renomear bloco" close={close}>
      <form className="ddae-form" onSubmit={(e) => void submit(e)}>
        <label>Título<input value={title} maxLength={200} autoFocus onChange={(e) => setTitle(e.target.value)} /></label>
        {ops.error && <p className="ddae-error" role="alert">{ops.error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={ops.busy || !title.trim() || title.trim() === block.title}>Renomear</button>
        </div>
      </form>
    </Modal>
  );
}

export function BlocksTab({ ops }: { ops: SessionOps }) {
  const v = ops.view;
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [renaming, setRenaming] = useState<DdaeBlock | null>(null);
  const [removing, setRemoving] = useState<DdaeBlock | null>(null);
  async function add(e: FormEvent) {
    e.preventDefault();
    if (await ops.run(() => api("ddae_add_block", { sessionId: v.id, title, description }), "Bloco adicionado.")) {
      setTitle("");
      setDescription("");
    }
  }
  return (
    <div className="sd-tab-body">
      <Banner ops={ops} />
      {v.blocks.length === 0 ? <p className="muted">Esta sessão ainda não tem blocos.</p> : (
        <ol className="sd-blocks">
          {v.blocks.map((b, i) => {
            const a = blockActions(v, b);
            const startWhy = v.status !== "active" ? "A sessão precisa estar ativa." : v.currentBlock ? `Conclua “${v.currentBlock.title}” antes de iniciar outro.` : undefined;
            return (
              <li key={b.id} className={`sd-block is-${b.status}`}>
                <span className="sd-block-pos">{i + 1}</span>
                <div className="sd-block-main">
                  <strong>{b.title}</strong>
                  {b.description && <p className="muted">{b.description}</p>}
                </div>
                <span className={`sd-block-status is-${b.status}`}>{b.status === "completed" ? "Concluído" : b.status === "in_progress" ? "Em andamento" : "Pendente"}</span>
                <div className="sd-block-actions">
                  {b.status === "pending" && <button type="button" className="button" disabled={ops.busy || !a.start} title={a.start ? undefined : startWhy} onClick={() => void ops.run(() => api("ddae_start_block", { sessionId: v.id, blockId: b.id }), "Bloco iniciado.")}>Iniciar</button>}
                  {b.status === "in_progress" && <button type="button" className="button primary" disabled={ops.busy || !a.complete} title={a.complete ? undefined : "A sessão precisa estar ativa."} onClick={() => void ops.run(() => api("ddae_complete_block", { sessionId: v.id, blockId: b.id }), "Bloco concluído.")}>Concluir</button>}
                  {a.rename && <button type="button" className="icon-button" aria-label={`Renomear ${b.title}`} onClick={() => { ops.setError(null); setRenaming(b); }}><Pencil size={14} /></button>}
                  {a.remove && <button type="button" className="icon-button" aria-label={`Remover ${b.title}`} onClick={() => { ops.setError(null); setRemoving(b); }}><Trash2 size={14} /></button>}
                </div>
              </li>
            );
          })}
        </ol>
      )}
      <p className="muted sd-hint">Concluir o último bloco não finaliza a sessão: finalizar é uma ação explícita. Só blocos pendentes podem ser removidos.</p>
      {!locked(v) && (
        <form className="sd-form" onSubmit={(e) => void add(e)}>
          <h4>Novo bloco</h4>
          <label>Título<input value={title} maxLength={200} onChange={(e) => setTitle(e.target.value)} /></label>
          <label>Descrição <span className="muted">(opcional)</span><textarea rows={2} value={description} maxLength={1000} onChange={(e) => setDescription(e.target.value)} /></label>
          <button type="submit" className="button primary" disabled={ops.busy || !title.trim()}><Plus size={14} /> Adicionar bloco</button>
        </form>
      )}
      {renaming && <RenameDialog block={renaming} ops={ops} close={() => setRenaming(null)} />}
      {removing && (
        <ConfirmDialog title="Remover bloco" confirm="Remover" busy={ops.busy} error={ops.error} close={() => setRemoving(null)} submit={() => void ops.run(() => api("ddae_remove_block", { sessionId: v.id, blockId: removing.id }), "Bloco removido.").then((ok) => { if (ok) setRemoving(null); })}>
          <p>Remover o bloco pendente “{removing.title}”? Decisões ligadas a ele permanecem, sem o vínculo.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ Plano da sessão

function ListEditor({ label, items, onChange, disabled }: { label: string; items: string[]; onChange: (items: string[]) => void; disabled: boolean }) {
  const [draft, setDraft] = useState("");
  const add = () => { if (draft.trim()) { onChange([...items, draft.trim()]); setDraft(""); } };
  return (
    <div className="sd-editor">
      <h4>{label}</h4>
      <ul className="sd-edit-list">
        {items.map((item, i) => (
          <li key={`${i}-${item}`}>
            <input value={item} disabled={disabled} aria-label={`${label} ${i + 1}`} onChange={(e) => onChange(items.map((x, j) => (j === i ? e.target.value : x)))} />
            <button type="button" className="icon-button" disabled={disabled} aria-label={`Remover ${label.toLowerCase()} ${i + 1}`} onClick={() => onChange(items.filter((_, j) => j !== i))}><Trash2 size={14} /></button>
          </li>
        ))}
      </ul>
      <div className="sd-add-row">
        <input value={draft} disabled={disabled} placeholder="Adicionar…" onChange={(e) => setDraft(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); add(); } }} />
        <button type="button" className="button" disabled={disabled || !draft.trim()} onClick={add}><Plus size={14} /></button>
      </div>
    </div>
  );
}

export function PlanTab({ ops }: { ops: SessionOps }) {
  const v = ops.view;
  const [objective, setObjective] = useState(v.objective);
  const [outcome, setOutcome] = useState(v.desiredOutcome ?? "");
  const [constraints, setConstraints] = useState(v.constraints ?? []);
  const [criterion, setCriterion] = useState("");
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  // Relê os campos de texto quando o estado salvo muda (rascunho descartado pelo backend).
  useEffect(() => {
    setObjective(v.objective);
    setOutcome(v.desiredOutcome ?? "");
    setConstraints(v.constraints ?? []);
  }, [v.objective, v.desiredOutcome, v.constraints]);
  const criteria = v.criteria ?? [];
  const summary = criteriaSummary(criteria);
  const dirty = objective !== v.objective || outcome !== (v.desiredOutcome ?? "") || JSON.stringify(constraints) !== JSON.stringify(v.constraints ?? []);
  const disabled = ops.busy || locked(v);
  const status = contextStatus(v.readyForAi);
  // Os critérios salvam na hora e NÃO levam junto o rascunho dos campos de texto.
  const saveCriteria = (next: DdaeCriterion[], message: string) => saveDetails(ops, { criteria: next }, message);
  return (
    <div className="sd-tab-body">
      <Banner ops={ops} />
      <p className="muted sd-hint">O plano da sessão é a definição estruturada da feature. Os blocos ficam na aba Blocos.</p>
      <div className="sd-form">
        <label>Objetivo<textarea rows={3} value={objective} disabled={disabled} maxLength={4000} onChange={(e) => setObjective(e.target.value)} /></label>
        <label>Resultado desejado<textarea rows={3} value={outcome} disabled={disabled} maxLength={2000} onChange={(e) => setOutcome(e.target.value)} /></label>
        <ListEditor label="Restrições" items={constraints} onChange={setConstraints} disabled={disabled} />
        <div className="sd-actions">
          <button type="button" className="button primary" disabled={disabled || !dirty} onClick={() => void saveDetails(ops, { objective, desiredOutcome: outcome, constraints }, "Plano atualizado.")}>Salvar plano</button>
          <button type="button" className="button" disabled={!dirty} onClick={() => { setObjective(v.objective); setOutcome(v.desiredOutcome ?? ""); setConstraints(v.constraints ?? []); }}>Descartar alterações</button>
        </div>
      </div>

      <div className="sd-editor">
        <h4>Critérios de conclusão <span className="muted">{summary.done} / {summary.total}</span></h4>
        <p className="muted sd-hint">Critérios não são blocos nem progresso de execução. Se existirem, todos precisam estar concluídos para finalizar a sessão.</p>
        <ul className="sd-criteria">
          {criteria.map((c) => (
            <li key={c.id} className={c.completed ? "is-completed" : ""}>
              <button type="button" className="icon-button" disabled={disabled} aria-pressed={c.completed} aria-label={c.completed ? `Reabrir: ${c.text}` : `Concluir: ${c.text}`} onClick={() => void saveCriteria(criteria.map((x) => (x.id === c.id ? { ...x, completed: !x.completed } : x)), c.completed ? "Critério reaberto." : "Critério concluído.")}>
                {c.completed ? <CircleCheck size={16} /> : <Circle size={16} />}
              </button>
              {editing?.id === c.id ? (
                <input value={editing.text} autoFocus maxLength={500} aria-label="Texto do critério" onChange={(e) => setEditing({ id: c.id, text: e.target.value })} onKeyDown={(e) => {
                  if (e.key === "Escape") setEditing(null);
                  if (e.key === "Enter" && editing.text.trim()) { void saveCriteria(criteria.map((x) => (x.id === c.id ? { ...x, text: editing.text } : x)), "Critério atualizado.").then(() => setEditing(null)); }
                }} />
              ) : <span>{c.text}</span>}
              <button type="button" className="icon-button" disabled={disabled} aria-label={`Editar: ${c.text}`} onClick={() => setEditing({ id: c.id, text: c.text })}><Pencil size={14} /></button>
              <button type="button" className="icon-button" disabled={disabled} aria-label={`Remover: ${c.text}`} onClick={() => void saveCriteria(criteria.filter((x) => x.id !== c.id), "Critério removido.")}><Trash2 size={14} /></button>
            </li>
          ))}
        </ul>
        {criteria.length === 0 && <p className="muted">Nenhum critério definido.</p>}
        <form className="sd-add-row" onSubmit={(e) => { e.preventDefault(); if (criterion.trim()) void saveCriteria([...criteria, { id: "", text: criterion.trim(), completed: false }], "Critério adicionado.").then((ok) => { if (ok) setCriterion(""); }); }}>
          <input value={criterion} disabled={disabled} maxLength={500} placeholder="Novo critério de conclusão…" onChange={(e) => setCriterion(e.target.value)} />
          <button type="submit" className="button" disabled={disabled || !criterion.trim()}><Plus size={14} /> Adicionar</button>
        </form>
      </div>

      <div className="sd-editor">
        <h4>Contexto IA</h4>
        <p className={`ddae-context is-${status.tone}`}><small>Estado</small>{status.label}<span>{status.detail}</span></p>
        <p className="muted sd-hint">Derivado do plano e dos blocos a cada alteração; não existe botão “marcar pronto”.</p>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ Decisões

export function DecisionsTab({ ops }: { ops: SessionOps }) {
  const v = ops.view;
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [blockId, setBlockId] = useState("");
  async function add(e: FormEvent) {
    e.preventDefault();
    const ok = await ops.run(() => api("ddae_add_decision", { sessionId: v.id, title, body, blockId: blockId || null }), "Decisão registrada.");
    if (ok) { setTitle(""); setBody(""); setBlockId(""); }
  }
  const blockTitle = (id?: string) => v.blocks.find((b) => b.id === id)?.title;
  return (
    <div className="sd-tab-body">
      <Banner ops={ops} />
      {v.decisions.length === 0 ? <p className="muted">Nenhuma decisão registrada.</p> : (
        <ul className="sd-decisions">
          {[...v.decisions].reverse().map((d) => (
            <li key={d.id}>
              <strong>{d.title}</strong>
              {d.body && <p>{d.body}</p>}
              <small className="muted">
                {relativeTime(d.createdAt)}{blockTitle(d.blockId) ? ` · Bloco: ${blockTitle(d.blockId)}` : ""}
              </small>
            </li>
          ))}
        </ul>
      )}
      <p className="muted sd-hint">Decisões são registro histórico: não são editadas nem apagadas.</p>
      {!locked(v) && (
        <form className="sd-form" onSubmit={(e) => void add(e)}>
          <h4>Nova decisão</h4>
          <label>Decisão<input value={title} maxLength={200} onChange={(e) => setTitle(e.target.value)} /></label>
          <label>Detalhe <span className="muted">(opcional)</span><textarea rows={3} value={body} maxLength={8000} onChange={(e) => setBody(e.target.value)} /></label>
          <label>Bloco <span className="muted">(opcional)</span>
            <select value={blockId} onChange={(e) => setBlockId(e.target.value)}>
              <option value="">Nenhum</option>
              {v.blocks.map((b) => <option key={b.id} value={b.id}>{b.title}</option>)}
            </select>
          </label>
          <button type="submit" className="button primary" disabled={ops.busy || !title.trim()}><Plus size={14} /> Registrar decisão</button>
        </form>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ Arquivos

export function FilesTab({ ops }: { ops: SessionOps }) {
  const v = ops.view;
  const [kind, setKind] = useState<DdaeReference["kind"]>("project_path");
  const [value, setValue] = useState("");
  const [label, setLabel] = useState("");
  const refs = v.references ?? [];
  const problem = value ? referenceInputProblem(kind, value) : null;
  async function add(e: FormEvent) {
    e.preventDefault();
    const ok = await ops.run(() => api("ddae_add_reference", { sessionId: v.id, kind, value, label: label.trim() || null }), "Referência adicionada.");
    if (ok) { setValue(""); setLabel(""); }
  }
  return (
    <div className="sd-tab-body">
      <Banner ops={ops} />
      {refs.length === 0 ? <p className="muted">Nenhuma referência.</p> : (
        <ul className="sd-files">
          {refs.map((r, i) => (
            <li key={r.kind + r.value}>
              <span className="chip">{r.kind === "url" ? "URL" : "Projeto"}</span>
              <div><strong>{referenceTitle(r)}</strong>{r.label && <p className="mono muted">{r.value}</p>}</div>
              <button type="button" className="icon-button" disabled={ops.busy || locked(v)} aria-label={`Remover ${referenceTitle(r)}`} onClick={() => void saveDetails(ops, { references: refs.filter((_, j) => j !== i) }, "Referência removida.")}><Trash2 size={14} /></button>
            </li>
          ))}
        </ul>
      )}
      {!locked(v) && (
        <form className="sd-form" onSubmit={(e) => void add(e)}>
          <h4>Nova referência</h4>
          <label>Tipo
            <select value={kind} onChange={(e) => setKind(e.target.value as DdaeReference["kind"])}>
              <option value="project_path">Arquivo do projeto</option>
              <option value="url">URL</option>
            </select>
          </label>
          <label>{kind === "url" ? "URL (https)" : "Caminho relativo ao projeto"}
            <input value={value} onChange={(e) => setValue(e.target.value)} placeholder={kind === "url" ? "https://…" : "docs/ddae/…"} />
          </label>
          {kind === "project_path" && <p className="muted sd-hint">O caminho é guardado sempre RELATIVO ao projeto. Um caminho absoluto de um arquivo do projeto é convertido; fora do projeto é recusado.</p>}
          <label>Rótulo <span className="muted">(opcional)</span><input value={label} maxLength={100} onChange={(e) => setLabel(e.target.value)} /></label>
          {problem && <p className="ddae-error" role="alert">{problem}</p>}
          <button type="submit" className="button primary" disabled={ops.busy || !value.trim() || !!problem}><Plus size={14} /> Adicionar</button>
        </form>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ Anotações

export function NotesTab({ ops }: { ops: SessionOps }) {
  const v = ops.view;
  const [text, setText] = useState("");
  const notes = v.notes ?? [];
  return (
    <div className="sd-tab-body">
      <Banner ops={ops} />
      {notes.length === 0 ? <p className="muted">Nenhuma anotação.</p> : (
        <ul className="sd-notes">
          {notes.map((n, i) => (
            <li key={`${i}-${n}`}>
              <p>{n}</p>
              <button type="button" className="icon-button" disabled={ops.busy || locked(v)} aria-label={`Remover anotação ${i + 1}`} onClick={() => void saveDetails(ops, { notes: notes.filter((_, j) => j !== i) }, "Anotação removida.")}><Trash2 size={14} /></button>
            </li>
          ))}
        </ul>
      )}
      {!locked(v) && (
        <form className="sd-form" onSubmit={(e) => { e.preventDefault(); void saveDetails(ops, { notes: [...notes, text.trim()] }, "Anotação adicionada.").then((ok) => { if (ok) setText(""); }); }}>
          <h4>Nova anotação</h4>
          <textarea rows={3} value={text} maxLength={500} aria-label="Nova anotação" onChange={(e) => setText(e.target.value)} />
          <button type="submit" className="button primary" disabled={ops.busy || !text.trim()}><Plus size={14} /> Adicionar anotação</button>
        </form>
      )}
    </div>
  );
}
