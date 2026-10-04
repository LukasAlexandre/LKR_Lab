import { useEffect, useMemo, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { Ban, CircleCheck, CircleDot, Ellipsis, ExternalLink, FileText, Play, Plus, RefreshCw, RotateCcw, Search, Settings2, Target, Zap } from "lucide-react";
import { projectHash } from "../../app/projectRoute";
import { api, desktop, errorText } from "../../shared/api";
import { deriveDdaeNextAction } from "../../shared/ddae";
import { MOVE_LABEL, PHASE_BADGE, PLANNING_FILTERS, progressFraction, progressText, rowActions, subBadge, visibleRows } from "../../shared/planning";
import type { PlanningFilter, RowAction } from "../../shared/planning";
import { deriveProjectNextAction } from "../../shared/projectContext";
import { relativeTime } from "../../shared/projectOverview";
import { Empty, Modal } from "../../shared/ui";
import type { DdaeSession, PlanningMove, PlanningOverview, PlanningPhase, PlanningRow, PlanningStartDraft } from "../../shared/types";
import { notifyLocalChange } from "../../state/sync";
import { useResource, workspace } from "../../state/workspace";
import { NewSessionDialog } from "./DdaeSessions";

const PHASE_ICON: Record<PlanningPhase, typeof Target> = { planned: Target, executing: Play, completed: CircleCheck, cancelled: Ban };

type Dialog =
  | { kind: "new" }
  | { kind: "edit"; row: PlanningRow }
  | { kind: "cancel"; row: PlanningRow }
  | { kind: "start"; row: PlanningRow; draft: PlanningStartDraft }
  | null;

interface FormProps { busy: boolean; error: string | null; close: () => void }

function Bar({ row }: { row: PlanningRow }) {
  const fraction = progressFraction(row);
  return (
    <span className={`pl-bar is-${row.session?.status ?? "none"}`} role="img" aria-label={`${progressText(row)} blocos concluídos`}>
      <i style={{ width: `${Math.round(fraction * 100)}%` }} />
    </span>
  );
}

function PhaseBadge({ row }: { row: PlanningRow }) {
  const sub = subBadge(row);
  return (
    <span className="pl-states">
      <span className={`pl-badge is-${row.phase}`}><i aria-hidden="true" />{PHASE_BADGE[row.phase]}</span>
      {sub && <span className={`pl-sub is-${sub.status}`}>{sub.label}</span>}
    </span>
  );
}

function RowMenu({ row, up, act }: { row: PlanningRow; up: boolean; act: (action: RowAction, row: PlanningRow) => void }) {
  const actions = rowActions(row);
  if (actions.length === 0) return null;
  const label: Record<RowAction, string> = {
    edit: "Editar", up: MOVE_LABEL.up, down: MOVE_LABEL.down, top: MOVE_LABEL.top, cancel: "Cancelar", open: "Abrir sessão", restore: "Restaurar",
  };
  return (
    <details className={`wt-menu pl-menu ${up ? "is-up" : ""}`} onClick={(e) => { if ((e.target as HTMLElement).closest("button")) e.currentTarget.removeAttribute("open"); }}>
      <summary aria-label={`Mais ações de ${row.title}`}><Ellipsis size={16} /></summary>
      <div className="wt-menu-list">
        {actions.map((action) => (
          <button type="button" key={action} className={action === "cancel" ? "danger" : ""} onClick={() => act(action, row)}>{label[action]}</button>
        ))}
      </div>
    </details>
  );
}

function ItemDialog({ row, busy, error, close, submit }: FormProps & { row: PlanningRow | null; submit: (title: string, description: string) => void }) {
  const [title, setTitle] = useState(row?.title ?? "");
  const [description, setDescription] = useState(row?.description ?? "");
  return (
    <Modal title={row ? "Editar item" : "Novo item"} close={close}>
      <form className="ddae-form" onSubmit={(e: FormEvent) => { e.preventDefault(); submit(title, description); }}>
        <label>
          Título
          <input value={title} maxLength={120} autoFocus onChange={(e) => setTitle(e.target.value)} placeholder="Ex.: Configuração de ambientes por projeto" />
        </label>
        <label>
          Descrição <span className="muted">(opcional)</span>
          <textarea value={description} maxLength={2000} rows={4} onChange={(e) => setDescription(e.target.value)} />
        </label>
        <p className="muted">{row ? "A descrição vira o objetivo da Session quando o item for iniciado." : "Vai para o fim da fila. Objetivo, critérios e blocos são definidos na Session, ao iniciar."}</p>
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy || !title.trim()}>{row ? "Salvar" : "Criar item"}</button>
        </div>
      </form>
    </Modal>
  );
}

function CancelDialog({ row, busy, error, close, submit }: FormProps & { row: PlanningRow; submit: (reason: string) => void }) {
  const [reason, setReason] = useState("");
  return (
    <Modal title="Cancelar item" close={close}>
      <form className="ddae-form" onSubmit={(e: FormEvent) => { e.preventDefault(); submit(reason); }}>
        <p>Cancelar “{row.title}”? Ele sai da fila e do total operacional, mas continua no histórico e pode ser restaurado.</p>
        <label>
          Motivo <span className="muted">(opcional)</span>
          <textarea value={reason} maxLength={500} rows={3} autoFocus onChange={(e) => setReason(e.target.value)} />
        </label>
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Voltar</button>
          <button type="submit" className="button primary" disabled={busy}>Cancelar item</button>
        </div>
      </form>
    </Modal>
  );
}

function Summary({ icon, value, label, tone }: { icon: ReactNode; value: number; label: string; tone: string }) {
  return (
    <section className={`pl-summary is-${tone}`} aria-label={label}>
      <span className="pl-summary-icon" aria-hidden="true">{icon}</span>
      <div><strong>{value}</strong><span>{label}</span></div>
    </section>
  );
}

/**
 * Planejamento (Concept 09): a fila ordenada de features ainda não iniciadas. Abrir a página é 100%
 * LEITURA: nenhum item, evento, Session ou posição é criado/alterado sem uma ação explícita.
 * Planejado / Em execução / Concluído são derivados no backend a partir da Session vinculada.
 */
export function PlanningWorkspace({ projectId, notify }: { projectId: string; notify: (message: string) => void }) {
  const sources = workspace.forProject(projectId);
  const state = useResource(sources.planning);
  const ddae = useResource(sources.ddae);
  const overviews = useResource(workspace.overviews);
  const [filter, setFilter] = useState<PlanningFilter>("all");
  const [query, setQuery] = useState("");
  const [dialog, setDialog] = useState<Dialog>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pageError, setPageError] = useState<string | null>(null);

  useEffect(() => {
    if (!desktop) return;
    void sources.planning.refresh();
    void sources.ddae.refresh();
  }, [sources]);

  const overview: PlanningOverview | null = state.data;
  const rows = useMemo(() => overview?.items ?? [], [overview]);
  const shown = useMemo(() => visibleRows(rows, filter, query), [rows, filter, query]);
  /** Número da posição na fila COMPLETA (não muda com filtro ou busca). */
  const numbers = useMemo(() => new Map(rows.map((row, index) => [row.id, index + 1])), [rows]);
  const projectOverview = overviews.data?.projects.find((p) => p.id === projectId);
  const activeRef = overview?.activeSession ?? null;
  const activeSession = ddae.data?.sessions.find((s) => s.id === activeRef?.id) ?? null;
  const next = useMemo(() => {
    const planning = overview ? { counts: overview.counts, next: overview.next, activeSession: overview.activeSession } : null;
    return projectOverview ? deriveProjectNextAction(projectOverview, null, ddae.data, planning) : null;
  }, [projectOverview, ddae.data, overview]);
  const ddaeNext = deriveDdaeNextAction(ddae.data);

  async function refreshAll() {
    await Promise.all([sources.planning.refresh(), sources.planningSummary.refresh(), sources.ddae.refresh()]);
  }
  async function run(action: () => Promise<unknown>, message: string): Promise<boolean> {
    setBusy(true);
    setError(null);
    try {
      await action();
      await refreshAll();
      notifyLocalChange();
      notify(message);
      return true;
    } catch (failure) {
      setError(errorText(failure));
      void sources.planning.refresh();
      return false;
    } finally {
      setBusy(false);
    }
  }
  const close = () => { setDialog(null); setError(null); };

  async function openStart(row: PlanningRow) {
    setPageError(null);
    try {
      const draft = await api<PlanningStartDraft>("planning_prepare_start", { id: row.id });
      if (!draft.canStart) { setPageError(draft.disabledReason ?? "Este item não pode ser iniciado agora."); return; }
      setError(null);
      setDialog({ kind: "start", row, draft });
    } catch (failure) {
      setPageError(errorText(failure));
    }
  }
  function act(action: RowAction, row: PlanningRow) {
    setError(null);
    setPageError(null);
    if (action === "edit") setDialog({ kind: "edit", row });
    else if (action === "cancel") setDialog({ kind: "cancel", row });
    else if (action === "restore") void run(() => api("planning_restore", { id: row.id }), "Item restaurado.");
    else if (action === "open" && row.session) window.location.hash = projectHash(projectId, "ddae", row.session.id);
    else if (action === "up" || action === "down" || action === "top") {
      void run(() => api("planning_move", { id: row.id, to: action satisfies PlanningMove }), "Fila reordenada.");
    }
  }

  if (!desktop) {
    return <section className="panel"><Empty title="Disponível apenas no aplicativo desktop"><p>O Planejamento fica no banco local do LKR LAB. Abra o aplicativo com npm run tauri dev.</p></Empty></section>;
  }
  if (!overview) {
    return (
      <section className="panel" aria-busy={state.status !== "error"}>
        {state.status === "error" ? (
          <Empty title="Não foi possível carregar o Planejamento"><p>{state.error}</p><button type="button" className="button" onClick={() => void sources.planning.refresh()}>Tentar de novo</button></Empty>
        ) : <p className="muted">Carregando Planejamento…</p>}
      </section>
    );
  }

  const counts = overview.counts;
  const nextItem = overview.next;
  const empty = rows.length === 0;
  const lastIndex = shown.length - 1;
  const nextCta = (() => {
    if (!next) return null;
    if ((next.id === "continue-block" || next.id === "start-block") && activeRef) {
      return { label: `Continuar ${activeRef.label}`, href: projectHash(projectId, "ddae", activeRef.id) };
    }
    if (next.target.kind === "area") return { label: next.cta ?? "Abrir", href: projectHash(projectId, next.target.area) };
    return null;
  })();

  return (
    <div className="pl" aria-label="Planejamento">
      <div className="pl-head">
        <div>
          <span className="eyebrow">PROJETO</span>
          <h2>Planejamento</h2>
          <p className="muted">Organize o que vem a seguir e transforme planos em execução.</p>
        </div>
        <div className="pl-head-actions">
          <button type="button" className="button" disabled={state.loading} onClick={() => void sources.planning.refresh()}><RefreshCw size={14} className={state.loading ? "spin" : ""} /> Atualizar</button>
          <button type="button" className="button primary" disabled={busy} onClick={() => { setError(null); setDialog({ kind: "new" }); }}><Plus size={14} /> Novo item</button>
        </div>
      </div>

      {pageError && <p className="wt-banner is-error" role="alert">{pageError}</p>}

      <div className="pl-summaries">
        <Summary icon={<FileText size={22} />} value={counts.operational} label="Itens operacionais" tone="total" />
        <Summary icon={<Target size={22} />} value={counts.planned} label="Planejados" tone="planned" />
        <Summary icon={<Play size={22} />} value={counts.executing} label="Em execução" tone="executing" />
        <Summary icon={<CircleCheck size={22} />} value={counts.completed} label="Concluídos" tone="completed" />
        <Summary icon={<Ban size={22} />} value={counts.cancelled} label={counts.cancelled === 1 ? "Cancelado" : "Cancelados"} tone="cancelled" />
      </div>

      <section className="pl-next" aria-label="Próximo">
        <span className="pl-next-label">PRÓXIMO</span>
        <Settings2 size={26} aria-hidden="true" className="pl-next-icon" />
        {nextItem ? (
          <>
            <div className="pl-next-main">
              <strong>{nextItem.title}</strong>
              {nextItem.description && <span>{nextItem.description}</span>}
            </div>
            <div className="pl-tip">
              <button
                type="button"
                className="button primary"
                disabled={!nextItem.canStart || busy}
                aria-describedby={nextItem.disabledReason ? "pl-next-reason" : undefined}
                onClick={() => { const row = rows.find((r) => r.id === nextItem.id); if (row) void openStart(row); }}
              ><Play size={14} /> Iniciar</button>
              {nextItem.disabledReason && <span id="pl-next-reason" role="tooltip" className="pl-tip-bubble">{nextItem.disabledReason}</span>}
            </div>
          </>
        ) : (
          <div className="pl-next-main"><strong>Nenhum item planejado</strong><span>Adicione um item para definir o que vem a seguir.</span></div>
        )}
      </section>

      <div className="pl-filters">
        <label className="ddae-search"><Search size={15} aria-hidden="true" /><input type="search" placeholder="Buscar itens de planejamento…" aria-label="Buscar itens de planejamento" value={query} onChange={(e) => setQuery(e.target.value)} /></label>
        <div className="ddae-filters" role="group" aria-label="Filtrar por estado">
          {PLANNING_FILTERS.map((item) => (
            <button key={item.id} type="button" className={`ddae-filter ${filter === item.id ? "is-on" : ""}`} aria-pressed={filter === item.id} onClick={() => setFilter(item.id)}>{item.label}</button>
          ))}
        </div>
      </div>

      {empty ? (
        <section className="panel pl-empty">
          <Empty title="Nenhum item no planejamento.">
            <p>Anote a próxima feature em segundos: título e, se quiser, uma descrição curta.</p>
            <button type="button" className="button primary" disabled={busy} onClick={() => { setError(null); setDialog({ kind: "new" }); }}><Plus size={14} /> Novo item</button>
          </Empty>
        </section>
      ) : (
        <div className="pl-table-wrap">
          <table className="pl-table">
            <thead>
              <tr>
                <th scope="col" className="pl-col-n">#</th>
                <th scope="col" colSpan={2}>Item / Descrição</th>
                <th scope="col">Estado</th>
                <th scope="col">Session DDAE</th>
                <th scope="col">Última atualização</th>
                <th scope="col" className="pl-col-actions">Ações</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((row, index) => {
                const Icon = PHASE_ICON[row.phase];
                return (
                  <tr key={row.id} className={`is-${row.phase}`}>
                    <td className="pl-col-n">{String(numbers.get(row.id) ?? 0).padStart(2, "0")}</td>
                    <td className="pl-col-icon"><span className={`pl-icon is-${row.phase}`} aria-hidden="true"><Icon size={18} /></span></td>
                    <td className="pl-col-item">
                      <strong>{row.title}</strong>
                      {row.description && <span>{row.description}</span>}
                      {row.phase === "cancelled" && row.cancelReason && <span className="pl-reason">Motivo: {row.cancelReason}</span>}
                    </td>
                    <td><PhaseBadge row={row} /></td>
                    <td>
                      {row.session ? (
                        <span className="pl-session">
                          <i className={`pl-dot is-${row.session.status}`} aria-hidden="true" />
                          <span>
                            <a href={projectHash(projectId, "ddae", row.session.id)}>{row.session.label}</a>
                            {progressText(row) && <small>{progressText(row)}</small>}
                          </span>
                          <Bar row={row} />
                        </span>
                      ) : <span className="muted" aria-label="Sem Session">—</span>}
                    </td>
                    <td className="muted">{relativeTime(row.lastActivityAt)}</td>
                    <td className="pl-col-actions">
                      <span className="pl-actions">
                        {row.phase === "planned" && (
                          <span className="pl-tip">
                            <button type="button" className="button" disabled={!row.canStart || busy} aria-label={`Iniciar ${row.title}`} title={row.disabledReason ?? undefined} onClick={() => void openStart(row)}><Play size={14} /> Iniciar</button>
                          </span>
                        )}
                        {(row.phase === "executing" || row.phase === "completed") && row.session && (
                          <a className="button" href={projectHash(projectId, "ddae", row.session.id)} aria-label={`Abrir sessão de ${row.title}`}><ExternalLink size={14} /> Abrir sessão</a>
                        )}
                        {row.phase === "cancelled" && (
                          <button type="button" className="button" disabled={busy} aria-label={`Restaurar ${row.title}`} onClick={() => act("restore", row)}><RotateCcw size={14} /> Restaurar</button>
                        )}
                        <RowMenu row={row} up={index >= 3 && index === lastIndex} act={act} />
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {shown.length === 0 && <p className="muted pl-none">Nenhum item corresponde à busca ou ao filtro.</p>}
        </div>
      )}

      <div className="pl-footer">
        <section className="sd-card" aria-label="Session ativa">
          <header><h3><CircleDot size={14} className="pl-live" aria-hidden="true" /> Session ativa</h3></header>
          {activeRef && activeSession ? (
            <div className="pl-active">
              <div className="pl-active-main">
                <a href={projectHash(projectId, "ddae", activeSession.id)}><strong>{activeSession.label}</strong></a>
                <span className="muted">{activeSession.title}</span>
              </div>
              <div><small>Bloco atual</small><strong>{activeSession.currentBlock?.title ?? "—"}</strong></div>
              <div>
                <small>Progresso</small>
                <strong>{activeSession.progress.completed} / {activeSession.progress.total}</strong>
                <span className="pl-bar is-active" aria-hidden="true"><i style={{ width: `${activeSession.progress.total ? Math.round((activeSession.progress.completed / activeSession.progress.total) * 100) : 0}%` }} /></span>
              </div>
              <div><small>Próximo</small><strong>{activeSession.nextBlock?.title ?? "—"}</strong></div>
            </div>
          ) : <p className="muted">Nenhuma sessão ativa neste projeto.</p>}
        </section>
        <section className="sd-card" aria-label="Próxima ação">
          <header><h3><Zap size={14} aria-hidden="true" /> Próxima ação</h3></header>
          {next || ddaeNext ? (
            <>
              <strong>{next?.title ?? ddaeNext?.title}</strong>
              <p className="muted">{next?.description ?? ddaeNext?.description}</p>
              {nextCta && <a className="button primary" href={nextCta.href}><Play size={14} /> {nextCta.label}</a>}
            </>
          ) : <p className="muted">Nenhuma ação pendente.</p>}
        </section>
      </div>

      {dialog?.kind === "new" && (
        <ItemDialog row={null} busy={busy} error={error} close={close} submit={(title, description) => void run(async () => { await api("planning_create_item", { projectId, title, description }); setDialog(null); }, "Item criado.")} />
      )}
      {dialog?.kind === "edit" && (
        <ItemDialog row={dialog.row} busy={busy} error={error} close={close} submit={(title, description) => void run(async () => { await api("planning_update_item", { id: dialog.row.id, title, description }); setDialog(null); }, "Item atualizado.")} />
      )}
      {dialog?.kind === "cancel" && (
        <CancelDialog row={dialog.row} busy={busy} error={error} close={close} submit={(reason) => void run(async () => { await api("planning_cancel", { id: dialog.row.id, reason: reason.trim() || null }); setDialog(null); }, "Item cancelado.")} />
      )}
      {dialog?.kind === "start" && (
        <NewSessionDialog
          active={activeRef}
          busy={busy}
          error={error}
          close={close}
          initial={{ title: dialog.draft.title, objective: dialog.draft.objective }}
          note="Revise o título e o objetivo. Ao confirmar, a Session é criada já vinculada a este item; nada é criado antes disso."
          submit={(title, objective) => void run(async () => { await api<DdaeSession>("planning_start_session", { id: dialog.row.id, title, objective }); setDialog(null); }, "Session criada a partir do item.")}
        />
      )}
    </div>
  );
}
