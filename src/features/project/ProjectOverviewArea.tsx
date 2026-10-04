import { useEffect, useState } from "react";
import { BookOpenCheck, Box, Circle, Clock, GitBranch, ListChecks, MapPin, Play, Sparkles } from "lucide-react";
import type { ReactNode } from "react";
import { api } from "../../shared/api";
import { deriveProjectNextAction, runtimeHeadline } from "../../shared/projectContext";
import { branchLabel, displayPath, gitLabel, relativeTime, runtimeLabel, syncLabel } from "../../shared/projectOverview";
import { Badge } from "../../shared/ui";
import type { ProjectActivity, ProjectOverview } from "../../shared/types";
import { useResource, workspace } from "../../state/workspace";
import { useMachine } from "../../state/machine";
import { desktop } from "../../shared/api";
import type { ProjectArea } from "../../app/projectRoute";

function SummaryCard({ icon, title, tone, children, cta, onCta }: {
  icon: ReactNode; title: string; tone?: string; children: ReactNode; cta?: string; onCta?: () => void;
}) {
  return (
    <section className="pcc-card">
      <header>{icon}<h3>{title}</h3></header>
      <div className={`pcc-card-body ${tone ? `tone-${tone}` : ""}`}>{children}</div>
      {cta && onCta && <button type="button" className="text-button" onClick={onCta}>{cta}</button>}
    </section>
  );
}

const Unavailable = ({ what }: { what: string }) => (
  <>
    <strong className="tone-neutral">Ainda não disponível</strong>
    <small>{what}</small>
  </>
);

/** Visão geral (Concept 05): só dados reais. O DDAE vem do backend; Planejamento ainda não existe e diz isso. */
export function ProjectOverviewArea({ project: p, go, locate, generateContext, refreshKey }: {
  project: ProjectOverview;
  go: (area: ProjectArea) => void;
  locate: () => void;
  generateContext: () => void;
  /** Muda quando algo registrável aconteceu (ex.: contexto gerado) para reler a atividade. */
  refreshKey: number;
}) {
  const machine = useMachine().status?.machine;
  const available = p.location === "available";
  const sources = workspace.forProject(p.id);
  const { data: wtCounts } = useResource(sources.worktreeSummary);
  const { data: runtime } = useResource(sources.runtime);
  const [activity, setActivity] = useState<ProjectActivity | null>(null);
  const { data: ddae, status: ddaeStatus } = useResource(sources.ddae);
  useEffect(() => {
    if (desktop) void sources.ddae.refresh();
  }, [sources, refreshKey]);

  useEffect(() => {
    if (!desktop) return;
    let current = true;
    void api<ProjectActivity>("project_activity", { id: p.id }).then((value) => { if (current) setActivity(value); }, () => { if (current) setActivity(null); });
    return () => { current = false; };
  }, [p.id, refreshKey]);
  useEffect(() => {
    if (available && desktop) void sources.worktreeSummary.refresh(30_000);
  }, [available, sources]);
  const running = p.runtime.data?.running === true;
  useEffect(() => {
    if (available && running && desktop) void sources.runtime.refresh(5000);
  }, [available, running, sources]);

  const git = gitLabel(p);
  const runtimeText = runtimeLabel(p);
  const next = deriveProjectNextAction(p, activity ? activity.contextGenerated : null, ddae);
  const ddaeActive = ddae?.sessions.find((session) => session.id === ddae.activeSessionId) ?? null;
  const act = () => {
    if (next.target.kind === "locate") locate();
    else if (next.target.kind === "context") generateContext();
    else if (next.target.kind === "area") go(next.target.area);
  };
  const services = available && running ? runtime?.services ?? [] : [];

  return (
    <div className="pcc-overview">
      <div className="pcc-cards">
        <SummaryCard icon={<GitBranch size={16} />} title="Git" tone={available ? git.tone : "neutral"} cta={available ? "Abrir Git" : undefined} onCta={() => go("git")}>
          {available ? (
            <>
              <strong>{git.text}</strong>
              <small className="mono">{branchLabel(p)}{syncLabel(p) && ` · ${syncLabel(p)}`}</small>
              {p.git.status === "error" && <small>{p.git.message}</small>}
            </>
          ) : (
            <><strong className="tone-neutral">—</strong><small>Sem pasta nesta máquina</small></>
          )}
        </SummaryCard>
        <SummaryCard icon={<Play size={16} />} title="Runtime" tone={available ? runtimeText.tone : "neutral"} cta={available ? "Abrir Runtime" : undefined} onCta={() => go("runtime")}>
          {available ? (
            <>
              <strong>{runtimeText.text}</strong>
              <small>{runtimeHeadline(p) === runtimeText.text ? "Nenhuma execução ou porta ativa" : runtimeHeadline(p).replace(`${runtimeText.text} · `, "")}</small>
            </>
          ) : (
            <><strong className="tone-neutral">Indisponível</strong><small>Sem pasta nesta máquina</small></>
          )}
        </SummaryCard>
        <SummaryCard icon={<Box size={16} />} title="Worktrees" cta={available ? "Abrir Worktrees" : undefined} onCta={() => go("worktrees")}>
          {available ? (
            <>
              {wtCounts ? (
                <>
                  <strong>{wtCounts.managed} {wtCounts.managed === 1 ? "gerenciado" : "gerenciados"}</strong>
                  <small>{wtCounts.active} {wtCounts.active === 1 ? "ativo" : "ativos"} · {wtCounts.missing} não localizado{wtCounts.missing === 1 ? "" : "s"}</small>
                  <small>{wtCounts.gitTotal} no Git · {wtCounts.unmanaged} não gerenciado{wtCounts.unmanaged === 1 ? "" : "s"}</small>
                </>
              ) : (
                <><strong className="tone-neutral">{desktop ? "Carregando…" : "—"}</strong><small>{desktop ? "Lendo o Git" : "Disponível apenas no aplicativo desktop"}</small></>
              )}
            </>
          ) : (
            <><strong className="tone-neutral">—</strong><small>Sem pasta nesta máquina</small></>
          )}
        </SummaryCard>
        <SummaryCard icon={<BookOpenCheck size={16} />} title="DDAE / Sessões" tone={ddaeActive ? "blue" : undefined} cta="Abrir DDAE" onCta={() => go("ddae")}>
          {!desktop ? (
            <><strong className="tone-neutral">—</strong><small>Disponível apenas no aplicativo desktop</small></>
          ) : !ddae ? (
            <><strong className="tone-neutral">{ddaeStatus === "error" ? "Indisponível" : "Carregando…"}</strong><small>Sessões do projeto</small></>
          ) : ddaeActive ? (
            <>
              <strong>{ddaeActive.label}</strong>
              <small>{ddaeActive.currentBlock ? `Atual: ${ddaeActive.currentBlock.title}` : "Sem bloco em andamento"}</small>
              <small>{ddaeActive.progress.completed} / {ddaeActive.progress.total} blocos{ddaeActive.nextBlock ? ` · Próximo: ${ddaeActive.nextBlock.title}` : ""}</small>
            </>
          ) : (
            <>
              <strong className="tone-neutral">Nenhuma sessão ativa</strong>
              <small>{ddae.counts.total === 0 ? "Nenhuma sessão neste projeto" : `${ddae.counts.total} ${ddae.counts.total === 1 ? "sessão" : "sessões"} no projeto`}</small>
            </>
          )}
        </SummaryCard>
        <SummaryCard icon={<ListChecks size={16} />} title="Planejamento">
          <Unavailable what="O módulo de Planejamento ainda não foi implementado." />
        </SummaryCard>
      </div>

      {!available && (
        <div className="pcc-missing" role="note">
          <MapPin size={18} />
          <div>
            <strong>Não localizado nesta máquina</strong>
            <span>
              {p.location === "missing"
                ? "A pasta vinculada não existe mais aqui. Git, runtime e worktrees ficam indisponíveis até o projeto ser localizado."
                : "Este projeto pertence ao workspace, mas ainda não tem pasta vinculada neste computador. Git, runtime e worktrees ficam indisponíveis."}
            </span>
          </div>
          <button type="button" className="button primary" disabled={!desktop} onClick={locate}>Localizar</button>
        </div>
      )}

      <div className="pcc-columns">
        <div className="pcc-column">
          <section className="panel">
            <div className="panel-title"><h2>Informações do projeto</h2></div>
            <dl className="facts">
              <div><dt>Computador</dt><dd>{machine?.name ?? "—"}</dd></div>
              <div><dt>Localização</dt><dd className="mono">{available ? displayPath(p.localPath) : "Não localizado nesta máquina"}</dd></div>
              <div><dt>Repositório</dt><dd className="mono">{p.locator?.remote ?? (p.repository || "—")}</dd></div>
              <div>
                <dt>Stack{p.stackSource === "registered" ? " (do cadastro)" : ""}</dt>
                <dd>{p.stack.length ? p.stack.join(" · ") : "Não identificada"}</dd>
              </div>
            </dl>
          </section>
          <section className="panel">
            <div className="panel-title"><h2>Serviços em execução</h2></div>
            {services.length ? services.map((s, i) => (
              <div className="service-row" key={`${s.label}-${s.port}-${i}`}>
                <span>{s.label}{s.port ? <> <code>:{s.port}</code></> : null}</span>
                <Badge tone="blue">{s.managed ? "LKR LAB" : "Externo"}</Badge>
              </div>
            )) : (
              <p className="muted">{available ? (running ? "Em execução, sem serviços TCP identificados." : "Nenhum serviço em execução.") : "Indisponível sem pasta nesta máquina."}</p>
            )}
          </section>
        </div>
        <div className="pcc-column">
          <section className="panel pcc-next" aria-label="Próxima ação">
            <div className="panel-title"><h2><Sparkles size={16} /> Próxima ação</h2></div>
            <strong>{next.title}</strong>
            <p className="muted">{next.description}</p>
            {next.cta && <button type="button" className="button primary" onClick={act}>{next.cta}</button>}
            {available && next.id !== "generate-context" && (
              <button type="button" className="button" onClick={generateContext}><Sparkles size={14} /> Gerar contexto</button>
            )}
          </section>
          <section className="panel">
            <div className="panel-title"><h2><Clock size={16} /> Última atividade</h2></div>
            {activity === null ? (
              <p className="muted">{desktop ? "Carregando…" : "Disponível apenas no aplicativo desktop."}</p>
            ) : activity.items.length === 0 ? (
              <p className="muted">Nenhuma atividade registrada.</p>
            ) : (
              <ul className="pcc-activity">
                {activity.items.map((item) => (
                  <li key={item.id}>
                    <Circle size={8} fill="currentColor" />
                    <span>{item.action}</span>
                    <time dateTime={item.createdAt}>{relativeTime(item.createdAt)}</time>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
