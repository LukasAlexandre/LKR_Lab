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
  /** Identidade portátil do repositório (opcional; projetos antigos não têm). */
  locator?: RepositoryLocator;
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
/** Um Git worktree REAL (git worktree list --porcelain). O path é da máquina; nunca portátil. */
export interface Worktree {
  path: string;
  head: string;
  /** Vazia quando detached ou bare. */
  branch: string;
  isPrimary: boolean;
  detached: boolean;
  bare: boolean;
  locked: boolean;
  lockedReason?: string | null;
  prunable: boolean;
  prunableReason?: string | null;
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
  endedAt: number | null;
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
  /** Instante (ms desde a época Unix) em que o LKR LAB recebeu a linha. */
  ts: number;
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
/** Machine Registry (hub-core::machine). Estado desta máquina: nunca é portátil. */
export type MachineUsage = "home" | "work" | "other";
export interface MachineSnapshot {
  hostname: string | null;
  osName: string | null;
  osVersion: string | null;
  osBuild: string | null;
  cpuModel: string | null;
  cpuCores: number | null;
  cpuThreads: number | null;
  memoryTotal: number | null;
  gpus: { name: string; memory: number | null }[];
  storage: { mount: string; kind: "ssd" | "hdd" | "unknown"; total: number; removable: boolean }[];
  networkInterfaces: { name: string; ipv4: string[] }[];
  activeInterface: string | null;
  localIpv4: string | null;
  uptime: number | null;
  detectedAt: number;
}
export interface Machine {
  machineId: string;
  name: string;
  usage: MachineUsage;
  description: string;
  createdAt: string;
  updatedAt: string;
  lastDetectedAt: number | null;
}
export interface MachineInput {
  name: string;
  usage: MachineUsage;
  description: string;
}
export interface MachineStatus {
  registered: boolean;
  machine: Machine | null;
  snapshot: MachineSnapshot | null;
  stale: boolean;
  ttlMs: number;
}
/** Machine Telemetry / Health (hub-core::telemetry, hub-core::health). Estado dinâmico, nunca persistido. */
export type Availability = "available" | "unavailable";
export type HealthStatus = "healthy" | "attention" | "critical";
export type SensorLevel = "normal" | "attention" | "critical" | "unrated" | "unavailable";
export interface TelemetryCapabilities {
  cpuUsage: Availability;
  cpuClock: Availability;
  memoryUsage: Availability;
  gpuUsage: Availability;
  gpuMemory: Availability;
  gpuProcessUsage: Availability;
  cpuPackageTemperature: Availability;
  gpuTemperature: Availability;
  storageTemperature: Availability;
  thermalZoneTemperature: Availability;
  motherboardTemperature: Availability;
  diskIo: Availability;
  diskActivity: Availability;
  networkRate: Availability;
  processDiskIo: Availability;
  storagePhysicalHealth: Availability;
  cpuPerCore: Availability;
  cpuBaseClock: Availability;
  memoryCommit: Availability;
  battery: Availability;
  batteryHealth: Availability;
  diskPerDevice: Availability;
  networkInterfaces: Availability;
}
/** Quão completa é a leitura de um domínio: a ausência de sensor NÃO é falha. */
export type Domain = "available" | "partial" | "unavailable";
export interface DomainAvailability {
  cpu: Domain;
  memory: Domain;
  gpu: Domain;
  disk: Domain;
  network: Domain;
  battery: Domain;
  temperatures: Domain;
}
export interface DiskDevice {
  instance: string;
  number: number;
  model: string | null;
  nvme: boolean;
  volumes: string[];
  readPerSec: number | null;
  writePerSec: number | null;
  readOpsPerSec: number | null;
  writeOpsPerSec: number | null;
  activity: number | null;
}
export interface NetworkInterfaceTelemetry {
  name: string;
  description: string | null;
  kind: "ethernet" | "wifi" | "tunnel" | "other" | string;
  up: boolean | null;
  linkSpeedBps: number | null;
  ipv4: string[];
  ipv6: string[];
  receivedBytes: number;
  sentBytes: number;
  downloadBps: number | null;
  uploadBps: number | null;
  active: boolean;
}
export interface BatteryTelemetry {
  present: boolean;
  percent: number | null;
  charging: boolean | null;
  acOnline: boolean | null;
  remainingSecs: number | null;
}
export interface ProcessEntry {
  pid: number;
  name: string;
  cpu: number;
  memory: number;
  gpu: number | null;
  diskRead: number;
  diskWrite: number;
}
export type ProcessMetric = "cpu" | "memory" | "gpu" | "disk";
export interface TemperatureReading {
  id: string;
  label: string;
  source: "cpu" | "gpu" | "storage" | "thermalZone" | "motherboard";
  celsius: number | null;
  warning: number | null;
  critical: number | null;
  level: SensorLevel;
}
export interface MachineAlert {
  severity: HealthStatus;
  source: string;
  title: string;
  detail: string;
}
export interface Telemetry {
  timestamp: number;
  active: boolean;
  cpu: { usage: number; clockMhz: number | null; baseMhz: number | null; cores: number[]; ready: boolean };
  memory: {
    total: number;
    used: number;
    available: number;
    percent: number;
    swapTotal: number;
    swapUsed: number;
    commitUsed: number | null;
    commitLimit: number | null;
    pagefileUsed: number | null;
  };
  gpus: {
    /** Identidade da GPU neste boot (endereço PCI): distingue placas do mesmo modelo. */
    id: string;
    name: string;
    usage: number | null;
    dedicatedUsed: number | null;
    dedicatedTotal: number | null;
    sharedUsed: number | null;
    sharedTotal: number | null;
    temperature: number | null;
    vendor: string | null;
    driverVersion: string | null;
    capabilities: { usage: Availability; dedicatedMemory: Availability; sharedMemory: Availability; temperature: Availability };
  }[];
  diskIo: {
    readPerSec: number;
    writePerSec: number;
    activity: number | null;
    busiestDisk: string | null;
    devices: DiskDevice[];
    ready: boolean;
  };
  volumes: { mount: string; kind: string; total: number; available: number; removable: boolean }[];
  network: {
    interface: string | null;
    ipv4: string | null;
    downloadBps: number;
    uploadBps: number;
    interfaces: NetworkInterfaceTelemetry[];
    ready: boolean;
  };
  temperatures: TemperatureReading[];
  processes: { cpu: ProcessEntry[]; memory: ProcessEntry[]; gpu: ProcessEntry[]; disk: ProcessEntry[]; total: number } | null;
  uptime: number;
  bootTime: number;
  battery: BatteryTelemetry;
  capabilities: TelemetryCapabilities;
  availability: DomainAvailability;
  health: {
    status: HealthStatus;
    alerts: MachineAlert[];
    checks: { id: string; label: string; ok: boolean }[];
  };
}
export interface TelemetryPoint {
  at: number;
  cpu: number;
  memory: number;
  disk: number | null;
  gpu: number | null;
  downloadBps: number;
  uploadBps: number;
}
export interface TelemetryState {
  latest: Telemetry | null;
  history: TelemetryPoint[];
}
/** Identidade do repositório: remote canônico + subpasta dentro do repositório. Nunca caminho local. */
export interface RepositoryLocator {
  remote: string;
  path: string;
}
export interface GitInspection {
  branch: string;
  detached: boolean;
  remoteName: string | null;
  remote: string | null;
  remotes: string[];
  clean: boolean;
  changes: number;
  subpath: string;
}
export type RegistrationStatus = "new" | "already_here" | "known" | "ambiguous" | "invalid";
export interface MatchedProject {
  id: string;
  name: string;
  location: "available" | "missing" | "unbound";
  boundPath: string | null;
  canLocate: boolean;
  reason: string | null;
}
export interface Registration {
  status: RegistrationStatus;
  matches: MatchedProject[];
  message: string;
}
export interface ProjectInspection {
  folder: string;
  valid: boolean;
  error: string | null;
  suggestedName: string;
  git: GitInspection | null;
  locator: RepositoryLocator | null;
  locatorNote: string | null;
  repository: string;
  stack: { id: string; label: string; evidence: string }[];
  packageManager: { name: string; evidence: string } | null;
  packageManagerNote: string | null;
  scripts: { name: string; kind: "service" | "task" | "other" }[];
  importantFiles: string[];
  structure: { name: string; kind: "dir" | "file" }[];
  registration: Registration;
  warnings: string[];
}
export interface RegisterResult {
  registered: boolean;
  project: Project | null;
  registration: Registration;
}

/** Página Projetos (Concept 03): uma dimensão consultada sozinha (Git, runtime). */
export interface Dimension<T> {
  status: "available" | "not_applicable" | "error";
  data: T | null;
  message: string | null;
}
export interface GitSummary {
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
export interface RuntimeSummary {
  running: boolean;
  managedRuns: number;
  listeningPorts: number[];
}
export interface ProjectOverview extends Project {
  location: "available" | "missing" | "unbound";
  git: Dimension<GitSummary>;
  runtime: Dimension<RuntimeSummary>;
  stack: string[];
  stackSource: "detected" | "registered";
  /** Última ação registrada pelo LKR LAB para o projeto (UTC); null = sem registro. */
  lastActivity: string | null;
}
export interface OverviewTotals {
  total: number;
  available: number;
  missing: number;
  unbound: number;
  running: number;
  dirty: number;
}
export interface ProjectsOverview {
  projects: ProjectOverview[];
  totals: OverviewTotals;
}

/** Atividades recentes de um projeto + se o contexto de IA já foi gerado alguma vez. */
export interface ProjectActivity {
  items: Activity[];
  contextGenerated: boolean;
}

/** DDAE (Concept 06): espelha hub-core::ddae. SQLite é a fonte; progresso/atual/próximo vêm derivados. */
export type DdaeSessionStatus = "active" | "frozen" | "stopped" | "completed";
export type DdaeBlockStatus = "pending" | "in_progress" | "completed";
export interface DdaeBlock {
  id: string;
  title: string;
  description?: string;
  status: DdaeBlockStatus;
}
/** Critério de conclusão marcável (NÃO é progresso de execução; isso são os Blocks). */
export interface DdaeCriterion {
  id: string;
  text: string;
  completed: boolean;
}
export type DdaeEventType =
  | "SESSION_CREATED" | "SESSION_FROZEN" | "SESSION_STOPPED" | "SESSION_RESUMED" | "SESSION_COMPLETED" | "LEGACY_IMPORTED"
  | "BLOCK_ADDED" | "BLOCK_STARTED" | "BLOCK_COMPLETED" | "BLOCK_RENAMED" | "BLOCK_REMOVED"
  | "CRITERION_ADDED" | "CRITERION_COMPLETED" | "CRITERION_REOPENED" | "CRITERION_REMOVED"
  | "DECISION_ADDED" | "NOTE_ADDED" | "NOTE_REMOVED" | "DETAILS_UPDATED"
  | "WORKTREE_LINKED" | "WORKTREE_UNLINKED" | "WORKTREE_BLOCK_LINKED" | "WORKTREE_BLOCK_UNLINKED";
/** Histórico semântico e PORTÁTIL da Session (append-only). */
export interface DdaeEvent {
  id: string;
  type: DdaeEventType;
  blockId?: string;
  payload?: Record<string, string | number | boolean>;
  createdAt: string;
}
export interface DdaeDecision {
  id: string;
  blockId?: string;
  title: string;
  body: string;
  createdAt: string;
}
export type DdaeReferenceKind = "project_path" | "url";
export interface DdaeReference {
  kind: DdaeReferenceKind;
  value: string;
  label?: string;
}
export type DdaeContextState = "ready" | "incomplete" | "available";
/** Derivado no backend (nunca gravado): a Session tem o necessário para um agente continuá-la? */
export interface DdaeReadyForAi {
  state: DdaeContextState;
  ready: boolean;
  missing: ("objective" | "desired_outcome" | "blocks" | "criteria" | "actionable_block")[];
}
export interface DdaeSessionContext {
  markdown: string;
  readyForAi: DdaeReadyForAi;
}
export interface DdaeSession {
  id: string;
  projectId: string;
  number: number;
  title: string;
  objective: string;
  desiredOutcome?: string;
  constraints?: string[];
  criteria?: DdaeCriterion[];
  notes?: string[];
  references?: DdaeReference[];
  status: DdaeSessionStatus;
  pauseReason?: string;
  result?: string;
  blocks: DdaeBlock[];
  decisions: DdaeDecision[];
  createdAt: string;
  updatedAt: string;
  completedAt?: string;
  events?: DdaeEvent[];
  /** Item de Planejamento de origem (só quando a Session nasceu do Planejamento). */
  planningItemId?: string;
}
export interface DdaeSessionView extends DdaeSession {
  /** `SESSION-001` */
  label: string;
  progress: { completed: number; total: number };
  currentBlock: DdaeBlock | null;
  nextBlock: DdaeBlock | null;
  canComplete: boolean;
  /** Por que não pode finalizar: no_blocks | block_in_progress | blocks_pending | criteria_pending. */
  completionBlockers: ("no_blocks" | "block_in_progress" | "blocks_pending" | "criteria_pending")[];
  recentDecision: DdaeDecision | null;
  readyForAi: DdaeReadyForAi;
  /** Item de Planejamento de origem (derivado); nulo quando a Session não nasceu do Planejamento. */
  planningItem?: PlanningRef | null;
}
export interface DdaeCounts {
  total: number;
  active: number;
  frozen: number;
  stopped: number;
  completed: number;
}
export type DdaeLegacyImport = "not_applicable" | "imported" | "already_imported" | "skipped";
export interface DdaeOverview {
  projectId: string;
  /** Mais recente primeiro. */
  sessions: DdaeSessionView[];
  counts: DdaeCounts;
  blocksTotal: number;
  activeSessionId: string | null;
  legacyImport: DdaeLegacyImport;
}

/** Worktrees do LKR LAB (Concept 08): espelha hub-core::worktrees. */
export type WorktreeStatus = "active" | "frozen" | "stopped" | "completed";
export type WorktreeKind = "primary" | "managed_available" | "managed_missing" | "unmanaged";
export type WorktreeEventType =
  | "WORKTREE_ADOPTED" | "WORKTREE_CREATED" | "WORKTREE_STATE_CHANGED" | "WORKTREE_RENAMED"
  | "WORKTREE_SESSION_LINKED" | "WORKTREE_SESSION_UNLINKED" | "WORKTREE_BLOCK_LINKED" | "WORKTREE_BLOCK_UNLINKED"
  | "WORKTREE_COMPLETED";
export interface WorktreeEvent {
  id: string;
  type: WorktreeEventType;
  payload?: Record<string, string | number | boolean>;
  createdAt: string;
}
/** Metadata PORTÁTIL (sem path). A identidade é o UUID; branch e locator são só dicas. */
export interface ManagedWorktree {
  id: string;
  projectId: string;
  displayName: string;
  description?: string;
  status: WorktreeStatus;
  stateReason?: string;
  result?: string;
  repositoryLocator?: RepositoryLocator;
  branchHint?: string;
  detachedHeadHint?: string;
  sessionId?: string;
  blockId?: string;
  createdAt: string;
  updatedAt: string;
  completedAt?: string;
  events?: WorktreeEvent[];
}
export interface WorktreeManagedView extends ManagedWorktree {
  session: { id: string; label: string; title: string; status: DdaeSessionStatus } | null;
  block: { id: string; title: string; status: DdaeBlockStatus } | null;
  /** Último evento OPERACIONAL (nunca mtime nem commit). */
  lastEventAt: string | null;
}
export interface WorktreeItem {
  kind: WorktreeKind;
  git: Worktree | null;
  managed: WorktreeManagedView | null;
  gitSummary: Dimension<GitSummary> | null;
  warnings: ("session_completed_worktree_open" | "worktree_completed_session_active")[];
}
export interface WorktreeCounts {
  gitTotal: number;
  /** Total GERENCIADO: active + frozen + stopped + completed fecham com ele. */
  managed: number;
  active: number;
  frozen: number;
  stopped: number;
  completed: number;
  missing: number;
  unmanaged: number;
  withChanges: number;
}
export interface WorktreeOverview {
  projectId: string;
  projectAvailable: boolean;
  items: WorktreeItem[];
  counts: WorktreeCounts;
  gitError: string | null;
}
export interface SessionWorktree {
  id: string;
  displayName: string;
  status: WorktreeStatus;
  branchHint: string | null;
  blockId: string | null;
  available: boolean;
}

/** Planejamento (Concept 09): espelha hub-core::planning. Só `open|cancelled` são gravados; a fase é derivada. */
export type PlanningStoredStatus = "open" | "cancelled";
export type PlanningPhase = "planned" | "executing" | "completed" | "cancelled";
export type PlanningEventType =
  | "PLANNING_ITEM_CREATED"
  | "PLANNING_ITEM_UPDATED"
  | "PLANNING_ITEM_CANCELLED"
  | "PLANNING_ITEM_RESTORED"
  | "PLANNING_ITEM_SESSION_LINKED";
export interface PlanningItem {
  id: string;
  projectId: string;
  title: string;
  description?: string;
  position: number;
  storedStatus: PlanningStoredStatus;
  cancelReason?: string;
  createdAt: string;
  updatedAt: string;
  cancelledAt?: string;
}
export interface PlanningEvent {
  id: string;
  itemId: string;
  type: PlanningEventType;
  payload?: Record<string, string | number | boolean>;
  createdAt: string;
}
/** A Session vinculada, já derivada: o progresso é o da Session, nunca do item. */
export interface PlanningSessionRef {
  id: string;
  number: number;
  label: string;
  title: string;
  status: DdaeSessionStatus;
  progress: { completed: number; total: number };
}
export interface PlanningRow extends PlanningItem {
  phase: PlanningPhase;
  session: PlanningSessionRef | null;
  canStart: boolean;
  disabledReason: string | null;
  canEdit: boolean;
  canCancel: boolean;
  canRestore: boolean;
  canMoveUp: boolean;
  canMoveDown: boolean;
  lastActivityAt: string;
}
export interface PlanningCounts {
  /** planejados + em execução + concluídos (cancelados ficam de fora). */
  operational: number;
  planned: number;
  executing: number;
  completed: number;
  cancelled: number;
}
export interface PlanningNext {
  id: string;
  title: string;
  description: string;
  canStart: boolean;
  disabledReason: string | null;
}
export interface PlanningActiveSession {
  id: string;
  number: number;
  label: string;
}
export interface PlanningOverview {
  projectId: string;
  items: PlanningRow[];
  counts: PlanningCounts;
  next: PlanningNext | null;
  activeSession: PlanningActiveSession | null;
}
export interface PlanningSummary {
  counts: PlanningCounts;
  next: PlanningNext | null;
  activeSession: PlanningActiveSession | null;
}
export interface PlanningStartDraft {
  itemId: string;
  title: string;
  objective: string;
  canStart: boolean;
  disabledReason: string | null;
}
/** Origem de Planejamento mostrada na lista DDAE e no detalhe da Session. */
export interface PlanningRef {
  id: string;
  title: string;
  phase: PlanningPhase;
}
export type PlanningMove = "up" | "down" | "top";

/* ---- Control Plane (SESSION-002): espelho do contrato serializado por `hub-core::control_plane`. ---- */
export type Confidence = "unknown" | "medium" | "high" | "exact";
export interface Evidence { kind: string; detail: string }
/** Relação PROCESSO → PROJECT → WORKTREE → SESSION. Campos vazios = sem evidência ("—" na tela). */
export interface Association {
  confidence: Confidence;
  projectId: string | null;
  projectName: string | null;
  worktreeId: string | null;
  worktreeName: string | null;
  sessionId: string | null;
  sessionLabel: string | null;
  blockId: string | null;
  blockTitle: string | null;
  evidence: Evidence[];
}
export interface PortObservation {
  port: number;
  address: string;
  protocol: string;
  ipVersion: "v4" | "v6";
  pid: number | null;
}
export interface TreeNode { pid: number; parentPid: number | null; name: string; depth: number; cpu: number; memory: number }
export interface ConsoleStream { available: boolean; runId: string | null; reason: string | null }
export interface ManagedExecution { runId: string; commandId: string; command: string; projectId: string; exitCode: number | null; observer: boolean }
export interface RuntimeObservation {
  id: string;
  origin: "managed" | "discovered";
  category: "dev" | "system" | "other";
  label: string;
  technology: string | null;
  state: RunState;
  rootPid: number | null;
  pids: number[];
  ports: PortObservation[];
  tree: TreeNode[];
  command: string;
  cwd: string | null;
  startedAt: number | null;
  cpu: number;
  memory: number;
  association: Association;
  console: ConsoleStream;
  execution: ManagedExecution | null;
  isSelf: boolean;
}
export interface HealthSignal { id: string; domain: string; severity: "info" | "warning" | "critical"; message: string }
export interface Limitation { id: string; detail: string; requiresElevation: boolean }
export interface ControlPlaneSnapshot {
  takenAt: number;
  scope: "local";
  processTotal: number;
  listeningTotal: number;
  runtimes: RuntimeObservation[];
  signals: HealthSignal[];
  limitations: Limitation[];
}
