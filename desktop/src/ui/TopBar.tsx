import { useState } from "react";
import { Dropdown, Tooltip, Typography, type MenuProps } from "antd";
import { BellOutlined, PlusOutlined, SearchOutlined, SettingOutlined, SwapOutlined, SyncOutlined, WifiOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { backend } from "../lib/backend";
import { askText } from "./ask";
import type { SessionEntry } from "../lib/types";

const { Text } = Typography;

export function TopBar() {
  const session = useStore((s) => s.session);
  const agents = useStore((s) => s.agents);
  const setDialog = useStore((s) => s.setDialog);
  const serving = useStore((s) => s.serving);
  const loops = useStore((s) => s.loops);
  const peers = useStore((s) => s.peers);
  const tasks = useStore((s) => s.tasks);
  const boardOpen = useStore((s) => s.boardOpen);
  const activityOpen = useStore((s) => s.activityOpen);
  const pendingCount = useStore((s) => s.pending.length);
  const needs = agents.filter((a) => a.status === "waiting").length + pendingCount;
  const running = Object.values(loops).filter((l) => !l.paused && !l.done).length;
  const peersDown = peers.filter((p) => !p.ok).length;
  const openTasks = tasks.filter((t) => t.status !== "done").length;
  const view = boardOpen ? "board" : activityOpen ? "activity" : "canvas";
  const show = (v: string) => {
    const s = useStore.getState();
    if (v === "cost") return setDialog("cost");
    if (v === "rooms") return setDialog("rooms");
    if ((v === "board") !== s.boardOpen) s.toggleBoard();
    if ((v === "activity") !== s.activityOpen) s.toggleActivity();
  };
  return (
    <header className="topbar" data-tauri-drag-region>
      <div className="tb-pill tb-place">
        <Tooltip title={peers.length ? peers.map((p) => `${p.name}: ${p.ok ? `${p.agents} agents` : p.error}`).join("\n") : "Devices: Settings → Devices"}>
          <span className={`tb-device${peersDown ? " is-down" : ""}`}>
            this device{peers.length > 0 && <span className="tb-device-more">+{peers.length}</span>}
          </span>
        </Tooltip>
        <SessionSwitcher current={session?.session ?? ""} />
      </div>

      <nav className="tb-pill tb-views" aria-label="Views">
        {[
          { key: "canvas", label: "Canvas" },
          { key: "board", label: "Board", count: openTasks },
          { key: "activity", label: "Activity" },
          { key: "cost", label: "Cost" },
          { key: "rooms", label: "Rooms" },
        ].map((v) => (
          <button key={v.key} type="button" className={`tb-view${view === v.key ? " is-on" : ""}`} aria-pressed={view === v.key} onClick={() => show(v.key)}>
            {v.label}
            {!!v.count && <span className="tb-count">{v.count}</span>}
          </button>
        ))}
      </nav>

      <button type="button" className="tb-pill tb-search" onClick={() => setDialog("palette")}>
        <SearchOutlined />
        <span>Search agents, tasks and actions</span>
        <kbd>⌘K</kbd>
      </button>

      <div className="tb-pill tb-actions">
        <button type="button" className="tb-act tb-new" onClick={() => setDialog("new-agent")}><PlusOutlined /> New agent</button>
        {running > 0 && <Tooltip title={`${running} loop${running === 1 ? "" : "s"} running`}><span className="tb-act tb-stat"><SyncOutlined spin /> {running}</span></Tooltip>}
        {serving && <Tooltip title={`Phones and teammates: http://${serving}/`}><span className="tb-act tb-stat is-live"><WifiOutlined /></span></Tooltip>}
        <Tooltip title={needs ? `${needs} waiting on you · ⌘I` : "Inbox · ⌘I"}>
          <button type="button" className={`tb-act${needs ? " is-needs" : ""}`} aria-label="Inbox" onClick={() => setDialog("inbox")}>
            <BellOutlined />{needs > 0 && <span className="tb-needs">{needs}</span>}
          </button>
        </Tooltip>
        <Tooltip title="Settings · ⌘,"><button type="button" className="tb-act" aria-label="Settings" onClick={() => setDialog("settings")}><SettingOutlined /></button></Tooltip>
      </div>
    </header>
  );
}

/** The tmux session this window shows: pick another project or start one. Switching restarts the app into it. */
function SessionSwitcher({ current }: { current: string }) {
  const notify = useStore((s) => s.notify);
  const [list, setList] = useState<SessionEntry[]>([]);
  const [open, setOpen] = useState(false);
  const load = () => backend.sessions().then(setList).catch(() => setList([]));
  const go = (name: string) => {
    notify(`Switching to ${name}…`);
    backend.switchSession(name).catch((e) => notify(String(e), "error"));
  };
  const create = () => {
    setOpen(false);
    void askText("New session", { placeholder: "project name, one word", hint: "A tmux session of its own: separate agents, board, layout and transcript. The app restarts into it." }).then((name) => {
      if (name) go(name.replace(/\s+/g, "-"));
    });
  };
  const items: MenuProps["items"] = [
    ...list.map((s) => ({
      key: s.name,
      label: (
        <span className="session-item">
          <span className="mono">{s.name}</span>
          <Text type="secondary">{s.current ? "this window" : s.running ? `${s.agents} agent${s.agents === 1 ? "" : "s"}` : s.saved ? "saved, not running" : ""}</Text>
        </span>
      ),
      disabled: s.current,
      onClick: () => go(s.name),
    })),
    { type: "divider" as const },
    { key: "new", icon: <PlusOutlined />, label: "New session…", onClick: create },
  ];
  return (
    <Dropdown menu={{ items }} trigger={["click"]} open={open} onOpenChange={(o) => { setOpen(o); if (o) void load(); }}>
      <button type="button" className="session-pill" title="Switch session (project)">
        <span className="session-dot" />
        <span className="mono">{current || "…"}</span>
        <SwapOutlined rotate={90} className="session-caret" />
      </button>
    </Dropdown>
  );
}
