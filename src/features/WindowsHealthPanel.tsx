import type { ReactNode } from "react";
import { AlertTriangle, ChevronDown, Stethoscope } from "lucide-react";
import { Badge } from "../shared/ui";
import {
  DOMAIN_LABEL,
  START_TYPE_LABEL,
  SERVICE_STATE_LABEL,
  UNKNOWN_TEXT,
  WIN_HEALTH_LABEL,
  checkedLabel,
  countOrDash,
  formatDateTime,
  isStale,
  restartLabel,
  showBadge,
  sourceText,
  summaryItems,
  unavailableSources,
  updateSummary,
} from "../shared/windowsHealth";
import { formatUptime } from "../shared/telemetry";
import type { WinHealth, WinSection, WindowsHealthSnapshot } from "../shared/types";
import type { WindowsHealthState } from "../state/windowsHealth";

const STATUS_DOT = { healthy: "good", attention: "warn", critical: "danger", unknown: "" } as const;
/** O `Badge` não tem tom "danger": crítico usa o mesmo tom de atenção, com o rótulo "Crítico". */
const BADGE_TONE = { healthy: "good", attention: "warn", critical: "warn", unknown: "neutral" } as const;

function Status({ status }: { status: WinHealth }) {
  return <Badge tone={BADGE_TONE[status]}>{WIN_HEALTH_LABEL[status]}</Badge>;
}

/** Um domínio expansível: selo de saúde, motivo visível, fontes e quando foi lido. */
function Domain({ title, section, now, summary, children }: {
  title: string;
  section: WinSection<object>;
  now: number;
  summary?: string;
  children?: ReactNode;
}) {
  const stale = isStale(section, now);
  return (
    <details className="mh-win-domain" aria-label={title}>
      <summary>
        <ChevronDown size={14} className="chev" aria-hidden="true" />
        <strong>{title}</strong>
        {showBadge(section) && <Status status={section.status} />}
        {summary && <span className="muted">{summary}</span>}
        {stale && <small className="mh-stale" title="A leitura passou do dobro da validade deste domínio.">desatualizado</small>}
      </summary>
      <div className="mh-win-body">
        {section.reasons.length > 0 && (
          <ul className={`mh-win-reasons ${section.status}`} aria-label="Motivo">
            {section.reasons.map((reason) => <li key={reason}>{reason}</li>)}
          </ul>
        )}
        {children}
        {section.sources.filter((s) => s.state !== "available").map((source) => (
          <p className="muted mh-win-source" key={source.id}>{source.label}: {sourceText(source)}.</p>
        ))}
        <small className="muted">Última leitura {checkedLabel(section.checkedAt, now)}.</small>
      </div>
    </details>
  );
}

const when = (ms: number | null | undefined) => formatDateTime(ms) ?? UNKNOWN_TEXT;

/** Windows Health & Integrity: somente observação (nenhum reparo, reinício ou instalação). */
export function WindowsHealthPanel({ state, now }: { state: WindowsHealthState; now: number }) {
  const { snapshot, error } = state;
  if (!snapshot) {
    return (
      <section className="panel mh-winhealth" aria-label="Windows Health">
        <div className="panel-title"><h2><Stethoscope size={17} />Windows Health</h2></div>
        <p className="muted" role="status">{error ?? "Lendo o estado do Windows…"}</p>
      </section>
    );
  }
  return <Loaded snapshot={snapshot} error={error} now={now} />;
}

function Loaded({ snapshot: s, error, now }: { snapshot: WindowsHealthSnapshot; error: string | null; now: number }) {
  const overall = s.overall;
  const missing = unavailableSources(s.capabilities);
  const system = s.system;
  return (
    <section className="panel mh-winhealth" aria-label="Windows Health">
      <div className="panel-title">
        <h2><Stethoscope size={17} />Windows Health</h2>
        <span className={`mh-win-overall status-${overall.status}`}>
          <span className={`dot ${STATUS_DOT[overall.status]}`} />
          {WIN_HEALTH_LABEL[overall.status]}
          <small>{overall.evaluated} de {overall.rateable} domínios avaliados</small>
        </span>
      </div>

      {overall.reasons.length > 0 && (
        <ul className={`mh-win-reasons ${overall.status}`} role="alert" aria-label="Por que este estado">
          {overall.reasons.map((reason) => (
            <li key={`${reason.domain}-${reason.text}`}>
              <AlertTriangle size={14} />
              <span><strong>{DOMAIN_LABEL[reason.domain] ?? reason.domain}:</strong> {reason.text}</span>
            </li>
          ))}
        </ul>
      )}
      {overall.status === "unknown" && (
        <p className="muted" role="status">Não foi possível avaliar nenhum domínio com segurança; nada é tratado como saudável.</p>
      )}
      {error && <p className="muted" role="status">A última atualização falhou ({error}); mostrando a leitura anterior.</p>}

      <div className="mh-win-summary" aria-label="Resumo">
        {summaryItems(s).map((item) => (
          <div key={item.id} className={`mh-win-item ${item.status ?? ""}`}>
            <small>{item.label}</small>
            <strong>{item.value}</strong>
          </div>
        ))}
      </div>

      <p className="mh-win-system">
        <strong>{system.productName ?? "Windows"}</strong>
        {" "}
        <span className="muted">
          {[system.edition, system.version && `versão ${system.version}`, system.build && `build ${system.build}`, system.architecture].filter(Boolean).join(" · ") || UNKNOWN_TEXT}
          {system.uptimeSecs ? ` · ligado há ${formatUptime(system.uptimeSecs)}` : ""}
        </span>
      </p>

      <Domain title="Reinício pendente" section={s.restart} now={now} summary={restartLabel(s.restart.pending)}>
        <p className="muted">
          {s.restart.pending === false && "Nenhuma fonte indica reinício pendente."}
          {s.restart.pending === true && "O Windows pede um reinício. Nada é reiniciado automaticamente."}
          {s.restart.pending == null && "Não foi possível confirmar; uma fonte não respondeu."}
          {s.restart.fileRenameOperations ? " Há renomeações de arquivo pendentes, algo comum e informativo (não é um alerta)." : ""}
        </p>
      </Domain>

      <Domain title="Windows Update" section={s.updates} now={now} summary={updateSummary(s.updates)}>
        <dl className="facts">
          <div><dt>Serviço</dt><dd>{s.updates.service ? `${SERVICE_STATE_LABEL[s.updates.service.state]} · ${START_TYPE_LABEL[s.updates.service.start]}` : UNKNOWN_TEXT}</dd></div>
          <div><dt>Última instalação bem-sucedida</dt><dd>{when(s.updates.lastInstallSuccessAt)}</dd></div>
          <div><dt>Última verificação</dt><dd>{when(s.updates.lastScanSuccessAt)}</dd></div>
          <div><dt>Falhas (7 dias)</dt><dd>{countOrDash(s.updates.failures7d)}</dd></div>
          <div><dt>Atualizações pendentes</dt><dd title="Contar pendentes exige uma busca no serviço de atualização, que o LKR LAB não dispara.">{UNKNOWN_TEXT}</dd></div>
        </dl>
      </Domain>

      <Domain title="Eventos do sistema" section={s.events} now={now} summary={s.events.status === "unknown" ? undefined : `${s.events.last24h.critical} críticos · ${s.events.last24h.error} erros (24 h)`}>
        <dl className="facts">
          <div><dt>Últimas 24 h</dt><dd>{s.events.last24h.critical} críticos · {s.events.last24h.error} erros · {countOrDash(s.events.last24h.warning)}{s.events.warningsCapped ? "+" : ""} avisos</dd></div>
          <div><dt>Últimos 7 dias</dt><dd>{s.events.last7d.critical} críticos · {s.events.last7d.error} erros · avisos {countOrDash(s.events.last7d.warning)}</dd></div>
        </dl>
        <p className="muted">Erros e avisos comuns são ruído e não mudam o estado; só os sinais abaixo contam.</p>
        {s.events.signals.length > 0 ? (
          <ul className="mh-win-list" aria-label="Sinais">
            {s.events.signals.map((signal) => (
              <li key={signal.kind}>
                <strong>{signal.label}</strong>
                <span>{signal.count24h} em 24 h · {signal.count7d} em 7 dias{signal.lastAt ? ` · último ${when(signal.lastAt)}` : ""}</span>
              </li>
            ))}
          </ul>
        ) : (
          s.events.status !== "unknown" && <p className="muted">Nenhum sinal de desligamento inesperado, tela azul, erro de disco ou falha de serviço.</p>
        )}
        {s.events.recent.length > 0 && (
          <ul className="mh-win-list" aria-label="Eventos recentes">
            {s.events.recent.map((entry) => (
              <li key={`${entry.at}-${entry.id}`}><span className="mono">{entry.provider} · ID {entry.id}</span><span>{when(entry.at)}</span></li>
            ))}
          </ul>
        )}
        {s.events.truncated && <p className="muted">Leitura limitada aos eventos mais recentes.</p>}
      </Domain>

      <Domain title="Serviços essenciais" section={s.services} now={now}>
        <ul className="mh-win-list" aria-label="Serviços">
          {s.services.items.map((item) => (
            <li key={item.id} className={item.health}>
              <span><strong>{item.label}</strong> <small className="muted mono">{item.id}</small></span>
              <span>
                {SERVICE_STATE_LABEL[item.state]} · {START_TYPE_LABEL[item.start]}
                {item.health === "unknown" ? <Badge>{UNKNOWN_TEXT}</Badge> : item.health !== "healthy" && <Status status={item.health} />}
              </span>
              {item.reason && <small className="reason">{item.reason}</small>}
            </li>
          ))}
        </ul>
        <p className="muted">Serviço parado só é problema quando o início é automático. Serviços sob demanda parados são normais.</p>
      </Domain>

      <Domain title="Dispositivos" section={s.devices} now={now} summary={s.devices.status === "unknown" ? undefined : `${s.devices.total} presentes`}>
        {s.devices.issues.length > 0 ? (
          <ul className="mh-win-list" aria-label="Dispositivos com problema">
            {s.devices.issues.map((issue) => (
              <li key={`${issue.name}-${issue.problemCode}`}>
                <span><strong>{issue.name}</strong>{issue.class && <small className="muted"> · {issue.class}</small>}{issue.manufacturer && <small className="muted"> · {issue.manufacturer}</small>}</span>
                <span>{issue.problem}</span>
              </li>
            ))}
          </ul>
        ) : (
          s.devices.status !== "unknown" && <p className="muted">Nenhum dispositivo presente reporta problema.</p>
        )}
        {s.devices.disabled > 0 && <p className="muted">{s.devices.disabled} desabilitado(s) de propósito (não contam como problema).</p>}
      </Domain>

      <Domain title="Volumes" section={s.volumes} now={now}>
        <ul className="mh-win-list" aria-label="Volumes">
          {s.volumes.items.map((volume) => (
            <li key={volume.mount} className={volume.status}>
              <span><strong>{volume.mount}</strong> <small className="muted">{volume.filesystem ?? UNKNOWN_TEXT}</small></span>
              <span>
                {volume.readOnly == null ? UNKNOWN_TEXT : volume.readOnly ? "Somente leitura" : "Leitura e escrita"}
                {" · "}
                {volume.dirty == null ? "Sujo: requer privilégio administrativo" : volume.dirty ? "Marcado como sujo" : "Não está sujo"}
              </span>
              {volume.reasons.map((reason) => <small className="reason" key={reason}>{reason}</small>)}
            </li>
          ))}
        </ul>
        <p className="muted">Saúde física (SMART): {UNKNOWN_TEXT.toLowerCase()} nesta fase.</p>
      </Domain>

      <Domain title="Confiabilidade" section={s.reliability} now={now} summary="informativo">
        <dl className="facts">
          <div><dt>Falhas de aplicativo (7 dias)</dt><dd>{countOrDash(s.reliability.appCrashes7d)}</dd></div>
          <div><dt>Aplicativos sem resposta (7 dias)</dt><dd>{countOrDash(s.reliability.appHangs7d)}</dd></div>
        </dl>
        <p className="muted">Sem pontuação de confiabilidade: o Monitor de Confiabilidade depende de WMI e não é consultado.</p>
      </Domain>

      <Domain title="Integridade do sistema (passiva)" section={s.integrity} now={now} summary="somente sinais">
        <ul className="mh-win-list" aria-label="Sinais passivos">
          {s.integrity.signals.map((signal) => (
            <li key={signal.id} className={signal.present ? "attention" : ""}>
              <span>{signal.label}</span><span>{signal.present ? "Presente" : "Ausente"}</span>
            </li>
          ))}
        </ul>
        <p className="muted">
          SFC, DISM e CHKDSK não são executados: {s.integrity.onDemand.map((c) => c.label).join("; ")}. Ficam para uma etapa de diagnóstico sob demanda e exigem privilégio administrativo.
        </p>
      </Domain>

      {missing.length > 0 && (
        <details className="mh-win-missing">
          <summary>Fontes não disponíveis ({missing.length})</summary>
          <ul className="mh-win-list" aria-label="Fontes indisponíveis">
            {missing.map((note) => <li key={note.id}><span>{note.label}</span><span className="muted">{sourceText(note)}</span></li>)}
          </ul>
        </details>
      )}
      <p className="footnote">Somente observação: o LKR LAB não repara, reinicia, instala nem altera nada no Windows.</p>
    </section>
  );
}
