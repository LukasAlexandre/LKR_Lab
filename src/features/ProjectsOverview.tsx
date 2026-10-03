import { useEffect, useMemo, useState } from "react";
import {
  Circle, CircleCheck, Clock, FolderOpen, GitBranch, Info, MapPin, MoreHorizontal, Pencil, Play, Plus,
  Search, Trash2, TriangleAlert, Upload,
} from "lucide-react";
import { desktop, errorText } from "../shared/api";
import { locateProject } from "../shared/bind";
import {
  branchLabel, cardAction, displayPath, emptyKind, FILTERS, gitLabel, relativeTime, runtimeLabel, summarize, syncLabel,
  visibleProjects, visibleStack,
  type ProjectFilter, type ProjectSort,
} from "../shared/projectOverview";
import { Refresh } from "../shared/ui";
import type { ProjectOverview } from "../shared/types";
import { useActiveProjectId, useResource, workspace } from "../state/workspace";

const SKELETONS = [0, 1, 2];

function Metric({ icon, value, label }: { icon: React.ReactNode; value: number | string; label: string }) {
  return (
    <div className="overview-metric">
      <span className="overview-metric-icon">{icon}</span>
      <div>
        <strong>{value}</strong>
        <span>{label}</span>
      </div>
    </div>
  );
}

export function ProjectOverviewCard({
  project: p, active, locating, open, locate, edit, remove,
}: {
  project: ProjectOverview;
  active: boolean;
  locating: boolean;
  open: (p: ProjectOverview) => void;
  locate: (p: ProjectOverview) => void;
  edit: (p: ProjectOverview) => void;
  remove: (p: ProjectOverview) => void;
}) {
  const git = gitLabel(p);
  const runtime = runtimeLabel(p);
  const sync = syncLabel(p);
  const stack = visibleStack(p.stack);
  const unlocated = cardAction(p) === "locate";
  return (
    <article className={`overview-card ${active ? "is-active" : ""} ${unlocated ? "is-unlocated" : ""}`} data-project={p.id}>
      <header>
        <span className="overview-card-icon"><FolderOpen size={22} /></span>
        <div className="overview-card-title">
          <h3 title={p.name}>{p.name}</h3>
          <p>{p.description || "Sem descrição"}</p>
        </div>
        <details className="card-menu">
          <summary aria-label={`Ações de ${p.name}`}><MoreHorizontal size={16} /></summary>
          <div role="menu">
            <button type="button" role="menuitem" onClick={() => edit(p)}><Pencil size={13} /> Editar</button>
            <button type="button" role="menuitem" className="danger-text" onClick={() => remove(p)}>
              <Trash2 size={13} /> Remover cadastro
            </button>
          </div>
        </details>
      </header>
      <div className="overview-stack" aria-label="Stack">
        {stack.shown.length ? stack.shown.map((s) => <span className="chip" key={s}>{s}</span>) : <span className="muted">Stack não identificada</span>}
        {stack.extra > 0 && <span className="chip chip-more">+{stack.extra}</span>}
      </div>
      {unlocated ? (
        <div className="overview-notice" role="note">
          <Info size={16} />
          <div>
            <strong>Não localizado nesta máquina</strong>
            <span>
              {p.location === "missing"
                ? "A pasta vinculada não existe mais aqui. Localize a pasta para voltar a usar o projeto."
                : "Este projeto pertence ao workspace, mas ainda não possui uma pasta vinculada neste computador."}
            </span>
          </div>
        </div>
      ) : (
        <div className="overview-path mono" title={displayPath(p.localPath)}><FolderOpen size={13} /> <span>{displayPath(p.localPath)}</span></div>
      )}
      <dl className="overview-facts">
        <div>
          <dt>Branch</dt>
          <dd className="mono" title={branchLabel(p)}><GitBranch size={13} /> {branchLabel(p)}{sync && <small>{sync}</small>}</dd>
        </div>
        <div>
          <dt>Git</dt>
          <dd className={`tone-${git.tone}`} title={p.git.message ?? undefined}>
            {git.tone !== "neutral" && <Circle size={9} fill="currentColor" />} {git.text}
          </dd>
        </div>
        <div>
          <dt>Runtime</dt>
          <dd className={`tone-${runtime.tone}`} title={p.runtime.message ?? undefined}>
            {runtime.tone !== "neutral" && <Circle size={9} fill="currentColor" />} {runtime.text}
          </dd>
        </div>
      </dl>
      <footer>
        <div className="overview-activity">
          <Clock size={15} />
          <span>
            <small>Última atividade</small>
            {relativeTime(p.lastActivity)}
          </span>
        </div>
        {unlocated ? (
          <button type="button" className="button primary" disabled={!desktop || locating} onClick={() => locate(p)}>
            <MapPin size={14} /> {locating ? "Localizando…" : "Localizar"}
          </button>
        ) : (
          <button type="button" className="button primary" onClick={() => open(p)}>Abrir projeto</button>
        )}
      </footer>
    </article>
  );
}

/** Concept 03 · Projetos. Uma única operação de backend alimenta resumo, filtros e cards. */
export function ProjectsOverviewPage({
  add, open, edit, remove, report, notify,
}: {
  add: () => void;
  open: (p: ProjectOverview) => void;
  edit: (p: ProjectOverview) => void;
  remove: (p: ProjectOverview) => void;
  report: (error: unknown) => void;
  notify: (message: string) => void;
}) {
  const source = useResource(workspace.overviews);
  const registry = useResource(workspace.projects);
  const activeId = useActiveProjectId();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<ProjectFilter>("all");
  const [sort, setSort] = useState<ProjectSort>("name");
  const [locatingId, setLocatingId] = useState<string | null>(null);

  // Lê ao abrir a página e quando o cadastro muda (novo projeto, edição, remoção, Localizar).
  const registryStamp = registry.lastUpdated;
  useEffect(() => {
    if (desktop) void workspace.overviews.refresh();
  }, [registryStamp]);

  const list = useMemo(() => source.data?.projects ?? [], [source.data]);
  const totals = useMemo(() => summarize(list), [list]);
  const shown = useMemo(() => visibleProjects(list, query, filter, sort), [list, query, filter, sort]);
  const empty = emptyKind(list.length, shown.length);
  const firstLoad = !source.data && (source.status === "loading" || source.status === "idle") && desktop;
  const failed = source.status === "error";

  async function locate(p: ProjectOverview) {
    setLocatingId(p.id);
    try {
      const result = await locateProject(p.id);
      if (result?.bound) {
        await workspace.loadRegistry();
        await workspace.overviews.refresh();
        notify(`${p.name} localizado nesta máquina.`);
      }
    } catch (error) {
      report(error);
    } finally {
      setLocatingId(null);
    }
  }

  return (
    <section className="projects-overview" aria-label="Projetos">
      <div className="page-heading">
        <div>
          <div className="eyebrow">WORKSPACE</div>
          <h1>Projetos</h1>
          <p>Gerencie os projetos disponíveis nesta workstation</p>
        </div>
        <div className="heading-actions">
          <Refresh onClick={() => void workspace.overviews.refresh()} busy={source.loading} />
          <button type="button" className="button primary" onClick={add}>
            <Plus size={16} /> Novo projeto
          </button>
        </div>
      </div>

      {!desktop && (
        <div className="notice" role="status">
          Git, runtime e pastas locais são recursos do aplicativo desktop; a prévia web não simula esses dados.
        </div>
      )}

      <div className="overview-summary" aria-label="Resumo dos projetos">
        <Metric icon={<FolderOpen size={20} />} value={totals.total} label="projetos cadastrados" />
        <Metric icon={<CircleCheck size={20} />} value={totals.available} label="disponíveis" />
        <Metric icon={<Play size={20} />} value={totals.running} label="em execução" />
        <Metric icon={<Upload size={20} />} value={totals.dirty} label="com alterações locais" />
      </div>

      <div className="overview-controls">
        <label className="overview-search">
          <Search size={15} />
          <input
            aria-label="Buscar projetos"
            placeholder="Buscar projetos…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <div className="overview-filters" role="group" aria-label="Filtrar projetos">
          {FILTERS.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              className={`filter-chip ${filter === id ? "is-on" : ""}`}
              aria-pressed={filter === id}
              onClick={() => setFilter(id)}
            >
              {label}
            </button>
          ))}
        </div>
        <label className="overview-sort">
          <span>Ordenar por</span>
          <select value={sort} onChange={(event) => setSort(event.target.value as ProjectSort)}>
            <option value="name">Nome</option>
            <option value="recent">Última atividade</option>
          </select>
        </label>
      </div>

      {failed && (
        <div className="error" role="alert">
          <TriangleAlert size={15} /> Não foi possível ler os projetos: {source.error ?? errorText("erro desconhecido")}
          {source.data ? " Mostrando a última leitura." : ""}
        </div>
      )}

      <div className="overview-grid" aria-busy={firstLoad}>
        {firstLoad && SKELETONS.map((i) => <div className="overview-card skeleton" key={i} aria-hidden="true" />)}
        {!firstLoad && shown.map((p) => (
          <ProjectOverviewCard
            key={p.id}
            project={p}
            active={p.id === activeId}
            locating={locatingId === p.id}
            open={open}
            locate={(project) => void locate(project)}
            edit={edit}
            remove={remove}
          />
        ))}
        {!firstLoad && empty === null && (
          <button type="button" className="overview-card overview-new" onClick={add}>
            <span className="overview-new-plus"><Plus size={26} /></span>
            <strong>Novo projeto</strong>
            <span>Cadastre um novo projeto nesta máquina e comece a gerenciar seu ambiente.</span>
            <span className="button primary"><Plus size={14} /> Cadastrar projeto</span>
          </button>
        )}
      </div>

      {!firstLoad && empty === "no-results" && (
        <div className="empty" role="status">
          <Info size={25} />
          <h3>Nenhum projeto encontrado</h3>
          <p>Nenhum dos {list.length} projetos corresponde à busca ou ao filtro atual.</p>
          <button type="button" className="button" onClick={() => { setQuery(""); setFilter("all"); }}>Limpar busca e filtros</button>
        </div>
      )}
      {!firstLoad && empty === "no-projects" && (
        <div className="empty" role="status">
          <Info size={25} />
          <h3>Seu workspace começa com um projeto</h3>
          <p>Escolha uma pasta existente: o LKR LAB lê arquivos e Git em modo somente leitura.</p>
          <button type="button" className="button primary" onClick={add}><Plus size={16} /> Cadastrar projeto</button>
        </div>
      )}
    </section>
  );
}
