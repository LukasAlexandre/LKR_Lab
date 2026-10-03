import { useEffect, useRef } from "react";
import { BookOpenCheck, Box, GitBranch, LayoutGrid, ListChecks, Play, ScrollText, Sparkles } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { PROJECT_AREAS, PROJECT_AREA_TITLES, projectHash } from "../../app/projectRoute";
import type { ProjectArea } from "../../app/projectRoute";

const ICONS: Record<ProjectArea, LucideIcon> = {
  overview: LayoutGrid,
  ddae: BookOpenCheck,
  worktrees: Box,
  planning: ListChecks,
  runtime: Play,
  git: GitBranch,
  logs: ScrollText,
  context: Sparkles,
};

/** Seção "PROJETO ATUAL" da sidebar: só existe dentro de #project/<id>/…; a navegação global continua acima. */
export function ProjectContextNav({ projectId, name, area, compact }: { projectId: string; name: string; area: ProjectArea; compact: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  // A sidebar rola: mantém a seção do projeto à vista ao abrir/trocar de área.
  useEffect(() => { ref.current?.scrollIntoView?.({ block: "nearest" }); }, [projectId, area]);
  return (
    <div className="project-nav" aria-label="Projeto atual" ref={ref}>
      <div className="nav-group">PROJETO ATUAL</div>
      <div className="project-nav-name" title={name}>{name}</div>
      {PROJECT_AREAS.map((id) => {
        const Icon = ICONS[id];
        return (
          <a
            key={id}
            href={projectHash(projectId, id)}
            className={`nav-item ${area === id ? "active" : ""}`}
            aria-current={area === id ? "page" : undefined}
            title={compact ? PROJECT_AREA_TITLES[id] : undefined}
          >
            <Icon size={17} />
            <span className="nav-label">{PROJECT_AREA_TITLES[id]}</span>
          </a>
        );
      })}
    </div>
  );
}
