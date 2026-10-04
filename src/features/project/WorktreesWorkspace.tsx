import { useEffect, useMemo, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { Box, Check, CircleCheck, Code2, Copy, Ellipsis, FolderOpen, Pause, Plus, RefreshCw, Search, Snowflake, Terminal, TriangleAlert, Zap } from "lucide-react";
import { projectHash } from "../../app/projectRoute";
import { api, desktop, errorText } from "../../shared/api";
import { deriveDdaeNextAction } from "../../shared/ddae";
import { relativeTime, runtimeLabel } from "../../shared/projectOverview";
import { GIT_FILTERS, KIND_BADGE, OPERATIONAL_FILTERS, WARNING_TEXT, createProblem, finalizeGitWarning, gitFacts, gitText, itemBranch, itemName, pauseGitWarning, showPrimary, stateActions, statusBadge, suggestPath, syncText, visibleItems } from "../../shared/worktrees";
import type { Filters, GitFilter, OperationalFilter, StateAction } from "../../shared/worktrees";
import { Empty, Modal } from "../../shared/ui";
import type { DdaeSessionView, ManagedWorktree, WorktreeItem, WorktreeStatus } from "../../shared/types";
import { useMachine } from "../../state/machine";
import { notifyLocalChange } from "../../state/sync";
import { useResource, workspace } from "../../state/workspace";

const STATUS_ICON: Record<WorktreeStatus, typeof Box> = { active: Zap, frozen: Snowflake, stopped: Pause, completed: CircleCheck };

function StatusIcon({ status }: { status: WorktreeStatus }) {
  const Icon = STATUS_ICON[status];
  return <span className={`wt-icon is-${status}`} aria-hidden="true"><Icon size={18} /></span>;
}

/** PARADO é slate; só ATIVO pulsa (CSS respeita prefers-reduced-motion). */
function StatusBadge({ status }: { status: WorktreeStatus }) {
  return <span className={`wt-badge is-${status}`}><i aria-hidden="true" />{statusBadge(status)}</span>;
}

const Row = ({ label, children, mono = false }: { label: string; children: ReactNode; mono?: boolean }) => (
  <div className="wt-row"><dt>{label}</dt><dd className={mono ? "mono" : ""}>{children}</dd></div>
);

type Dialog =
  | { kind: "new" }
  | { kind: "adopt"; item: WorktreeItem }
  | { kind: "relation"; item: WorktreeItem }
  | { kind: "state"; item: WorktreeItem; action: StateAction }
  | { kind: "rename"; item: WorktreeItem }
  | { kind: "remove"; item: WorktreeItem }
  | { kind: "locate"; item: WorktreeItem }
  | null;

interface Actions {
  busy: boolean;
  open: (dialog: Dialog) => void;
  launch: (item: WorktreeItem, action: string) => void;
}

function CopyHead({ head }: { head: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(head);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* área de transferência bloqueada: sem efeito */
    }
  }
  return (
    <span className="wt-head">
      <code>{head.slice(0, 7) || "—"}</code>
      {head && <button type="button" className="icon-button" aria-label="Copiar HEAD" onClick={() => void copy()}>{copied ? <Check size={13} /> : <Copy size={13} />}</button>}
    </span>
  );
}

function Menu({ children }: { children: ReactNode }) {
  return (
    <details className="wt-menu" onClick={(e) => { if ((e.target as HTMLElement).closest("button")) e.currentTarget.removeAttribute("open"); }}>
      <summary aria-label="Mais ações"><Ellipsis size={16} /></summary>
      <div className="wt-menu-list">{children}</div>
    </details>
  );
}

function GitRows({ item, runtime }: { item: WorktreeItem; runtime: string }) {
  const git = gitText(item);
  const facts = gitFacts(item);
  const sync = syncText(facts);
  return (
    <>
      <Row label="Branch" mono>{itemBranch(item) || (item.git?.detached ? "Detached HEAD" : "—")}{item.git?.locked ? " · bloqueado" : ""}{item.git?.prunable ? " · prunable" : ""}</Row>
      <Row label="Path" mono>{item.git ? item.git.path : "Não localizado nesta máquina"}</Row>
      <Row label="Git"><span className={`tone-${git.tone}`}>{git.text}</span>{sync && <span className="muted"> · {sync}</span>}</Row>
      <Row label="HEAD">{item.git ? <CopyHead head={item.git.head} /> : "—"}</Row>
      <Row label="Runtime">{runtime}</Row>
    </>
  );
}

function PrimaryCard({ item, runtime, actions }: { item: WorktreeItem; runtime: string; actions: Actions }) {
  return (
    <article className="wt-card is-primary" aria-label="Checkout principal">
      <header>
        <span className="wt-icon is-primary" aria-hidden="true"><FolderOpen size={18} /></span>
        <h3>{itemName(item)}</h3>
        <span className="wt-badge is-kind">{KIND_BADGE.primary}</span>
      </header>
      <p className="muted">Branch principal do projeto. Fica fora do ciclo operacional e dos contadores.</p>
      <dl><GitRows item={item} runtime={runtime} /></dl>
      <div className="wt-actions">
        <button type="button" className="button primary" onClick={() => actions.launch(item, "folder")}><FolderOpen size={14} /> Abrir worktree</button>
        <button type="button" className="icon-button" aria-label="Terminal" onClick={() => actions.launch(item, "terminal")}><Terminal size={15} /></button>
        <button type="button" className="icon-button" aria-label="VS Code" onClick={() => actions.launch(item, "vscode")}><Code2 size={15} /></button>
      </div>
    </article>
  );
}

function WorktreeCard({ item, actions, projectId }: { item: WorktreeItem; actions: Actions; projectId: string }) {
  const m = item.managed;
  const available = item.kind === "managed_available";
  const unmanaged = item.kind === "unmanaged";
  const missing = item.kind === "managed_missing";
  const facts = gitFacts(item);
  const pauseWarning = m && (m.status === "frozen" || m.status === "stopped") ? pauseGitWarning(facts) : "";
  const acts = m ? stateActions(m.status) : [];
  const label: Record<StateAction, string> = { freeze: "Congelar", stop: "Parar", resume: "Retomar", complete: "Finalizar" };
  return (
    <article className={`wt-card ${m ? `is-${m.status}` : "is-neutral"} ${missing ? "is-missing" : ""}`} aria-label={itemName(item)}>
      <header>
        {m ? <StatusIcon status={m.status} /> : <span className="wt-icon is-neutral" aria-hidden="true"><Box size={18} /></span>}
        <h3 title={itemName(item)}>{itemName(item)}</h3>
        {m && <StatusBadge status={m.status} />}
        {KIND_BADGE[item.kind] && <span className="wt-badge is-kind">{KIND_BADGE[item.kind]}</span>}
      </header>
      {m?.description ? <p className="muted">{m.description}</p> : unmanaged ? <p className="muted">Existe no Git, mas o LKR LAB ainda não o gerencia. Nada foi gravado.</p> : null}
      <dl>
        <GitRows item={item} runtime="Não disponível" />
        <Row label="DDAE">
          {m?.session ? <a href={projectHash(projectId, "ddae", m.session.id)}>{m.session.label}<span className="muted"> {m.session.title}</span></a> : "—"}
        </Row>
        <Row label="Bloco relacionado">{m?.block ? m.block.title : "—"}</Row>
        {m?.status === "completed" && <Row label="Resultado">{m.result ?? "—"}</Row>}
        {m && (m.status === "frozen" || m.status === "stopped") && m.stateReason && <Row label="Motivo">{m.stateReason}</Row>}
        <Row label="Última atividade">{m?.lastEventAt ? relativeTime(m.lastEventAt) : "—"}</Row>
      </dl>
      {item.warnings.map((w) => <p key={w} className="wt-warning" role="note"><TriangleAlert size={13} /> {WARNING_TEXT[w]}</p>)}
      {pauseWarning && <p className="wt-warning" role="note"><TriangleAlert size={13} /> {pauseWarning}</p>}
      {missing && <p className="wt-warning" role="note"><TriangleAlert size={13} /> Não localizado nesta máquina: o estado e o histórico continuam, mas não há Git nem runtime a mostrar.</p>}
      <div className="wt-actions">
        {unmanaged && <button type="button" className="button primary" disabled={actions.busy} onClick={() => actions.open({ kind: "adopt", item })}>Adotar</button>}
        {missing && <button type="button" className="button primary" disabled={actions.busy} onClick={() => actions.open({ kind: "locate", item })}>Localizar</button>}
        {(available || unmanaged) && item.git && (
          <>
            <button type="button" className={`button ${available ? "primary" : ""}`} onClick={() => actions.launch(item, "folder")}><FolderOpen size={14} /> Abrir worktree</button>
            <button type="button" className="icon-button" aria-label={`Terminal ${itemName(item)}`} onClick={() => actions.launch(item, "terminal")}><Terminal size={15} /></button>
            <button type="button" className="icon-button" aria-label={`VS Code ${itemName(item)}`} onClick={() => actions.launch(item, "vscode")}><Code2 size={15} /></button>
          </>
        )}
        {(m || (item.git && !item.git.isPrimary)) && (
          <Menu>
            {m && m.status !== "completed" && <button type="button" onClick={() => actions.open({ kind: "rename", item })}>Renomear…</button>}
            {m && m.status !== "completed" && <button type="button" onClick={() => actions.open({ kind: "relation", item })}>Associar Session…</button>}
            {acts.map((a) => <button type="button" key={a} disabled={actions.busy} onClick={() => actions.open({ kind: "state", item, action: a })}>{label[a]}{a === "complete" ? "…" : ""}</button>)}
            {item.git && !item.git.isPrimary && <button type="button" className="danger" disabled={actions.busy || item.git.locked} onClick={() => actions.open({ kind: "remove", item })}>Remover worktree do Git…</button>}
          </Menu>
        )}
      </div>
    </article>
  );
}

function SessionPicker({ sessions, sessionId, blockId, setSession, setBlock }: {
  sessions: DdaeSessionView[]; sessionId: string; blockId: string; setSession: (id: string) => void; setBlock: (id: string) => void;
}) {
  const session = sessions.find((s) => s.id === sessionId);
  return (
    <>
      <label>Session <span className="muted">(opcional)</span>
        <select value={sessionId} onChange={(e) => { setSession(e.target.value); setBlock(""); }}>
          <option value="">Nenhuma</option>
          {sessions.map((s) => <option key={s.id} value={s.id}>{s.label} — {s.title}</option>)}
        </select>
      </label>
      {session && (
        <label>Bloco <span className="muted">(opcional)</span>
          <select value={blockId} onChange={(e) => setBlock(e.target.value)}>
            <option value="">Nenhum</option>
            {session.blocks.map((b) => <option key={b.id} value={b.id}>{b.title}</option>)}
          </select>
        </label>
      )}
    </>
  );
}

interface FormProps { busy: boolean; error: string | null; close: () => void; run: (fn: () => Promise<unknown>, message: string) => Promise<boolean> }

function NewDialog({ projectId, projectPath, sessions, busy, error, close, run }: FormProps & { projectId: string; projectPath: string; sessions: DdaeSessionView[] }) {
  const [mode, setMode] = useState<"new_branch" | "existing_branch">("new_branch");
  const [name, setName] = useState("");
  const [branch, setBranch] = useState("");
  const [base, setBase] = useState("HEAD");
  const [path, setPath] = useState("");
  const [touchedPath, setTouchedPath] = useState(false);
  const [description, setDescription] = useState("");
  const [sessionId, setSessionId] = useState("");
  const [blockId, setBlockId] = useState("");
  const shownPath = touchedPath ? path : suggestPath(projectPath, branch);
  const problem = branch || shownPath ? createProblem({ mode, branch, baseRef: base, path: shownPath, blockId, sessionId }) : null;
  async function submit(e: FormEvent) {
    e.preventDefault();
    const request = { mode, displayName: name, description, branch, baseRef: mode === "new_branch" ? base : "", path: shownPath, sessionId: sessionId || null, blockId: blockId || null };
    if (await run(() => api("worktree_create", { projectId, request }), "Worktree criado.")) close();
  }
  return (
    <Modal title="Novo worktree" close={close} wide>
      <form className="ddae-form" onSubmit={(e) => void submit(e)}>
        <fieldset className="wt-mode">
          <legend>Modo</legend>
          <label><input type="radio" name="mode" checked={mode === "new_branch"} onChange={() => setMode("new_branch")} /> Nova branch</label>
          <label><input type="radio" name="mode" checked={mode === "existing_branch"} onChange={() => setMode("existing_branch")} /> Branch existente</label>
        </fieldset>
        <label>Nome de exibição <span className="muted">(opcional; padrão: a branch)</span><input value={name} maxLength={100} onChange={(e) => setName(e.target.value)} /></label>
        <label>{mode === "new_branch" ? "Nova branch" : "Branch existente"}<input value={branch} autoFocus placeholder="feature/minha-tarefa" onChange={(e) => setBranch(e.target.value)} /></label>
        {mode === "new_branch" && <label>Base da nova branch<input value={base} onChange={(e) => setBase(e.target.value)} placeholder="HEAD, main, uma tag ou commit" /></label>}
        <label>Destino local (esta máquina)<input value={shownPath} onChange={(e) => { setTouchedPath(true); setPath(e.target.value); }} placeholder="C:\\Dev\\projeto-tarefa" /></label>
        <p className="muted wt-hint">O destino é local: vira só o vínculo desta máquina e nunca entra no estado portátil. Nenhum fetch, pull, push ou merge é feito.</p>
        <label>Descrição <span className="muted">(opcional)</span><textarea rows={2} value={description} maxLength={1000} onChange={(e) => setDescription(e.target.value)} /></label>
        <SessionPicker sessions={sessions} sessionId={sessionId} blockId={blockId} setSession={setSessionId} setBlock={setBlockId} />
        {(problem || error) && <p className="ddae-error" role="alert">{error ?? problem}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy || !!createProblem({ mode, branch, baseRef: base, path: shownPath, blockId, sessionId })}>Criar worktree</button>
        </div>
      </form>
    </Modal>
  );
}

function AdoptDialog({ projectId, item, sessions, busy, error, close, run }: FormProps & { projectId: string; item: WorktreeItem; sessions: DdaeSessionView[] }) {
  const [name, setName] = useState(itemName(item));
  const [sessionId, setSessionId] = useState("");
  const [blockId, setBlockId] = useState("");
  async function submit(e: FormEvent) {
    e.preventDefault();
    if (await run(() => api("worktree_adopt", { projectId, path: item.git?.path, name, sessionId: sessionId || null, blockId: blockId || null }), "Worktree adotado.")) close();
  }
  return (
    <Modal title="Adotar worktree" close={close}>
      <form className="ddae-form" onSubmit={(e) => void submit(e)}>
        <p>Adotar cria a metadata do LKR LAB (estado ATIVO) para este worktree. Nada muda no Git.</p>
        <label>Nome de exibição<input value={name} maxLength={100} autoFocus onChange={(e) => setName(e.target.value)} /></label>
        <SessionPicker sessions={sessions} sessionId={sessionId} blockId={blockId} setSession={setSessionId} setBlock={setBlockId} />
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy || !name.trim()}>Adotar</button>
        </div>
      </form>
    </Modal>
  );
}

function RelationDialog({ m, sessions, busy, error, close, run }: FormProps & { m: ManagedWorktree; sessions: DdaeSessionView[] }) {
  const [sessionId, setSessionId] = useState(m.sessionId ?? "");
  const [blockId, setBlockId] = useState(m.blockId ?? "");
  async function submit(e: FormEvent) {
    e.preventDefault();
    if (await run(() => api("worktree_set_relation", { id: m.id, sessionId: sessionId || null, blockId: blockId || null }), "Vínculo atualizado.")) close();
  }
  return (
    <Modal title={`Associar Session — ${m.displayName}`} close={close}>
      <form className="ddae-form" onSubmit={(e) => void submit(e)}>
        <p className="muted">Mostra só as Sessions deste projeto. Associar não altera o estado da Session nem do worktree.</p>
        <SessionPicker sessions={sessions} sessionId={sessionId} blockId={blockId} setSession={setSessionId} setBlock={setBlockId} />
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy || (sessionId === (m.sessionId ?? "") && blockId === (m.blockId ?? ""))}>Salvar vínculo</button>
        </div>
      </form>
    </Modal>
  );
}

function StateDialog({ item, action, busy, error, close, run }: FormProps & { item: WorktreeItem; action: StateAction }) {
  const m = item.managed!;
  const [text, setText] = useState("");
  const facts = gitFacts(item);
  const complete = action === "complete";
  const warning = complete ? finalizeGitWarning(facts) : { level: "none" as const, text: "" };
  const target: WorktreeStatus = action === "freeze" ? "frozen" : action === "stop" ? "stopped" : action === "resume" ? "active" : "completed";
  const title = { freeze: "Congelar", stop: "Parar", resume: "Retomar", complete: "Finalizar" }[action];
  async function submit(e: FormEvent) {
    e.preventDefault();
    const args = { id: m.id, status: target, reason: complete || action === "resume" ? null : text, result: complete ? text : null };
    if (await run(() => api("worktree_set_state", args), `Worktree ${title.toLowerCase()}.`)) close();
  }
  return (
    <Modal title={`${title} — ${m.displayName}`} close={close}>
      <form className="ddae-form" onSubmit={(e) => void submit(e)}>
        {complete && <p><strong>Finalizar é terminal e só altera o estado no LKR LAB.</strong> Não faz commit, merge, push, checkout nem remove a pasta, o worktree ou a branch.</p>}
        {action === "freeze" && <p>Congelar é aguardar algo externo. Nada muda no Git.</p>}
        {action === "stop" && <p>Parar é encerrar o ritmo sem retomada imediata. Nada muda no Git.</p>}
        {action === "resume" && <p>O worktree volta a ATIVO.</p>}
        {warning.level !== "none" && <p className={`wt-warning ${warning.level === "conflicts" ? "is-strong" : ""}`} role="note"><TriangleAlert size={14} /> {warning.text}</p>}
        {(action === "freeze" || action === "stop") && pauseGitWarning(facts) && <p className="wt-warning" role="note"><TriangleAlert size={14} /> {pauseGitWarning(facts)}</p>}
        {action !== "resume" && (
          <label>{complete ? "Resultado" : "Motivo"} <span className="muted">({complete ? "opcional" : "recomendado"})</span>
            <textarea rows={3} value={text} maxLength={complete ? 2000 : 500} autoFocus onChange={(e) => setText(e.target.value)} />
          </label>
        )}
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy}>{complete ? "Finalizar worktree" : title}</button>
        </div>
      </form>
    </Modal>
  );
}

function RenameDialog({ m, busy, error, close, run }: FormProps & { m: ManagedWorktree }) {
  const [name, setName] = useState(m.displayName);
  const [description, setDescription] = useState(m.description ?? "");
  async function submit(e: FormEvent) {
    e.preventDefault();
    if (await run(() => api("worktree_update", { id: m.id, displayName: name, description }), "Worktree atualizado.")) close();
  }
  return (
    <Modal title="Renomear worktree" close={close}>
      <form className="ddae-form" onSubmit={(e) => void submit(e)}>
        <p className="muted">Só a metadata do LKR LAB muda: a branch e a pasta no Git continuam como estão.</p>
        <label>Nome<input value={name} maxLength={100} autoFocus onChange={(e) => setName(e.target.value)} /></label>
        <label>Descrição <span className="muted">(opcional)</span><textarea rows={2} value={description} maxLength={1000} onChange={(e) => setDescription(e.target.value)} /></label>
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy || !name.trim()}>Salvar</button>
        </div>
      </form>
    </Modal>
  );
}

function RemoveDialog({ projectId, item, busy, error, close, run }: FormProps & { projectId: string; item: WorktreeItem }) {
  const git = item.git!;
  return (
    <Modal title="Remover worktree do Git" close={close}>
      <div className="ddae-form">
        <p><strong>Isto executa <code>git worktree remove</code>:</strong> a pasta <code>{git.path}</code> será removida do disco.</p>
        <ul className="sd-list">
          <li>A branch continua no repositório.</li>
          <li>A metadata do LKR LAB e o histórico são preservados; o worktree passa a “não localizado”.</li>
          <li>Isto <strong>não é</strong> finalizar: o estado operacional não muda.</li>
          <li>É recusado se houver alterações, arquivos não rastreados ou ignorados, se estiver bloqueado, ou se for o principal. Nunca usa --force.</li>
        </ul>
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="button" className="button danger" disabled={busy} onClick={() => void run(() => api("worktree_git_remove", { projectId, path: git.path, confirmed: true }), "Worktree removido do Git; metadata preservada.").then((ok) => { if (ok) close(); })}>Remover do Git</button>
        </div>
      </div>
    </Modal>
  );
}

function LocateDialog({ item, candidates, busy, error, close, run }: FormProps & { item: WorktreeItem; candidates: WorktreeItem[] }) {
  const m = item.managed!;
  const [path, setPath] = useState(candidates[0]?.git?.path ?? "");
  async function submit(e: FormEvent) {
    e.preventDefault();
    if (await run(() => api("worktree_locate", { id: m.id, path }), "Worktree localizado nesta máquina.")) close();
  }
  return (
    <Modal title={`Localizar — ${m.displayName}`} close={close}>
      <form className="ddae-form" onSubmit={(e) => void submit(e)}>
        <p>Escolha o worktree real desta máquina que corresponde a este. O UUID não muda e nenhuma metadata nova é criada. {m.branchHint ? <>Branch esperada: <code>{m.branchHint}</code>.</> : null}</p>
        {candidates.length ? (
          <label>Worktree do Git
            <select value={path} onChange={(e) => setPath(e.target.value)}>
              {candidates.map((c) => <option key={c.git?.path} value={c.git?.path}>{itemName(c)} — {c.git?.path}</option>)}
            </select>
          </label>
        ) : <p className="muted">Nenhum worktree não gerenciado disponível nesta máquina para vincular.</p>}
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy || !path}>Localizar</button>
        </div>
      </form>
    </Modal>
  );
}

/**
 * Worktrees (Concept 08). Abrir a página é 100% LEITURA (git worktree list + git status): nada é
 * adotado, vinculado nem gravado sem uma ação explícita. O estado operacional é metadata do LKR LAB.
 */
export function WorktreesWorkspace({ projectId, notify }: { projectId: string; notify: (message: string) => void }) {
  const sources = workspace.forProject(projectId);
  const state = useResource(sources.worktreeOverview);
  const ddae = useResource(sources.ddae);
  const projects = useResource(workspace.projects);
  const overviews = useResource(workspace.overviews);
  const machine = useMachine().status?.machine;
  const project = projects.data.find((p) => p.id === projectId);
  const projectOverview = overviews.data?.projects.find((p) => p.id === projectId);
  const [filters, setFilters] = useState<Filters>({ query: "", operational: "all", git: "all" });
  const [dialog, setDialog] = useState<Dialog>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pageError, setPageError] = useState<string | null>(null);

  useEffect(() => {
    if (!desktop) return;
    void sources.worktreeOverview.refresh();
    void sources.ddae.refresh();
  }, [sources]);

  const overview = state.data;
  const sessions = ddae.data?.sessions ?? [];
  const items = useMemo(() => overview?.items ?? [], [overview]);
  const primary = items.find((i) => i.kind === "primary");
  const grid = useMemo(() => visibleItems(items, filters), [items, filters]);
  const unmanaged = items.filter((i) => i.kind === "unmanaged");
  const counts = overview?.counts;
  const activeSession = ddae.data?.sessions.find((s) => s.id === ddae.data?.activeSessionId) ?? null;
  const next = deriveDdaeNextAction(ddae.data);

  async function run(action: () => Promise<unknown>, message: string): Promise<boolean> {
    setBusy(true);
    setError(null);
    try {
      await action();
      await Promise.all([sources.worktreeOverview.refresh(), sources.worktreeSummary.refresh(), sources.ddae.refresh()]);
      notifyLocalChange();
      notify(message);
      return true;
    } catch (failure) {
      setError(errorText(failure));
      // Falha parcial (Git criou, metadata não): a próxima leitura mostra o worktree como NÃO GERENCIADO.
      void sources.worktreeOverview.refresh();
      return false;
    } finally {
      setBusy(false);
    }
  }
  const actions: Actions = {
    busy,
    open: (d) => { setError(null); setDialog(d); },
    launch: (item, action) => {
      if (!item.git) return;
      setPageError(null);
      void api("launch_worktree", { id: projectId, path: item.git.path, action }).catch((e: unknown) => setPageError(errorText(e)));
    },
  };
  const close = () => { setDialog(null); setError(null); };
  const formProps: FormProps = { busy, error, close, run };

  if (!desktop) {
    return <section className="panel"><Empty title="Disponível apenas no aplicativo desktop"><p>Os worktrees são lidos do Git desta máquina. Abra o aplicativo com npm run tauri dev.</p></Empty></section>;
  }
  if (!overview) {
    return (
      <section className="panel" aria-busy={state.status !== "error"}>
        {state.status === "error" ? (
          <Empty title="Não foi possível ler os worktrees"><p>{state.error}</p><button type="button" className="button" onClick={() => void sources.worktreeOverview.refresh()}>Tentar de novo</button></Empty>
        ) : <p className="muted">Lendo worktrees…</p>}
      </section>
    );
  }
  const runtime = projectOverview ? runtimeLabel(projectOverview).text : "—";
  const primaryGit = primary ? gitText(primary) : null;
  const filtered = filters.query || filters.operational !== "all" || filters.git !== "all";

  return (
    <div className="wt" aria-label="Worktrees">
      <div className="wt-head-row">
        <div>
          <span className="eyebrow">PROJETO / GIT</span>
          <h2>Worktrees</h2>
          <p className="muted">Organize ambientes isolados de desenvolvimento deste projeto</p>
        </div>
        <dl className="wt-context">
          <div><dt>Projeto</dt><dd>{project?.name ?? "—"}</dd></div>
          <div><dt>Máquina</dt><dd>{machine?.name ?? "—"}</dd></div>
          <div><dt>Branch principal</dt><dd className="mono">{primary?.git?.branch || "—"}</dd></div>
          <div><dt>Git</dt><dd>{primaryGit ? <span className={`tone-${primaryGit.tone}`}>{primaryGit.text}</span> : "—"}</dd></div>
        </dl>
        <div className="wt-head-actions">
          <button type="button" className="button" disabled={state.loading} onClick={() => void sources.worktreeOverview.refresh()}><RefreshCw size={14} className={state.loading ? "spin" : ""} /> Atualizar</button>
          <button type="button" className="button primary" disabled={!overview.projectAvailable || busy} onClick={() => actions.open({ kind: "new" })}><Plus size={14} /> Novo worktree</button>
        </div>
      </div>

      {!overview.projectAvailable && <p className="wt-banner" role="note">Projeto não localizado nesta máquina: não há Git para ler. Os worktrees gerenciados aparecem como não localizados.</p>}
      {overview.gitError && <p className="wt-banner is-error" role="alert">Não foi possível ler o Git: {overview.gitError}</p>}
      {pageError && <p className="wt-banner is-error" role="alert">{pageError}</p>}

      {counts && (
        <div className="wt-summaries">
          <section className="wt-summary"><Box size={20} /><div><strong>{counts.managed}</strong><span>Gerenciados</span></div></section>
          <section className="wt-summary is-active"><Zap size={20} /><div><strong>{counts.active}</strong><span>Ativos</span></div></section>
          <section className="wt-summary is-frozen"><Snowflake size={20} /><div><strong>{counts.frozen}</strong><span>Congelados</span></div></section>
          <section className="wt-summary is-stopped"><Pause size={20} /><div><strong>{counts.stopped}</strong><span>Parados</span></div></section>
          <section className="wt-summary is-completed"><CircleCheck size={20} /><div><strong>{counts.completed}</strong><span>Finalizados</span></div></section>
          <section className="wt-summary is-warn"><TriangleAlert size={20} /><div><strong>{counts.withChanges}</strong><span>com alterações Git</span></div></section>
        </div>
      )}
      {counts && (counts.missing > 0 || counts.unmanaged > 0) && (
        <p className="muted wt-hint">{counts.missing > 0 ? `${counts.missing} não localizado${counts.missing === 1 ? "" : "s"} nesta máquina` : ""}{counts.missing > 0 && counts.unmanaged > 0 ? " · " : ""}{counts.unmanaged > 0 ? `${counts.unmanaged} não gerenciado${counts.unmanaged === 1 ? "" : "s"} (Git sem metadata do LKR LAB)` : ""}. Os contadores acima contam só worktrees gerenciados; o principal fica fora.</p>
      )}

      <div className="wt-filters">
        <label className="ddae-search"><Search size={15} aria-hidden="true" /><input type="search" placeholder="Buscar worktrees…" aria-label="Buscar worktrees" value={filters.query} onChange={(e) => setFilters({ ...filters, query: e.target.value })} /></label>
        <div className="ddae-filters" role="group" aria-label="Filtrar por estado operacional">
          {OPERATIONAL_FILTERS.map((f) => <button key={f.id} type="button" className={`ddae-filter ${filters.operational === f.id ? "is-on" : ""}`} aria-pressed={filters.operational === f.id} onClick={() => setFilters({ ...filters, operational: f.id as OperationalFilter })}>{f.label}</button>)}
        </div>
        <div className="ddae-filters" role="group" aria-label="Filtrar por Git"><span className="muted wt-filter-label">Git:</span>
          {GIT_FILTERS.map((f) => <button key={f.id} type="button" className={`ddae-filter ${filters.git === f.id ? "is-on" : ""}`} aria-pressed={filters.git === f.id} onClick={() => setFilters({ ...filters, git: f.id as GitFilter })}>{f.label}</button>)}
        </div>
      </div>

      <div className="wt-grid">
        {showPrimary(primary, filters) && primary && <PrimaryCard item={primary} runtime={runtime} actions={actions} />}
        {grid.map((item) => <WorktreeCard key={item.managed?.id ?? item.git?.path} item={item} actions={actions} projectId={projectId} />)}
      </div>
      {grid.length === 0 && (
        <p className="muted wt-none">
          {filtered ? "Nenhum worktree corresponde à busca ou aos filtros." : "Este projeto ainda não tem worktrees além do checkout principal. Use “Novo worktree” ou adote um worktree existente."}
        </p>
      )}

      <div className="wt-footer">
        <section className="sd-card" aria-label="Session ativa">
          <header><h3>Session ativa</h3>{activeSession && <a className="text-button" href={projectHash(projectId, "ddae", activeSession.id)}>Ver no DDAE</a>}</header>
          {activeSession ? (
            <div className="sd-status">
              <div><small>{activeSession.label}</small><strong>{activeSession.title}</strong></div>
              <div><small>Bloco atual</small><strong>{activeSession.currentBlock?.title ?? "—"}</strong></div>
              <div><small>Progresso</small><strong>{activeSession.progress.completed} / {activeSession.progress.total}</strong></div>
              <div><small>Próximo</small><strong>{activeSession.nextBlock?.title ?? "—"}</strong></div>
            </div>
          ) : <p className="muted">Nenhuma sessão ativa neste projeto.</p>}
        </section>
        <section className="sd-card" aria-label="Próxima ação">
          <header><h3>Próxima ação</h3></header>
          {next && activeSession ? <><strong>{next.title}</strong><p className="muted">{next.description}</p><a className="button primary" href={projectHash(projectId, "ddae", activeSession.id)}>Abrir {activeSession.label}</a></> : <p className="muted">Nenhuma ação pendente na Session.</p>}
        </section>
      </div>

      {dialog?.kind === "new" && <NewDialog {...formProps} projectId={projectId} projectPath={project?.localPath ?? ""} sessions={sessions} />}
      {dialog?.kind === "adopt" && <AdoptDialog {...formProps} projectId={projectId} item={dialog.item} sessions={sessions} />}
      {dialog?.kind === "relation" && dialog.item.managed && <RelationDialog {...formProps} m={dialog.item.managed} sessions={sessions} />}
      {dialog?.kind === "state" && dialog.item.managed && <StateDialog {...formProps} item={dialog.item} action={dialog.action} />}
      {dialog?.kind === "rename" && dialog.item.managed && <RenameDialog {...formProps} m={dialog.item.managed} />}
      {dialog?.kind === "remove" && dialog.item.git && <RemoveDialog {...formProps} projectId={projectId} item={dialog.item} />}
      {dialog?.kind === "locate" && dialog.item.managed && <LocateDialog {...formProps} item={dialog.item} candidates={unmanaged} />}
    </div>
  );
}
