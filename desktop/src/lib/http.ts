// The same canvas, served by `hubcodes serve` to a browser on the tailnet:
// a Backend over the JSON API instead of Tauri commands. No PTYs here (cards
// show previews, the composer and approvals work); anything that needs the
// desktop app says so.
import type { Agent, Backend, Config, Layout, ProviderStatus, SessionInfo, Snapshot, Task } from "./types";

const base = () => `${window.location.origin}`;
const token = new URLSearchParams(window.location.search).get("token");
const q = token ? `?token=${encodeURIComponent(token)}` : "";

async function api<T>(path: string, body?: unknown, method = body === undefined ? "GET" : "POST"): Promise<T> {
  const r = await fetch(`${base()}${path}${q}`, { method, headers: body === undefined ? {} : { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await r.text();
  let j: unknown = null;
  try { j = JSON.parse(text); } catch { /* text */ }
  if (!r.ok) throw new Error((j as { error?: string })?.error ?? text ?? r.statusText);
  return j as T;
}

interface View {
  session: string;
  me: string;
  role: string;
  agents: (Agent & { preview: string[] })[];
  messages: { seq: number; at: number; from: string; to: string; text: string; kind: string }[];
  loops: Snapshot["loops"];
  notes: string;
  edges: { from: string; to: string; label: string }[];
  tasks: Task[];
  awaiting: Snapshot["awaiting"];
  lastMessage: { seq: number; from: string; to: string; text: string } | null;
  presence?: Snapshot["presence"];
  pending?: Snapshot["pending"];
}

let lastView: View | null = null;
const notHere = (what: string) => Promise.reject(new Error(`${what} needs the hubcodes desktop app on the machine that runs the agents`));

function toSnapshot(v: View): Snapshot {
  return {
    session: v.session,
    // Cards render the preview (no PTY in a browser); names stay plain so links resolve.
    agents: v.agents.map((a) => ({ ...a, peer: null, usage: a.usage ?? null, changes: a.changes ?? null, worktree: a.worktree ?? null, test: a.test ?? null, holder: a.holder ?? null, role: a.role ?? null, model: a.model ?? null, private: a.private ?? false })),
    edges: v.edges,
    lastMessage: v.lastMessage,
    loops: v.loops ?? {},
    awaiting: v.awaiting ?? {},
    peers: [],
    selfName: v.session,
    tasks: v.tasks ?? [],
    notes: v.notes ?? "",
    serving: window.location.host,
    merges: [],
    restorable: [],
    staleBrief: [],
    runs: [],
    presence: (v.presence ?? []).filter((p) => p.who !== v.me),
    pending: v.pending ?? [],
  };
}

/** Contributors' messages come back as 202 with a pending id; say so instead of pretending it went. */
async function sendOrQueue(to: string, text: string): Promise<number> {
  const r = await api<{ seq?: number; pending?: number }>("/api/send", { to, text });
  if (r.pending) throw new Error(`Sent for approval (#${r.pending}): an operator has to approve it before ${to} sees it`);
  return r.seq ?? 0;
}

const layoutKey = () => `hubcodes-layout-${lastView?.session ?? "remote"}`;

export const httpBackend: Backend = {
  sessionInfo: async () => {
    const v = await api<View>("/api/view");
    lastView = v;
    const info: SessionInfo = { session: v.session, tmuxAvailable: true, tmuxVersion: "remote", configPath: null, home: "", defaultFolder: null, roles: [], cliPath: null, cliNote: `connected as ${v.me} (${v.role})`, cliOnPath: true };
    return info;
  },
  doctor: () => notHere("Doctor"),
  diagnostics: () => notHere("Diagnostics"),
  logTail: async () => "",
  usageLog: async () => [],
  daemonStatus: () => notHere("The background watcher"),
  daemonSet: () => notHere("The background watcher"),
  sessions: async () => [],
  pendingDecide: (id, approve) => api<{ result: string }>("/api/pending", { id, approve }).then((r) => r.result),
  // Rooms are kept on the owner's Mac and changed from there only.
  roomList: async () => [],
  roomLog: async () => [],
  roomCreate: () => Promise.reject(new Error("Rooms are managed in the hubcodes app on the owner's device")),
  roomMembers: () => Promise.reject(new Error("Rooms are managed in the hubcodes app on the owner's device")),
  roomDelete: () => Promise.reject(new Error("Rooms are managed in the hubcodes app on the owner's device")),
  roomPost: () => Promise.reject(new Error("Rooms are managed in the hubcodes app on the owner's device")),
  auditLog: () => api("/api/audit"),
  presence: (x, y, selected) => api("/api/presence", { x, y, selected }).then(() => undefined).catch(() => undefined),
  switchSession: () => notHere("Switching sessions"),
  updateCheck: async () => null,
  updateInstall: () => notHere("Updating the app"),
  onUpdateProgress: async () => () => {},
  providers: async () => [],
  providerStatus: () => notHere("Provider setup"),
  accountList: async () => ["default"],
  accountAdd: () => notHere("Accounts"),
  accountRemove: () => notHere("Accounts"),
  accountLogin: () => notHere("Accounts"),
  loginTerminal: () => notHere("Signing in"),
  snapshot: async () => {
    const v = await api<View>("/api/view");
    lastView = v;
    return toSnapshot(v);
  },
  spawn: () => notHere("Starting an agent"),
  setRole: () => notHere("Changing a role"),
  setPrivate: () => notHere("Marking an agent private"),
  kill: () => notHere("Removing an agent"),
  // The page server routes it like the app does; it does not say where it went.
  instruct: (id, text) => sendOrQueue(id, text).then(() => null),
  saveAttachment: () => notHere("Attaching files"),
  composeMessage: async (_id, text, paths) => (paths.length ? notHere("Attaching files") : text),
  sendMessage: (_from, to, text) => sendOrQueue(to, text),
  connect: (a, b) => api("/api/link", { a, b }).then(() => undefined),
  disconnect: (a, b) => api("/api/link", { a, b, remove: true }).then(() => undefined),
  attach: async () => {},
  detach: async () => {},
  write: () => notHere("Typing into a terminal"),
  resize: async () => {},
  loadLayout: async () => {
    try {
      const raw = localStorage.getItem(layoutKey());
      return raw ? (JSON.parse(raw) as Layout) : { nodes: {}, viewport: null };
    } catch {
      return { nodes: {}, viewport: null };
    }
  },
  saveLayout: async (l) => {
    try { localStorage.setItem(layoutKey(), JSON.stringify(l)); } catch { /* private window */ }
  },
  openPath: () => notHere("Opening a folder"),
  getConfig: () => notHere("Settings") as Promise<Config>,
  saveConfig: () => notHere("Settings"),
  connectionCheck: () => notHere("Accounts"),
  twgProfiles: async () => [],
  connectionChoices: () => notHere("Accounts"),
  mcpDiscover: () => notHere("MCP servers"),
  testNotification: () => notHere("Notifications"),
  onSnapshot: async (cb) => {
    const id = window.setInterval(() => httpBackend.snapshot().then(cb).catch(() => {}), 2500);
    return () => window.clearInterval(id);
  },
  onPane: async () => () => {},
  history: (limit, query) => api(`/api/history${query ? `?q=${encodeURIComponent(query)}` : ""}`).then((rows) => (rows as never[]).slice(0, limit)),
  answerPrompt: (id, approve) => api("/api/answer", { agent: id, approve }).then(() => undefined),
  setLoop: (a, b, on) => api("/api/loop", { a, b, on }).then(() => undefined),
  resetLoop: (a, b) => api("/api/loop", { a, b, reset: true }).then(() => undefined),
  startLoop: (a, b, goal) => api<{ seq: number }>("/api/loop", { a, b, goal }).then((r) => r.seq),
  checkPeer: () => notHere("Devices"),
  deviceStatus: () => notHere("Devices"),
  thisDevice: () => notHere("Devices"),
  createSshKey: () => notHere("Devices"),
  deviceRetry: () => notHere("Devices"),
  deviceModels: () => notHere("Devices"),
  openRemote: () => notHere("The live terminal"),
  taskAdd: (title, _detail, role, owner) => api<Task>("/api/task", { title, role, owner }),
  taskSet: (id, status, result) => api<Task>("/api/task", { id, status, result }),
  taskUpdate: () => notHere("Editing a task"),
  taskPlace: (id, column) => api<Task>("/api/task", { id, status: column }),
  taskHistory: async () => [],
  boardColumns: async () => [{ id: "todo", label: "To do" }, { id: "doing", label: "Doing" }, { id: "review", label: "Review" }, { id: "done", label: "Done" }],
  boardColumnsSet: () => notHere("Changing the board's columns"),
  taskAssign: (id, agent) => api<Task>("/api/task", { id, status: "doing", owner: agent }),
  taskRemove: () => notHere("Removing a task"),
  taskResume: () => notHere("Resuming feedback"),
  prApprove: (id) => api<{ result: string }>("/api/pr", { id, action: "approve" }).then((r) => r.result),
  prIgnore: (id) => api<{ result: string }>("/api/pr", { id, action: "ignore" }).then((r) => r.result),
  tasksAll: () => api<Task[]>("/api/tasks"),
  notesWrite: () => notHere("Rewriting the notes (use Add)"),
  notesAdd: (text) => api("/api/note", text as unknown as object).then(() => undefined),
  fanoutStart: () => notHere("Fan-out"),
  fanouts: async () => [],
  templates: async () => [],
  teamStart: () => notHere("Starting a team"),
  teamSave: () => notHere("Saving a team"),
  promptList: async () => [],
  promptPreview: async (_id, text) => text,
  roleBrief: async () => ({ text: "", file: null }),
  rolesList: async () => [],
  roleGet: async () => ({ default: "", text: "", file: null }),
  roleSave: async () => { throw new Error("Roles are edited in the desktop app"); },
  serveStart: async () => window.location.host,
  tailscaleInfo: async () => ({ ip: null }),
  onTend: async () => () => {},
  memoryPing: () => notHere("Memory settings"),
  memoryRecall: async () => [],
  memoryRetain: () => notHere("Memory"),
  kbCheck: () => notHere("Knowledge settings"),
  kbSearch: async () => ({ hits: [], errors: ["Knowledge search runs in the desktop app"] }),
  kbGet: () => notHere("Knowledge"),
  playbookStart: () => notHere("Playbooks"),
  playbookApprove: () => notHere("Playbooks"),
  playbookAbort: () => notHere("Playbooks"),
  playbookRuns: async () => [],
  playbooks: async () => [],
  trackerCheck: () => notHere("Trackers"),
  trackerPull: () => notHere("Trackers"),
  trackerOverlaps: async () => [],
  skillsList: () => notHere("Skills"),
  skillCopy: () => notHere("Skills"),
  skillsSync: () => notHere("Skills"),
  skillAdd: () => notHere("Skills"),
  skillsPull: () => notHere("Skills"),
  policyTest: () => notHere("Policy"),
  runTests: () => notHere("Running tests"),
  mergeEnqueue: () => notHere("Merging"),
  mergesList: async () => [],
  sessionDef: async () => null,
  restoreSession: () => notHere("Restoring"),
  discardSavedSession: () => notHere("Forgetting saved agents"),
  restartAgents: () => notHere("Restarting agents"),
  switchAgent: () => notHere("Switching an agent's provider"),
  taskApprove: (id) => api<Task>("/api/task", { id, status: "done" }),
  taskRequestChanges: (id, note) => api<Task>("/api/task", { id, status: "doing", result: note }),
  providerExport: () => notHere("Provider files"),
  providerImport: () => notHere("Provider files"),
  teamInit: () => notHere("Repo teams"),
  agentDiff: () => notHere("The diff"),
  commitAgent: () => notHere("Committing"),
  openPr: () => notHere("Opening a PR"),
};

export const isRemoteUi = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window) && window.location.pathname.startsWith("/canvas");
export type { ProviderStatus };
