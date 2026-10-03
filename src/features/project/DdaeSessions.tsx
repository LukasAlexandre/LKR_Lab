import { useEffect, useMemo, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { ArrowRight, Brain, ChevronRight, CircleAlert, CircleCheck, FileText, LayoutGrid, Layers, Play, Plus, Search, Snowflake, Target } from "lucide-react";
import { api, desktop, errorText } from "../../shared/api";
import {
  FILTERS,
  contextLine,
  deriveDdaeNextAction,
  filterCounts,
  progressFraction,
  progressText,
  statusBadge,
  summarize,
  visibleSessions,
} from "../../shared/ddae";
import type { DdaeFilter } from "../../shared/ddae";
import { relativeTime } from "../../shared/projectOverview";
import { Empty, Modal } from "../../shared/ui";
import type { DdaeOverview, DdaeSession, DdaeSessionStatus, DdaeSessionView } from "../../shared/types";
import { notifyLocalChange } from "../../state/sync";
import { useResource, workspace } from "../../state/workspace";

const STATUS_ICON: Record<DdaeSessionStatus, typeof Brain> = {
  active: Brain,
  frozen: Snowflake,
  stopped: CircleAlert,
  completed: CircleCheck,
};

/** Só a sessão ATIVA pulsa (CSS respeita prefers-reduced-motion). */
function StatusBadge({ status }: { status: DdaeSessionStatus }) {
  return (
    <span className={`ddae-badge is-${status}`}>
      <i aria-hidden="true" />
      {statusBadge(status)}
    </span>
  );
}

function StatusIcon({ status, size = 40 }: { status: DdaeSessionStatus; size?: number }) {
  const Icon = STATUS_ICON[status];
  return (
    <span className={`ddae-icon is-${status}`} style={{ width: size, height: size }} aria-hidden="true">
      <Icon size={Math.round(size * 0.5)} />
    </span>
  );
}

function SummaryCard({ icon, label, value, detail }: { icon: ReactNode; label: string; value: string; detail?: string | null }) {
  return (
    <section className="ddae-summary" aria-label={label}>
      <span className="ddae-summary-icon" aria-hidden="true">{icon}</span>
      <div>
        <small>{label}</small>
        <strong>{value}</strong>
        {detail ? <span>{detail}</span> : null}
      </div>
    </section>
  );
}

function ProgressRing({ session }: { session: DdaeSessionView }) {
  const radius = 26;
  const circumference = 2 * Math.PI * radius;
  const fraction = progressFraction(session);
  return (
    <svg className="ddae-ring" viewBox="0 0 64 64" role="img" aria-label={`${session.progress.completed} de ${session.progress.total} blocos concluídos`}>
      <circle cx="32" cy="32" r={radius} className="ddae-ring-track" />
      <circle cx="32" cy="32" r={radius} className="ddae-ring-value" strokeDasharray={`${fraction * circumference} ${circumference}`} transform="rotate(-90 32 32)" />
    </svg>
  );
}

type Pending =
  | { kind: "freeze" | "stop"; session: DdaeSessionView }
  | { kind: "complete-session"; session: DdaeSessionView }
  | { kind: "complete-block" | "start-block"; session: DdaeSessionView; blockId: string; title: string };

function ReasonDialog({ pending, busy, error, submit, close }: {
  pending: { kind: "freeze" | "stop"; session: DdaeSessionView };
  busy: boolean;
  error: string | null;
  submit: (reason: string) => void;
  close: () => void;
}) {
  const [reason, setReason] = useState("");
  const freezing = pending.kind === "freeze";
  return (
    <Modal title={`${freezing ? "Congelar" : "Parar"} ${pending.session.label}`} close={close}>
      <form className="ddae-form" onSubmit={(e: FormEvent) => { e.preventDefault(); submit(reason); }}>
        <p className="muted">
          {freezing
            ? "Congelar é aguardar algo externo. O bloco em andamento continua em andamento até a sessão ser retomada."
            : "Parar é encerrar o ritmo sem retomada imediata. Ela pode ser retomada depois."}
        </p>
        <label>
          Motivo
          <textarea value={reason} maxLength={500} rows={3} autoFocus onChange={(e) => setReason(e.target.value)} placeholder={freezing ? "Ex.: Aguardando revisão do time" : "Ex.: Sem previsão de retomada"} />
        </label>
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy || !reason.trim()}>{freezing ? "Congelar" : "Parar"}</button>
        </div>
      </form>
    </Modal>
  );
}

function ConfirmDialog({ title, children, confirm, busy, error, submit, close }: {
  title: string;
  children: ReactNode;
  confirm: string;
  busy: boolean;
  error: string | null;
  submit: () => void;
  close: () => void;
}) {
  return (
    <Modal title={title} close={close}>
      <div className="ddae-form">
        {children}
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="button" className="button primary" disabled={busy} onClick={submit}>{confirm}</button>
        </div>
      </div>
    </Modal>
  );
}

function NewSessionDialog({ active, busy, error, submit, close }: {
  active: DdaeSessionView | null;
  busy: boolean;
  error: string | null;
  submit: (title: string, objective: string) => void;
  close: () => void;
}) {
  const [title, setTitle] = useState("");
  const [objective, setObjective] = useState("");
  return (
    <Modal title="Nova sessão" close={close}>
      <form className="ddae-form" onSubmit={(e: FormEvent) => { e.preventDefault(); submit(title, objective); }}>
        {active && (
          <p className="ddae-warning" role="note">
            {active.label} já está ativa neste projeto. Congele ou pare a sessão ativa antes de criar outra.
          </p>
        )}
        <label>
          Título
          <input value={title} maxLength={120} autoFocus onChange={(e) => setTitle(e.target.value)} placeholder="Ex.: Auto Update Foundation" />
        </label>
        <label>
          Objetivo <span className="muted">(opcional)</span>
          <textarea value={objective} maxLength={4000} rows={4} onChange={(e) => setObjective(e.target.value)} />
        </label>
        <p className="muted">Uma sessão é uma feature do projeto e nasce ativa. Os blocos são adicionados depois.</p>
        {error && <p className="ddae-error" role="alert">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="button" onClick={close}>Cancelar</button>
          <button type="submit" className="button primary" disabled={busy || !title.trim() || !!active}>Criar sessão</button>
        </div>
      </form>
    </Modal>
  );
}

/**
 * DDAE / Sessões (Concept 06): lista real de Sessions do Project com busca, filtros por estado e
 * preview (master/detail). Só dados do backend; nenhuma sessão de exemplo.
 */
export function DdaeSessions({ projectId, notify }: { projectId: string; notify: (message: string) => void }) {
  const source = workspace.forProject(projectId).ddae;
  const state = useResource(source);
  const [filter, setFilter] = useState<DdaeFilter>("all");
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [pending, setPending] = useState<Pending | null>(null);
  const [busy, setBusy] = useState(false);
  const [dialogError, setDialogError] = useState<string | null>(null);

  useEffect(() => {
    if (desktop) void source.refresh();
  }, [source]);

  const overview: DdaeOverview | null = state.data;
  const sessions = useMemo(() => overview?.sessions ?? [], [overview]);
  const counts = useMemo(() => filterCounts(sessions), [sessions]);
  const visible = useMemo(() => visibleSessions(sessions, filter, query), [sessions, filter, query]);
  const summary = overview ? summarize(overview) : null;
  const next = deriveDdaeNextAction(overview);
  const selected = sessions.find((s) => s.id === selectedId) ?? visible.find((s) => s.id === overview?.activeSessionId) ?? visible[0] ?? null;

  /** Executa uma mudança do backend; devolve o erro (ou null em caso de sucesso). */
  async function perform(action: () => Promise<unknown>, message: string): Promise<string | null> {
    setBusy(true);
    setDialogError(null);
    try {
      await action();
      await source.refresh();
      notifyLocalChange();
      void workspace.overviews.refresh();
      notify(message);
      return null;
    } catch (error) {
      const text = errorText(error);
      setDialogError(text);
      return text;
    } finally {
      setBusy(false);
    }
  }
  const close = () => { setCreating(false); setPending(null); setDialogError(null); };

  async function create(title: string, objective: string) {
    const made: { session?: DdaeSession } = {};
    const failure = await perform(async () => { made.session = await api<DdaeSession>("ddae_create_session", { projectId, title, objective }); }, "Sessão criada.");
    if (!failure) {
      if (made.session) setSelectedId(made.session.id);
      setFilter("all");
      setQuery("");
      close();
    }
  }
  async function confirm(change: Pending, reason?: string) {
    const id = change.session.id;
    const run: Record<Pending["kind"], () => Promise<unknown>> = {
      freeze: () => api("ddae_freeze", { sessionId: id, reason: reason ?? "" }),
      stop: () => api("ddae_stop", { sessionId: id, reason: reason ?? "" }),
      "complete-session": () => api("ddae_complete", { sessionId: id, result: "" }),
      "start-block": () => api("ddae_start_block", { sessionId: id, blockId: "blockId" in change ? change.blockId : "" }),
      "complete-block": () => api("ddae_complete_block", { sessionId: id, blockId: "blockId" in change ? change.blockId : "" }),
    };
    const done: Record<Pending["kind"], string> = {
      freeze: `${change.session.label} congelada.`,
      stop: `${change.session.label} parada.`,
      "complete-session": `${change.session.label} finalizada.`,
      "start-block": "Bloco iniciado.",
      "complete-block": "Bloco concluído.",
    };
    if (!(await perform(run[change.kind], done[change.kind]))) close();
  }
  async function resume(session: DdaeSessionView) {
    const failure = await perform(() => api("ddae_resume", { sessionId: session.id }), `${session.label} retomada.`);
    if (failure) notify(failure);
  }

  if (!desktop) {
    return (
      <section className="panel">
        <Empty title="Disponível apenas no aplicativo desktop">
          <p>As sessões DDAE ficam no banco local do LKR LAB. Abra o aplicativo com npm run tauri dev.</p>
        </Empty>
      </section>
    );
  }
  if (!overview) {
    return (
      <section className="panel" aria-busy={state.status !== "error"}>
        {state.status === "error" ? (
          <Empty title="Não foi possível carregar o DDAE">
            <p>{state.error}</p>
            <button type="button" className="button" onClick={() => void source.refresh()}>Tentar de novo</button>
          </Empty>
        ) : (
          <p className="muted">Carregando sessões…</p>
        )}
      </section>
    );
  }

  const empty = sessions.length === 0;
  const activeView = summary?.active ?? null;

  return (
    <div className="ddae" aria-label="DDAE / Sessões">
      <div className="ddae-head">
        <div>
          <span className="eyebrow">PROJETO</span>
          <h2>DDAE / Sessões</h2>
          <p className="muted">Gerencie as sessões de desenvolvimento deste projeto</p>
        </div>
        <button type="button" className="button primary" onClick={() => { setDialogError(null); setCreating(true); }}>
          <Plus size={15} /> Nova sessão
        </button>
      </div>

      {overview.legacyImport === "imported" && (
        <p className="ddae-info" role="status">A sessão histórica SESSION-001 foi importada da documentação do projeto para o banco local.</p>
      )}

      {summary && (
        <div className="ddae-summaries">
          <SummaryCard icon={<Layers size={20} />} label="Sessões totais" value={String(summary.total)} detail={summary.breakdown} />
          <SummaryCard icon={<Play size={20} />} label="Sessão ativa" value={activeView ? activeView.label : "Nenhuma"} detail={activeView ? (activeView.currentBlock?.title ?? "Sem bloco em andamento") : "Nenhuma sessão ativa"} />
          <SummaryCard icon={<LayoutGrid size={20} />} label="Blocos (total)" value={String(summary.blocksTotal)} detail={summary.blocksInActive} />
          <SummaryCard icon={<Target size={20} />} label="Próximo bloco" value={summary.nextBlock ?? "—"} detail={activeView ? activeView.label : "Sem sessão ativa"} />
        </div>
      )}

      {empty ? (
        <section className="panel">
          <Empty title="Nenhuma sessão neste projeto">
            <p>Crie a primeira sessão para organizar uma feature em blocos.</p>
          </Empty>
        </section>
      ) : (
        <div className="ddae-body">
          <div className="ddae-list-col">
            <label className="ddae-search">
              <Search size={15} aria-hidden="true" />
              <input type="search" value={query} placeholder="Buscar sessões…" aria-label="Buscar sessões" onChange={(e) => setQuery(e.target.value)} />
            </label>
            <div className="ddae-filters" role="group" aria-label="Filtrar por estado">
              {FILTERS.map((item) => (
                <button key={item.id} type="button" className={`ddae-filter ${filter === item.id ? "is-on" : ""}`} aria-pressed={filter === item.id} onClick={() => setFilter(item.id)}>
                  {item.label} <span>{counts[item.id]}</span>
                </button>
              ))}
            </div>
            <ul className="ddae-list">
              {visible.map((s) => {
                const line = contextLine(s);
                return (
                  <li key={s.id}>
                    <button type="button" className={`ddae-item is-${s.status} ${selected?.id === s.id ? "is-selected" : ""}`} aria-current={selected?.id === s.id ? "true" : undefined} onClick={() => setSelectedId(s.id)}>
                      <StatusIcon status={s.status} />
                      <span className="ddae-item-main">
                        <span className="ddae-item-top">
                          <strong>{s.label}</strong>
                          <StatusBadge status={s.status} />
                        </span>
                        <span className="ddae-item-title">{s.title}</span>
                        <span className="ddae-item-meta">{progressText(s)} · Atualizada {relativeTime(s.updatedAt).toLowerCase()}</span>
                        {line && <span className="ddae-item-line">{line}</span>}
                      </span>
                      <ChevronRight size={16} aria-hidden="true" />
                    </button>
                  </li>
                );
              })}
            </ul>
            {visible.length === 0 && (
              <p className="muted ddae-none">Nenhuma sessão corresponde à busca ou ao filtro.</p>
            )}
          </div>

          {selected && (
            <aside className="ddae-preview" aria-label={`Prévia de ${selected.label}`}>
              <div className="ddae-preview-head">
                <StatusIcon status={selected.status} size={46} />
                <div>
                  <h3>{selected.label}</h3>
                  <p>{selected.title}</p>
                </div>
                <StatusBadge status={selected.status} />
              </div>

              <div className="ddae-box">
                <FileText size={18} aria-hidden="true" />
                <div>
                  <small>Objetivo</small>
                  <p>{selected.objective || "Sem objetivo registrado."}</p>
                </div>
              </div>

              <div className="ddae-trio">
                <div className="ddae-box">
                  <div>
                    <small>Bloco atual</small>
                    <strong>{selected.currentBlock?.title ?? "—"}</strong>
                  </div>
                </div>
                <div className="ddae-box">
                  <ProgressRing session={selected} />
                  <div>
                    <small>Progresso</small>
                    <strong>{selected.progress.completed} / {selected.progress.total}</strong>
                    <span>blocos concluídos</span>
                  </div>
                </div>
                <div className="ddae-box">
                  <div>
                    <small>Próximo bloco</small>
                    <strong>{selected.nextBlock?.title ?? "—"}</strong>
                  </div>
                </div>
              </div>

              <dl className="ddae-facts">
                <div><dt>Início</dt><dd>{new Date(selected.createdAt).toLocaleDateString("pt-BR")}</dd></div>
                <div><dt>Última atualização</dt><dd>{relativeTime(selected.updatedAt)}</dd></div>
                <div><dt>Tipo</dt><dd>Feature</dd></div>
              </dl>

              <div className="ddae-counts">
                <span className="is-done">{selected.blocks.filter((b) => b.status === "completed").length} concluídos</span>
                <span className="is-doing">{selected.blocks.filter((b) => b.status === "in_progress").length} em andamento</span>
                <span className="is-todo">{selected.blocks.filter((b) => b.status === "pending").length} pendentes</span>
              </div>

              {selected.recentDecision && (
                <p className="ddae-decision"><small>Decisão recente</small>{selected.recentDecision.title}</p>
              )}
              {contextLine(selected) && <p className="ddae-reason">{contextLine(selected)}</p>}

              {/* Abrir sessão leva ao Concept 07 (Detalhe da Sessão), ainda não implementado. */}
              <button type="button" className="button primary ddae-open" disabled title="O detalhe da sessão (Concept 07) ainda não foi implementado.">
                <Play size={15} /> Abrir sessão
              </button>

              <div className="ddae-actions" role="group" aria-label={`Ações de ${selected.label}`}>
                {selected.status === "active" && (
                  <>
                    {selected.currentBlock ? (
                      <button type="button" className="button" disabled={busy} onClick={() => { setDialogError(null); setPending({ kind: "complete-block", session: selected, blockId: selected.currentBlock!.id, title: selected.currentBlock!.title }); }}>Concluir bloco</button>
                    ) : selected.nextBlock ? (
                      <button type="button" className="button" disabled={busy} onClick={() => { setDialogError(null); setPending({ kind: "start-block", session: selected, blockId: selected.nextBlock!.id, title: selected.nextBlock!.title }); }}>Iniciar próximo bloco</button>
                    ) : null}
                    <button type="button" className="button" disabled={busy} onClick={() => { setDialogError(null); setPending({ kind: "freeze", session: selected }); }}>Congelar</button>
                    <button type="button" className="button" disabled={busy} onClick={() => { setDialogError(null); setPending({ kind: "stop", session: selected }); }}>Parar</button>
                    <button type="button" className="button" disabled={busy || !selected.canComplete} title={selected.canComplete ? undefined : "Finalizar exige todos os blocos concluídos e nenhum em andamento."} onClick={() => { setDialogError(null); setPending({ kind: "complete-session", session: selected }); }}>Finalizar</button>
                  </>
                )}
                {(selected.status === "frozen" || selected.status === "stopped") && (
                  <button type="button" className="button" disabled={busy || (!!activeView && activeView.id !== selected.id)} title={activeView && activeView.id !== selected.id ? `${activeView.label} já está ativa.` : undefined} onClick={() => void resume(selected)}>Retomar</button>
                )}
              </div>
            </aside>
          )}

          <section className="ddae-next" aria-label="Próxima ação">
            <div>
              <h3>Próxima ação</h3>
              {next ? (
                <>
                  <strong>{next.title}</strong>
                  <p className="muted">{next.description}</p>
                </>
              ) : (
                <p className="muted">{activeView ? `${activeView.label} não tem blocos pendentes ou em andamento.` : "Nenhuma sessão ativa. Crie uma nova ou retome uma sessão congelada ou parada."}</p>
              )}
            </div>
            {next && activeView && (
              <button
                type="button"
                className="button primary"
                disabled={busy}
                onClick={() => {
                  setDialogError(null);
                  setPending(next.kind === "continue"
                    ? { kind: "complete-block", session: activeView, blockId: next.blockId, title: activeView.currentBlock?.title ?? "" }
                    : { kind: "start-block", session: activeView, blockId: next.blockId, title: activeView.nextBlock?.title ?? "" });
                }}
              >
                {next.kind === "continue" ? "Concluir bloco" : "Iniciar bloco"} <ArrowRight size={15} />
              </button>
            )}
          </section>
        </div>
      )}

      {creating && (
        <NewSessionDialog active={activeView} busy={busy} error={dialogError} submit={(t, o) => void create(t, o)} close={close} />
      )}
      {pending && (pending.kind === "freeze" || pending.kind === "stop") && (
        <ReasonDialog pending={pending} busy={busy} error={dialogError} submit={(reason) => void confirm(pending, reason)} close={close} />
      )}
      {pending && pending.kind === "complete-session" && (
        <ConfirmDialog title={`Finalizar ${pending.session.label}`} confirm="Finalizar" busy={busy} error={dialogError} submit={() => void confirm(pending)} close={close}>
          <p>Finalizar é terminal: a sessão não poderá ser reaberta nem alterada.</p>
        </ConfirmDialog>
      )}
      {pending && pending.kind === "complete-block" && (
        <ConfirmDialog title="Concluir bloco" confirm="Concluir" busy={busy} error={dialogError} submit={() => void confirm(pending)} close={close}>
          <p>Concluir “{pending.title}”? O próximo bloco não é iniciado automaticamente.</p>
        </ConfirmDialog>
      )}
      {pending && pending.kind === "start-block" && (
        <ConfirmDialog title="Iniciar bloco" confirm="Iniciar" busy={busy} error={dialogError} submit={() => void confirm(pending)} close={close}>
          <p>Iniciar “{pending.title}” em {pending.session.label}?</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
