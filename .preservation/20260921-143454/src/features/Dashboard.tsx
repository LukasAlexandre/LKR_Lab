import { memo } from "react";
import type { ReactNode } from "react";
import { Activity as ActivityIcon, ArrowRight, Blocks, Bot, Cpu, FileText, FolderOpen, GitBranch, Network, Plus, Workflow } from "lucide-react";
import { desktop } from "../shared/api";
import { bytes, percent } from "../shared/logic";
import { Panel } from "../shared/ui";
import { useResource, workspace } from "../state/workspace";
import { ProjectCard } from "./Projects";
import { Ports } from "./Ports";

export const Dashboard = memo(function Dashboard({ navigate, openProject, launch, add, confirmKill, report, gitPanel, agentPanel }: {
 navigate: (id: string) => void; openProject: (id: string) => void;
 launch: (id: string, action: string) => void; add: () => void;
 confirmKill: (pid: number, startTime: number, name: string) => void;
 report: (error: unknown) => void; gitPanel: ReactNode; agentPanel: ReactNode;
}) {
 const environment = useResource(workspace.environment);
 const system = environment.data;
 const { data: projects } = useResource(workspace.projects);
 const portsSource = useResource(workspace.ports);
 const ports = portsSource.data;
 const { data: activities } = useResource(workspace.activities);
 const lastRefresh = environment.lastUpdated ? new Date(environment.lastUpdated).toLocaleTimeString("pt-BR") : "";
 return (<>
              <div className="dashboard-top">
                <Panel
                  title="Resumo do ambiente"
                  icon={<Blocks size={17} />}
                  action={
                    <span className="muted small">
                      {lastRefresh || "Aguardando consulta"}
                    </span>
                  }
                >
                  <div className="integrations">
                    {(
                      system?.integrations ??
                      [
                        "GitHub",
                        "Git",
                        "Claude",
                        "Docker",
                        "MySQL",
                        "Obsidian",
                      ].map((name) => ({
                        name,
                        status: "unknown",
                        detail: "Detecção disponível no aplicativo desktop",
                      }))
                    )
                      .slice(0, 9)
                      .map((i) => (
                        <div
                          className="integration"
                          key={i.name}
                          title={i.detail}
                        >
                          <div className="row spread">
                            <strong>{i.name}</strong>
                            <span
                              className={`dot ${i.status === "connected" ? "green" : i.status === "available" ? "blue" : ""}`}
                            />
                          </div>
                          <small>
                            {
                              (
                                {
                                  connected: "Conectado",
                                  available: "Disponível",
                                  unavailable: "Indisponível",
                                  unknown: environment.loading ? "Consultando…" : "Não verificado",
                                  not_configured: desktop
                                    ? "Não configurado"
                                    : "Não verificado",
                                } as Record<string, string>
                              )[i.status]
                            }
                          </small>
                        </div>
                      ))}
                  </div>
                </Panel>
                <Panel title="Performance do sistema" icon={<Cpu size={17} />}>
                  <div className="metrics">
                    {[
                      {
                        label: "CPU",
                        value: system?.cpu != null ? Math.round(system.cpu) : null,
                        detail: "Uso atual",
                      },
                      {
                        label: "Memória",
                        value: system
                          ? percent(system.memoryUsed, system.memoryTotal)
                          : null,
                        detail: system
                          ? `${bytes(system.memoryUsed)} / ${bytes(system.memoryTotal)}`
                          : "RAM do sistema",
                      },
                      {
                        label: "Disco",
                        value: system
                          ? percent(system.diskUsed, system.diskTotal)
                          : null,
                        detail: "Volumes montados",
                      },
                    ].map((m) => (
                      <div className="metric" key={m.label}>
                        <div
                          className="metric-ring"
                          style={{
                            background: `conic-gradient(var(--accent) ${(m.value ?? 0) * 3.6}deg, #1b2b3c 0deg)`,
                          }}
                        >
                          <span>{m.value === null ? "—" : `${m.value}%`}</span>
                        </div>
                        <strong>{m.label}</strong>
                        <small>{m.detail}</small>
                      </div>
                    ))}
                  </div>
                </Panel>
              </div>
              <div className="dashboard-middle">
                <Panel
                  title="Projetos recentes"
                  icon={<FolderOpen size={17} />}
                  action={
                    <button
                      className="text-button"
                      onClick={() => navigate("projects")}
                    >
                      Ver todos <ArrowRight size={13} />
                    </button>
                  }
                >
                  {projects.length ? (
                    <div className="recent-projects">
                      {projects.slice(0, 3).map((p) => (
                        <ProjectCard
                          key={p.id}
                          project={p}
                          open={openProject}
                          launch={launch}
                        />
                      ))}
                    </div>
                  ) : (
                    <div className="onboarding">
                      <div className="onboarding-icon">
                        <FolderOpen size={31} />
                      </div>
                      <h3>Conecte seu primeiro projeto</h3>
                      <p>
                        Reúna repositório, serviços e contexto de IA.
                        <br />
                        Tudo começa pela pasta do seu projeto.
                      </p>
                      <button
                        className="button primary"
                        onClick={add}
                      >
                        <Plus size={16} />
                        Adicionar projeto existente
                      </button>
                      <small>
                        Detecção de stack e Git · Dados locais em SQLite
                      </small>
                    </div>
                  )}
                </Panel>
                <Panel
                  title="Monitor de portas"
                  icon={<Network size={17} />}
                  action={
                    <button
                      className="text-button"
                      onClick={() => navigate("ports")}
                    >
                      Ver todas <ArrowRight size={13} />
                    </button>
                  }
                >
                  <Ports
                    ports={ports}
                    projects={projects}
                    refresh={() => void workspace.ports.refresh()}
                    busy={portsSource.loading}
                    confirmKill={confirmKill}
                    report={report}
                    compact
                  />
                </Panel>
              </div>
              <div className="dashboard-bottom">
                <Panel
                  title="Git / GitHub"
                  icon={<GitBranch size={17} />}
                  action={
                    <button
                      className="text-button"
                      onClick={() => navigate("git")}
                    >
                      Detalhes
                    </button>
                  }
                >
                  {gitPanel}
                  <div className="panel-bottom-note">
                    <span className="dot" />
                    PR e CI: consulta explícita em Git / PRs
                  </div>
                </Panel>
                <Panel title="AI Workspace" icon={<Bot size={17} />}>
                  {agentPanel}
                </Panel>
                <Panel
                  title="Seu fluxo, com contexto"
                  icon={<Workflow size={17} />}
                >
                  <div className="workflow-step">
                    <span>01</span>
                    <div>
                      <strong>Conecte um projeto</strong>
                      <p>Pasta, repositório, stack e portas.</p>
                    </div>
                  </div>
                  <div className="workflow-step">
                    <span>02</span>
                    <div>
                      <strong>Confira o ambiente</strong>
                      <p>Git, processos e ferramentas reais.</p>
                    </div>
                  </div>
                  <div className="workflow-step">
                    <span>03</span>
                    <div>
                      <strong>Gere o próximo contexto</strong>
                      <p>Leve o estado atual para seu agente.</p>
                    </div>
                  </div>
                  <button
                    className="button full"
                    onClick={() => navigate("prompts")}
                  >
                    <FileText size={15} />
                    Explorar prompts
                    <ArrowRight size={14} />
                  </button>
                </Panel>
              </div>
              <Panel
                title="Atividade recente"
                icon={<ActivityIcon size={17} />}
                className="activity-panel"
              >
                {activities.length ? (
                  <div className="activity-list">
                    {activities.slice(0, 4).map((a) => (
                      <div key={a.id}>
                        <span className="dot blue" />
                        <div>
                          <small>
                            {new Date(a.createdAt).toLocaleTimeString("pt-BR")}
                          </small>
                          <p>{a.action}</p>
                        </div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="muted">
                    As ações realizadas no aplicativo aparecerão aqui.
                  </p>
                )}
              </Panel>
            </>);
});
