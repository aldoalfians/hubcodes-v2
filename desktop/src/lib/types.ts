export type Status = "idle" | "working" | "waiting" | "error" | "offline";

export interface Agent {
  id: string;
  name: string;
  provider: string;
  status: Status;
  cwd: string;
  command: string;
  dead: boolean;
  quietSecs: number;
  /** Last non-empty line of the terminal, ANSI stripped. */
  lastLine: string;
  /** Job in the team: orchestrator, frontend, reviewer… */
  role: string | null;
  /** Model the agent was started with. */
  model: string | null;
  /** Device the agent runs on; null = this one. */
  peer: string | null;
  /** Last screen lines for remote agents (no live terminal over SSH). */
  preview: string[];
  worktree: string | null;
  changes: Changes | null;
  usage: Usage | null;
  /** Last test run: [ok, unix, summary]. */
  test: [boolean, number, string] | null;
  /** Human typing into this agent right now. */
  holder: string | null;
  /** Messages waiting for this agent to finish its turn. */
  queued?: number;
  /** Guests of the shared canvas see its status, not its screen. */
  private: boolean;
  /** The login it runs under; null = the tool's own. */
  account: string | null;
  /** Added by auto team; removed again when idle with nothing waiting. */
  extra?: boolean;
  /** The model it really answered with (read from its transcript), when known. */
  modelSeen?: string | null;
}

export interface DaemonStatus {
  installed: boolean;
  running: boolean;
  pid: number | null;
  plist: string;
  log: string;
  tender: string | null;
  /** A daemon answers on its socket (launchd or started by hand with `hubcodes daemon run`). */
  socket: boolean;
  /** Sessions the daemon tends right now. */
  sessions: string[];
  subscribers: number;
  version: string | null;
}

export interface SessionEntry {
  name: string;
  running: boolean;
  agents: number;
  saved: boolean;
  current: boolean;
}

export interface UsageRow {
  at: number;
  agent: string;
  model: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  run: number;
}

export interface TestRun {
  ok: boolean;
  command: string;
  at: number;
  secs: number;
  tail: string;
  summary: string;
}

export interface MergeItem {
  id: number;
  at: number;
  updated: number;
  repo: string;
  branch: string;
  into: string;
  agent: string;
  status: "queued" | "merging" | "merged" | "conflict" | "tests_failed" | "failed";
  log: string;
}

export interface SessionDef {
  session: string;
  savedAt: number;
  agents: { name: string; provider: string; role: string | null; model: string | null; cwd: string; worktree: string | null }[];
  edges: [string, string, string][];
}

/** `kept`: `conversation` (same provider, continued) or `handoff` (a new provider got a hand-over note). */
export interface Switched {
  restored: Restored;
  kept: "conversation" | "handoff";
}

export interface Restored {
  started: string[];
  skipped: string[];
  errors: string[];
}

export interface SandboxConfig {
  kind: string;
  command: string;
  writable: string[];
}

/** A board column. The built-in ids (todo, doing, review, done) carry the engine's meaning; others are the user's. */
export interface BoardColumn {
  id: string;
  label: string;
  /** Most tasks it should hold, 0 = no limit; dispatch stops while Doing is full. */
  wip?: number;
}

export interface BoardConfig {
  columns: BoardColumn[];
}

/** `[skills]`: keep every tool's skill folder in sync on this device. */
export interface SkillsConfig {
  sync: boolean;
  sources?: string[];
}

/** Provider or `provider:model` entries, tried in order when a tool says it ran out. */
export interface AutoTeamConfig {
  enabled: boolean;
  /** Extra agents per role at most. */
  per_role: number;
  /** Per-role override of per_role (orchestrators get 0 unless listed). */
  roles?: Record<string, number>;
  max_agents: number;
  idle_minutes: number;
  cooldown_minutes: number;
  /** Extra software engineers work on their own branch. */
  worktree: boolean;
}

export interface FailoverConfig {
  enabled: boolean;
  chain: string[];
  rest_minutes: number;
}

export interface AlertsConfig {
  idle_minutes: number;
  stale_loop_rounds: number;
  repeated_error_polls: number;
  /** Slack, Discord or ntfy URL that every alert is also posted to. */
  webhook?: string;
}

export interface Usage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  turns: number;
}

export interface Task {
  id: number;
  at: number;
  updated: number;
  title: string;
  detail: string;
  /** A board column id: `todo`, `doing`, `review`, `done`, or one the user added. */
  status: string;
  role: string;
  owner: string;
  createdBy: string;
  result: string;
  /** `<tracker>:<key>` when imported from an issue tracker. */
  external: string;
  externalUrl: string;
  synced: string;
  gate: string;
  /** Playbook run id, 0 = none. */
  run: number;
  /** Position inside its column, lower first. */
  rank: number;
}

/** Something that happened to a task, from the message and event logs. */
export interface TaskHistoryItem {
  at: number;
  who: string;
  kind: string;
  text: string;
}

export interface PolicyConfig {
  mode: "off" | "auto";
  allow: string[];
  deny: string[];
  allowTools: string[];
  denyPaths: string[];
}

export interface PolicyDecision {
  kind: "allow" | "deny" | "ask";
  rule?: string;
}

export interface PlaybookStep {
  name: string;
  agent: string;
  role: string;
  prompt: string;
  action: string;
  gate: string;
  retries: number;
}

export interface Playbook {
  name: string;
  description: string;
  step: PlaybookStep[];
}

export interface PlaybookRun {
  id: number;
  at: number;
  updated: number;
  playbook: string;
  goal: string;
  status: "running" | "paused" | "testing" | "done" | "failed" | "aborted";
  step: number;
  task: number;
  merge: number;
  attempt: number;
  log: string;
  createdBy: string;
  cwd: string;
  prevResult: string;
  prevAgent: string;
}

export interface Tracker {
  name: string;
  kind: "jira" | "plane" | "github" | "github-review" | "github-feedback" | "command" | "mcp";
  url: string;
  email: string;
  token: string;
  api_key: string;
  workspace: string;
  project: string;
  repo: string;
  query: string;
  instruction: string;
  role: string;
  on_start: string;
  on_done: string;
  comment_on_done: boolean;
  limit: number;
  poll_minutes: number;
  run: string;
  command: string;
  args: string[];
  headers: Record<string, string>;
  list_tool: string;
  list_args: unknown;
  transition_tool: string;
  transition_args: unknown;
  comment_tool: string;
  comment_args: unknown;
  /** Feedback trackers: ignore feedback older than this date (YYYY-MM-DD). */
  since?: string;
  /** Feedback trackers: logins whose comments never start a round. */
  ignore?: string;
  /** Review trackers: review again after the author pushes (default on). */
  rereview?: boolean;
  auto_approve?: boolean;
  enabled: boolean;
}

/** A tracker on another device that reads the same issues as one here. */
/** A `<name>/SKILL.md` folder and the providers on that device that load it. */
export interface Skill {
  name: string;
  description: string;
  providers: string[];
  path: string;
}

export interface TrackerOverlap {
  tracker: string;
  device: string;
  name: string;
  enabled: boolean;
}

export interface TrackerCheck {
  name: string;
  ok: boolean;
  detail: string;
  ms: number;
  issues: { key: string; title: string; url: string; description: string }[];
}

export interface Fanout {
  id: number;
  at: number;
  prompt: string;
  collector: string;
  expected: number;
  received: number;
  done: boolean;
}

export interface Prompt {
  name: string;
  text: string;
  description: string;
}

/** What an agent with a role is told at start: the generated brief with the role file (if any) layered on. */
export interface RoleEntry {
  role: string;
  /** orchestrator | reviewer | tester | worker */
  kind: string;
  /** generated | extends | replaces */
  mode: string;
  file: string | null;
}

export interface RoleDetail {
  /** hubcodes' own text for the role's job; {lead} is the orchestrator's name. */
  default: string;
  /** What the role is told now: the default, or the saved version. */
  text: string;
  file: string | null;
}

export interface RoleBrief {
  text: string;
  file: string | null;
}

export interface TemplateAgent {
  name: string;
  provider: string;
  role: string | null;
  model: string | null;
  worktree: string | null;
  /** Start it on this device (a peer name); shows as `device/name`. */
  device?: string | null;
  /** The login (account) it runs under; null = the tool's own. */
  account?: string | null;
}

export interface Template {
  name: string;
  description: string;
  agents: TemplateAgent[];
  links: string[][];
}

export interface TeamStarted {
  spawned: string[];
  skipped: string[];
  links: number;
  errors: string[];
}

export interface Trigger {
  name: string;
  on: string;
  action: string;
  enabled: boolean;
}

export interface Share {
  user: string;
  role: string;
}

export interface ServeConfig {
  bind: string;
  port: number;
  token: string;
  shares: Share[];
  /** Folder served at /canvas/; empty = the app's bundled one. */
  webRoot?: string;
}

export interface McpTarget {
  url: string;
  command: string;
  args: string[];
  headers: Record<string, string>;
  env: Record<string, string>;
}

export interface MemoryConfig {
  provider: "builtin" | "hindsight" | "mcp" | "command";
  auto_recall_on_start: boolean;
  auto_retain: boolean;
  hindsight: { url: string; api_key: string; bank: string };
  mcp: McpTarget & { retain_tool: string; retain_args: unknown; recall_tool: string; recall_args: unknown };
  command: { retain: string; recall: string };
}

export interface MemoryPing {
  ok: boolean;
  provider: string;
  detail: string;
  ms: number;
}

export interface MemoryItem {
  text: string;
  source: string;
  at: number;
}

export interface KbSource {
  name: string;
  kind: "mcp" | "folder" | "git" | "confluence" | "jira" | "command";
  url: string;
  command: string;
  args: string[];
  headers: Record<string, string>;
  search_tool: string;
  search_args: unknown;
  get_tool: string;
  get_args: unknown;
  path: string;
  repo: string;
  email: string;
  token: string;
  space: string;
  project: string;
  run: string;
  enabled: boolean;
}

export interface KbHit {
  source: string;
  title: string;
  reference: string;
  snippet: string;
}

export interface KbCheck {
  name: string;
  ok: boolean;
  detail: string;
  ms: number;
}

export interface TendEvent {
  kind: string;
  text: string;
}

export interface Changes {
  branch: string;
  files: number;
  insertions: number;
  deletions: number;
  status: string[];
  ahead: number;
  behind: number;
}

export interface DiffInfo {
  dir: string;
  changes: Changes;
  diff: string;
}

export interface LoopState {
  rounds: number;
  max: number;
  paused: boolean;
  done: boolean;
  goal: string;
}

export interface Awaiting {
  to: string;
  seq: number;
  nudged: boolean;
}

/** A `hubcodes` command kept for a device that was unreachable; retried by every tend pass for two days. */
export interface QueuedCommand {
  id: number;
  peer: string;
  args: string[];
  at: number;
  attempts: number;
  error: string;
}

/** What another device needs to reach this Mac (Settings → Devices → This Mac). */
export interface ThisDevice {
  user: string;
  host: string;
  tailscale: boolean;
  tailscaleInstalled: boolean;
  remoteLogin: boolean;
  publicKey: string | null;
  authorizeCommand: string | null;
}

export interface DeviceStatus {
  name: string;
  host: string;
  ok: boolean;
  error: string | null;
  agents: number;
  polledSecsAgo: number | null;
  nextPollSecs: number;
  queued: QueuedCommand[];
  /** What is off about the pairing (version skew, names), one line each. */
  issues: string[];
  version: string | null;
}

export interface PeerInfo {
  name: string;
  host: string;
  ok: boolean;
  error: string | null;
  agents: number;
}

/** A device in hubcodes.toml `[[peer]]`. */
export interface Peer {
  name: string;
  host: string;
  user: string | null;
  session: string;
  hubcodes: string;
  /** Refuse to start more agents than this there (0 = no limit). Serialized as the core writes it. */
  max_agents: number;
  /** What that device may do here over SSH: send, task, spawn, approve, kill, restart. Empty = send, task, spawn. */
  allow?: string[];
  /** How to reach it: "" / "ssh" (default), "auto" (its hubcodesd listener, SSH when that is not there), "http" (listener only). */
  transport?: string;
  /** Port of its federation listener (default 7434). */
  port?: number | null;
}

export interface Edge {
  from: string;
  to: string;
  label: string;
}

export interface Message {
  seq: number;
  from: string;
  to: string;
  text: string;
}

export interface Snapshot {
  session: string;
  /** What a person types to an agent goes to the orchestrator first (Settings → Agents). */
  routeThroughOrchestrator?: boolean;
  agents: Agent[];
  edges: Edge[];
  lastMessage: Message | null;
  /** Keyed by unordered pair "a|b". */
  loops: Record<string, LoopState>;
  awaiting: Record<string, Awaiting>;
  peers: PeerInfo[];
  selfName: string;
  tasks: Task[];
  notes: string;
  serving: string | null;
  merges: MergeItem[];
  /** Saved agents that are not running (restore offer). */
  restorable: string[];
  /** Running agents started with an older team brief; restart them to apply the current one. */
  staleBrief: string[];
  runs: PlaybookRun[];
  /** Teammates on the web canvas (pointer in flow coordinates). */
  presence?: Presence[];
  /** Contributor messages waiting for an operator's approval. */
  pending?: PendingSend[];
}

export interface Presence {
  who: string;
  role: string;
  x: number;
  y: number;
  selected: string;
  at: number;
}

export interface PendingSend {
  id: number;
  at: number;
  by: string;
  to: string;
  text: string;
}

export interface AuditRow {
  id: number;
  at: number;
  agent: string;
  kind: string;
  detail: string;
}

/** Unordered pair key, same as the backend. */
export function pairKey(a: string, b: string) {
  return a <= b ? `${a}|${b}` : `${b}|${a}`;
}

export interface Provider {
  name: string;
  command: string;
  glyph: string;
  color: number | null;
  installed: boolean;
  models: string[];
  takesModel: boolean;
  takesRole: boolean;
  /** Logins known for it: `default` plus the account folders. */
  accounts: string[];
}

export interface SessionInfo {
  session: string;
  tmuxAvailable: boolean;
  tmuxVersion: string;
  configPath: string | null;
  home: string;
  defaultFolder: string | null;
  roles: string[];
  cliPath: string | null;
  cliNote: string;
  cliOnPath: boolean;
}

export interface ModelInfo {
  id: string;
  label: string;
  note: string;
  current: boolean;
}

export interface ProviderStatus {
  name: string;
  installed: boolean;
  path: string | null;
  version: string | null;
  loggedIn: boolean | null;
  configPath: string | null;
  currentModel: string | null;
  models: ModelInfo[];
  source: string;
  setup: { install: string | null; login: string | null; docs: string | null; modelsFrom: string };
}

export interface ProviderEdit {
  name: string;
  command: string;
  glyph: string;
  color: number | null;
  waitingPattern: string | null;
  modelFlag: string | null;
  roleFlag: string | null;
  models: string[];
  modelsCommand: string | null;
  approveKeys: string[];
  denyKeys: string[];
  /** Flag that continues the last conversation (`-c`, `resume --last`). */
  resumeFlag: string | null;
  /** Hook protocol: `claude`, `codex`, or null for none. */
  hookKind: string | null;
  builtin: boolean;
  installed: boolean;
}

/** Everything Settings can change; `path` is the hubcodes.toml it writes. */
export interface Config {
  path: string;
  exists: boolean;
  providers: ProviderEdit[];
  notifications: boolean;
  defaultFolder: string | null;
  roles: string[];
  maxRounds: number;
  /** What you type to an agent goes to the orchestrator first. */
  routeThroughOrchestrator: boolean;
  selfName: string;
  peers: Peer[];
  prompts: Prompt[];
  templates: Template[];
  triggers: Trigger[];
  maxTokensPerAgent: number;
  maxTokensPerLoop: number;
  serve: ServeConfig;
  memory: MemoryConfig;
  kbs: KbSource[];
  sandbox: SandboxConfig;
  alerts: AlertsConfig;
  /** `[failover]`: move an agent whose provider ran out to the next one. */
  failover?: FailoverConfig;
  /** `[autoteam]`: extra agents for a role whose queue backs up. */
  autoteam?: AutoTeamConfig;
  skills?: SkillsConfig;
  board: BoardConfig;
  policy: PolicyConfig;
  playbooks: Playbook[];
  trackers: Tracker[];
  connections: Connection[];
}

/** An account connected once (Settings → Accounts) and picked by trackers, knowledge sources and triggers. */
/** A site the twg CLI has a login for. */
export interface TwgProfile {
  name: string;
  url: string;
  user: string;
}

export interface Connection {
  name: string;
  kind: "atlassian" | "plane" | "github";
  url: string;
  email: string;
  /** A Keychain reference once saved (`${keychain:connection:NAME}`); a pasted token until then. */
  token: string;
  workspace: string;
}

/** One option a picker offers, read from the account. */
export interface Choice {
  value: string;
  label: string;
}

/** What `connectionChoices` returns: the choices, or why there are none. */
export interface Choices {
  ok: boolean;
  choices: Choice[];
  error?: string;
}

/** An MCP server's tools and the one picked per purpose (search, get, list, …), with its arguments. */
export interface Discovered {
  tools: { name: string; description: string; input_schema?: unknown }[];
  picks: Record<string, [string, unknown]>;
}

/** What a settings save did. */
export interface Saved {
  path: string;
  /** What moved from the file into the Keychain. */
  moved: string[];
}

export interface DoctorCheck {
  id: string;
  title: string;
  level: "ok" | "warn" | "fail";
  detail: string;
  fix: string;
  /** A command that does the fix (Copy button). */
  command?: string;
  /** A page or app that does the fix (Open button). */
  open?: string;
}

export interface DoctorReport {
  checks: DoctorCheck[];
  ok: number;
  warn: number;
  fail: number;
  facts: [string, string][];
}

export interface HistoryRow {
  seq: number;
  at: number;
  from: string;
  to: string;
  text: string;
  kind: string;
}

export interface Group {
  x: number;
  y: number;
  w: number;
  h: number;
  color?: string | null;
  collapsed?: boolean;
}

export interface Layout {
  nodes: Record<string, [number, number]>;
  /** Per-agent card size [width, height]; missing = default. */
  sizes?: Record<string, [number, number]>;
  viewport: [number, number, number] | null;
  /** Bands: titled boxes that group cards by position. */
  groups?: Record<string, Group>;
}

export interface UpdateInfo { version: string; notes: string; date: string | null; current: string }
export interface UpdateProgress { downloaded: number; total: number | null }

/** A room that lives on this device (`hubcodes room`). Members are agents here (`lead`) or on a device (`mini/dev`). */
export interface RoomInfo {
  name: string;
  created_at: number;
  created_by: string;
  members: string[];
}

export interface RoomMessage {
  id: number;
  at: number;
  from: string;
  text: string;
}

export interface Backend {
  sessionInfo(): Promise<SessionInfo>;
  doctor(): Promise<DoctorReport>;
  diagnostics(): Promise<string>;
  logTail(lines: number): Promise<string>;
  usageLog(days: number): Promise<UsageRow[]>;
  daemonStatus(): Promise<DaemonStatus>;
  daemonSet(on: boolean, serve: boolean): Promise<DaemonStatus>;
  sessions(): Promise<SessionEntry[]>;
  pendingDecide(id: number, approve: boolean): Promise<string>;
  roomList(): Promise<RoomInfo[]>;
  roomLog(name: string, limit?: number): Promise<RoomMessage[]>;
  roomCreate(name: string, members: string[]): Promise<void>;
  roomMembers(name: string, add: string[], remove: string[]): Promise<void>;
  roomDelete(name: string): Promise<void>;
  /** Post as the person at this Mac; resolves with how many members it went to. */
  roomPost(name: string, text: string): Promise<number>;
  auditLog(limit: number): Promise<AuditRow[]>;
  /** Where this viewer's pointer is on the canvas (web clients only; no-op in the app). */
  presence(x: number, y: number, selected: string): Promise<void>;
  switchSession(name: string): Promise<void>;
  /** Newer release on the feed, or null when this build is current. */
  updateCheck(): Promise<UpdateInfo | null>;
  /** Download, verify and install it; the app restarts on success. */
  updateInstall(): Promise<void>;
  onUpdateProgress(cb: (p: UpdateProgress) => void): Promise<() => void>;
  providers(): Promise<Provider[]>;
  providerStatus(name: string, refresh: boolean, account?: string | null): Promise<ProviderStatus>;
  snapshot(): Promise<Snapshot>;
  /** `device` (a peer name) starts it on that device; it shows here as `device/name`. */
  spawn(name: string, provider: string, cwd: string | null, role: string | null, model: string | null, worktree: string | null, sandbox: boolean, device?: string | null, account?: string | null): Promise<string>;
  accountList(provider: string): Promise<string[]>;
  accountAdd(provider: string, name: string): Promise<string[]>;
  accountRemove(provider: string, name: string): Promise<string[]>;
  /** Opens the tool under that account in its own window for signing in; returns the pane's agent name. */
  accountLogin(provider: string, name: string): Promise<string>;
  /** Open a terminal sign-in as a pane: `github` (gh auth login --web) or `twg` (twg login). Returns its name. */
  loginTerminal(kind: "github" | "twg" | `provider:${string}`): Promise<string>;
  runTests(id: string): Promise<TestRun>;
  mergeEnqueue(id: string, into: string | null): Promise<MergeItem>;
  mergesList(): Promise<MergeItem[]>;
  sessionDef(): Promise<SessionDef | null>;
  restoreSession(resume: boolean): Promise<Restored>;
  /** Forget the saved agents so the next start is an empty canvas. */
  discardSavedSession(): Promise<void>;
  /** Restart these agents with the current brief (links, roles, folders kept). */
  restartAgents(names: string[], resume: boolean): Promise<Restored>;
  /** Run an agent on another provider or model; name, role, folder and links stay. */
  switchAgent(name: string, provider: string, model: string | null): Promise<Switched>;
  taskApprove(id: number): Promise<Task>;
  taskRequestChanges(id: number, note: string): Promise<Task>;
  providerExport(name: string): Promise<string>;
  providerImport(text: string): Promise<string[]>;
  teamInit(cwd: string): Promise<string>;
  agentDiff(id: string): Promise<DiffInfo>;
  commitAgent(id: string, message: string): Promise<string>;
  openPr(id: string): Promise<string>;
  setRole(id: string, role: string): Promise<void>;
  /** Hide (or show again) the agent's screen to guests of the shared canvas. */
  setPrivate(id: string, on: boolean): Promise<void>;
  kill(id: string): Promise<void>;
  /** Resolves with the orchestrator it went to first, or null when it went straight to the agent. `direct` skips the orchestrator. */
  instruct(id: string, text: string, direct?: boolean): Promise<string | null>;
  /** Write a pasted or picked file (base64) to this Mac's attachments folder; returns its path. */
  saveAttachment(name: string, base64: string): Promise<string>;
  /** The message agent `id` gets: files copied to its device when it runs on another one, paths listed after the text. */
  composeMessage(id: string, text: string, paths: string[]): Promise<string>;
  sendMessage(from: string, to: string, text: string): Promise<number>;
  connect(from: string, to: string, label?: string): Promise<void>;
  history(limit: number, query?: string): Promise<HistoryRow[]>;
  taskAdd(title: string, detail: string, role: string, owner: string): Promise<Task>;
  taskSet(id: number, status: Task["status"], result?: string): Promise<Task>;
  taskUpdate(id: number, title: string, detail: string, role: string): Promise<Task>;
  /** Put a task in a column before another task of that column, or last. */
  taskPlace(id: number, column: string, before: number | null): Promise<Task>;
  taskHistory(id: number): Promise<TaskHistoryItem[]>;
  boardColumns(): Promise<BoardColumn[]>;
  /** Saves the columns as given; built-ins come back if left out, tasks of a removed column go to To do. */
  boardColumnsSet(columns: BoardColumn[]): Promise<BoardColumn[]>;
  taskAssign(id: number, agent: string): Promise<Task>;
  taskRemove(id: number): Promise<void>;
  /** Another feedback round on a pull request paused for rounds without a commit. */
  taskResume(id: number): Promise<void>;
  /** Approve the pull request on GitHub (a review that ended with `VERDICT: approve`). */
  prApprove(id: number): Promise<string>;
  /** Stop handling a pull request. */
  prIgnore(id: number): Promise<string>;
  tasksAll(includeDone: boolean): Promise<Task[]>;
  notesWrite(text: string): Promise<void>;
  notesAdd(text: string): Promise<void>;
  fanoutStart(targets: string[], collector: string | null, prompt: string): Promise<Fanout>;
  fanouts(): Promise<Fanout[]>;
  templates(cwd: string | null): Promise<Template[]>;
  /** `models` picks a model per agent on this machine; an empty id means the provider's default. */
  teamStart(name: string, cwd: string | null, models?: Record<string, string>): Promise<TeamStarted>;
  teamSave(name: string): Promise<Template>;
  promptList(): Promise<Prompt[]>;
  promptPreview(id: string, text: string): Promise<string>;
  roleBrief(role: string, cwd?: string | null): Promise<RoleBrief>;
  rolesList(): Promise<RoleEntry[]>;
  roleGet(role: string): Promise<RoleDetail>;
  roleSave(role: string, text: string): Promise<string | null>;
  serveStart(): Promise<string>;
  tailscaleInfo(): Promise<{ ip: string | null }>;
  onTend(cb: (e: TendEvent) => void): Promise<() => void>;
  memoryPing(config: MemoryConfig): Promise<MemoryPing>;
  memoryRecall(query: string, limit: number): Promise<MemoryItem[]>;
  memoryRetain(text: string, tags: string): Promise<string>;
  kbCheck(source: KbSource): Promise<KbCheck>;
  kbSearch(query: string, source: string | null, limit: number): Promise<{ hits: KbHit[]; errors: string[] }>;
  kbGet(source: string, reference: string): Promise<string>;
  /** Yes / no to the agent's permission prompt. */
  answerPrompt(id: string, approve: boolean): Promise<void>;
  setLoop(a: string, b: string, on: boolean, max?: number): Promise<void>;
  resetLoop(a: string, b: string): Promise<void>;
  startLoop(a: string, b: string, goal: string, max?: number): Promise<number>;
  checkPeer(peer: Peer): Promise<string>;
  deviceStatus(name: string): Promise<DeviceStatus>;
  thisDevice(): Promise<ThisDevice>;
  createSshKey(): Promise<ThisDevice>;
  /** Polls every device now and retries what is queued; one line per delivered or dropped command. */
  deviceRetry(): Promise<string[]>;
  /** The providers and model catalogs on that device (its `hubcodes providers --json`), cached 5 minutes. */
  deviceModels(name: string, refresh: boolean): Promise<ProviderStatus[]>;
  /** Local proxy pane (ssh + tmux attach) for a remote agent; returns the pane id to stream. */
  openRemote(id: string): Promise<string>;
  disconnect(a: string, b: string): Promise<void>;
  attach(id: string, cols: number, rows: number): Promise<void>;
  detach(id: string): Promise<void>;
  write(id: string, data: string): Promise<void>;
  resize(id: string, cols: number, rows: number): Promise<void>;
  loadLayout(): Promise<Layout>;
  saveLayout(layout: Layout): Promise<void>;
  openPath(path: string): Promise<void>;
  getConfig(): Promise<Config>;
  /** Returns the path written. Rejects with a validation message. */
  saveConfig(config: Config): Promise<Saved>;
  /** Who a connection signs in as. */
  connectionCheck(connection: Connection): Promise<string>;
  /** Atlassian sites the twg CLI is signed in to on this computer. */
  twgProfiles(): Promise<TwgProfile[]>;
  /** projects | statuses (arg = project) | spaces | repos | labels (arg = repo). */
  connectionChoices(connection: Connection, what: string, arg?: string): Promise<Choices>;
  /** List an MCP server's tools and pick one for each purpose. */
  mcpDiscover(target: McpTarget, purposes: string[]): Promise<Discovered>;
  testNotification(): Promise<void>;
  playbookStart(name: string, goal: string, cwd: string | null): Promise<PlaybookRun>;
  playbookApprove(id: number): Promise<PlaybookRun>;
  playbookAbort(id: number): Promise<PlaybookRun>;
  playbookRuns(all: boolean): Promise<PlaybookRun[]>;
  playbooks(cwd: string | null): Promise<Playbook[]>;
  trackerCheck(tracker: Tracker): Promise<TrackerCheck>;
  trackerPull(name: string): Promise<string[]>;
  trackerOverlaps(trackers: Tracker[]): Promise<TrackerOverlap[]>;
  /** Skills on this Mac, or on `device`. */
  skillsList(device?: string): Promise<Skill[]>;
  /** Link a skill into every tool here, fetched from device `from` first when given; returns what changed. */
  skillCopy(name: string, from?: string): Promise<string[]>;
  /** Link every skill into every tool now; returns what changed. */
  skillsSync(): Promise<string[]>;
  /** Add a skill from a folder, a .zip/.skill file or a git URL; returns its names. */
  skillAdd(source: string): Promise<string[]>;
  /** Pull the skill sources that are git checkouts, then link what is new. */
  skillsPull(): Promise<string[]>;
  policyTest(policy: PolicyConfig, command: string): Promise<PolicyDecision>;
  onSnapshot(cb: (s: Snapshot) => void): Promise<() => void>;
  onPane(id: string, cb: (bytes: Uint8Array) => void): Promise<() => void>;
}
