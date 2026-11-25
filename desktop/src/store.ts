import { create } from "zustand";
import { backend, isRemoteUi } from "./lib/backend";
import type { Agent, Awaiting, Edge, Group, Layout, PendingSend, Presence, LoopState, Message, MergeItem, PeerInfo, PlaybookRun, Provider, SessionInfo, Snapshot, Task, DoctorReport, UpdateInfo, UpdateProgress } from "./lib/types";
import { pairKey } from "./lib/types";

export interface Pulse {
  key: string; // "from>to" in edge orientation
  reverse: boolean;
  at: number;
}

export interface ActivityItem {
  seq: number;
  at: number;
  from: string;
  to: string | null;
  text: string;
}

interface Store {
  ready: boolean;
  session: SessionInfo | null;
  providers: Provider[];
  agents: Agent[];
  edges: Edge[];
  loops: Record<string, LoopState>;
  awaiting: Record<string, Awaiting>;
  peers: PeerInfo[];
  selfName: string;
  /** Show other devices' agents as cards here (off: a badge per device in the rail). */
  showRemote: boolean;
  setShowRemote: (on: boolean) => void;
  /** Pane id streamed in the sheet (a proxy pane for remote agents). */
  openPane: string | null;
  tasks: Task[];
  notes: string;
  serving: string | null;
  merges: MergeItem[];
  restorable: string[];
  staleBrief: string[];
  runs: PlaybookRun[];
  boardOpen: boolean;
  notesOpen: boolean;
  positions: Record<string, [number, number]>;
  /**
   * Agents just started or switched, by name, shown with a spinner until their
   * tool is up: when, the pane being replaced (a switch), and what it starts on.
   */
  starting: Record<string, { at: number; from: string | null; to: string }>;
  markStarting(name: string, to?: string): void;
  doneStarting(name: string): void;
  sizes: Record<string, [number, number]>;
  groups: Record<string, Group>;
  viewport: [number, number, number] | null;
  selectedId: string | null;
  selectedEdge: string | null;
  /** Agent id whose "Link to…" is active: the next node click connects. */
  linkingFrom: string | null;
  menu: { x: number; y: number; agentId: string } | null;
  confirm: { title: string; body: string; label: string; run: () => void } | null;
  openId: string | null;
  activity: ActivityItem[];
  pulses: Pulse[];
  lastSeq: number;
  dialog: "none" | "new-agent" | "settings" | "palette" | "loop" | "inbox" | "fanout" | "task" | "knowledge" | "playbook" | "cost" | "rooms";
  /** Which Settings page opens next (Doctor from the setup banner, …). */
  settingsTab: string | null;
  presence: Presence[];
  pending: PendingSend[];
  /** First-run checks: null until run, then the report. */
  doctor: DoctorReport | null;
  doctorDismissed: boolean;
  /** Agents preselected for the fan-out dialog. */
  fanoutTargets: string[];
  /** Pair the loop dialog is for. */
  loopPair: [string, string] | null;
  activityOpen: boolean;
  toast: { text: string; kind: "info" | "error"; at: number } | null;
  /** A newer release on the feed; the bar at the bottom offers it. */
  update: UpdateInfo | null;
  updateState: "idle" | "checking" | "downloading" | "failed";
  updateProgress: UpdateProgress | null;
  updateError: string;
  /** Versions the user said "Later" to, for this run of the app. */
  updateSnoozed: string[];
  checkUpdate(manual?: boolean): Promise<UpdateInfo | null>;
  installUpdate(): Promise<void>;
  snoozeUpdate(): void;

  boot(): Promise<void>;
  /** Re-read session info and providers (after Settings saved). */
  reload(): Promise<void>;
  applySnapshot(s: Snapshot): void;
  select(id: string | null): void;
  selectEdge(key: string | null): void;
  setSize(name: string, w: number, h: number): void;
  openMenu(x: number, y: number, agentId: string): void;
  closeMenu(): void;
  askStop(id: string): void;
  setConfirm(c: Store["confirm"]): void;
  startLink(id: string | null): void;
  open(id: string | null): void;
  setDialog(d: Store["dialog"]): void;
  openSettings(tab?: string): void;
  runDoctor(): Promise<void>;
  dismissDoctor(): void;
  toggleActivity(): void;
  setPosition(name: string, x: number, y: number): void;
  addGroup(name: string, g: Group): void;
  setGroup(name: string, patch: Partial<Group>): void;
  renameGroup(name: string, next: string): void;
  removeGroup(name: string): void;
  /** Move every card inside a band by (dx, dy). */
  moveGroupMembers(name: string, dx: number, dy: number, members: string[]): void;
  setViewport(v: [number, number, number]): void;
  persistLayout(): void;
  notify(text: string, kind?: "info" | "error"): void;
  logHuman(to: string, text: string): void;

  spawn(name: string, provider: string, cwd: string | null, role: string | null, model: string | null, worktree: string | null, sandbox: boolean, device?: string | null, account?: string | null): Promise<void>;
  setRole(id: string, role: string): Promise<void>;
  kill(id: string): Promise<void>;
  routeThroughOrchestrator: boolean;
  /** True when the message went out. */
  instruct(id: string, text: string, direct?: boolean): Promise<boolean>;
  message(from: string, to: string, text: string): Promise<void>;
  connect(from: string, to: string): Promise<void>;
  disconnect(a: string, b: string): Promise<void>;
  setLoop(a: string, b: string, on: boolean, max?: number): Promise<void>;
  resetLoop(a: string, b: string): Promise<void>;
  startLoop(a: string, b: string, goal: string, max?: number): Promise<void>;
  openLoopDialog(a: string, b: string): void;
  answer(id: string, approve: boolean): Promise<void>;
  toggleBoard(): void;
  toggleNotes(): void;
  openFanout(targets: string[]): void;
}

let saveTimer: number | undefined;

export const useStore = create<Store>((set, get) => ({
  ready: false,
  session: null,
  providers: [],
  agents: [],
  edges: [],
  loops: {},
  awaiting: {},
  peers: [],
  selfName: "",
  showRemote: remoteShown(),
  setShowRemote: (on) => {
    try { localStorage.setItem(REMOTE_KEY, on ? "1" : "0"); } catch { /* private window */ }
    set({ showRemote: on });
  },
  routeThroughOrchestrator: true,
  openPane: null,
  tasks: [],
  notes: "",
  serving: null,
  merges: [],
  restorable: [],
  staleBrief: [],
  runs: [],
  boardOpen: false,
  notesOpen: false,
  fanoutTargets: [],
  positions: {},
  starting: {},
  markStarting(name, to = "") {
    const from = get().agents.find((a) => a.name === name && !a.peer)?.id ?? null;
    set({ starting: { ...get().starting, [name]: { at: Date.now(), from, to } } });
  },
  doneStarting(name) {
    const { [name]: _, ...rest } = get().starting;
    set({ starting: rest });
  },
  sizes: {},
  groups: {},
  viewport: null,
  selectedId: null,
  selectedEdge: null,
  linkingFrom: null,
  menu: null,
  confirm: null,
  openId: null,
  activity: [],
  pulses: [],
  lastSeq: 0,
  dialog: "none",
  settingsTab: null,
  presence: [],
  pending: [],
  doctor: null,
  doctorDismissed: false,
  loopPair: null,
  activityOpen: false,
  toast: null,
  update: null,
  updateState: "idle",
  updateProgress: null,
  updateError: "",
  updateSnoozed: [],

  async boot() {
    const [session, providers, layout] = await Promise.all([backend.sessionInfo(), backend.providers(), backend.loadLayout()]);
    set({ session, providers, positions: layout.nodes ?? {}, sizes: layout.sizes ?? {}, groups: layout.groups ?? {}, viewport: layout.viewport ?? null });
    try {
      get().applySnapshot(await backend.snapshot());
      void get().runDoctor();
    } catch (e) {
      get().notify(String(e), "error");
    }
    await backend.onSnapshot((s) => get().applySnapshot(s));
    await backend.onTend((e) => get().notify(e.text, e.kind === "error" ? "error" : "info"));
    set({ ready: true });
  },

  async reload() {
    const [session, providers] = await Promise.all([backend.sessionInfo(), backend.providers()]);
    set({ session, providers });
  },

  applySnapshot(s) {
    const prev = get();
    // A chime when an agent starts needing you (per-Mac preference, off by default).
    if (prev.agents.length > 0 && bellEnabled()) {
      const was = new Map(prev.agents.map((a) => [a.id, a.status]));
      if (s.agents.some((a) => a.status === "waiting" && was.get(a.id) !== undefined && was.get(a.id) !== "waiting")) chime();
    }
    let { activity, pulses, lastSeq } = prev;
    const m = s.lastMessage;
    if (m && m.seq > lastSeq) {
      lastSeq = m.seq;
      if (prev.lastSeq > 0 || activity.length === 0) {
        activity = [{ seq: m.seq, at: Date.now(), from: m.from, to: m.to, text: m.text }, ...activity].slice(0, 200);
      }
      const e = s.edges.find((x) => (x.from === m.from && x.to === m.to) || (x.from === m.to && x.to === m.from));
      if (e) pulses = [...pulses, { key: `${e.from}>${e.to}`, reverse: e.from !== m.from, at: Date.now() }];
    }
    pulses = pulses.filter((p) => Date.now() - p.at < 1200);
    // A switch or restart closes the agent's window before the new one is up.
    // While it is marked as starting, keep its card (the spinner covers it)
    // instead of letting it vanish for a few polls.
    const now = Date.now();
    const starting = Object.fromEntries(Object.entries(prev.starting).filter(([, v]) => now - v.at < 20000));
    const kept = prev.agents.filter((a) => a.name in starting && !s.agents.some((b) => b.name === a.name));
    const agents = kept.length ? [...s.agents, ...kept] : s.agents;
    // The new window has a new pane id: follow the agent by name.
    const byName = (id: string | null) => {
      if (!id || agents.some((a) => a.id === id)) return id;
      const name = prev.agents.find((a) => a.id === id)?.name;
      return agents.find((a) => a.name === name)?.id ?? null;
    };
    let selectedId = byName(prev.selectedId);
    if (!selectedId && agents.length) selectedId = agents[0].id;
    let openId = byName(prev.openId);
    let openPane = prev.openPane;
    if (openId && openId !== prev.openId && !isRemoteUi && !agents.find((a) => a.id === openId)?.peer) openPane = openId;
    if (openId && !agents.some((a) => a.id === openId)) {
      openId = null;
      openPane = null;
    }
    set({ agents, starting, edges: s.edges, loops: s.loops ?? {}, awaiting: s.awaiting ?? {}, peers: s.peers ?? [], selfName: s.selfName ?? "", routeThroughOrchestrator: s.routeThroughOrchestrator ?? true, tasks: s.tasks ?? [], notes: s.notes ?? "", serving: s.serving ?? null, merges: s.merges ?? [], restorable: s.restorable ?? [], staleBrief: s.staleBrief ?? [], runs: s.runs ?? [], presence: s.presence ?? [], pending: s.pending ?? [], activity, pulses, lastSeq, selectedId, openId, openPane });
  },

  select: (id) => set({ selectedId: id, selectedEdge: id ? null : get().selectedEdge }),
  selectEdge: (key) => set({ selectedEdge: key }),
  setSize(name, w, h) {
    set({ sizes: { ...get().sizes, [name]: [Math.round(w), Math.round(h)] } });
    get().persistLayout();
  },
  openMenu: (x, y, agentId) => set({ menu: { x, y, agentId }, selectedId: agentId }),
  closeMenu: () => set({ menu: null }),
  setConfirm: (confirm) => set({ confirm }),
  askStop(id) {
    const a = get().agents.find((x) => x.id === id);
    if (!a) return;
    set({
      menu: null,
      confirm: {
        title: `Remove ${a.name}?`,
        body: `${a.provider === "shell" ? "The shell" : a.provider} is stopped, its tmux pane closes and its links are removed. This cannot be undone.`,
        label: "Remove agent",
        run: () => void get().kill(id),
      },
    });
  },
  startLink(id) {
    set({ linkingFrom: id, menu: null });
    if (id) {
      const a = get().agents.find((x) => x.id === id);
      get().notify(`Click another agent to link it with ${a?.name ?? "this one"} · Esc cancels`);
    }
  },
  open: (id) => {
    if (!id) {
      set({ openId: null, openPane: null });
      return;
    }
    const a = get().agents.find((x) => x.id === id);
    if (isRemoteUi) {
      set({ openId: id, openPane: null, selectedId: id });
      return;
    }
    if (a?.peer) {
      set({ selectedId: id });
      backend
        .openRemote(id)
        .then((pane) => set({ openId: id, openPane: pane }))
        .catch((e) => get().notify(`Could not open ${a.name}: ${e}`, "error"));
      return;
    }
    set({ openId: id, openPane: id, selectedId: id });
  },
  setDialog: (dialog) => set({ dialog }),
  openSettings: (tab) => set({ dialog: "settings", settingsTab: tab ?? null }),
  async runDoctor() {
    try {
      set({ doctor: await backend.doctor() });
    } catch {
      /* remote UI has no doctor */
    }
  },
  dismissDoctor: () => set({ doctorDismissed: true }),
  toggleActivity: () => set({ activityOpen: !get().activityOpen }),

  setPosition(name, x, y) {
    set({ positions: { ...get().positions, [name]: [x, y] } });
    get().persistLayout();
  },
  addGroup(name, g) {
    set({ groups: { ...get().groups, [name]: g } });
    get().persistLayout();
  },
  setGroup(name, patch) {
    const cur = get().groups[name];
    if (!cur) return;
    set({ groups: { ...get().groups, [name]: { ...cur, ...patch } } });
    get().persistLayout();
  },
  renameGroup(name, next) {
    const g = get().groups;
    if (!g[name] || !next.trim() || (next !== name && g[next])) return;
    const copy = { ...g };
    const v = copy[name];
    delete copy[name];
    copy[next.trim()] = v;
    set({ groups: copy });
    get().persistLayout();
  },
  removeGroup(name) {
    const copy = { ...get().groups };
    delete copy[name];
    set({ groups: copy });
    get().persistLayout();
  },
  moveGroupMembers(_name, dx, dy, members) {
    const p = { ...get().positions };
    for (const m of members) {
      const cur = p[m];
      if (cur) p[m] = [Math.round(cur[0] + dx), Math.round(cur[1] + dy)];
    }
    set({ positions: p });
    get().persistLayout();
  },
  setViewport(v) {
    set({ viewport: v });
    get().persistLayout();
  },
  persistLayout() {
    window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => {
      const l: Layout = { nodes: get().positions, sizes: get().sizes, viewport: get().viewport, groups: get().groups };
      backend.saveLayout(l).catch(() => {});
    }, 400);
  },

  async checkUpdate(manual = false) {
    if (isRemoteUi) return null;
    set({ updateState: "checking", updateError: "" });
    try {
      const u = await backend.updateCheck();
      set({ update: u, updateState: "idle" });
      if (manual && !u) get().notify("You are on the latest version");
      return u;
    } catch (e) {
      set({ updateState: "idle", updateError: String(e) });
      if (manual) get().notify(`Update check failed: ${e}`, "error");
      return null;
    }
  },
  async installUpdate() {
    set({ updateState: "downloading", updateProgress: { downloaded: 0, total: null }, updateError: "" });
    const off = await backend.onUpdateProgress((p) => set({ updateProgress: p }));
    try {
      await backend.updateInstall();
      // The app restarts from here; nothing else to do.
    } catch (e) {
      set({ updateState: "failed", updateError: String(e) });
    } finally {
      off();
    }
  },
  snoozeUpdate() {
    const u = get().update;
    if (u) set({ updateSnoozed: [...get().updateSnoozed, u.version] });
  },

  notify(text, kind = "info") {
    set({ toast: { text, kind, at: Date.now() } });
    window.setTimeout(() => {
      const t = get().toast;
      if (t && Date.now() - t.at >= (kind === "error" ? 8000 : 3000) - 50) set({ toast: null });
    }, kind === "error" ? 8000 : 3000);
  },
  logHuman(to, text) {
    set({ activity: [{ seq: 0, at: Date.now(), from: "you", to, text }, ...get().activity].slice(0, 200) });
  },

  async spawn(name, provider, cwd, role, model, worktree, sandbox, device, account) {
    try {
      get().markStarting(name);
      const id = await backend.spawn(name, provider, cwd, role, model, worktree, sandbox, device ?? null, account ?? null);
      get().notify(device ? `Started ${name} on ${device}` : account ? `Started ${name} (account ${account})` : `Started ${name}`);
      set({ selectedId: id, dialog: "none" });
    } catch (e) {
      get().notify(`Could not start ${name}: ${e}`, "error");
      throw e;
    }
  },
  async setRole(id, role) {
    try {
      await backend.setRole(id, role);
    } catch (e) {
      get().notify(`Could not set role: ${e}`, "error");
    }
  },
  async kill(id) {
    const a = get().agents.find((x) => x.id === id);
    try {
      await backend.kill(id);
      if (a) get().notify(`Removed ${a.name}`);
      if (get().openId === id) set({ openId: null });
    } catch (e) {
      get().notify(`Could not remove: ${e}`, "error");
    }
  },
  async instruct(id, text, direct) {
    const a = get().agents.find((x) => x.id === id);
    try {
      const lead = await backend.instruct(id, text, direct);
      if (lead && a) {
        get().logHuman(a.name, text);
        get().notify(`Sent to ${a.name}; it checks with ${lead} before acting. ⌥Enter skips that.`);
      } else if (a) get().logHuman(a.name, text);
      return true;
    } catch (e) {
      get().notify(`Send failed: ${e}`, "error");
      return false;
    }
  },
  async message(from, to, text) {
    try {
      await backend.sendMessage(from, to, text);
    } catch (e) {
      get().notify(`Message failed: ${e}`, "error");
    }
  },
  async connect(from, to) {
    try {
      await backend.connect(from, to);
    } catch (e) {
      get().notify(`Connect failed: ${e}`, "error");
    }
  },
  async disconnect(a, b) {
    try {
      await backend.disconnect(a, b);
      if (get().loops[pairKey(a, b)]) await backend.setLoop(a, b, false);
    } catch (e) {
      get().notify(`Disconnect failed: ${e}`, "error");
    }
  },
  async setLoop(a, b, on, max) {
    try {
      await backend.setLoop(a, b, on, max);
      get().notify(on ? `Loop on: ${a} ↔ ${b}` : `Loop off: ${a} ↔ ${b}`);
    } catch (e) {
      get().notify(`Loop failed: ${e}`, "error");
    }
  },
  async resetLoop(a, b) {
    try {
      await backend.resetLoop(a, b);
      get().notify(`Loop ${a} ↔ ${b} resumed`);
    } catch (e) {
      get().notify(`Reset failed: ${e}`, "error");
    }
  },
  async startLoop(a, b, goal, max) {
    try {
      await backend.startLoop(a, b, goal, max);
      get().logHuman(a, goal);
      get().notify(`Loop started: ${a} ↔ ${b}`);
      set({ dialog: "none", loopPair: null, selectedEdge: null });
    } catch (e) {
      get().notify(`Could not start the loop: ${e}`, "error");
      throw e;
    }
  },
  openLoopDialog: (a, b) => set({ dialog: "loop", loopPair: [a, b], menu: null }),
  toggleBoard: () => set({ boardOpen: !get().boardOpen }),
  toggleNotes: () => set({ notesOpen: !get().notesOpen }),
  openFanout: (targets) => set({ dialog: "fanout", fanoutTargets: targets, menu: null }),
  async answer(id, approve) {
    const a = get().agents.find((x) => x.id === id);
    try {
      await backend.answerPrompt(id, approve);
      get().notify(`${approve ? "Approved" : "Denied"} ${a?.name ?? id}`);
    } catch (e) {
      get().notify(`Could not answer: ${e}`, "error");
    }
  },
}));

/** Other devices' agents are cards on their own Mac; here a badge, unless asked. */
const REMOTE_KEY = "hubcodes.showRemote";
function remoteShown(): boolean {
  try { return localStorage.getItem(REMOTE_KEY) === "1"; } catch { return false; }
}

export const BELL_KEY = "hubcodes.bell";
export function bellEnabled(): boolean {
  try { return localStorage.getItem(BELL_KEY) === "1"; } catch { return false; }
}
export function setBell(on: boolean) {
  try { localStorage.setItem(BELL_KEY, on ? "1" : "0"); } catch { /* private window */ }
}
/** Two short tones from the Web Audio API: no asset, works offline. */
export function chime() {
  try {
    const AC = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    const ctx = new AC();
    const play = (freq: number, at: number) => {
      const o = ctx.createOscillator();
      const g = ctx.createGain();
      o.type = "sine";
      o.frequency.value = freq;
      g.gain.setValueAtTime(0.0001, ctx.currentTime + at);
      g.gain.exponentialRampToValueAtTime(0.18, ctx.currentTime + at + 0.02);
      g.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + at + 0.28);
      o.connect(g).connect(ctx.destination);
      o.start(ctx.currentTime + at);
      o.stop(ctx.currentTime + at + 0.3);
    };
    play(880, 0);
    play(1174.66, 0.16);
    window.setTimeout(() => void ctx.close(), 800);
  } catch { /* no audio */ }
}

export const statusGlyph: Record<Agent["status"], string> = {
  idle: "○",
  working: "●",
  waiting: "◐",
  error: "✖",
  offline: "·",
};
export const statusLabel: Record<Agent["status"], string> = {
  idle: "idle",
  working: "working",
  waiting: "needs you",
  error: "error",
  offline: "offline",
};
export function edgeKey(e: Edge) {
  return `${e.from}>${e.to}`;
}
export type { Message };
