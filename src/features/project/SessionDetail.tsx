import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { ArrowLeft, Check, Circle, CircleCheck, Copy, FileText, Pencil, Play } from "lucide-react";
import { SESSION_NOT_FOUND, projectHash } from "../../app/projectRoute";
import { api, desktop, errorText } from "../../shared/api";
import { contextStatus, deriveDdaeNextAction, progressFraction, statusBadge } from "../../shared/ddae";
import { PHASE_BADGE } from "../../shared/planning";
import { STATUS_LABEL } from "../../shared/worktrees";
import { DETAIL_TABS, criteriaSummary, eventLabel, finalizeStatus, lifecycleActions, recentEvents, referenceTitle, resolveSessionLoad, scopeProjection } from "../../shared/ddaeDetail";
import type { DetailTab } from "../../shared/ddaeDetail";
import { relativeTime } from "../../shared/projectOverview";
import { Empty, Modal } from "../../shared/ui";
import type { DdaeSessionContext, DdaeSessionView, SessionWorktree } from "../../shared/types";
import { notifyLocalChange } from "../../state/sync";
import { workspace } from "../../state/workspace";
import { ReasonDialog } from "./DdaeSessions";
import { BlocksTab, DecisionsTab, FilesTab, NotesTab, PlanTab } from "./SessionTabs";
import type { SessionOps } from "./SessionTabs";

const dateOf = (value: string) => (value ? new Date(value).toLocaleDateString("pt-BR") : "—");

function ProgressRing({ view }: { view: DdaeSessionView }) {
  const radius = 26;
  const circumference = 2 * Math.PI * radius;
  return (
    <svg className="ddae-ring" viewBox="0 0 64 64" role="img" aria-label={`${view.progress.completed} de ${view.progress.total} blocos concluídos`}>
      <circle cx="32" cy="32" r={radius} className="ddae-ring-track" />
      <circle cx="32" cy="32" r={radius} className="ddae-ring-value" strokeDasharray={`${progressFraction(view) * circumference} ${circumference}`} transform="rotate(-90 32 32)" />
    </svg>
  );
}

const Card = ({ title, action, children, className = "" }: { title: string; action?: ReactNode; children: ReactNode; className?: string }) => (
  <section className={`sd-card ${className}`}>
    <header><h3>{title}</h3>{action}</header>
    {children}
  </section>
);

function ContextDialog({ sessionId, close }: { sessionId: string; close: () => void }) {
  const [state, setState] = useState<{ context?: DdaeSessionContext; error?: string }>({});
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    let current = true;
    void api<DdaeSessionContext>("ddae_generate_context", { sessionId }).then(
      (context) => { if (current) setState({ context }); },
      (error: unknown) => { if (current) setState({ error: errorText(error) }); },
    );
    return () => { current = false; };
  }, [sessionId]);
  async function copy() {
    try {
      await navigator.clipboard.writeText(state.context?.markdown ?? "");
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* área de transferência bloqueada: sem efeito */
    }
  }
  const status = state.context ? contextStatus(state.context.readyForAi) : null;
  return (
    <Modal title="Contexto da Session" close={close} wide>
      <div className="ddae-form">
        <p className="muted">Gerado de forma determinística a partir do estado atual da sessão. Nada é enviado a nenhum serviço nem a nenhuma IA.</p>
        {status && <p className={`ddae-context is-${status.tone}`}><small>Contexto IA</small>{status.label}<span>{status.detail}</span></p>}
        {state.error && <p className="ddae-error" role="alert">{state.error}</p>}
        {state.context ? <pre className="sd-context">{state.context.markdown}</pre> : !state.error && <p className="muted">Gerando…</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Fechar</button>
          <button type="button" className="button primary" disabled={!state.context} onClick={() => void copy()}>
            {copied ? <Check size={14} /> : <Copy size={14} />} {copied ? "Copiado" : "Copiar"}
          </button>
        </div>
      </div>
    </Modal>
  );
}

function CompleteDialog({ view, busy, error, submit, close }: {
  view: DdaeSessionView; busy: boolean; error: string | null; submit: (result: string) => void; close: () => void;
}) {
  const [result, setResult] = useState("");
  return (
    <Modal title={`Finalizar ${view.label}`} close={close}>
      <form className="ddae-form" onSubmit={(e) => { e.preventDefault(); submit(result); }}>
        <p>Finalizar é terminal: a sessão não poderá ser reaberta nem alterada.</p>
        <label>
          Resultado <span className="muted">(opcional)</span>
          <textarea value={result} maxLength={2000} rows={3} autoFocus onChange={(e) => setResult(e.target.value)} />
        </label>
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy}>Finalizar</button>
        </div>
      </form>
    </Modal>
  );
}

function TitleDialog({ view, busy, error, submit, close }: {
  view: DdaeSessionView; busy: boolean; error: string | null; submit: (title: string) => void; close: () => void;
}) {
  const [title, setTitle] = useState(view.title);
  return (
    <Modal title="Editar título" close={close}>
      <form className="ddae-form" onSubmit={(e) => { e.preventDefault(); submit(title); }}>
        <p className="muted">O número ({view.label}) e a identidade interna da sessão não mudam.</p>
        <label>Título<input value={title} maxLength={120} autoFocus onChange={(e) => setTitle(e.target.value)} /></label>
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy || !title.trim() || title.trim() === view.title}>Salvar</button>
        </div>
      </form>
    </Modal>
  );
}


/** Visão geral (Concept 07): só projeções do estado real da Session. */
export function SessionOverview({ view, setTab, openContext, worktrees = [] }: { view: DdaeSessionView; setTab: (tab: DetailTab) => void; openContext: () => void; worktrees?: SessionWorktree[] }) {
  const next = deriveDdaeNextAction({ projectId: view.projectId, sessions: [view], counts: { total: 1, active: view.status === "active" ? 1 : 0, frozen: 0, stopped: 0, completed: 0 }, blocksTotal: view.blocks.length, activeSessionId: view.status === "active" ? view.id : null, legacyImport: "not_applicable" });
  const status = contextStatus(view.readyForAi);
  const scope = scopeProjection(view);
  const criteria = criteriaSummary(view.criteria);
  return (
    <div className="sd-overview">
      <Card title="Status operacional da sessão" className="sd-wide">
        <div className="sd-status">
          <div><small>Bloco atual</small><strong>{view.currentBlock?.title ?? "—"}</strong></div>
          <div><small>Status</small><strong>{view.currentBlock ? "Em andamento" : view.status === "completed" ? "Finalizada" : "Sem bloco em andamento"}</strong></div>
          <div className="sd-progress"><ProgressRing view={view} /><div><small>Progresso</small><strong>{view.progress.completed} / {view.progress.total}</strong><span>blocos concluídos</span></div></div>
          <div className="sd-counts">
            <span className="is-done">{scope.completed} concluídos</span>
            <span className="is-doing">{scope.inProgress} em andamento</span>
            <span className="is-todo">{scope.pending} pendentes</span>
          </div>
          <div><small>Próximo bloco</small><strong>{view.nextBlock?.title ?? "—"}</strong></div>
        </div>
      </Card>
      <Card title="Objetivo"><p>{view.objective || <span className="muted">Não informado.</span>}</p></Card>
      <Card title="Resultado desejado"><p>{view.desiredOutcome || <span className="muted">Não informado.</span>}</p></Card>
      <Card title="Decisões" action={<button type="button" className="text-button" onClick={() => setTab("decisions")}>Ver decisões</button>}>
        {view.decisions.length ? <ul className="sd-list">{view.decisions.slice(-5).reverse().map((d) => <li key={d.id}>{d.title}</li>)}</ul> : <p className="muted">Nenhuma decisão registrada.</p>}
      </Card>
      <Card title="Escopo da Session" action={<span className="muted">{scope.completed} concluídos · {scope.inProgress} em andamento · {scope.pending} pendentes</span>}>
        {view.blocks.length ? (
          <ul className="sd-scope">
            {view.blocks.map((b) => (
              <li key={b.id} className={`is-${b.status}`}>
                {b.status === "completed" ? <CircleCheck size={15} /> : <Circle size={15} />}
                <span>{b.title}</span>
                <em>{b.status === "completed" ? "Concluído" : b.status === "in_progress" ? "Em andamento" : "Pendente"}</em>
              </li>
            ))}
          </ul>
        ) : <p className="muted">Nenhum bloco ainda.</p>}
      </Card>
      <Card title="Critérios de conclusão" action={<span className="muted">{criteria.done} / {criteria.total}</span>}>
        {view.criteria?.length ? (
          <ul className="sd-scope">
            {view.criteria.map((c) => <li key={c.id} className={c.completed ? "is-completed" : "is-pending"}>{c.completed ? <CircleCheck size={15} /> : <Circle size={15} />}<span>{c.text}</span></li>)}
          </ul>
        ) : <p className="muted">Nenhum critério definido.</p>}
      </Card>
      <Card title="Restrições">
        {view.constraints?.length ? <ul className="sd-list">{view.constraints.map((c) => <li key={c}>{c}</li>)}</ul> : <p className="muted">Nenhuma restrição.</p>}
      </Card>
      <Card title="Contexto da Session" action={<button type="button" className="button" onClick={() => openContext()}><FileText size={14} /> Gerar contexto</button>}>
        <p className={`ddae-context is-${status.tone}`}><small>Contexto IA</small>{status.label}<span>{status.detail}</span></p>
        <p className="muted">O contexto é determinístico e gerado do estado atual; é separado dos dados estruturados da sessão.</p>
      </Card>
      <Card title="Arquivos e referências" action={<button type="button" className="text-button" onClick={() => setTab("files")}>Ver arquivos</button>}>
        {view.references?.length ? <ul className="sd-list">{view.references.slice(0, 4).map((r) => <li key={r.kind + r.value} className="mono">{referenceTitle(r)}</li>)}</ul> : <p className="muted">Nenhuma referência.</p>}
      </Card>
      {view.planningItem && (
        <Card title="Item de planejamento" action={<a className="text-button" href={projectHash(view.projectId, "planning")}>Ver Planejamento</a>}>
          <p><strong>{view.planningItem.title}</strong></p>
          <p className="muted">Fase derivada da Session: {PHASE_BADGE[view.planningItem.phase]}.</p>
        </Card>
      )}
      <Card title="Worktrees relacionados" action={<a className="text-button" href={projectHash(view.projectId, "worktrees")}>Ver worktrees</a>}>
          {worktrees.length ? (
            <ul className="sd-scope">
              {worktrees.map((w) => (
                <li key={w.id}>
                  <span>{w.displayName}{w.branchHint ? <em> {w.branchHint}</em> : null}{!w.available ? <em> · não localizado nesta máquina</em> : null}</span>
                  <em>{STATUS_LABEL[w.status]}</em>
                </li>
              ))}
            </ul>
          ) : <p className="muted">Nenhum worktree vinculado.</p>}
        </Card>
      <Card title="Histórico recente">
        {view.events?.length ? (
          <ul className="sd-history">
            {recentEvents(view.events).map((e) => <li key={e.id}><span>{eventLabel(e)}</span><time dateTime={e.createdAt}>{relativeTime(e.createdAt)}</time></li>)}
          </ul>
        ) : <p className="muted">Sem eventos registrados.</p>}
      </Card>
      <Card title="Próxima ação" className="sd-next">
        {next ? <><strong>{next.title}</strong><p className="muted">{next.description}</p></> : <p className="muted">{view.status === "active" ? "Nenhum bloco em andamento ou pendente." : "Sem ação pendente nesta sessão."}</p>}
        <div className="sd-actions">
          {next && <button type="button" className="button primary" onClick={() => setTab("blocks")}>Abrir blocos</button>}
          <button type="button" className="button" onClick={() => openContext()}><FileText size={14} /> Gerar contexto</button>
        </div>
      </Card>
    </div>
  );
}

type Dialog = "context" | "title" | "freeze" | "stop" | "complete" | null;

/**
 * Detalhe da Session (Concept 07): a MESMA Session DDAE do Concept 06, lida pelo par
 * (Project, Session) da rota. Nenhuma segunda store: tudo vem de \`ddae_session_detail\` e cada
 * mutação do backend devolve o estado que a tela relê.
 */
export function SessionDetail({ projectId, sessionId, notify }: { projectId: string; sessionId: string; notify: (message: string) => void }) {
  const [view, setView] = useState<DdaeSessionView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [worktrees, setWorktrees] = useState<SessionWorktree[]>([]);
  const [tab, setTab] = useState<DetailTab>("overview");
  const [dialog, setDialog] = useState<Dialog>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const next = await api<DdaeSessionView>("ddae_session_detail", { projectId, sessionId });
      setView(next);
      setLoadError(null);
      // Relações reais com worktrees (leve: banco, sem Git); falha aqui não esconde a Session.
      void api<SessionWorktree[]>("worktrees_for_session", { projectId, sessionId }).then(setWorktrees, () => setWorktrees([]));
    } catch (failure) {
      const message = errorText(failure);
      if (resolveSessionLoad({ ok: false, message }) === "redirect") {
        // Sessão inexistente ou de outro Project: nunca renderiza nem escolhe outra; volta para a lista.
        notify(SESSION_NOT_FOUND);
        window.location.replace(projectHash(projectId, "ddae"));
      } else {
        setLoadError(message);
      }
    }
  }, [projectId, sessionId, notify]);
  useEffect(() => {
    setView(null);
    setTab("overview");
    if (desktop) void load();
  }, [load]);

  /** Executa uma mudança do backend; relê o detalhe e a lista; devolve se deu certo. */
  const run = useCallback(async (action: () => Promise<unknown>, message: string): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      await action();
      await load();
      void workspace.forProject(projectId).ddae.refresh();
      void workspace.overviews.refresh();
      notifyLocalChange();
      if (message) notify(message);
      return true;
    } catch (failure) {
      setError(errorText(failure));
      return false;
    } finally {
      setBusy(false);
    }
  }, [load, notify, projectId]);

  if (!desktop) {
    return <section className="panel"><Empty title="Disponível apenas no aplicativo desktop"><p>O detalhe da sessão usa o banco local do LKR LAB.</p></Empty></section>;
  }
  if (loadError) {
    return (
      <section className="panel">
        <Empty title="Não foi possível carregar a sessão">
          <p>{loadError}</p>
          <button type="button" className="button" onClick={() => void load()}>Tentar de novo</button>
        </Empty>
      </section>
    );
  }
  if (!view) return <section className="panel" aria-busy="true"><p className="muted">Carregando sessão…</p></section>;

  const ops: SessionOps = { view, busy, error, run, setError };
  const actions = lifecycleActions(view);
  const finalize = finalizeStatus(view);
  const close = () => { setDialog(null); setError(null); };
  const confirm = async (action: () => Promise<unknown>, message: string) => { if (await run(action, message)) close(); };

  return (
    <div className="sd" aria-label={`Detalhe de ${view.label}`}>
      <a className="sd-back" href={projectHash(projectId, "ddae")}><ArrowLeft size={14} /> DDAE / Sessões</a>
      <header className="sd-head">
        <div className="sd-head-main">
          <span className="eyebrow">DDAE / SESSION</span>
          <h2>
            {view.label} <span className={`ddae-badge is-${view.status}`}><i aria-hidden="true" />{statusBadge(view.status)}</span>
          </h2>
          <p className="sd-title">
            {view.title}
            {view.status !== "completed" && (
              <button type="button" className="icon-button" aria-label="Editar título" onClick={() => { setError(null); setDialog("title"); }}><Pencil size={13} /></button>
            )}
          </p>
        </div>
        <dl className="sd-facts">
          <div><dt>Tipo</dt><dd>Feature</dd></div>
          <div><dt>Início</dt><dd>{dateOf(view.createdAt)}</dd></div>
          <div><dt>Última atualização</dt><dd>{relativeTime(view.updatedAt)}</dd></div>
        </dl>
        <div className="sd-actions">
          {actions.includes("resume") && <button type="button" className="button primary" disabled={busy} onClick={() => void run(() => api("ddae_resume", { sessionId }), `${view.label} retomada.`)}><Play size={14} /> Retomar</button>}
          {actions.includes("freeze") && <button type="button" className="button" disabled={busy} onClick={() => { setError(null); setDialog("freeze"); }}>Congelar</button>}
          {actions.includes("stop") && <button type="button" className="button" disabled={busy} onClick={() => { setError(null); setDialog("stop"); }}>Parar</button>}
          {actions.includes("complete") && (
            <button type="button" className="button" disabled={busy || !finalize.eligible} title={finalize.eligible ? undefined : finalize.reasons.join(" ")} onClick={() => { setError(null); setDialog("complete"); }}>Finalizar</button>
          )}
          <button type="button" className="button" onClick={() => setDialog("context")}><FileText size={14} /> Gerar contexto</button>
        </div>
      </header>
      {error && !dialog && <p className="ddae-error sd-banner" role="alert">{error}</p>}
      {view.status === "frozen" || view.status === "stopped" ? (
        <p className="sd-note" role="note">
          Sessão {view.status === "frozen" ? "congelada" : "parada"}{view.pauseReason ? ` — ${view.pauseReason}` : ""}.
        </p>
      ) : view.status === "completed" ? (
        <p className="sd-note" role="note">Sessão finalizada{view.result ? ` — ${view.result}` : ""}. Estado terminal: não pode ser reaberta nem alterada.</p>
      ) : null}

      <div className="sd-tabs" role="tablist" aria-label="Seções da sessão">
        {DETAIL_TABS.map((t) => (
          <button key={t.id} type="button" role="tab" id={`sd-tab-${t.id}`} aria-selected={tab === t.id} aria-controls="sd-panel" className={`sd-tab ${tab === t.id ? "is-on" : ""}`} onClick={() => setTab(t.id)}>
            {t.label}
          </button>
        ))}
      </div>

      <div id="sd-panel" role="tabpanel" aria-labelledby={`sd-tab-${tab}`} className="sd-panel">
        {tab === "overview" && <SessionOverview view={view} setTab={setTab} openContext={() => setDialog("context")} worktrees={worktrees} />}
        {tab === "blocks" && <BlocksTab ops={ops} />}
        {tab === "plan" && <PlanTab ops={ops} />}
        {tab === "decisions" && <DecisionsTab ops={ops} />}
        {tab === "files" && <FilesTab ops={ops} />}
        {tab === "notes" && <NotesTab ops={ops} />}
      </div>

      {dialog === "context" && <ContextDialog sessionId={sessionId} close={close} />}
      {dialog === "title" && (
        <TitleDialog view={view} busy={busy} error={error} close={close} submit={(title) => void confirm(() => api("ddae_update_details", { sessionId, details: { title, objective: view.objective, desiredOutcome: view.desiredOutcome ?? "", constraints: view.constraints ?? [], criteria: view.criteria ?? [], notes: view.notes ?? [], references: view.references ?? [] } }), "Título atualizado.")} />
      )}
      {(dialog === "freeze" || dialog === "stop") && (
        <ReasonDialog pending={{ kind: dialog, session: view }} busy={busy} error={error} close={close} submit={(reason) => void confirm(() => api(dialog === "freeze" ? "ddae_freeze" : "ddae_stop", { sessionId, reason }), dialog === "freeze" ? `${view.label} congelada.` : `${view.label} parada.`)} />
      )}
      {dialog === "complete" && (
        <CompleteDialog view={view} busy={busy} error={error} close={close} submit={(result) => void confirm(() => api("ddae_complete", { sessionId, result }), `${view.label} finalizada.`)} />
      )}
    </div>
  );
}

