export interface ProjectPort {
  name: string;
  port: number;
}
export interface ProjectCommand {
  name: string;
  program: string;
  args: string[];
}
export interface ProjectInput {
  name: string;
  description: string;
  localPath: string;
  repository: string;
  stack: string[];
  tags: string[];
  ports: ProjectPort[];
  commands: ProjectCommand[];
}
export interface Project extends ProjectInput {
  id: string;
  slug: string;
  createdAt: string;
  updatedAt: string;
  /** Observação desta máquina (list_projects): a pasta existe aqui? Nunca é salva. */
  pathAvailable?: boolean;
}
export interface Discovery {
  name: string;
  localPath: string;
  repository: string;
  stack: string[];
  isGit: boolean;
}
export interface Integration {
  name: string;
  status: "available" | "unavailable" | "connected" | "not_configured";
  detail: string;
}
export interface SystemState {
  cpu: number | null;
  memoryUsed: number;
  memoryTotal: number;
  diskUsed: number;
  diskTotal: number;
  integrations: Integration[];
}
export interface WorkspaceState {
  system: SystemState;
  ports: PortInfo[];
  processes: ProcessInfo[];
  portError: string | null;
}
export interface GitState {
  files: { path: string; status: string; original: string | null }[];
  remote: string | null;
  stashes: number;
  branch: string;
  head: string;
  upstream: string | null;
  ahead: number | null;
  behind: number | null;
  staged: number;
  unstaged: number;
  untracked: number;
  clean: boolean;
  commits: { hash: string; subject: string }[];
  hasOrigin: boolean;
}
export interface PortInfo {
  port: number;
  address: string;
  protocol: string;
  pid: number | null;
  process: string;
  executable: string | null;
  startTime: number | null;
  projectId: string | null;
  expectedBy: string[];
  confidence: string;
  conflict: boolean;
}
export interface ProcessInfo {
  pid: number;
  name: string;
  executable: string | null;
  memory: number;
  startTime: number;
  projectId: string | null;
  confidence: string;
}
export interface Prompt {
  id: string;
  title: string;
  category: string;
  projectId: string | null;
  body: string;
}
export interface Activity {
  id: number;
  projectId: string | null;
  action: string;
  createdAt: string;
}
export interface KnowledgeEntry {
  id: string;
  projectId: string | null;
  title: string;
  kind: "note" | "decision" | "architecture" | "bug" | "documentation";
  body: string;
  tags: string;
  updatedAt: string;
}
export interface AgentContext {
  provider: string;
  available: boolean;
  accountStatus: string;
  instructions: string[];
  skills: string[];
  mcpStatus: string;
  sessionsStatus: string;
}
export interface AgentProviderStatus {
  provider: string;
  availability: "available" | "unavailable" | "unsupported" | "stale";
  accountStatus: string;
  usageStatus: string;
  sessionsStatus: string;
  detail: string;
}
export interface Worktree {
  path: string;
  head: string;
  branch: string;
  locked: boolean;
}
export interface PullRequest {
  number: number;
  title: string;
  url: string;
  headRefName: string;
  reviewDecision: string;
  mergeable: string;
  statusCheckRollup: {
    name?: string;
    conclusion?: string;
    status?: string;
    state?: string;
  }[];
}
export interface HostingState {
  provider: string;
  authenticated: boolean;
  pullRequests: PullRequest[];
  issues: { number: number; title: string; url: string }[];
}
