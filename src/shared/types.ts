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
  /** Observação desta máquina (list_projects), nunca salva: pasta existe, sumiu ou nunca foi vinculada. */
  location?: "available" | "missing" | "unbound";
}
export interface BindResult {
  bound: boolean;
  needsConfirmation: boolean;
  message: string;
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

// ---- Project Runtime Manager (estado desta máquina; nunca vai para o workspace portátil)
export type ScriptKind = "service" | "task" | "other";
export type RunState = "starting" | "running" | "stopping" | "stopped" | "failed" | "completed";
export type RuntimeStatus = "unbound" | "missing" | "ready" | "running" | "partial" | "error";
export interface RuntimeScript {
  name: string;
  command: string;
  kind: ScriptKind;
}
export interface RuntimeGit {
  isRepo: boolean;
  branch: string;
  detached: boolean;
  upstream: string | null;
  ahead: number | null;
  behind: number | null;
  staged: number;
  unstaged: number;
  untracked: number;
  conflicts: number;
  changes: number;
  clean: boolean;
  error: string | null;
}
export type RunSource = "node" | "cargo" | "tauri" | "compose";
export type ActionGroup = "run" | "quality" | "build" | "control";
export interface CommandChoice {
  id: string;
  label: string;
}
/** Ação executável do projeto. Vem do backend (arquivos locais); a interface só devolve o id. */
export interface RuntimeCommand {
  id: string;
  label: string;
  detail: string;
  source: RunSource;
  program: string;
  args: string[];
  cwd: string;
  group: ActionGroup;
  kind: ScriptKind;
  longRunning: boolean;
  observer: boolean;
  choices: CommandChoice[];
  selectionRequired: boolean;
  available: boolean;
  unavailableReason: string | null;
}
export interface ToolStatus {
  id: string;
  label: string;
  available: boolean;
  version: string | null;
  reason: string | null;
}
export interface StackComposition {
  headline: string;
  parts: { role: string; label: string }[];
}
export interface ComposeServiceRuntime {
  name: string;
  state: "running" | "restarting" | "paused" | "exited" | "created" | "dead" | "absent" | string;
  health: string | null;
  exitCode: number | null;
  ports: { published: number | null; target: number; protocol: string }[];
  profiles: string[];
}
export interface ComposeRuntime {
  file: string;
  overrideFiles: string[];
  note: string | null;
  projectName: string | null;
  services: ComposeServiceRuntime[];
  containers: number;
  running: number;
  expected: number;
  startedHere: boolean;
  error: string | null;
}
export interface RunInfo {
  id: string;
  projectId: string;
  script: string;
  commandId: string;
  label: string;
  source: RunSource;
  observer: boolean;
  selection: string | null;
  command: string;
  kind: ScriptKind;
  state: RunState;
  pid: number | null;
  exitCode: number | null;
  startedAt: number;
  lastSeq: number;
}
export interface ProjectRuntime {
  projectId: string;
  status: RuntimeStatus;
  statusDetail: string | null;
  stack: { id: string; label: string; evidence: string }[];
  packageManager: { name: string; evidence: string } | null;
  packageManagerNote: string | null;
  scripts: RuntimeScript[];
  git: RuntimeGit | null;
  processes: { pid: number; name: string; memory: number; startTime: number; managed: boolean; evidence: string }[];
  ports: { port: number; protocol: string; pid: number | null; process: string; managed: boolean }[];
  declaredPorts: { name: string; port: number; state: "listening" | "free" | "occupied_unverified" }[];
  services: { label: string; port: number | null; pid: number | null; managed: boolean }[];
  runs: RunInfo[];
  externalRunning: boolean;
  canRun: boolean;
  runBlockedReason: string | null;
  tauri: { version: number | null; versionEvidence: string | null; confDir: string; devUrlPort: number | null; hasScript: boolean; localCli: boolean } | null;
  rust: { dir: string; workspace: boolean; virtualManifest: boolean; packages: string[]; bins: { package: string; name: string; packageDir: string; isTauri: boolean }[]; defaultRun: string | null; notes: string[] } | null;
  docker: { dockerfile: boolean; composeFiles: string[]; overrideFiles: string[]; kind: "compose" | "dockerfile" } | null;
  composition: StackComposition;
  tools: ToolStatus[];
  commands: RuntimeCommand[];
  primaryCommand: string | null;
  compose: ComposeRuntime | null;
  lastTask: { command: string; state: RunState; exitCode: number | null } | null;
}
export interface LogLine {
  seq: number;
  stream: "out" | "err";
  text: string;
  /** Quem escreveu, quando reconhecível (tauri dev mistura Tauri, Vite e Cargo). */
  source: "cargo" | "vite" | "tauri" | null;
}
export interface LogChunk {
  lines: LogLine[];
  nextSeq: number;
  truncated: boolean;
}
export type RuntimeEvent =
  | { kind: "state"; projectId: string; runId: string; state: RunState; pid: number | null; exitCode: number | null }
  | { kind: "output"; projectId: string; runId: string; seq: number };
