import { useEffect, useState } from "react";
import { Check, Code2, Copy, FolderOpen, Info, MapPin, Terminal } from "lucide-react";
import type { ReactNode } from "react";
import { PROJECT_AREA_TITLES, projectHash } from "../../app/projectRoute";
import type { ProjectArea } from "../../app/projectRoute";
import { desktop } from "../../shared/api";
import { locateProject } from "../../shared/bind";
import { displayPath, runtimeLabel } from "../../shared/projectOverview";
import { Empty, Panel } from "../../shared/ui";
import type { Project } from "../../shared/types";
import { useResource, workspace } from "../../state/workspace";
import { ProjectRuntime } from "../ProjectRuntime";
import { DdaeSessions } from "./DdaeSessions";
import { SessionDetail } from "./SessionDetail";
import { WorktreesWorkspace } from "./WorktreesWorkspace";
import { ProjectLogs } from "./ProjectLogs";
import { ProjectOverviewArea } from "./ProjectOverviewArea";

/** Áreas que ainda não têm módulo: dizem isso, sem dados de exemplo. */
const UNAVAILABLE: Partial<Record<ProjectArea, string>> = {
  planning: "O módulo de Planejamento ainda não foi implementado.",
};

/** Áreas que dependem de uma pasta nesta máquina (Git, runtime, worktrees, logs). */
const NEEDS_FOLDER: ProjectArea[] = ["worktrees", "runtime", "git", "logs", "context"];

interface Views {
  /** Interface Git atual (a mesma da rota global #git). */
  git: ReactNode;
  /** Contexto de IA atual (a mesma da rota global #agents). */
  context: ReactNode;
}

/**
 * Project Control Center (Concept 05): cabeçalho do projeto + a área escolhida na URL.
 * O Project vem da ROTA; as telas reaproveitadas leem o Project ativo, que o App sincroniza
 * com a URL antes de montar este componente.
 */
export function ProjectControlCenter({ projectId, area, sessionId, views, launch, generateContext, report, notify, contextVersion }: {
  projectId: string;
  area: ProjectArea;
  /** Só no DDAE: abre o detalhe de UMA Session (o UUID da rota). */
  sessionId?: string;
  views: Views;
  launch: (id: string, action: string) => void;
  generateContext: () => void;
  report: (error: unknown) => void;
  notify: (message: string) => void;
  contextVersion: number;
}) {
  const overviewSource = useResource(workspace.overviews);
  const registry = useResource(workspace.projects);
  const [copied, setCopied] = useState(false);
  const [locating, setLocating] = useState(false);

  const stamp = registry.lastUpdated;
  useEffect(() => {
    if (desktop) void workspace.overviews.refresh(5000);
  }, [projectId, stamp]);

  const base: Project | undefined = registry.data.find((item) => item.id === projectId);
  const overview = overviewSource.data?.projects.find((item) => item.id === projectId);

  if (!base) return null;
  if (!overview) {
    return (
      <section className="pcc" aria-busy="true">
        <div className="pcc-head"><div><h1>{base.name}</h1><p className="muted">Carregando…</p></div></div>
      </section>
    );
  }

  const available = overview.location === "available";
  const go = (next: ProjectArea) => { window.location.hash = projectHash(projectId, next); };
  async function locate() {
    setLocating(true);
    try {
      const result = await locateProject(projectId);
      if (result?.bound) {
        await workspace.loadRegistry();
        await workspace.overviews.refresh();
        notify(`${overview?.name ?? "Projeto"} localizado nesta máquina.`);
      }
    } catch (error) {
      report(error);
    } finally {
      setLocating(false);
    }
  }
  async function copyPath() {
    try {
      await navigator.clipboard.writeText(displayPath(overview!.localPath));
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* área de transferência bloqueada: sem efeito */
    }
  }
  const runtime = runtimeLabel(overview);

  const unlocatedNotice = (
    <Panel title={PROJECT_AREA_TITLES[area]}>
      <div className="pcc-missing" role="note">
        <MapPin size={18} />
        <div>
          <strong>Projeto não localizado nesta máquina</strong>
          <span>Esta área depende de uma pasta local. Localize o projeto para usá-la.</span>
        </div>
        <button type="button" className="button primary" disabled={!desktop || locating} onClick={() => void locate()}>
          {locating ? "Localizando…" : "Localizar"}
        </button>
      </div>
    </Panel>
  );

  let content: ReactNode;
  if (UNAVAILABLE[area]) {
    content = (
      <Panel title={PROJECT_AREA_TITLES[area]}>
        <Empty title="Ainda não disponível">
          <p>{UNAVAILABLE[area]}</p>
        </Empty>
      </Panel>
    );
  } else if (area === "ddae") {
    // Sessões pertencem ao Project (estado portátil): não dependem da pasta nesta máquina.
    content = sessionId
      ? <SessionDetail key={`${projectId}/${sessionId}`} projectId={projectId} sessionId={sessionId} notify={notify} />
      : <DdaeSessions key={projectId} projectId={projectId} notify={notify} />;
  } else if (NEEDS_FOLDER.includes(area) && !available) {
    content = unlocatedNotice;
  } else if (area === "overview") {
    content = <ProjectOverviewArea project={overview} go={go} locate={() => void locate()} generateContext={generateContext} refreshKey={contextVersion} />;
  } else if (area === "runtime") {
    content = <ProjectRuntime project={base} context={generateContext} report={report} />;
  } else if (area === "git") {
    content = views.git;
  } else if (area === "worktrees") {
    content = <WorktreesWorkspace key={projectId} projectId={projectId} notify={notify} />;
  } else if (area === "logs") {
    content = <ProjectLogs projectId={projectId} />;
  } else {
    content = views.context;
  }

  return (
    <section className="pcc" aria-label={`Projeto ${overview.name}`}>
      <header className="pcc-head">
        <div className="pcc-head-main">
          <h1>
            {overview.name}
            {available && overview.runtime.data?.running && <span className="pcc-live" title="Em execução" aria-label="Em execução" />}
          </h1>
          <p>{overview.description || "Sem descrição"}</p>
          <div className="pcc-meta">
            {available ? (
              <span className="mono pcc-path" title={displayPath(overview.localPath)}>
                <FolderOpen size={13} /> {displayPath(overview.localPath)}
                <button type="button" className="icon-button" aria-label="Copiar caminho" onClick={() => void copyPath()}>
                  {copied ? <Check size={13} /> : <Copy size={13} />}
                </button>
              </span>
            ) : (
              <span className="pcc-path"><Info size={13} /> Não localizado nesta máquina</span>
            )}
            {overview.stack.slice(0, 5).map((s) => <span className="chip" key={s}>{s}</span>)}
            {available && <span className={`tone-${runtime.tone}`}>{runtime.text}</span>}
          </div>
        </div>
        <div className="pcc-head-actions">
          {available ? (
            <>
              <button type="button" className="button" onClick={() => launch(projectId, "folder")}><FolderOpen size={15} /> Abrir pasta</button>
              <button type="button" className="button" onClick={() => launch(projectId, "terminal")}><Terminal size={15} /> Terminal</button>
              <button type="button" className="button" onClick={() => launch(projectId, "vscode")}><Code2 size={15} /> Code</button>
            </>
          ) : (
            <button type="button" className="button primary" disabled={!desktop || locating} onClick={() => void locate()}>
              <MapPin size={15} /> {locating ? "Localizando…" : "Localizar"}
            </button>
          )}
        </div>
      </header>
      {content}
    </section>
  );
}
