// Browser-only stand-in so the UI can be designed and screenshotted without
// Tauri or tmux. Three scripted agents, fake terminal output.
import type { Agent, Backend, RoomInfo, RoomMessage, Config, Layout, LoopState, Message, MergeItem, Playbook, PlaybookRun, ProviderStatus, Snapshot, Task, Template, Prompt, Skill, TendEvent } from "./types";
import { pairKey } from "./types";
import { normalizeConfig } from "./configShape";
import { routeLead } from "./route";

const mockRooms: RoomInfo[] = [
  { name: "auth", created_at: Date.now() / 1000 - 3600, created_by: "human:you", members: ["builder", "planner", "mini/tester"] },
  { name: "release", created_at: Date.now() / 1000 - 86400, created_by: "human:you", members: ["planner", "reviewer"] },
];
const mockRoomLog: Record<string, RoomMessage[]> = {
  auth: [
    { id: 1, at: Date.now() / 1000 - 1500, from: "planner", text: "Token refresh is split: builder takes the store, mini/tester the e2e suite." },
    { id: 2, at: Date.now() / 1000 - 900, from: "builder", text: "Store is in src/auth/store.rs; refresh keeps the old token until the new one is written." },
    { id: 3, at: Date.now() / 1000 - 420, from: "mini/tester", text: "3 of 8 suites red, see tests/auth.spec.ts. Expired-token case fails on the second request." },
    { id: 4, at: Date.now() / 1000 - 120, from: "human:you", text: "Fix the expired-token case before the refresh endpoint ships." },
  ],
  release: [],
};

/** Mock: agents started with an older brief (cleared by restartAgents). */
const mockThisDevice: { user: string; host: string; tailscale: boolean; tailscaleInstalled: boolean; remoteLogin: boolean; publicKey: string | null; authorizeCommand: string | null } = { user: "demo", host: "macbook", tailscale: false, tailscaleInstalled: false, remoteLogin: false, publicKey: null, authorizeCommand: null };
let mockStale: string[] = ["planner"];
const mockRoles: Record<string, string> = {};
const mockKind = (r: string) => (/orchestr|lead|manager|coordinator|planner/i.test(r) ? "orchestrator" : /review/i.test(r) ? "reviewer" : /test|qa/i.test(r) ? "tester" : "worker");
const MOCK_JOBS: Record<string, string> = {
  orchestrator: "Your job is to coordinate the team, not to do the work yourself. Split each request into clear tasks and send each one to the right teammate with hubcodes send <name> \"goal, files, how we know it is done\".",
  reviewer: "Your job is to review what the engineers send you, not to rewrite it. Reply with findings (file:line, what is wrong, the fix) to the sender and to {lead}; say clearly when it is approved.",
  tester: "Your job is to run and write tests for what the engineers send you and report the results to the sender and to {lead}.",
  worker: "Do the work your orchestrator sends you, in your own area of the code. When it is done, report back with hubcodes send {lead} \"done: what changed, how to verify\".",
};
const MOCK_TOOLS = ["shared", "claude", "opencode", "pi"];
const mockSkills: Skill[] = [
  { name: "jira-task", description: "Work a Jira ticket end-to-end with human gates", providers: ["shared", "claude", "opencode", "pi"], path: "/Users/me/.agents/skills/jira-task" },
  { name: "pr-review", description: "Reviews pull requests where you are a requested reviewer", providers: ["claude"], path: "/Users/me/.claude/skills/pr-review" },
];

let mockSession = "hubcodes";
let discarded = false;
const progressCbs: ((p: { downloaded: number; total: number | null }) => void)[] = [];

const enc = new TextEncoder();
const agents: Agent[] = [
  { id: "%1", name: "planner", provider: "claude", status: "idle", cwd: "~/code/api", command: "claude", dead: false, quietSecs: 40, lastLine: "❯", role: "orchestrator", model: "opus", peer: null, preview: [], worktree: null, changes: null, usage: { inputTokens: 18200, outputTokens: 4100, cacheReadTokens: 120000, cacheWriteTokens: 9000, turns: 7 }, test: null, holder: "budi", private: false, account: null },
  { id: "%2", name: "builder", queued: 1, provider: "codex", status: "working", cwd: "~/code/api", command: "codex", dead: false, quietSecs: 1, lastLine: "running cargo test …", role: "software engineer", model: "gpt-5-codex", peer: null, preview: [], worktree: "~/code/api/.hubcodes/worktrees/builder", changes: { branch: "feat/token-store", files: 3, insertions: 42, deletions: 7, status: [" M src/auth/store.rs", " M src/auth/mod.rs", "?? tests/token.rs"], ahead: 0, behind: 0 }, usage: { inputTokens: 9000, outputTokens: 2200, cacheReadTokens: 50000, cacheWriteTokens: 1000, turns: 4 }, test: [true, Date.now() / 1000 - 120, "14 passed"], holder: null, private: false, account: null },
  { id: "%3", name: "reviewer", provider: "opencode", status: "waiting", cwd: "~/code/api", command: "opencode", dead: false, quietSecs: 12, lastLine: "? Allow opencode to run `cargo clippy`? (y/N)", role: "reviewer", model: null, peer: null, preview: [], worktree: null, changes: null, usage: null, test: null, holder: null, private: false, account: null },
  { id: "%4", name: "shell", provider: "shell", status: "idle", cwd: "~/code/api", command: "zsh", dead: false, quietSecs: 300, lastLine: "~/code/api ❯", role: null, model: null, peer: null, preview: [], worktree: null, changes: null, usage: null, test: null, holder: null, private: false, account: null },
  { id: "mini/tester", name: "mini/tester", provider: "codex", status: "working", cwd: "~/code/api", command: "codex", dead: false, quietSecs: 1, lastLine: "running e2e suite 3/8", role: "tests", model: "gpt-5.6-terra", worktree: null, changes: null, usage: null, test: [false, Date.now() / 1000 - 60, "3 failed"], holder: null, private: false, account: null, peer: "mini", preview: ["[from builder] run the e2e suite on the new token store", "", "Running playwright …", "  auth.spec.ts  ✓ 12 passed", "  token.spec.ts ✓ 4 passed", "", "running e2e suite 3/8"] },
];
let edges = [
  { from: "planner", to: "builder", label: "task" },
  { from: "builder", to: "reviewer", label: "diff" },
  { from: "builder", to: "mini/tester", label: "loop" },
];
let tasks: Task[] = [
  { id: 1, at: Date.now() / 1000 - 600, updated: Date.now() / 1000 - 60, title: "Token store with refresh", detail: "src/auth/store.rs, keep the API", status: "doing", role: "software engineer", owner: "builder", createdBy: "planner", result: "", external: "", externalUrl: "", synced: "", gate: "", run: 1, rank: 0 },
  { id: 2, at: Date.now() / 1000 - 500, updated: Date.now() / 1000 - 500, title: "PE-418 Login form uses the new store", detail: "/jira-task PE-418", status: "todo", role: "software engineer", owner: "", createdBy: "tracker:jira", result: "", external: "jira:PE-418", externalUrl: "https://hubstudio.atlassian.net/browse/PE-418", synced: "", gate: "", run: 0, rank: 0 },
  { id: 3, at: Date.now() / 1000 - 400, updated: Date.now() / 1000 - 100, title: "e2e suite for auth", detail: "", status: "review", role: "tests", owner: "mini/tester", createdBy: "planner", result: "3 of 8 suites red, see tests/auth.spec.ts", external: "", externalUrl: "", synced: "", gate: "review", run: 0, rank: 0 },
  { id: 5, at: Date.now() / 1000 - 2500, updated: Date.now() / 1000 - 900, title: "Feedback round 2 on acme/web#107 Settings page (2 new)", detail: "", status: "done", role: "pr-fixer", owner: "fixer", createdBy: "tracker:feedback", result: "NEEDS HUMAN: merge order with acme/web#103 (https://github.com/acme/web/pull/107#issuecomment-1)", external: "feedback:acme/web#107", externalUrl: "https://github.com/acme/web/pull/107", synced: "asked", gate: "", run: 0, rank: 0 },
  { id: 6, at: Date.now() / 1000 - 2400, updated: Date.now() / 1000 - 700, title: "Feedback round 3 on acme/web#98 Sidebar (1 new)", detail: "", status: "done", role: "pr-fixer", owner: "fixer", createdBy: "tracker:feedback", result: "Explained why the prop stays", external: "feedback:acme/web#98", externalUrl: "https://github.com/acme/web/pull/98", synced: "stalled", gate: "", run: 0, rank: 0 },
  { id: 8, at: Date.now() / 1000 - 1800, updated: Date.now() / 1000 - 300, title: "Review acme/api#51 Retry webhooks", detail: "", status: "done", role: "reviewer", owner: "reviewer", createdBy: "tracker:reviews", result: "Two nits, nothing blocking; tests pass.\nVERDICT: approve", external: "reviews:acme/api#51", externalUrl: "https://github.com/acme/api/pull/51", synced: "done", gate: "", run: 0, rank: 0 },
  { id: 7, at: Date.now() / 1000 - 200, updated: Date.now() / 1000 - 200, title: "Review acme/api#55 Rate limits", detail: "", status: "todo", role: "reviewer", owner: "", createdBy: "tracker:reviews", result: "", external: "reviews:acme/api#55", externalUrl: "https://github.com/acme/api/pull/55", synced: "", gate: "", run: 0, rank: 1 },
  { id: 4, at: Date.now() / 1000 - 3000, updated: Date.now() / 1000 - 2000, title: "Break the goal into tasks", detail: "", status: "done", role: "", owner: "planner", createdBy: "human:you", result: "4 tasks", external: "", externalUrl: "", synced: "", gate: "", run: 0, rank: 0 },
];
const playbooksList: Playbook[] = [
  { name: "feature", description: "Plan, build with tests, review, merge", step: [
    { name: "plan", agent: "", role: "orchestrator", prompt: "Write a short implementation plan for: {goal}…", action: "", gate: "human", retries: 0 },
    { name: "build", agent: "", role: "software engineer", prompt: "Implement the plan for: {goal}…", action: "", gate: "test", retries: 2 },
    { name: "review", agent: "", role: "reviewer", prompt: "Review what {prev_agent} built…", action: "", gate: "review", retries: 0 },
    { name: "merge", agent: "", role: "", prompt: "", action: "merge {prev_agent}", gate: "", retries: 0 },
  ] },
];
let runs: PlaybookRun[] = [
  { id: 1, at: Date.now() / 1000 - 700, updated: Date.now() / 1000 - 60, playbook: "feature", goal: "auth token refresh", status: "running", step: 1, task: 1, merge: 0, attempt: 0, log: "[..] started\n[..] step plan: task #4 → planner\n[..] step plan done by planner: 4 tasks\n[..] step build: task #1 for an idle software engineer\n", createdBy: "human:you", cwd: "~/code/api", prevResult: "4 tasks", prevAgent: "planner" },
];
let notesText = "- **planner** (2026-11-11 09:10): token store lives in src/auth/store.rs; keep the public API\n- **human:you** (2026-11-11 09:12): ship by Friday, tests must be green\n";
const templates: Template[] = [
  { name: "pair", description: "Builder and reviewer in a loop", agents: [{ name: "builder", provider: "claude", role: "software engineer", model: null, worktree: null }, { name: "reviewer", provider: "codex", role: "reviewer", model: "o3-pro", worktree: null }], links: [["builder", "reviewer", "loop"]] },
  { name: "full-stack", description: "Orchestrator, software engineer, reviewer", agents: [{ name: "lead", provider: "claude", role: "orchestrator", model: null, worktree: null }, { name: "web", provider: "claude", role: "software engineer", model: null, worktree: null }, { name: "api", provider: "claude", role: "software engineer", model: null, worktree: "agent/api" }, { name: "reviewer", provider: "codex", role: "reviewer", model: null, worktree: null }], links: [["lead", "web"], ["lead", "api"], ["web", "reviewer"], ["api", "reviewer", "loop"]] },
];
const mockAccounts: Record<string, string[]> = { claude: ["default", "work", "personal"], codex: ["default"] };
const promptsList: Prompt[] = [
  { name: "review", description: "Review the agent's current changes", text: "Review the changes on branch {branch} in {cwd}…\n\n{last_diff}" },
  { name: "test", description: "Run the tests and fix failures", text: "Run the test suite in {cwd}…" },
  { name: "handoff", description: "Summarise for the next agent", text: "Write a handoff…" },
  { name: "status", description: "One-paragraph status", text: "In one paragraph: what you are doing…" },
  { name: "plan", description: "Plan before coding", text: "Before writing code, list the steps…" },
];
const tendSubs = new Set<(e: TendEvent) => void>();
let merges: MergeItem[] = [{ id: 1, at: Date.now() / 1000 - 900, updated: Date.now() / 1000 - 880, repo: "~/code/api", branch: "feat/token-store", into: "main", agent: "builder", status: "merged", log: "main → a1b2c3d" }];
let loops: Record<string, LoopState> = { [pairKey("builder", "mini/tester")]: { rounds: 3, max: 20, paused: false, done: false, goal: "token store passes the e2e suite" } };
let seq = 0;
let last: Message | null = null;
const snapshotSubs = new Set<(s: Snapshot) => void>();
const paneSubs = new Map<string, Set<(b: Uint8Array) => void>>();
let layout: Layout = { nodes: {}, viewport: null };
let config: Config = {
  path: "/Users/you/.config/hubcodes/hubcodes.toml",
  exists: false,
  notifications: true,
  defaultFolder: null,
  roles: ["orchestrator", "software engineer", "reviewer", "pr-fixer", "tests", "docs", "research"],
  maxRounds: 20,
  routeThroughOrchestrator: true,
  selfName: "mac",
  peers: [{ name: "mini", host: "mac-mini", user: null, session: "hubcodes", hubcodes: "hubcodes", max_agents: 3, allow: [] }],
  prompts: promptsList,
  templates,
  triggers: [{ name: "nightly-review", on: "cron 0 9 * * 1-5", action: "loop start planner reviewer \"Review every open PR\"", enabled: true }],
  maxTokensPerAgent: 0,
  maxTokensPerLoop: 0,
  serve: { bind: "tailscale", port: 7433, token: "", shares: [{ user: "budi@hubstudio.id", role: "operator" }] },
  sandbox: { kind: "macos", command: "", writable: [] },
  alerts: { idle_minutes: 10, stale_loop_rounds: 6, repeated_error_polls: 3 },
  failover: { enabled: false, chain: [], rest_minutes: 60 },
  autoteam: { enabled: false, per_role: 2, roles: {}, max_agents: 8, idle_minutes: 10, cooldown_minutes: 2, worktree: true },
  skills: { sync: false, sources: [] },
  board: { columns: [{ id: "backlog", label: "Backlog" }, { id: "todo", label: "To do" }, { id: "doing", label: "Doing" }, { id: "review", label: "Review" }, { id: "done", label: "Done" }] },
  policy: { mode: "auto", allow: ["git status*", "git diff*", "git log*", "cargo test*", "pnpm test*", "npm test*", "ls*", "cat *", "rg *"], deny: ["rm -rf *", "git push --force*", "git reset --hard*", "sudo *", "* | sh"], allowTools: ["Read", "Glob", "Grep", "WebFetch"], denyPaths: ["~/.ssh/*", "*.env", "*.env.*"] },
  playbooks: playbooksList,
  connections: [
    { name: "hubstudio", kind: "atlassian", url: "https://hubstudio.atlassian.net", email: "${JIRA_EMAIL}", token: "${keychain:connection:hubstudio}", workspace: "" },
  ],
  trackers: [
    { name: "jira", kind: "jira", url: "https://hubstudio.atlassian.net", email: "${JIRA_EMAIL}", token: "${JIRA_TOKEN}", api_key: "", workspace: "", project: "", repo: "", query: "project = PE AND status = \"Selected for Development\" AND assignee = currentUser()", instruction: "/jira-task {key}", role: "software engineer", on_start: "In Development", on_done: "In Review", comment_on_done: true, limit: 2, poll_minutes: 2, run: "", command: "", args: [], headers: {}, list_tool: "", list_args: null, transition_tool: "", transition_args: null, comment_tool: "", comment_args: null, enabled: true },
  ],
  memory: { provider: "mcp", auto_recall_on_start: true, auto_retain: true, hindsight: { url: "", api_key: "", bank: "" }, mcp: { url: "https://memories.hubstudio.id/mcp/me/", command: "", args: [], headers: { Authorization: "Bearer ${HINDSIGHT_API_KEY}" }, env: {}, retain_tool: "retain", retain_args: { content: "{text}" }, recall_tool: "recall", recall_args: { query: "{query}" } }, command: { retain: "", recall: "" } },
  kbs: [
    { name: "product", kind: "mcp", url: "https://kb.hubstudio.id/mcp/", command: "", args: [], headers: {}, search_tool: "product_kb_search", search_args: { query: "{query}" }, get_tool: "product_kb_get", get_args: { id: "{id}" }, path: "", repo: "", email: "", token: "", space: "", project: "", run: "", enabled: true },
    { name: "hermina-docs", kind: "git", url: "", command: "", args: [], headers: {}, search_tool: "", search_args: null, get_tool: "", get_args: null, path: "", repo: "git@github.com:hubstudio/hermina-docs.git", email: "", token: "", space: "", project: "", run: "", enabled: true },
  ],
  providers: [
    { name: "claude", command: "claude", glyph: "✦", color: 215, waitingPattern: null, modelFlag: "--model {model}", roleFlag: "--append-system-prompt '…{role}…'", models: ["opus", "sonnet", "haiku"], modelsCommand: null, approveKeys: [], denyKeys: [], resumeFlag: null, hookKind: null, builtin: true, installed: true },
    { name: "codex", command: "codex", glyph: "◇", color: 252, waitingPattern: null, modelFlag: "--model {model}", roleFlag: null, models: ["gpt-5-codex", "gpt-5"], modelsCommand: null, approveKeys: [], denyKeys: [], resumeFlag: null, hookKind: null, builtin: true, installed: true },
    { name: "opencode", command: "opencode", glyph: "◈", color: 80, waitingPattern: null, modelFlag: "--model {model}", roleFlag: null, models: [], modelsCommand: null, approveKeys: [], denyKeys: [], resumeFlag: null, hookKind: null, builtin: true, installed: true },
    { name: "pi", command: "pi", glyph: "π", color: 176, waitingPattern: null, modelFlag: "--model {model}", roleFlag: null, models: [], modelsCommand: null, approveKeys: [], denyKeys: [], resumeFlag: null, hookKind: null, builtin: true, installed: false },
    { name: "omp", command: "omp", glyph: "◆", color: 114, waitingPattern: null, modelFlag: "--model={model}", roleFlag: null, models: [], modelsCommand: null, approveKeys: [], denyKeys: [], resumeFlag: null, hookKind: null, builtin: true, installed: false },
    { name: "gemini", command: "gemini", glyph: "✧", color: 75, waitingPattern: null, modelFlag: "--model {model}", roleFlag: null, models: ["gemini-2.5-pro", "gemini-2.5-flash"], modelsCommand: null, approveKeys: [], denyKeys: [], resumeFlag: null, hookKind: null, builtin: true, installed: false },
    { name: "hermes", command: "hermes chat", glyph: "☤", color: 178, waitingPattern: null, modelFlag: "--model {model}", roleFlag: null, models: [], modelsCommand: null, approveKeys: [], denyKeys: [], resumeFlag: "-c", hookKind: null, builtin: true, installed: true },
    { name: "shell", command: "$SHELL", glyph: "$", color: 245, waitingPattern: null, modelFlag: null, roleFlag: null, models: [], modelsCommand: null, approveKeys: [], denyKeys: [], resumeFlag: null, hookKind: null, builtin: true, installed: true },
  ],
};

const screens: Record<string, string[]> = {
  "%1": [
    "\x1b[36m✦ Claude Code\x1b[0m  ~/code/api",
    "",
    "\x1b[2m> \x1b[0mbreak down: auth token refresh",
    "",
    "  1. token store (builder)",
    "  2. refresh endpoint (builder)",
    "  3. review + tests (reviewer)",
    "",
    "\x1b[2m❯\x1b[0m ",
  ],
  "%2": [
    "\x1b[2m[from planner]\x1b[0m implement the token store in src/auth/store.rs",
    "",
    "\x1b[32m+\x1b[0m fn refresh(&mut self, now: Instant) -> Result<Token>",
    "\x1b[32m+\x1b[0m     if self.expires_at <= now { self.fetch()? }",
    "\x1b[32m+\x1b[0m     Ok(self.current.clone())",
    "",
    "running cargo test …",
  ],
  "%3": [
    "\x1b[2m[from builder]\x1b[0m diff is ready in src/auth",
    "",
    "Reviewing 2 files…",
    "",
    "\x1b[33m?\x1b[0m Allow opencode to run `cargo clippy`? \x1b[2m(y/N)\x1b[0m ",
  ],
  "%4": ["\x1b[2m~/code/api\x1b[0m \x1b[36m❯\x1b[0m "],
};

function snap(): Snapshot {
  return {
    session: mockSession,
    agents: agents.map((a) => ({ ...a })),
    edges: [...edges],
    presence: [{ who: "budi@hubstudio.id", role: "operator", x: 980, y: 140, selected: "builder", at: Date.now() / 1000 }],
    pending: [{ id: 7, at: Date.now() / 1000 - 300, by: "sari@hubstudio.id", to: "builder", text: "Please also cover the refresh endpoint with a test for an expired token." }],
    lastMessage: last,
    loops: { ...loops },
    awaiting: {},
    peers: [{ name: "mini", host: "mac-mini", ok: true, error: null, agents: 1 }],
    selfName: "mac",
    tasks: tasks.filter((t) => t.status !== "done"),
    notes: notesText,
    serving: null,
    merges: merges,
    restorable: discarded || agents.some((a) => a.name === "docs-writer") ? [] : ["docs-writer"],
    staleBrief: mockStale,
    runs: runs.filter((r) => ["running", "paused", "testing"].includes(r.status)),
  };
}
function emitSnapshot() {
  snapshotSubs.forEach((cb) => cb(snap()));
}
function paneWrite(id: string, text: string) {
  paneSubs.get(id)?.forEach((cb) => cb(enc.encode(text)));
}

setInterval(() => {
  const b = agents[1];
  b.quietSecs = 0.4;
  paneWrite("%2", `\r\n\x1b[2m${new Date().toLocaleTimeString()}\x1b[0m test auth::refresh … \x1b[32mok\x1b[0m`);
  emitSnapshot();
}, 2500);

export const mockBackend: Backend = {
  doctor: async () => ({
    ok: 6, warn: 2, fail: 0,
    facts: [["hubcodes", "0.1.0"], ["os", "macOS 26.5"], ["LANG", "en_US.UTF-8"]],
    checks: [
      { id: "tmux", title: "tmux", level: "ok", detail: "tmux 3.8", fix: "" },
      { id: "locale", title: "Locale", level: "ok", detail: "en_US.UTF-8", fix: "" },
      { id: "state", title: "State folder", level: "ok", detail: "~/.local/state/hubcodes", fix: "" },
      { id: "config", title: "Config file", level: "ok", detail: "~/.config/hubcodes/hubcodes.toml", fix: "" },
      { id: "providers", title: "Providers", level: "warn", detail: "ready: claude, codex · not signed in: opencode", fix: "Sign in to the ones you use: Settings → Providers → Check." },
      { id: "cli", title: "hubcodes on the login PATH", level: "ok", detail: "~/.local/bin/hubcodes", fix: "" },
      { id: "session", title: "tmux session", level: "ok", detail: "hubcodes: 5 agents", fix: "" },
      { id: "tailscale", title: "Tailscale", level: "ok", detail: "100.64.0.3", fix: "" },
      { id: "signing", title: "Code signature", level: "warn", detail: "Apple Development certificate (this Mac only)", fix: "Teammates' Gatekeeper rejects this build. Sign with a Developer ID certificate and notarize." },
    ],
  }),
  daemonStatus: async () => ({ installed: false, running: false, pid: null, plist: "~/Library/LaunchAgents/id.hubstudio.hubcodes.daemon.plist", log: "~/.local/state/hubcodes/hubcodesd.log", tender: "desktop", socket: false, sessions: [], subscribers: 0, version: null }),
  daemonSet: async (on) => ({ installed: on, running: on, pid: on ? 4242 : null, plist: "~/Library/LaunchAgents/id.hubstudio.hubcodes.daemon.plist", log: "~/.local/state/hubcodes/hubcodesd.log", tender: on ? "hubcodesd" : "desktop", socket: on, sessions: on ? ["hubcodes"] : [], subscribers: on ? 1 : 0, version: on ? "0.1.5" : null }),
  sessions: async () => [
    { name: "hubcodes", running: true, agents: 5, saved: true, current: mockSession === "hubcodes" },
    { name: "hermina-core", running: true, agents: 2, saved: true, current: mockSession === "hermina-core" },
    { name: "pmp", running: false, agents: 0, saved: true, current: mockSession === "pmp" },
  ],
  // The real app restarts into the session; the mock just renames the window.
  switchSession: async (name) => { mockSession = name; },
  updateCheck: async () => { await new Promise((r) => setTimeout(r, 1200)); return { version: "0.1.1", notes: "Flat board, opening screen, session switch fix.", date: "2026-11-13T02:00:00Z", current: "0.1.0" }; },
  updateInstall: async () => {
    const total = 16_500_000;
    for (let d = 0; d <= total; d += total / 20) { await new Promise((r) => setTimeout(r, 120)); for (const cb of progressCbs) cb({ downloaded: Math.min(d, total), total }); }
    await new Promise((r) => setTimeout(r, 800));
    throw new Error("mock: would restart here");
  },
  onUpdateProgress: async (cb) => { progressCbs.push(cb); return () => { const i = progressCbs.indexOf(cb); if (i >= 0) progressCbs.splice(i, 1); }; },
  pendingDecide: async (id) => `approved #${id}`,
  roomList: async () => mockRooms.map((r) => ({ ...r, members: [...r.members] })),
  roomLog: async (name) => (mockRoomLog[name] ?? []).slice(),
  roomCreate: async (name, members) => {
    if (!/^[a-z0-9][a-z0-9_-]{0,31}$/.test(name)) throw new Error(`room names are lower case letters, digits, - and _ (up to 32): '${name}'`);
    if (mockRooms.some((r) => r.name === name)) throw new Error(`room '${name}' already exists`);
    mockRooms.push({ name, created_at: Date.now() / 1000, created_by: "human:you", members });
    mockRoomLog[name] = [];
  },
  roomMembers: async (name, add, remove) => {
    const r = mockRooms.find((x) => x.name === name);
    if (!r) throw new Error(`no room '${name}' on this device`);
    r.members = [...new Set([...r.members.filter((m) => !remove.includes(m)), ...add])].sort();
  },
  roomDelete: async (name) => {
    const i = mockRooms.findIndex((x) => x.name === name);
    if (i >= 0) mockRooms.splice(i, 1);
    delete mockRoomLog[name];
  },
  roomPost: async (name, text) => {
    const r = mockRooms.find((x) => x.name === name);
    if (!r) throw new Error(`no room '${name}' on this device`);
    const log = (mockRoomLog[name] ??= []);
    log.push({ id: log.length + 1, at: Date.now() / 1000, from: "human:you", text });
    return r.members.length;
  },
  auditLog: async () => [
    { id: 3, at: Date.now() / 1000 - 120, agent: "budi@hubstudio.id", kind: "share", detail: "approved reviewer" },
    { id: 2, at: Date.now() / 1000 - 600, agent: "sari@hubstudio.id", kind: "share", detail: "asked to send to builder: please also cover the refresh endpoint" },
    { id: 1, at: Date.now() / 1000 - 3600, agent: "budi@hubstudio.id", kind: "share", detail: "sent to planner: status?" },
  ],
  presence: async () => {},
  usageLog: async (days) => {
    const out = [];
    const day = 86400;
    const t0 = Date.now() / 1000;
    const models = { planner: "claude-opus-4", builder: "claude-sonnet-4", reviewer: "gpt-5-codex", "mini/tester": "gpt-5-codex" } as Record<string, string>;
    for (let d = 0; d < Math.min(days, 14); d++) {
      for (const agent of ["planner", "builder", "reviewer", "mini/tester"]) {
        const turns = agent === "builder" ? 9 : 4;
        for (let i = 0; i < turns; i++) {
          out.push({ at: t0 - d * day - i * 900, agent, model: models[agent], inputTokens: 2000 + ((d * 7 + i * 13) % 5000), outputTokens: 400 + ((d * 3 + i * 11) % 1500), cacheReadTokens: 12000 + ((d + i) % 7) * 3000, cacheWriteTokens: 500, run: d === 0 && agent === "builder" ? 1 : 0 });
        }
      }
    }
    return out;
  },
  diagnostics: async () => "hubcodes diagnostics\n  hubcodes: 0.1.0\n\n[ok  ] tmux: tmux 3.8\n[warn] Providers: not signed in: opencode\n",
  logTail: async () => "[hubcodes] PATH from /bin/zsh: ok\n[hubcodes] no locale in the environment, using en_US.UTF-8\n[hubcodes] cli: ~/.local/bin/hubcodes → /Applications/hubcodes.app/Contents/MacOS/hubcodes\n[js] host.attach %1 ok",
  sessionInfo: async () => ({ session: mockSession, tmuxAvailable: true, tmuxVersion: "tmux 3.8 (mock)", configPath: null, home: "/Users/you", defaultFolder: config.defaultFolder, roles: config.roles, cliPath: "/Applications/hubcodes.app/Contents/MacOS/hubcodes", cliNote: "~/.local/bin/hubcodes → /Applications/hubcodes.app/Contents/MacOS/hubcodes", cliOnPath: true }),
  providers: async () =>
    config.providers.map(({ name, command, glyph, color, installed, models, modelFlag, roleFlag }) => ({ name, command, glyph, color, installed, models, takesModel: !!modelFlag, takesRole: !!roleFlag, accounts: mockAccounts[name] ?? ["default"] })),
  accountList: async (provider) => mockAccounts[provider] ?? ["default"],
  accountAdd: async (provider, name) => {
    const n = name.trim();
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(n)) throw new Error("an account name is one word of letters, digits, - _ .");
    mockAccounts[provider] = Array.from(new Set([...(mockAccounts[provider] ?? ["default"]), n]));
    return mockAccounts[provider];
  },
  accountRemove: async (provider, name) => {
    mockAccounts[provider] = (mockAccounts[provider] ?? ["default"]).filter((a) => a !== name);
    return mockAccounts[provider];
  },
  loginTerminal: async (kind) => `${kind.replace(/^provider:/, "")}-login`,
  accountLogin: async (provider, name) => {
    const full = `${provider}-${name}-login`;
    if (!agents.some((a) => a.name === full)) {
      agents.push({ id: full, name: full, provider, status: "waiting", cwd: "~", command: provider, dead: false, quietSecs: 0, lastLine: "", role: null, model: null, peer: null, preview: [`${provider} under account ${name}`, "", "/login to sign in"], worktree: null, changes: null, usage: null, test: null, holder: null, private: false, account: name });
      emitSnapshot();
    }
    return full;
  },
  providerStatus: async (name, _refresh, account) => {
    const under = account && account !== "default" ? account : null;
    const base: ProviderStatus = { name, installed: true, path: `/usr/local/bin/${name}`, version: "1.0.0", loggedIn: true, configPath: `~/.${name}/config`, currentModel: null, models: [], source: "", setup: { install: null, login: null, docs: null, modelsFrom: "" } };
    const table: Record<string, Partial<ProviderStatus>> = {
      claude: { version: "2.1.322 (Claude Code)", currentModel: "claude-fable-5-1[1m]", source: "~/.claude.json", setup: { install: "npm install -g @anthropic-ai/claude-code", login: "claude   # then /login", docs: "https://docs.anthropic.com/en/docs/claude-code", modelsFrom: "the model picker cache in ~/.claude.json plus the built-in aliases" }, models: [
        { id: "default", label: "Default", note: "your account's recommended model", current: false },
        { id: "opus", label: "Opus", note: "alias → latest Opus", current: false },
        { id: "sonnet", label: "Sonnet", note: "alias → latest Sonnet", current: false },
        { id: "haiku", label: "Haiku", note: "alias → latest Haiku", current: false },
        { id: "claude-fable-5-1[1m]", label: "Fable", note: "Fable 5.1 · Most capable for your hardest and longest-running tasks", current: true },
      ] },
      codex: { version: "codex-cli 0.163.0", source: "~/.codex/models_cache.json", models: [
        { id: "gpt-6-luna", label: "GPT-6-Luna", note: "Fast and affordable model for easier tasks.", current: false },
        { id: "gpt-5.6-terra", label: "GPT-5.6-Terra", note: "Older balanced model for straightforward work.", current: false },
      ] },
      opencode: { version: "1.18.29", currentModel: "openai/gpt-5.6-terra", source: "`opencode models`", models: [
        { id: "openai/gpt-5.6-terra", label: "gpt-5.6-terra", note: "current default", current: true },
        { id: "9router/cx/gpt-5.6-sol", label: "gpt-5.6-sol", note: "9router/cx", current: false },
        { id: "9router/cc/claude-fable-5-1", label: "claude-fable-5-1", note: "9router/cc", current: false },
      ] },
      gemini: { installed: false, path: null, version: null, loggedIn: false, source: "built-in list", setup: { install: "npm install -g @google/gemini-cli", login: "gemini   # signs in with Google on first run, or export GEMINI_API_KEY", docs: "https://github.com/google-gemini/gemini-cli", modelsFrom: "the built-in list; Gemini CLI keeps no local catalog" }, models: [
        { id: "gemini-2.5-pro", label: "gemini-2.5-pro", note: "", current: false },
        { id: "gemini-2.5-flash", label: "gemini-2.5-flash", note: "", current: false },
      ] },
      hermes: { version: "Hermes Agent v1.4.0", loggedIn: false, configPath: "~/.hermes/config.yaml", setup: { install: "curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash", login: "hermes model   # pick a provider, then sign in or paste its API key", docs: "https://hermes-agent.nousresearch.com/docs/getting-started/quickstart", modelsFrom: "the default in ~/.hermes/config.yaml" } },
      pi: { installed: false, path: null, version: null, loggedIn: false, setup: { install: null, login: "pi   # /login inside, then `pi update`", docs: null, modelsFrom: "~/.pi/agent/models.json" } },
      omp: { installed: false, path: null, version: null, loggedIn: false, setup: { install: null, login: "omp", docs: null, modelsFrom: "`model:` entries in ~/.omp/agent/config.yml" } },
      shell: { configPath: null, setup: { install: null, login: null, docs: null, modelsFrom: "A shell has no model." } },
    };
    await new Promise((r) => setTimeout(r, 250));
    const merged = { ...base, ...(table[name] ?? {}) };
    if (under) return { ...merged, loggedIn: under === "work", currentModel: under === "work" ? merged.currentModel : null, source: `${merged.source} (account ${under})` };
    return merged;
  },
  snapshot: async () => snap(),
  spawn: async (name, provider, _cwd, role, model, _worktree, _sandbox, device, account) => {
    const acct = account && account !== "default" ? account : null;
    if (device) {
      const full = `${device}/${name}`;
      agents.push({ id: full, name: full, provider, status: "idle", cwd: "~/code/api", command: provider, dead: false, quietSecs: 0, lastLine: "", role, model, peer: device, preview: [`${provider} starting on ${device}…`], worktree: null, changes: null, usage: null, test: null, holder: null, private: false, account: acct });
      emitSnapshot();
      return full;
    }
    const id = `%${agents.length + 1}`;
    agents.push({ id, name, provider, status: "idle", cwd: "~/code/api", command: provider, dead: false, quietSecs: 0, lastLine: "", role, model, peer: null, preview: [], worktree: null, changes: null, usage: null, test: null, holder: null, private: false, account: acct });
    screens[id] = [`\x1b[2m${provider}\x1b[0m starting…`, "", "\x1b[2m❯\x1b[0m "];
    emitSnapshot();
    return id;
  },
  setRole: async (id, role) => {
    const a = agents.find((x) => x.id === id);
    if (a) a.role = role.trim() || null;
    emitSnapshot();
  },
  setPrivate: async (id, on) => {
    const a = agents.find((x) => x.id === id);
    if (a) a.private = on;
    emitSnapshot();
  },
  kill: async (id) => {
    const i = agents.findIndex((a) => a.id === id);
    if (i >= 0) {
      const name = agents[i].name;
      agents.splice(i, 1);
      edges = edges.filter((e) => e.from !== name && e.to !== name);
    }
    emitSnapshot();
  },
  saveAttachment: async (name) => { await new Promise((r) => setTimeout(r, 120)); return `/Users/demo/.local/state/hubcodes/attachments/${Date.now()}-${name.replace(/[^\w.-]+/g, "-")}`; },
  composeMessage: async (id, text, paths) => {
    await new Promise((r) => setTimeout(r, paths.length && id.includes("/") ? 400 : 0));
    // An agent on another device gets the copies there.
    const far = id.includes("/") ? paths.map((p) => `/Users/mini/.local/state/hubcodes/attachments/${p.split("/").pop()}`) : paths;
    if (!far.length) return text.trim();
    const head = far.length === 1 ? "Attached file (open it with your file tools)" : "Attached files (open them with your file tools)";
    return `${text.trim() ? `${text.trim()}\n\n` : ""}${head}:\n${far.map((p) => `- ${p}`).join("\n")}`;
  },
  instruct: async (id, text, direct) => {
    const a = agents.find((x) => x.id === id);
    const lead = !direct && a ? routeLead(agents, edges, a) : null;
    // Stays with the agent, with a note to check with its lead first.
    if (lead) text = `${text} [checks with ${lead}]`;
    paneWrite(id, `\r\n\x1b[2m❯\x1b[0m ${text}\r\n\x1b[2mthinking…\x1b[0m`);
    if (a) {
      a.status = "working";
      a.quietSecs = 0;
      seq += 1;
      last = { seq, from: "human:you", to: a.name, text };
    }
    emitSnapshot();
    return lead;
  },
  taskAdd: async (title, detail, role, owner) => {
    const t: Task = { id: tasks.length + 1, at: Date.now() / 1000, updated: Date.now() / 1000, title, detail, role, owner, status: owner ? "doing" : "todo", createdBy: "human:you", result: "", external: "", externalUrl: "", synced: "", gate: "", run: 0, rank: 0 };
    tasks.push(t);
    emitSnapshot();
    return t;
  },
  taskUpdate: async (id, title, detail, role) => {
    const t = tasks.find((x) => x.id === id)!;
    Object.assign(t, { title: title.trim(), detail: detail.trim(), role: role.trim(), updated: Date.now() / 1000 });
    emitSnapshot();
    return t;
  },
  taskPlace: async (id, column, before) => {
    const t = tasks.find((x) => x.id === id)!;
    t.status = column;
    const col = tasks.filter((x) => x.status === column && x.id !== id).sort((a, b) => a.rank - b.rank || a.id - b.id);
    const at = before === null ? col.length : Math.max(0, col.findIndex((x) => x.id === before));
    col.splice(at, 0, t);
    col.forEach((x, i) => { x.rank = (i + 1) * 10; });
    t.updated = Date.now() / 1000;
    emitSnapshot();
    return t;
  },
  taskHistory: async (id) => [
    { at: Date.now() / 1000 - 3600, who: "human:you", kind: "task", text: `task #${id} added` },
    { at: Date.now() / 1000 - 1800, who: "hubcodes", kind: "task", text: `to builder: task #${id}: picked up` },
    { at: Date.now() / 1000 - 600, who: "builder", kind: "message", text: `to planner: [hubcodes] working on #${id}, store done, tests next` },
  ],
  boardColumns: async () => config.board.columns,
  boardColumnsSet: async (columns) => {
    const builtin = [["todo", "To do"], ["doing", "Doing"], ["review", "Review"], ["done", "Done"]];
    const slug = (x: string) => x.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
    const next = columns.map((c) => ({ id: slug(c.id || c.label), label: c.label.trim() || slug(c.id || c.label) })).filter((c) => c.id);
    for (const [id, label] of builtin) if (!next.some((c) => c.id === id)) next.push({ id, label });
    for (const t of tasks) if (!next.some((c) => c.id === t.status)) t.status = "todo";
    config = { ...config, board: { columns: next } };
    emitSnapshot();
    return next;
  },
  taskSet: async (id, status, result) => {
    const t = tasks.find((x) => x.id === id)!;
    t.status = status;
    if (result !== undefined) t.result = result;
    t.updated = Date.now() / 1000;
    emitSnapshot();
    return t;
  },
  taskAssign: async (id, agent) => {
    const t = tasks.find((x) => x.id === id)!;
    t.owner = agent;
    if (t.status === "todo") t.status = "doing";
    emitSnapshot();
    return t;
  },
  taskRemove: async (id) => {
    tasks = tasks.filter((x) => x.id !== id);
    emitSnapshot();
  },
  prApprove: async (id) => {
    const t = tasks.find((x) => x.id === id)!;
    t.synced = "approved";
    emitSnapshot();
    return `Approved ${t.external.split(":")[1]}`;
  },
  prIgnore: async (id) => {
    const t = tasks.find((x) => x.id === id)!;
    t.status = "done";
    t.synced = "ignored";
    emitSnapshot();
    return `Ignoring ${t.external.split(":")[1]}`;
  },
  taskResume: async (id) => {
    const t = tasks.find((x) => x.id === id);
    if (t) t.synced = "resumed";
    emitSnapshot();
  },
  tasksAll: async (includeDone) => tasks.filter((t) => includeDone || t.status !== "done"),
  notesWrite: async (text) => {
    notesText = text;
    emitSnapshot();
  },
  notesAdd: async (text) => {
    notesText += `- **human:you** (now): ${text}\n`;
    emitSnapshot();
  },
  fanoutStart: async (targets, collector, prompt) => {
    seq += 1;
    last = { seq, from: "human:you", to: targets.join(","), text: `[fanout #1] ${prompt}` };
    for (const n of targets) {
      const a = agents.find((x) => x.name === n);
      if (a) paneWrite(a.id, `\r\n${prompt}\r\n[fanout #1] reply with hubcodes send ${collector ?? "you"} …`);
    }
    emitSnapshot();
    return { id: 1, at: Date.now() / 1000, prompt, collector: collector ?? "", expected: targets.length, received: 0, done: false };
  },
  fanouts: async () => [],
  templates: async () => templates,
  teamStart: async (name, _cwd, models) => {
    const tpl = templates.find((t) => t.name === name)!;
    const spawned: string[] = [];
    const skipped: string[] = [];
    for (const a of tpl.agents) {
      if (agents.some((x) => x.name === a.name)) { skipped.push(a.name); continue; }
      const id = `%${agents.length + 1}`;
      const model = models && a.name in models ? models[a.name].trim() || null : a.model;
      agents.push({ id, name: a.name, provider: a.provider, status: "idle", cwd: "~/code/api", command: a.provider, dead: false, quietSecs: 0, lastLine: "", role: a.role, model, peer: null, preview: [], worktree: null, changes: null, usage: null, test: null, holder: null, private: false, account: null });
      screens[id] = [`\x1b[2m${a.provider}\x1b[0m starting…`, "", "\x1b[2m❯\x1b[0m "];
      spawned.push(a.name);
    }
    for (const l of tpl.links) {
      if (!edges.some((e) => (e.from === l[0] && e.to === l[1]) || (e.from === l[1] && e.to === l[0]))) edges.push({ from: l[0], to: l[1], label: l[2] === "loop" ? "loop" : "link" });
      if (l[2] === "loop") loops[pairKey(l[0], l[1])] = { rounds: 0, max: 20, paused: false, done: false, goal: "" };
    }
    emitSnapshot();
    return { spawned, skipped, links: tpl.links.length, errors: [] };
  },
  teamSave: async (name) => {
    const tpl: Template = { name, description: "", agents: agents.filter((a) => !a.peer).map((a) => ({ name: a.name, provider: a.provider, role: a.role, model: a.model, worktree: null })), links: edges.map((e) => (loops[pairKey(e.from, e.to)] ? [e.from, e.to, "loop"] : [e.from, e.to])) };
    templates.unshift(tpl);
    return tpl;
  },
  promptList: async () => promptsList,
  promptPreview: async (id, text) => {
    const m = /^\/(\w+)\s*(.*)$/s.exec(text.trim());
    const p = m && promptsList.find((x) => x.name === m[1]);
    if (!p) return text;
    const a = agents.find((x) => x.id === id);
    return p.text.replace("{branch}", a?.changes?.branch ?? "(no git)").replace("{cwd}", a?.cwd ?? "").replace("{last_diff}", "(diff)") + (m[2] ? `\n\n${m[2]}` : "");
  },
  rolesList: async () => ["orchestrator", "software engineer", "reviewer", "tester"].map((role) => ({ role, kind: mockKind(role), mode: mockRoles[role] ? "replaces" : "generated", file: mockRoles[role] ? `/Users/you/.config/hubcodes/roles/${role.replace(/ /g, "-")}.md` : null })),
  roleGet: async (role) => {
    const k = role.trim().toLowerCase();
    const def = MOCK_JOBS[mockKind(k)];
    return { default: def, text: mockRoles[k] ?? def, file: mockRoles[k] ? `/Users/you/.config/hubcodes/roles/${k.replace(/ /g, "-")}.md` : null };
  },
  roleSave: async (role, text) => {
    const k = role.trim().toLowerCase();
    if (!text.trim() || text.trim() === MOCK_JOBS[mockKind(k)]) { delete mockRoles[k]; return null; }
    mockRoles[k] = text.trim();
    return `/Users/you/.config/hubcodes/roles/${k.replace(/ /g, "-")}.md`;
  },
  roleBrief: async (role, cwd) => {
    const r = role.trim().toLowerCase();
    const text = r.includes("orchestr") || r.includes("lead")
      ? "Your job is to coordinate the team, not to do the work yourself. Do not edit files, write code, or run builds and tests on your own; the engineers do that and the reviewer checks it. Engineers available now: codex-1, opencode-1. Reviewer: claude-2."
      : r.includes("review")
        ? "Your job is to review what the engineers send you, not to rewrite it. Reply with findings (file:line, what is wrong, the fix) to the sender and to claude-1 with hubcodes send <name> \"...\"; say clearly when it is approved."
        : "Do the work your orchestrator sends you, in your own area of the code. When it is done, report back with hubcodes send claude-1 \"done: what changed, how to verify\". When a brief is unclear, ask instead of guessing.";
    const file = r.includes("review") ? `${cwd?.trim() || "/Users/you/code/app"}/.hubcodes/roles/reviewer.md` : null;
    return { text: file ? `${text}\n\nIn this repo, run pnpm test before approving and reject any change to packages/api without a migration.` : text, file };
  },
  serveStart: async () => "100.64.0.3:7433",
  tailscaleInfo: async () => ({ ip: "100.64.0.3" }),
  onTend: async (cb) => {
    tendSubs.add(cb);
    return () => tendSubs.delete(cb);
  },
  memoryPing: async (c) => {
    await new Promise((r) => setTimeout(r, 300));
    if (c.provider === "hindsight" && !c.hindsight.url) return { ok: false, provider: c.provider, detail: "url and bank are required", ms: 2 };
    return { ok: true, provider: c.provider, detail: c.provider === "builtin" ? "session memory: 12 memories" : "MCP server reachable, 9 tools, 'retain' and 'recall' present", ms: 212 };
  },
  memoryRecall: async (query) => [{ text: `Token store lives in src/auth/store.rs (asked: ${query})`, source: "planner", at: Date.now() / 1000 - 3600 }, { text: "Deploys happen Friday after the e2e suite is green", source: "human:you", at: Date.now() / 1000 - 86400 }],
  memoryRetain: async () => "stored",
  kbCheck: async (s) => {
    await new Promise((r) => setTimeout(r, 300));
    return { name: s.name, ok: s.kind !== "confluence", detail: s.kind === "confluence" ? "HTTP 401 (check email + API token)" : s.kind === "git" ? "cloned at ~/.local/state/hubcodes/kb/" + s.name : "MCP reachable, 6 tools; search via 'product_kb_search'", ms: 240 };
  },
  kbSearch: async (query) => ({ hits: [
    { source: "product", title: "Halo Hermina · Registrasi Online", reference: "kb://halo-hermina/registrasi", snippet: `Alur registrasi online pasien… (${query})` },
    { source: "hermina-docs", title: "Deploy runbook", reference: "/ops/deploy.md", snippet: "Deploys happen on Friday after the e2e suite." },
  ], errors: [] }),
  kbGet: async (source, reference) => `# ${reference}\n\n(full text from ${source})`,
  playbookStart: async (name, goal, cwd) => {
    const r: PlaybookRun = { id: runs.length + 1, at: Date.now() / 1000, updated: Date.now() / 1000, playbook: name, goal, status: "running", step: 0, task: 0, merge: 0, attempt: 0, log: `[..] started: ${goal}\n`, createdBy: "human:you", cwd: cwd ?? "", prevResult: "", prevAgent: "" };
    runs = [r, ...runs];
    emitSnapshot();
    return r;
  },
  playbookApprove: async (id) => { const r = runs.find((x) => x.id === id)!; r.status = "running"; r.step += 1; emitSnapshot(); return r; },
  playbookAbort: async (id) => { const r = runs.find((x) => x.id === id)!; r.status = "aborted"; emitSnapshot(); return r; },
  playbookRuns: async (all) => (all ? runs : runs.filter((r) => ["running", "paused", "testing"].includes(r.status))),
  playbooks: async () => playbooksList,
  trackerCheck: async (t) => { await new Promise((r) => setTimeout(r, 300)); return { name: t.name, ok: true, detail: "2 issues match the query", ms: 312, issues: [{ key: "PE-418", title: "Login form uses the new store", url: "", description: "" }, { key: "PE-420", title: "Refresh endpoint", url: "", description: "" }] }; },
  trackerPull: async () => ["jira: PE-420 → task #5"],
  // The mock's other device polls the same Jira project as the sample tracker.
  skillsList: async (device) => (device
    ? [{ name: "pr-review", description: "Reviews pull requests", providers: ["claude", "opencode", "pi"], path: "/Users/me/.claude/skills/pr-review" }, { name: "qa-automation", description: "Runs a QA ticket", providers: ["claude"], path: "/Users/me/.claude/skills/qa-automation" }]
    : mockSkills.map((s) => ({ ...s, providers: [...s.providers] }))),
  skillCopy: async (name, from) => {
    let s = mockSkills.find((x) => x.name === name);
    if (!s) {
      if (!from) throw new Error(`no skill ${name} on this Mac`);
      s = { name, description: "Runs a QA ticket", providers: ["shared"], path: `/Users/me/.agents/skills/${name}` };
      mockSkills.push(s);
    }
    const added = MOCK_TOOLS.filter((p) => !s.providers.includes(p));
    s.providers.push(...added);
    return added.map((p) => `${name} → ${p}`);
  },
  skillsSync: async () => mockSkills.flatMap((s) => {
    const added = MOCK_TOOLS.filter((p) => !s.providers.includes(p));
    s.providers.push(...added);
    return added.map((p) => `${s.name} → ${p}`);
  }),
  skillAdd: async (source) => {
    const name = source.replace(/\/(SKILL\.md)?$/i, "").split("/").pop()!.replace(/\.(git|zip|skill|md)$/, "");
    mockSkills.push({ name, description: "Added from " + source, providers: [...MOCK_TOOLS], path: `/Users/me/.agents/skills/${name}` });
    return [name];
  },
  skillsPull: async () => ["/Users/me/.hub-skills: up to date"],
  trackerOverlaps: async (trackers) => trackers.filter((t) => t.kind === "jira" && t.url.includes("hubstudio.atlassian.net") && t.query.includes("project = PE")).map((t) => ({ tracker: t.name, device: "mini", name: "jira", enabled: true })),
  policyTest: async (p, c) => (p.deny.some((r) => c.startsWith(r.replace(/\*.*$/, ""))) ? { kind: "deny", rule: "deny" } : p.allow.some((r) => c.startsWith(r.replace(/\*.*$/, ""))) ? { kind: "allow", rule: "allow" } : { kind: "ask" }),
  runTests: async (id) => {
    const a = agents.find((x) => x.id === id);
    await new Promise((r) => setTimeout(r, 400));
    if (a) a.test = [true, Date.now() / 1000, "14 passed"];
    emitSnapshot();
    return { ok: true, command: "pnpm test", at: Date.now() / 1000, secs: 3, tail: "✓ auth.spec.ts (12)\n✓ token.spec.ts (2)\n\n14 passed", summary: "14 passed" };
  },
  mergeEnqueue: async (id, into) => {
    const a = agents.find((x) => x.id === id)!;
    const m: MergeItem = { id: merges.length + 1, at: Date.now() / 1000, updated: Date.now() / 1000, repo: "~/code/api", branch: a.changes?.branch ?? "branch", into: into ?? "main", agent: a.name, status: "queued", log: "" };
    merges.unshift(m);
    emitSnapshot();
    setTimeout(() => { m.status = "merged"; m.log = "main → b2c3d4e"; emitSnapshot(); }, 1500);
    return m;
  },
  mergesList: async () => merges,
  sessionDef: async () => ({ session: "hubcodes", savedAt: Date.now() / 1000 - 3600, agents: [{ name: "docs-writer", provider: "claude", role: "docs", model: null, cwd: "~/code/api", worktree: null }], edges: [] }),
  discardSavedSession: async () => { discarded = true; emitSnapshot(); },
  restoreSession: async () => {
    const id = `%${agents.length + 1}`;
    agents.push({ id, name: "docs-writer", provider: "claude", status: "idle", cwd: "~/code/api", command: "claude", dead: false, quietSecs: 0, lastLine: "", role: "docs", model: null, peer: null, preview: [], worktree: null, changes: null, usage: null, test: null, holder: null, private: false, account: null });
    screens[id] = ["\x1b[2mclaude -c\x1b[0m resuming…", "", "\x1b[2m❯\x1b[0m "];
    emitSnapshot();
    return { started: ["docs-writer"], skipped: [], errors: [] };
  },
  switchAgent: async (name, provider, model) => {
    const a = agents.find((x) => x.name === name);
    if (!a) throw new Error(`${name} is not a running or saved agent`);
    const kept = a.provider === provider ? "conversation" : "handoff";
    a.provider = provider;
    a.model = model;
    if (kept === "handoff") a.account = null;
    emitSnapshot();
    return { restored: { started: [name], skipped: [], errors: [] }, kept };
  },
  restartAgents: async (names) => {
    mockStale = mockStale.filter((n) => !names.includes(n));
    emitSnapshot();
    return { started: names, skipped: [], errors: [] };
  },
  taskApprove: async (id) => {
    const t = tasks.find((x) => x.id === id)!;
    t.status = "done";
    emitSnapshot();
    return t;
  },
  taskRequestChanges: async (id, note) => {
    const t = tasks.find((x) => x.id === id)!;
    t.status = "doing";
    t.result = note;
    emitSnapshot();
    return t;
  },
  providerExport: async (name) => `name = "${name}"\ncommand = "${name}"\n`,
  providerImport: async () => ["~/.config/hubcodes/providers/custom.toml"],
  teamInit: async (cwd) => `${cwd}/.hubcodes/team.toml`,
  agentDiff: async (id) => {
    const a = agents.find((x) => x.id === id);
    if (!a?.changes) throw new Error("~/code/api is not a git repository");
    return { dir: a.worktree ?? a.cwd, changes: a.changes, diff: "diff --git a/src/auth/store.rs b/src/auth/store.rs\n--- a/src/auth/store.rs\n+++ b/src/auth/store.rs\n@@ -10,6 +10,12 @@ impl TokenStore {\n     pub fn get(&self) -> Option<&Token> {\n         self.current.as_ref()\n     }\n+\n+    pub fn refresh(&mut self, now: Instant) -> Result<Token> {\n+        if self.expires_at <= now {\n+            self.fetch()?;\n+        }\n+        Ok(self.current.clone())\n+    }\n-    fn old(&self) {}\n }\n" };
  },
  commitAgent: async (id) => {
    const a = agents.find((x) => x.id === id);
    if (a?.changes) a.changes = { ...a.changes, files: 0, insertions: 0, deletions: 0, status: [], ahead: a.changes.ahead + 1 };
    emitSnapshot();
    return "a1b2c3d";
  },
  openPr: async () => "https://github.com/you/api/pull/42",
  history: async (limit, query) => {
    const rows = [
      { seq: 3, at: Date.now() / 1000 - 40, from: "builder", to: "reviewer", text: "diff is ready in src/auth", kind: "message" },
      { seq: 2, at: Date.now() / 1000 - 300, from: "planner", to: "builder", text: "implement the token store in src/auth/store.rs", kind: "message" },
      { seq: 1, at: Date.now() / 1000 - 900, from: "human:you", to: "planner", text: "break down: auth token refresh", kind: "message" },
    ];
    const q = (query ?? "").toLowerCase();
    return rows.filter((r) => !q || r.text.toLowerCase().includes(q) || r.from.includes(q) || r.to.includes(q)).slice(0, limit);
  },
  answerPrompt: async (id, approve) => {
    const a = agents.find((x) => x.id === id);
    if (a) {
      a.status = "working";
      a.quietSecs = 0;
      paneWrite(id, approve ? "\r\n\x1b[32m✓ approved\x1b[0m running cargo clippy…" : "\r\n\x1b[31m✗ denied\x1b[0m");
    }
    emitSnapshot();
  },
  setLoop: async (a, b, on, max) => {
    const k = pairKey(a, b);
    if (on) {
      if (!edges.some((e) => (e.from === a && e.to === b) || (e.from === b && e.to === a))) edges.push({ from: a, to: b, label: "loop" });
      loops[k] = { ...(loops[k] ?? { rounds: 0, goal: "", done: false }), max: max ?? 20, paused: false, done: false };
    } else delete loops[k];
    emitSnapshot();
  },
  resetLoop: async (a, b) => {
    const k = pairKey(a, b);
    if (loops[k]) loops[k] = { ...loops[k], rounds: 0, paused: false, done: false };
    emitSnapshot();
  },
  startLoop: async (a, b, goal, max) => {
    const k = pairKey(a, b);
    if (!edges.some((e) => (e.from === a && e.to === b) || (e.from === b && e.to === a))) edges.push({ from: a, to: b, label: "loop" });
    loops[k] = { rounds: 0, max: max ?? 20, paused: false, done: false, goal };
    seq += 1;
    last = { seq, from: "human:you", to: a, text: goal };
    const t = agents.find((x) => x.name === a);
    if (t) paneWrite(t.id, `\r\n${goal}\r\n[hubcodes loop 0/${max ?? 20}] When you finish this step, send the result to ${b}`);
    emitSnapshot();
    return seq;
  },
  deviceStatus: async (name) => ({ name, host: "mac-mini", ok: name === "mini", error: name === "mini" ? null : "ssh: connect to host timed out", agents: 1, polledSecsAgo: 4, nextPollSecs: 15, queued: name === "mini" ? [] : [{ id: 1, peer: name, args: ["task", "done", "3", "store built", "--as", `${name}/dev`], at: Date.now() / 1000 - 600, attempts: 7, error: "mac did not answer in 20 s" }], issues: name === "mini" ? ["mini runs hubcodes 0.1.7, this Mac 0.1.8; keep both on the same release"] : [], version: name === "mini" ? "0.1.7" : null }),
  deviceRetry: async () => ["delivered to mini: hubcodes task done 3"],
  thisDevice: async () => ({ ...mockThisDevice }),
  createSshKey: async () => {
    mockThisDevice.publicKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAImockkey hubcodes demo@macbook";
    mockThisDevice.authorizeCommand = `mkdir -p ~/.ssh && echo '${mockThisDevice.publicKey}' >> ~/.ssh/authorized_keys`;
    return { ...mockThisDevice };
  },
  deviceModels: async (name) => {
    const me = mockBackend as unknown as { providerStatus: (n: string, r: boolean) => Promise<ProviderStatus> };
    const all = await Promise.all(["claude", "codex", "opencode"].map((n) => me.providerStatus(n, false)));
    return all.map((s) => ({ ...s, source: `${s.source} on ${name}` }));
  },
  checkPeer: async (peer) => {
    await new Promise((r) => setTimeout(r, 400));
    if (peer.host === "nowhere") throw new Error(`ssh: Could not resolve hostname ${peer.host}`);
    return "hubcodes 0.1.0 · 1 agents";
  },
  openRemote: async (id) => {
    const pid = `proxy:${id}`;
    screens[pid] = ["\x1b[2mssh mac-mini · tmux attach -t hubcodes:tester\x1b[0m", "", ...(agents.find((a) => a.id === id)?.preview ?? [])];
    return pid;
  },
  sendMessage: async (from, to, text) => {
    seq += 1;
    last = { seq, from, to, text };
    const k = pairKey(from, to);
    if (loops[k] && !loops[k].paused && !loops[k].done) {
      if (/^done\b/i.test(text.trim())) loops[k] = { ...loops[k], done: true };
      else {
        loops[k] = { ...loops[k], rounds: loops[k].rounds + 1 };
        if (loops[k].max > 0 && loops[k].rounds > loops[k].max) loops[k] = { ...loops[k], paused: true };
      }
    }
    if (!edges.some((e) => (e.from === from && e.to === to) || (e.from === to && e.to === from))) edges.push({ from, to, label: "msg" });
    const t = agents.find((a) => a.name === to);
    if (t) paneWrite(t.id, `\r\n\x1b[2m[from ${from}]\x1b[0m ${text}`);
    emitSnapshot();
    return seq;
  },
  connect: async (from, to, label) => {
    if (!edges.some((e) => (e.from === from && e.to === to) || (e.from === to && e.to === from))) edges.push({ from, to, label: label ?? "link" });
    emitSnapshot();
  },
  disconnect: async (a, b) => {
    edges = edges.filter((e) => !((e.from === a && e.to === b) || (e.from === b && e.to === a)));
    emitSnapshot();
  },
  attach: async (id) => {
    setTimeout(() => paneWrite(id, "\x1b[2J\x1b[H" + (screens[id] ?? []).join("\r\n")), 30);
  },
  detach: async () => {},
  write: async (id, data) => {
    // What reached the pane, for tests.
    ((globalThis as { __paneWrites?: string[] }).__paneWrites ??= []).push(`${id}:${data}`);
    paneWrite(id, data === "\r" ? "\r\n\x1b[2m❯\x1b[0m " : data);
  },
  resize: async () => {},
  loadLayout: async () => layout,
  saveLayout: async (l) => {
    layout = l;
  },
  openPath: async () => {},
  // Tests can hand in a config exactly as the Tauri side sends it (sparse, snake_case where the core is).
  getConfig: async () => normalizeConfig(structuredClone((globalThis as { __hubcodesConfig?: Config }).__hubcodesConfig ?? config)),
  saveConfig: async (c) => {
    const names = c.providers.map((p) => p.name.trim());
    const dup = names.find((n, i) => names.indexOf(n) !== i);
    if (dup) throw new Error(`provider '${dup}' is listed twice`);
    if (names.some((n) => !n)) throw new Error("a provider has no name");
    // Plain tokens go to the Keychain on save; the file keeps a reference.
    const moved: string[] = [];
    const stash = (v: string, name: string, what: string) => { if (v.trim() && v.trim() !== "twg" && !v.includes("${")) { moved.push(what); return `\${keychain:${name}}`; } return v; };
    const next = structuredClone(c);
    next.connections = (next.connections ?? []).map((x) => ({ ...x, token: stash(x.token, `connection:${x.name}`, `connection ${x.name}`) }));
    next.trackers = next.trackers.map((t) => ({ ...t, token: stash(t.token, `tracker:${t.name}:token`, `tracker ${t.name}`), api_key: stash(t.api_key, `tracker:${t.name}:api_key`, `tracker ${t.name}`) }));
    next.kbs = next.kbs.map((k) => ({ ...k, token: stash(k.token, `kb:${k.name}:token`, `knowledge source ${k.name}`) }));
    config = { ...next, exists: true };
    return { path: config.path, moved: Array.from(new Set(moved)) };
  },
  twgProfiles: async () => [{ name: "demo-oauth", url: "https://demo.atlassian.net", user: "me@demo.test" }],
  connectionCheck: async (c) => {
    await new Promise((r) => setTimeout(r, 200));
    if (c.kind !== "github" && !c.token.trim()) throw new Error("HTTP 401 (check the email and token)");
    return c.kind === "github" ? "demo-user" : c.kind === "atlassian" ? "Demo Person" : "3 project(s)";
  },
  connectionChoices: async (c, what, arg) => {
    await new Promise((r) => setTimeout(r, 150));
    const ch = (xs: [string, string][]) => ({ ok: true, choices: xs.map(([value, label]) => ({ value, label })) });
    if (c.kind === "atlassian" && what === "projects") return ch([["PE", "PE · Platform Engineering"], ["PMP", "PMP · Procure to pay"]]);
    if (c.kind === "atlassian" && what === "statuses") return ch(["To Do", "Selected for Development", "In Development", "In Review", "Done"].map((s) => [s, s]));
    if (c.kind === "atlassian" && what === "spaces") return ch([["ENG", "ENG · Engineering"], ["PROD", "PROD · Product"]]);
    if (c.kind === "plane" && what === "projects") return ch([["5f1c0a2e-0000-4000-8000-000000000001", "WEB · Website"]]);
    if (c.kind === "plane" && what === "statuses") return ch(["Backlog", "Todo", "In Progress", "Done"].map((s) => [s, s]));
    if (c.kind === "github" && what === "repos") return ch([["hubstudio/app", "hubstudio/app"], ["hubstudio/web", "hubstudio/web"]]);
    if (c.kind === "github" && what === "labels") return ch(["bug", "ready", "in-progress"].map((s) => [s, s]));
    return { ok: false, choices: [], error: `a ${c.kind} connection has no ${what}${arg ? ` for ${arg}` : ""}` };
  },
  mcpDiscover: async (target, purposes) => {
    await new Promise((r) => setTimeout(r, 200));
    if (!target.url && !target.command) throw new Error("MCP target needs a url or a command");
    const all: Record<string, [string, unknown]> = { search: ["kb_search", { query: "{query}" }], get: ["kb_get", { doc_id: "{id}" }], list: ["issues_search", { jql: "{query}" }], transition: ["issue_transition", { issue_key: "{key}", to: "{to}" }], comment: ["issue_comment", { issue_key: "{key}", body: "{text}" }], retain: ["retain", { content: "{text}" }], recall: ["recall", { query: "{query}" }] };
    return { tools: Object.values(all).map(([name]) => ({ name, description: "" })), picks: Object.fromEntries(purposes.filter((p) => all[p]).map((p) => [p, all[p]])) };
  },
  testNotification: async () => {},
  onSnapshot: async (cb) => {
    snapshotSubs.add(cb);
    return () => snapshotSubs.delete(cb);
  },
  onPane: async (id, cb) => {
    if (!paneSubs.has(id)) paneSubs.set(id, new Set());
    paneSubs.get(id)!.add(cb);
    return () => paneSubs.get(id)?.delete(cb);
  },
};
