import { useState } from "react";
import { Bell, ChevronDown, Play, ShieldAlert, Square } from "lucide-react";
import { Badge } from "../shared/ui";
import {
  CONFIDENCE_LABEL,
  DEFAULT_ALERT_FILTER,
  DOMAIN_LABEL,
  SEVERITY_LABEL,
  SEVERITY_TONE,
  SOURCE_STATE_LABEL,
  STATUS_FILTER_LABEL,
  STATUS_LABEL,
  absolute,
  ago,
  ctaAction,
  diagnosticFor,
  diagnosticStatus,
  domainsOf,
  duration,
  filterAlerts,
  openCount,
  runLabel,
  runOutcome,
  summaryText,
  worstSeverity,
  type AlertFilter,
  type CtaAction,
  type StatusFilter,
} from "../shared/alerts";
import type { AlertRecord, AlertSeverity, AlertsSnapshot, DiagRun, DiagnosticsView } from "../shared/types";
import { liveActions, type AlertActions, type AlertsState } from "../state/alerts";

const SEVERITY_DOT = { critical: "danger", attention: "warn", info: "" } as const;
const RESULT_TONE = { clean: "good", problems_found: "warn", inconclusive: "neutral", failed: "warn", cancelled: "neutral" } as const;

function follow(action: CtaAction) {
  if (action.kind === "hash") window.location.hash = action.value;
  else document.getElementById(action.value)?.scrollIntoView({ behavior: "smooth", block: "start" });
}

function SeverityBadge({ severity }: { severity: AlertSeverity }) {
  return <Badge tone={SEVERITY_TONE[severity]}>{SEVERITY_LABEL[severity]}</Badge>;
}

/** Faixa compacta do Dashboard: contagens por severidade e o caminho para os diagnósticos. */
export function AlertsSummaryStrip({ state }: { state: AlertsState }) {
  const snapshot = state.snapshot;
  if (!snapshot) return null;
  const worst = worstSeverity(snapshot.summary);
  return (
    <section className={`panel mh-alerts-strip ${worst ?? "clear"}`} aria-label="Resumo de alertas">
      <span className="mh-alerts-strip-title"><Bell size={16} /><strong>Alertas</strong></span>
      <span className="mh-alerts-strip-counts">
        <span className={snapshot.summary.critical ? "critical" : ""}>{snapshot.summary.critical} {snapshot.summary.critical === 1 ? "crítico" : "críticos"}</span>
        <span className={snapshot.summary.attention ? "attention" : ""}>{snapshot.summary.attention} atenção</span>
        <span>{snapshot.summary.info} {snapshot.summary.info === 1 ? "informação" : "informações"}</span>
      </span>
      <button type="button" className="button" onClick={() => document.getElementById("alerts-diagnostics")?.scrollIntoView({ behavior: "smooth", block: "start" })}>
        Ver diagnósticos
      </button>
    </section>
  );
}

/** Alertas e diagnósticos: motor determinístico (sem IA nem pontuação) e diagnósticos só sob ação do usuário. */
export function AlertsPanel({ state, now, actions = liveActions }: { state: AlertsState; now: number; actions?: AlertActions }) {
  const { snapshot, error } = state;
  if (!snapshot) {
    return (
      <section className="panel mh-alerts" id="alerts-diagnostics" aria-label="Alertas e diagnósticos">
        <div className="panel-title"><h2><ShieldAlert size={17} />Alertas e diagnósticos</h2></div>
        <p className="muted" role="status">{error ?? "Avaliando a máquina…"}</p>
      </section>
    );
  }
  return <Loaded snapshot={snapshot} error={error} actionError={state.actionError} now={now} actions={actions} />;
}

function Loaded({ snapshot, error, actionError, now, actions }: { snapshot: AlertsSnapshot; error: string | null; actionError: string | null; now: number; actions: AlertActions }) {
  const [filter, setFilter] = useState<AlertFilter>(DEFAULT_ALERT_FILTER);
  const { summary, alerts, diagnostics } = snapshot;
  const shown = filterAlerts(alerts, filter);
  const worst = worstSeverity(summary);
  const evaluated = snapshot.sources.filter((s) => s.state === "evaluated").length;
  const notEvaluated = snapshot.sources.filter((s) => s.state !== "evaluated");
  return (
    <section className="panel mh-alerts" id="alerts-diagnostics" aria-label="Alertas e diagnósticos">
      <div className="panel-title">
        <h2><ShieldAlert size={17} />Alertas e diagnósticos</h2>
        <span className={`mh-win-overall status-${worst ?? "none"}`}>
          <span className={`dot ${worst ? SEVERITY_DOT[worst] : "good"}`} />
          {worst ? `${SEVERITY_LABEL[worst]} · ${openCount(summary)} aberto${openCount(summary) === 1 ? "" : "s"}` : "Nenhum alerta aberto"}
          <small>{evaluated} de {snapshot.sources.length} fontes avaliadas</small>
        </span>
      </div>
      {error && <p className="muted" role="status">A última atualização falhou ({error}); mostrando a leitura anterior.</p>}
      {actionError && <p className="mh-alert-error" role="alert">{actionError}</p>}

      <div className="mh-win-summary" aria-label="Resumo de alertas por severidade">
        <div className={`mh-win-item ${summary.critical ? "critical" : ""}`}><small>Críticos</small><strong>{summary.critical}</strong></div>
        <div className={`mh-win-item ${summary.attention ? "attention" : ""}`}><small>Atenção</small><strong>{summary.attention}</strong></div>
        <div className="mh-win-item"><small>Informações</small><strong>{summary.info}</strong></div>
        <div className="mh-win-item"><small>Resolvidos recentemente</small><strong>{summary.resolvedRecently}</strong></div>
      </div>
      <p className="muted" aria-label="Resumo em texto">{summaryText(summary)}{summary.acknowledged ? ` · ${summary.acknowledged} reconhecido${summary.acknowledged === 1 ? "" : "s"}` : ""}</p>

      <div className="mh-net-filters" aria-label="Filtros de alertas">
        <select aria-label="Filtrar por estado" value={filter.status} onChange={(e) => setFilter({ ...filter, status: e.target.value as StatusFilter })}>
          {(Object.keys(STATUS_FILTER_LABEL) as StatusFilter[]).map((key) => <option key={key} value={key}>{STATUS_FILTER_LABEL[key]}</option>)}
        </select>
        <select aria-label="Filtrar por severidade" value={filter.severity} onChange={(e) => setFilter({ ...filter, severity: e.target.value as AlertFilter["severity"] })}>
          <option value="all">Todas as severidades</option>
          {(["critical", "attention", "info"] as const).map((key) => <option key={key} value={key}>{SEVERITY_LABEL[key]}</option>)}
        </select>
        <select aria-label="Filtrar por domínio" value={filter.domain} onChange={(e) => setFilter({ ...filter, domain: e.target.value as AlertFilter["domain"] })}>
          <option value="all">Todos os domínios</option>
          {domainsOf(alerts).map((key) => <option key={key} value={key}>{DOMAIN_LABEL[key]}</option>)}
        </select>
      </div>

      {shown.length === 0 ? (
        <p className="muted" role="status">
          {alerts.length === 0
            ? evaluated === 0
              ? "Nenhuma fonte foi avaliada ainda."
              : "Nenhum alerta aberto. As regras não encontraram nada que exija atenção nas fontes avaliadas."
            : "Nenhum alerta corresponde ao filtro."}
        </p>
      ) : (
        <ul className="mh-alert-list" aria-label="Alertas">
          {shown.map((alert) => <AlertCard key={alert.alertId} alert={alert} now={now} diagnostics={diagnostics} actions={actions} />)}
        </ul>
      )}

      <DiagnosticsSection view={diagnostics} now={now} actions={actions} />

      {notEvaluated.length > 0 && (
        <details className="mh-win-missing">
          <summary>Fontes não avaliadas ({notEvaluated.length})</summary>
          <ul className="mh-win-list" aria-label="Fontes não avaliadas">
            {notEvaluated.map((source) => (
              <li key={source.id}><span>{source.label}</span><span className="muted">{SOURCE_STATE_LABEL[source.state]}{source.reason ? `: ${source.reason}` : ""}</span></li>
            ))}
          </ul>
          <p className="muted">Fonte sem dado, desatualizada ou que exige administrador não gera alerta: ausência de informação não é problema.</p>
        </details>
      )}
      <p className="footnote">
        Regras determinísticas e explicáveis, sem IA e sem pontuação. O LKR LAB não corrige nada automaticamente; “Reconhecer” só marca o alerta como visto, e diagnósticos rodam somente por ação sua e nunca reparam. Tudo fica só nesta máquina.
      </p>
    </section>
  );
}

function AlertCard({ alert, now, diagnostics, actions }: { alert: AlertRecord; now: number; diagnostics: DiagnosticsView; actions: AlertActions }) {
  const cta = ctaAction(alert.cta);
  const action = alert.diagnosticAction;
  const info = action ? diagnosticFor(diagnostics, action.id) : undefined;
  const status = action ? diagnosticStatus(info, diagnostics.current) : null;
  return (
    <li className={`mh-alertcard ${alert.severity} ${alert.status}`} aria-label={alert.title}>
      <div className="mh-alert-head">
        <SeverityBadge severity={alert.severity} />
        <strong>{alert.title}</strong>
        <Badge tone={alert.status === "resolved" ? "good" : "neutral"}>{STATUS_LABEL[alert.status]}</Badge>
        <small className="muted">{DOMAIN_LABEL[alert.domain]} · {CONFIDENCE_LABEL[alert.confidence]}</small>
      </div>
      <p>{alert.summary}</p>
      <p className="muted"><strong>Por quê:</strong> {alert.reason}</p>
      <dl className="mh-alert-evidence" aria-label="Evidência">
        {alert.evidence.map((item) => (
          <div key={`${item.label}-${item.value}`}><dt>{item.label}</dt><dd>{item.value}</dd></div>
        ))}
      </dl>
      <p className="mh-alert-step"><strong>O que verificar:</strong> {alert.recommendedNextStep}</p>
      <p className="muted mh-alert-times">
        Recurso <span className="mono">{alert.resource}</span>
        {" · "}primeira vez <span title={absolute(alert.firstSeen)}>{ago(alert.firstSeen, now)}</span>
        {" · "}última vez <span title={absolute(alert.lastSeen)}>{ago(alert.lastSeen, now)}</span>
        {alert.occurrenceCount > 1 ? ` · ocorrência ${alert.occurrenceCount}` : ""}
        {alert.status === "acknowledged" && alert.acknowledgedAt ? ` · reconhecido ${ago(alert.acknowledgedAt, now)}` : ""}
        {alert.status === "resolved" && alert.resolvedAt ? ` · resolvido ${ago(alert.resolvedAt, now)}` : ""}
      </p>
      <div className="mh-alert-actions">
        {cta && alert.status !== "resolved" && <button type="button" className="button" onClick={() => follow(cta)}>{cta.label}</button>}
        {action && status && alert.status !== "resolved" && (
          status.state === "available" ? (
            <button type="button" className="button" onClick={() => actions.start(action.id, action.target)}>
              <Play size={13} />Executar diagnóstico{action.target ? ` (${action.target})` : ""}
            </button>
          ) : status.state === "requires_elevation" ? (
            <span className="muted mh-alert-diag-note" title={status.reason}>Diagnóstico “{action.label}”: requer administrador</span>
          ) : status.state === "running" ? (
            <span className="muted mh-alert-diag-note">Um diagnóstico está em execução</span>
          ) : (
            <span className="muted mh-alert-diag-note">Diagnóstico indisponível: {status.reason}</span>
          )
        )}
        {alert.status === "active" && (
          <button type="button" className="button" onClick={() => actions.acknowledge(alert.alertId)} title="Marca o alerta como visto. Não altera a máquina nem resolve a condição.">
            Reconhecer
          </button>
        )}
      </div>
    </li>
  );
}

function RunSummary({ run, now }: { run: DiagRun; now: number }) {
  return (
    <div className={`mh-diag-run ${run.running ? "running" : run.result ?? ""}`} aria-label={`Execução: ${runLabel(run)}`}>
      <div className="mh-alert-head">
        <strong>{runLabel(run)}</strong>
        {run.running ? <Badge tone="blue">Em execução</Badge> : <Badge tone={run.result ? RESULT_TONE[run.result] : "neutral"}>{runOutcome(run)}</Badge>}
        <small className="muted">{duration(run.startedAt, run.finishedAt, now)}{run.exitCode != null ? ` · código ${run.exitCode}` : ""}</small>
      </div>
      <p>{run.summary}</p>
      {run.outputTail.length > 0 && (
        <details>
          <summary>{run.running ? "Saída parcial" : "Últimas linhas da saída"}</summary>
          <pre className="mh-diag-tail" aria-label="Saída">{run.outputTail.join("\n")}</pre>
          <small className="muted">A saída é local desta máquina e pode conter caminhos; só uma cauda curta é guardada.</small>
        </details>
      )}
    </div>
  );
}

function DiagnosticsSection({ view, now, actions }: { view: DiagnosticsView; now: number; actions: AlertActions }) {
  const [target, setTarget] = useState<string>(view.targets[0] ?? "");
  const current = view.current;
  const running = current?.running === true;
  return (
    <details className="mh-win-domain" id="diagnostics" aria-label="Diagnósticos" open={running || undefined}>
      <summary>
        <ChevronDown size={14} className="chev" aria-hidden="true" />
        <strong>Diagnósticos</strong>
        {running ? <Badge tone="blue">Em execução</Badge> : <span className="muted">somente leitura, sob ação sua</span>}
      </summary>
      <div className="mh-win-body">
        {!view.elevated && (
          <p className="muted" role="status">
            Os diagnósticos exigem administrador. O LKR LAB não solicita elevação automaticamente: abra o app como administrador para executá-los.
          </p>
        )}
        <ul className="mh-win-list" aria-label="Catálogo de diagnósticos">
          {view.catalog.map((info) => {
            const status = diagnosticStatus(info, current);
            return (
              <li key={info.id}>
                <span>
                  <strong>{info.label}</strong>{" "}
                  {status.state === "requires_elevation" && <Badge tone="warn">Requer administrador</Badge>}
                  {status.state === "running" && <Badge tone="blue">Outro em execução</Badge>}
                  {status.state === "unavailable" && <Badge>Indisponível</Badge>}
                </span>
                <span>{info.description}</span>
                {status.state === "available" && (
                  <span className="mh-alert-actions">
                    {info.needsTarget && (
                      <select aria-label="Volume" value={target} onChange={(e) => setTarget(e.target.value)}>
                        {view.targets.map((letter) => <option key={letter} value={letter}>{letter}</option>)}
                      </select>
                    )}
                    <button type="button" className="button" disabled={info.needsTarget && !target} onClick={() => actions.start(info.id, info.needsTarget ? target : null)}>
                      <Play size={13} />Executar
                    </button>
                  </span>
                )}
                {status.state !== "available" && status.state !== "running" && <small className="reason">{status.reason}</small>}
              </li>
            );
          })}
        </ul>
        {current && (
          <>
            <RunSummary run={current} now={now} />
            {running && (
              <button type="button" className="button" onClick={() => actions.cancel()}><Square size={13} />Cancelar diagnóstico</button>
            )}
          </>
        )}
        <h3 className="mh-diag-history-title">Histórico</h3>
        {view.history.length === 0 ? (
          <p className="muted">Nenhum diagnóstico foi executado nesta máquina.</p>
        ) : (
          <ul className="mh-win-list" aria-label="Histórico de diagnósticos">
            {view.history.map((run) => (
              <li key={run.id}>
                <span><strong>{runLabel(run)}</strong> <Badge tone={run.result ? RESULT_TONE[run.result] : "neutral"}>{runOutcome(run)}</Badge></span>
                <span>{absolute(run.startedAt)} · {duration(run.startedAt, run.finishedAt, now)}{run.exitCode != null ? ` · código ${run.exitCode}` : ""} — {run.summary}</span>
              </li>
            ))}
          </ul>
        )}
        <p className="muted">Nada é executado automaticamente. Reparos (como sfc /scannow, DISM /RestoreHealth ou chkdsk /f) não existem neste catálogo.</p>
      </div>
    </details>
  );
}
