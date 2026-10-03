import { useCallback, useEffect, useState } from "react";
import {
  Bot,
  Check,
  ChevronRight,
  CircleHelp,
  Code2,
  Copy,
  FileText,
  FolderOpen,
  GitPullRequest,
  Plus,
  Search,
  ShieldCheck,
  Terminal,
  Zap,
  X,
} from "lucide-react";
import { api, desktop, errorText } from "./shared/api";
import { Badge, Empty, Modal, Panel, Refresh } from "./shared/ui";
import { usePreference } from "./shared/preferences";
import { useActiveProjectId, useResource, workspace } from "./state/workspace";
import { Sidebar } from "./shell/Sidebar";
import { CommandPalette } from "./shell/CommandPalette";
import { ActivityCenter } from "./shell/ActivityCenter";
import { SourceStatus } from "./components/SourceStatus";
import type {
  Project,
} from "./shared/types";
import {
  Launchers,
  ProjectForm,
  Projects,
} from "./features/Projects";
import { Ports, Processes } from "./features/Ports";
import { Prompts } from "./features/Prompts";
import { Repositories } from "./features/Repositories";
import { MachineHealth } from "./features/MachineHealth";
import { GitSummary } from "./features/GitSummary";
import { Worktrees } from "./features/Worktrees";
import { AgentProviders } from "./features/AgentProviders";
import { Knowledge } from "./features/Knowledge";
import { routes, routeFromHash, isNewProjectHash, NEW_PROJECT_HASH } from "./app/routing";
import { WindowTitleBar } from "./shell/WindowTitleBar";
import { LocationNotice } from "./components/LocationNotice";
import { SyncIndicator } from "./components/SyncIndicator";
import { ProjectRuntime, RuntimeDesktopOnly } from "./features/ProjectRuntime";
import { ThisMachine } from "./features/ThisMachine";
import { useMachine } from "./state/machine";
import { NewProject } from "./features/NewProject";
import { useTelemetry } from "./state/telemetry";
import { HEALTH_LABEL } from "./shared/telemetry";
export default function App() {
  const machine = useMachine().status?.machine;
  const health = useTelemetry().latest?.health.status;
  const [route, setRoute] = useState(routeFromHash);
  const [creating, setCreating] = useState(isNewProjectHash);
  const selectedId = useActiveProjectId();
  const setSelectedId = workspace.selectProject;
  const projectsSource = useResource(workspace.projects);
  const { data: projects } = projectsSource;
  const [sidebarCompact, setSidebarCompact] = usePreference("sidebarCompact");
  const [density, setDensity] = usePreference("density");
  const environmentSource = useResource(workspace.environment);
  const toggleSidebar = useCallback(() => setSidebarCompact(value => !value), [setSidebarCompact]);
  const portsSource = useResource(workspace.ports);
  const processesSource = useResource(workspace.processes);
  const { data: ports } = portsSource;
  const { data: processes } = processesSource;
  const { data: prompts } = useResource(workspace.prompts);
  const sources = workspace.forProject(selectedId);
  const gitSource = useResource(sources.git);
  const hostingSource = useResource(sources.hosting);
  const agentSource = useResource(sources.agents);
  const git = gitSource.data, hosting = hostingSource.data,
    agent = agentSource.data;
  const lastRefresh = environmentSource.lastUpdated
    ? new Date(environmentSource.lastUpdated).toLocaleTimeString("pt-BR") : "";
  const [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [toast, setToast] = useState("");
  const [form, setForm] = useState<Project | "new" | null>(null),
    [snapshot, setSnapshot] = useState<string | null>(null),
    [palette, setPalette] = useState(false),
    [query, setQuery] = useState("");
  const [confirm, setConfirm] = useState<{
    title: string;
    description: string;
    run: () => Promise<void>;
  } | null>(null);
  const selected = projects.find((p) => p.id === selectedId);
  const report = useCallback((e: unknown) => setError(errorText(e)), []);
  const navigate = useCallback((id: string) => {
    window.location.hash = id;
    setRoute(id);
    setPalette(false);
  }, []);
  const openNewProject = useCallback(() => {
    window.location.hash = NEW_PROJECT_HASH;
    setRoute("projects");
    setCreating(true);
    setPalette(false);
  }, []);
  const closeNewProject = useCallback(() => {
    window.location.hash = "projects";
    setCreating(false);
  }, []);
  const closeForm = useCallback(() => setForm(null), []),
    closePalette = useCallback(() => setPalette(false), []),
    closeSnapshot = useCallback(() => setSnapshot(null), []),
    closeConfirm = useCallback(() => setConfirm(null), []);
  const loadRegistry = workspace.loadRegistry;
  const refresh = workspace.refreshEnvironment;
  useEffect(() => {
    void loadRegistry();
    if (desktop) void refresh();
    const onHash = () => {
      setRoute(routeFromHash());
      setCreating(isNewProjectHash());
    };
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((v) => !v);
        setQuery("");
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "p") {
        e.preventDefault();
        setPalette(true);
        setQuery("project ");
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "`") {
        e.preventDefault();
        navigate("terminal");
      }
    };
    window.addEventListener("hashchange", onHash);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("hashchange", onHash);
      window.removeEventListener("keydown", onKey);
    };
  }, [loadRegistry, navigate, refresh]);
  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(() => setToast(""), 4000);
    return () => clearTimeout(timer);
  }, [toast]);
  useEffect(() => {
    if (!desktop || !selectedId) return;
    void sources.git.refresh(30_000);
    void sources.agents.refresh(30_000);
    void sources.worktrees.refresh(30_000);
  }, [selectedId, sources]);
  useEffect(() => {
    if (desktop && projects.length) void workspace.refreshRepositories();
  }, [projects]);
  const launch = (id: string, action: string) => {
    void api("launch_project", { id, action }).catch(report);
  };
  async function context() {
    if (!selected) return;
    setBusy(true);
    try {
      setSnapshot(await api<string>("generate_context", { id: selected.id }));
      await loadRegistry();
    } catch (e) {
      report(e);
    } finally {
      setBusy(false);
    }
  }
  async function associatePort(projectId: string, port: number) {
    const p = projects.find((p) => p.id === projectId);
    if (!p) return;
    if (p.ports.some((p) => p.port === port)) {
      setToast("Esta porta já está declarada no projeto.");
      return;
    }
    try {
      await api("save_project", {
        id: p.id,
        input: {
          name: p.name,
          description: p.description,
          localPath: p.localPath,
          repository: p.repository,
          stack: p.stack,
          tags: p.tags,
          commands: p.commands,
          ports: [...p.ports, { name: `service-${port}`, port }],
        },
      });
      await loadRegistry();
      await refresh();
      setToast(
        "Expectativa associada. Propriedade do processo não foi alterada.",
      );
    } catch (e) {
      report(e);
    }
  }
  function remove(p: Project) {
    setConfirm({
      title: `Remover ${p.name}?`,
      description:
        "Remove somente o cadastro e os templates vinculados. Os arquivos do projeto serão preservados.",
      run: async () => {
        await api("delete_project", { id: p.id, confirmed: true });
        await loadRegistry();
        setToast("Cadastro removido.");
      },
    });
  }
  function confirmKill(pid: number, startTime: number, name: string) {
    setConfirm({
      title: `Encerrar ${name} · PID ${pid}?`,
      description:
        "O processo será encerrado pelo sistema operacional. Trabalho não salvo poderá ser perdido. A identidade do processo será conferida novamente antes da ação.",
      run: async () => {
        await api("kill_process", { pid, startTime, confirmed: true });
        await refresh();
        setToast("Solicitação de encerramento enviada.");
      },
    });
  }
  async function hostingRefresh() {
    if (!selected) return;
    await sources.hosting.refresh();
  }
  const projectSelect = (
    <select
      aria-label="Projeto ativo"
      value={selectedId}
      onChange={(e) => setSelectedId(e.target.value)}
    >
      <option value="">Selecionar projeto</option>
      {projects.map((p) => (
        <option key={p.id} value={p.id}>
          {p.name}
        </option>
      ))}
    </select>
  );
  const gitPanel = <GitSummary />;
  const agentPanel = (
    <>
      {agent ? (
        <>
          <dl className="facts">
            <div>
              <dt>Claude CLI</dt>
              <dd>{agent.available ? "Disponível" : "Indisponível"}</dd>
            </div>
            <div>
              <dt>Skills encontradas</dt>
              <dd>{agent.skills.length}</dd>
            </div>
            <div>
              <dt>Instruções</dt>
              <dd>{agent.instructions.join(", ") || "Não encontradas"}</dd>
            </div>
          </dl>
          <p className="muted">{agent.mcpStatus}</p>
          <p className="muted">{agent.accountStatus}</p>
        </>
      ) : (
        <Empty title="Contexto conectado ao projeto">
          <p>Instruções, skills e configuração de MCP em um só lugar.</p>
        </Empty>
      )}
      <div className="quick-actions">
        <button
          className="button"
          disabled={!selected || busy}
          onClick={() => selected && launch(selected.id, "claude")}
        >
          <Bot size={14} />
          Abrir Claude
        </button>
        <button
          className="button primary"
          disabled={!selected || busy}
          onClick={() => void context()}
        >
          <FileText size={14} />
          Gerar contexto
        </button>
      </div>
    </>
  );
  const pageTitle = routes.find((r) => r.id === route)?.title ?? "Dashboard";
  const activePorts = selected
    ? ports.filter(
        (port) =>
          port.projectId === selected.id || port.expectedBy.includes(selected.id),
      )
    : [];
  const activeProcesses = selected
    ? processes.filter((process) => process.projectId === selected.id)
    : [];
  return (
    <>
    {desktop && <WindowTitleBar />}
    <div
      className={`app ${sidebarCompact ? "sidebar-compact" : ""} density-${density}`}
    >
      <Sidebar route={route} sidebarCompact={sidebarCompact} toggle={toggleSidebar} projectCount={projects.length} />
      <div className="workspace">
        <header className="topbar">
          <div className="breadcrumb">
            Workspace <ChevronRight size={13} />
            <span>{pageTitle}</span>
          </div>
          <button
            className="search-trigger"
            onClick={() => {
              setQuery("");
              setPalette(true);
            }}
          >
            <Search size={15} />
            <span>Buscar projetos e ações…</span>
            <kbd>Ctrl K</kbd>
          </button>
          <button
            className={`project-context ${selected ? "is-active" : ""}`}
            onClick={() => {
              setQuery("project ");
              setPalette(true);
            }}
            title="Trocar projeto · Ctrl+P"
          >
            <span className={`status-orb ${git?.clean ? "healthy" : git ? "warning" : "idle"}`} />
            <span>
              <strong>{selected?.name ?? "Selecionar projeto"}</strong>
              <small>
                {selected
                  ? `${git?.branch ?? "Git pendente"} · ${activeProcesses.length} proc · ${activePorts.length} ports`
                  : "Ctrl+P"}
              </small>
            </span>
            <ChevronRight size={14} />
          </button>
          <div className="top-status" title={machine ? `Este computador${health ? ` · ${HEALTH_LABEL[health]}` : ""}` : undefined}>
            <span className={`dot ${!desktop ? "" : health === "critical" ? "danger" : health === "attention" ? "amber" : "green"}`} />
            {desktop ? (machine?.name ?? "Desktop local") : "Prévia web"}
          </div>
          <SyncIndicator />
          <ActivityCenter />
          <div className="avatar">LK</div>
        </header>
        <main>
          {creating ? (
            <NewProject
              close={closeNewProject}
              done={(id) => {
                setSelectedId(id);
                closeNewProject();
                setToast("Projeto cadastrado.");
              }}
            />
          ) : (<>
          {/* O Dashboard é a visão da máquina (Concept 02) e tem cabeçalho próprio. */}
          {route !== "dashboard" && (<>
          <div className="page-heading">
            <div>
              <div className="eyebrow">DEVELOPMENT CONTROL CENTER</div>
              <h1>{pageTitle}</h1>
              <p>Contexto local. Ações explícitas. Informação verificável.</p>
            </div>
            <div className="heading-actions">
              {projectSelect}
              <Refresh onClick={() => void refresh()} busy={busy} />
              <button className="button primary" onClick={openNewProject}>
                <Plus size={16} />
                Novo projeto
              </button>
            </div>
          </div>
          {selected && (
            <section className="active-context" aria-label="Contexto do projeto ativo">
              <div className="active-context-main">
                <span className={`status-orb ${git?.clean ? "healthy" : git ? "warning" : "idle"}`} />
                <div>
                  <strong>{selected.name}</strong>
                  <span className="mono">{git?.branch ?? "branch pendente"}</span>
                  <span>{git ? (git.clean ? "clean" : `${git.staged + git.unstaged + git.untracked} mudanças`) : "Git não consultado"}</span>
                </div>
              </div>
              <div className="context-signals">
                <span><strong>{activeProcesses.length}</strong> processos</span>
                <span><strong>{activePorts.length}</strong> portas</span>
                <span><strong>{selected.ports.length}</strong> esperadas</span>
              </div>
              <div className="context-actions">
                <button className="button subtle" onClick={() => launch(selected.id, "terminal")}>
                  <Terminal size={14} /> Terminal
                </button>
                <button className="button subtle" onClick={() => launch(selected.id, "vscode")}>
                  <Code2 size={14} /> Code
                </button>
                <button className="button primary compact-action" onClick={() => void context()}>
                  <Zap size={14} /> Contexto
                </button>
              </div>
            </section>
          )}
          </>)}
          {!desktop && (
            <div className="preview-banner">
              <CircleHelp size={16} />
              <div>
                <strong>Prévia da interface</strong> · Abra o aplicativo desktop
                para acessar SQLite, Git e processos do seu computador. Nenhuma
                conexão está sendo simulada.
              </div>
            </div>
          )}
          {error && (
            <div className="error global-error" role="alert">
              <span>{error}</span>
              <button
                className="icon-button"
                onClick={() => setError("")}
                aria-label="Fechar erro"
              >
                <X size={16} />
              </button>
            </div>
          )}
          {route === "dashboard" && <MachineHealth />}
          {route === "projects" && (
            <>
              <Projects
                projects={projects}
                open={setSelectedId}
                launch={launch}
                add={openNewProject}
                edit={setForm}
                remove={remove}
              />
              {selected && <ProjectRuntime project={selected} context={() => void context()} report={report} />}
              <RuntimeDesktopOnly />
              {selected && (
                <Panel
                  title={selected.name}
                  icon={<FolderOpen size={18} />}
                  className="detail-panel"
                  action={<Badge>Projeto ativo</Badge>}
                >
                  <p>{selected.description}</p>
                  {selected.localPath && (
                    <div className="path-box mono">{selected.localPath}</div>
                  )}
                  <LocationNotice project={selected} report={report} />
                  <Launchers
                    id={selected.id}
                    launch={launch}
                    context={() => void context()}
                  />
                  <div className="detail-grid">
                    <div>
                      <h3>Estado Git</h3>
                      {gitPanel}
                    </div>
                    <div>
                      <h3>Serviços / portas declaradas</h3>
                      {selected.ports.length ? (
                        selected.ports.map((p) => (
                          <div className="service-row" key={p.port}>
                            <span>
                              {p.name} <code>:{p.port}</code>
                            </span>
                            <Badge
                              tone={
                                ports.some((port) => port.port === p.port)
                                  ? "blue"
                                  : "neutral"
                              }
                            >
                              {ports.some((port) => port.port === p.port)
                                ? "Porta ocupada · vínculo a verificar"
                                : "Não observada"}
                            </Badge>
                          </div>
                        ))
                      ) : (
                        <p className="muted">
                          Edite o projeto para declarar portas.
                        </p>
                      )}
                      <h3>Comandos declarados</h3>
                      {selected.commands.map((c) => (
                        <div className="path-box" key={c.name}>
                          <strong>{c.name}</strong>
                          <code>
                            {c.program} {c.args.join(" ")}
                          </code>
                        </div>
                      ))}
                      <p className="footnote">
                        Estes comandos são só metadados do cadastro e nunca são
                        executados. O painel Runtime roda apenas scripts do
                        package.json local.
                      </p>
                    </div>
                  </div>
                </Panel>
              )}
            </>
          )}
          {route === "ports" && (
            <Panel title="Sockets locais · TCP em escuta / UDP">
              <SourceStatus source={workspace.ports} label="Portas" />
              <Ports
                ports={ports}
                projects={projects}
                refresh={() => void workspace.ports.refresh()}
                busy={portsSource.loading}
                confirmKill={confirmKill}
                report={report}
                associate={(id, port) => void associatePort(id, port)}
              />
            </Panel>
          )}
          {route === "processes" && (
            <Panel title="Processos do sistema">
              <SourceStatus source={workspace.processes} label="Processos" />
              <Processes
                processes={processes}
                projects={projects}
                refresh={() => void workspace.processes.refresh()}
                busy={processesSource.loading}
                confirmKill={confirmKill}
              />
            </Panel>
          )}
          {route === "prompts" && (
            <Prompts
              prompts={prompts}
              projects={projects}
              selected={selected}
              git={git}
              refresh={() => void loadRegistry()}
              report={report}
              notify={setToast}
            />
          )}
          {route === "repositories" && <Repositories report={report} />}
          {route === "git" && (
            <div className="two-columns">
              <Panel
                title="Git local"
                action={
                  <button
                    className="text-button"
                    disabled={!selected || busy}
                    onClick={() => { if (selected) void sources.git.refresh(); }}
                  >
                    Atualizar Git
                  </button>
                }
              >
                {gitPanel}
                {git && (
                  <>
                    <h3>Commits recentes</h3>
                    <p className="muted">{git.stashes ?? 0} stashes · {git.remote ?? "Remote HTTPS não informado"}</p>
                    <h3>Arquivos modificados</h3>
                    {(git.files ?? []).map((file, index) => <div className="commit" key={`${file.path}-${index}`}><code>{file.status}</code><span className="mono">{file.original ? `${file.original} → ` : ""}{file.path}</span></div>)}
                    {git.clean && <p className="muted">Árvore de trabalho limpa.</p>}
                    {git.commits.map((c) => (
                      <div className="commit" key={c.hash}>
                        <code>{c.hash}</code>
                        <span>{c.subject}</span>
                      </div>
                    ))}
                    <p className="footnote">
                      Ahead/behind usam refs locais. Nenhum fetch automático.
                    </p>
                  </>
                )}
              </Panel>
              <Panel
                title="GitHub · gh CLI"
                icon={<GitPullRequest size={17} />}
                action={
                  <button
                    className="button"
                    disabled={!selected || busy}
                    onClick={() => void hostingRefresh()}
                  >
                    Consultar GitHub
                  </button>
                }
              >
                <SourceStatus source={sources.hosting} label="GitHub" />
                {hosting ? (
                  <>
                    {hosting.pullRequests.length === 0 && (
                      <p>Nenhuma PR aberta encontrada.</p>
                    )}
                    {hosting.pullRequests.map((pr) => (
                      <article className="pr" key={pr.number}>
                        <h3>
                          #{pr.number} · {pr.title}
                        </h3>
                        <p className="mono">{pr.headRefName}</p>
                        <div className="tags">
                          <Badge>
                            Review: {pr.reviewDecision || "Pendente"}
                          </Badge>
                          <Badge>Merge: {pr.mergeable || "Desconhecido"}</Badge>
                        </div>
                        {pr.statusCheckRollup.map((c, i) => (
                          <p key={i}>
                            {c.name || "Check"}:{" "}
                            {c.conclusion || c.state || c.status || "Pendente"}
                          </p>
                        ))}
                      </article>
                    ))}
                    <h3>Issues abertas</h3>
                    {hosting.issues.map((i) => (
                      <p key={i.number}>
                        #{i.number} · {i.title}
                      </p>
                    ))}
                  </>
                ) : (
                  <Empty title="Consulta sob demanda">
                    <p>
                      Usa sua autenticação existente no gh. Nenhum token é
                      armazenado.
                    </p>
                  </Empty>
                )}
              </Panel>
            </div>
          )}
          {route === "agents" && (
            <div className="two-columns">
              <Panel title="Claude provider" icon={<Bot size={18} />}>
                {agentPanel}
                {agent && (
                  <>
                    <h3>Skills do projeto</h3>
                    {agent.skills.map((s) => (
                      <p key={s}>{s}</p>
                    ))}
                  </>
                )}
              </Panel>
              <Panel title="Providers locais">
                <AgentProviders />
              </Panel>
            </div>
          )}
          {route === "worktrees" && <Worktrees key={selectedId} />}
          {route === "terminal" && (
            <Panel title="Launchers nativos" icon={<Terminal size={18} />}>
              {selected ? (
                <>
                  <p>
                    Abrir ferramentas no diretório de{" "}
                    <strong>{selected.name}</strong>.
                  </p>
                  {selected.localPath && (
                    <div className="path-box mono">{selected.localPath}</div>
                  )}
                  <LocationNotice project={selected} report={report} />
                  <Launchers
                    id={selected.id}
                    launch={launch}
                    context={() => void context()}
                  />
                  <p className="footnote">
                    Windows Terminal e Claude CLI nativa precisam estar no PATH.
                    Terminal integrado previsto para fase futura.
                  </p>
                </>
              ) : (
                <Empty title="Selecione ou cadastre um projeto" />
              )}
            </Panel>
          )}
          {route === "environments" && (
            <Panel title="Ambientes">
              <Empty title="Validação de variáveis · v0.2">
                <p>
                  Esta versão informa a existência de .env no contexto gerado.
                  Valores não são lidos, exibidos ou armazenados. A validação de
                  nomes e chaves ainda não foi implementada.
                </p>
              </Empty>
            </Panel>
          )}
          {route === "knowledge" && <Knowledge />}
          {route === "settings" && <ThisMachine />}
          {route === "settings" && (
            <div className="two-columns">
              <Panel title="Aplicativo">
                <dl className="facts">
                  <div>
                    <dt>Versão</dt>
                    <dd>0.1.0 · Foundation</dd>
                  </div>
                  <div>
                    <dt>Modo</dt>
                    <dd>{desktop ? "Desktop Tauri" : "Prévia web"}</dd>
                  </div>
                  <div>
                    <dt>Persistência</dt>
                    <dd>SQLite · app data / hub.db</dd>
                  </div>
                  <div>
                    <dt>Telemetria</dt>
                    <dd>Não implementada</dd>
                  </div>
                  <div>
                    <dt>Atualização</dt>
                    <dd>Manual</dd>
                  </div>
                </dl>
                <div className="settings-group">
                  <div>
                    <strong>Densidade</strong>
                    <p className="muted">Ajusta tabelas, navegação e superfícies.</p>
                  </div>
                  <div className="segmented" role="group" aria-label="Densidade da interface">
                    <button
                      className={density === "comfortable" ? "active" : ""}
                      onClick={() => setDensity("comfortable")}
                    >
                      Comfortable
                    </button>
                    <button
                      className={density === "compact" ? "active" : ""}
                      onClick={() => setDensity("compact")}
                    >
                      Compact
                    </button>
                  </div>
                </div>
                <div className="settings-group">
                  <div>
                    <strong>Sidebar compacta</strong>
                    <p className="muted">Mantém apenas ícones e tooltips.</p>
                  </div>
                  <button
                    className={`toggle ${sidebarCompact ? "active" : ""}`}
                    role="switch"
                    aria-checked={sidebarCompact}
                    onClick={() => setSidebarCompact((value) => !value)}
                  >
                    <span />
                  </button>
                </div>
                <p className="muted">
                  As CLIs são resolvidas pelo PATH na inicialização.
                  Configuração de caminhos customizados fica para uma próxima
                  versão.
                </p>
              </Panel>
              <Panel
                title="Segurança por padrão"
                icon={<ShieldCheck size={18} />}
              >
                <p>
                  Execução nativa isolada no Rust. Sem shell genérico exposto ao
                  frontend.
                </p>
                <p>
                  Kill de processo e remoção de cadastro exigem confirmação. Não
                  há ações de merge, push ou reset.
                </p>
                <p className="warn-text">
                  Não inclua segredos nos campos, comandos ou templates. Eles
                  são armazenados localmente sem criptografia.
                </p>
                <h3>Keyboard shortcuts</h3>
                <div className="shortcut-list">
                  <div><span>Command Center</span><kbd>Ctrl K</kbd></div>
                  <div><span>Trocar projeto</span><kbd>Ctrl P</kbd></div>
                  <div><span>Terminal do projeto</span><kbd>Ctrl `</kbd></div>
                  <div><span>Fechar modal ou palette</span><kbd>Esc</kbd></div>
                  <div><span>Navegar resultados</span><span><kbd>↑</kbd> <kbd>↓</kbd> <kbd>Enter</kbd></span></div>
                </div>
              </Panel>
            </div>
          )}
          </>)}
          <footer className="statusbar">
            <span>
              <span className={`dot ${desktop ? "green" : ""}`} />
              {desktop
                ? "SQLite local · Tauri 2"
                : "Integrações nativas indisponíveis nesta prévia"}
            </span>
            <span>
              {projects.length} projetos <span className="separator">/</span>{" "}
              {busy
                ? "Consultando…"
                : lastRefresh
                  ? `Atualizado ${lastRefresh}`
                  : "Sem consulta nativa"}
            </span>
          </footer>
        </main>
      </div>
      {form && (
        <ProjectForm
          project={form === "new" ? undefined : form}
          close={closeForm}
          saved={() => void loadRegistry()}
        />
      )}
      {snapshot !== null && (
        <Modal title="Development Context" close={closeSnapshot} wide>
          <div className="form-body">
            <p className="muted">
              Revise antes de compartilhar: inclui caminhos locais, nomes de
              projetos e metadados Git.
            </p>
            <pre className="context-preview">{snapshot}</pre>
          </div>
          <div className="modal-footer">
            <button
              className="button"
              onClick={() =>
                void navigator.clipboard
                  .writeText(snapshot)
                  .then(() => setToast("Contexto copiado."))
                  .catch(report)
              }
            >
              <Copy size={15} />
              Copiar
            </button>
            <button
              className="button primary"
              onClick={() =>
                selected &&
                void api<boolean>("save_context", { id: selected.id })
                  .then((saved) => {
                    if (saved) setToast("Contexto salvo.");
                  })
                  .catch(report)
              }
            >
              Salvar Markdown…
            </button>
          </div>
        </Modal>
      )}
      {confirm && (
        <Modal title={confirm.title} close={closeConfirm}>
          <div className="form-body">
            <p>{confirm.description}</p>
          </div>
          <div className="modal-footer">
            <button className="button" disabled={busy} onClick={closeConfirm}>
              Cancelar
            </button>
            <button
              className="button danger"
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void confirm
                  .run()
                  .then(closeConfirm)
                  .catch(report)
                  .finally(() => setBusy(false));
              }}
            >
              Confirmar ação
            </button>
          </div>
        </Modal>
      )}
      {palette && <CommandPalette initialQuery={query} close={closePalette} navigate={navigate} context={() => void context()} report={report} confirmKill={confirmKill} />}
      {toast && (
        <div className="toast" role="status">
          <Check size={17} />
          {toast}
        </div>
      )}
    </div>
    </>
  );
}
