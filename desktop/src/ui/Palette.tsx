import { useEffect, useMemo, useRef, useState } from "react";
import { Input, Typography, type InputRef } from "antd";
import { SearchOutlined } from "@ant-design/icons";
import { useReactFlow } from "@xyflow/react";
import { useStore, statusGlyph } from "../store";
import { Dialog } from "./Dialog";
import { backend } from "../lib/backend";
import { askText } from "./ask";

const { Text } = Typography;

interface Cmd {
  id: string;
  label: string;
  hint?: string;
  run: () => void;
}

export function Palette() {
  const agents = useStore((s) => s.agents);
  const edges = useStore((s) => s.edges);
  const selectedId = useStore((s) => s.selectedId);
  const setDialog = useStore((s) => s.setDialog);
  const open = useStore((s) => s.open);
  const select = useStore((s) => s.select);
  const askStop = useStore((s) => s.askStop);
  const connect = useStore((s) => s.connect);
  const disconnect = useStore((s) => s.disconnect);
  const toggleActivity = useStore((s) => s.toggleActivity);
  const openLoopDialog = useStore((s) => s.openLoopDialog);
  const openFanout = useStore((s) => s.openFanout);
  const toggleBoard = useStore((s) => s.toggleBoard);
  const toggleNotes = useStore((s) => s.toggleNotes);
  const notify = useStore((s) => s.notify);
  const openSettings = useStore((s) => s.openSettings);
  const rf = useReactFlow();
  const [q, setQ] = useState("");
  const [idx, setIdx] = useState(0);
  const input = useRef<InputRef>(null);
  const sel = agents.find((a) => a.id === selectedId);

  const cmds: Cmd[] = useMemo(() => {
    const list: Cmd[] = [
      { id: "new", label: "New agent", hint: "⌘N", run: () => setDialog("new-agent") },
      { id: "fit", label: "Fit all in view", hint: "⌘0", run: () => { rf.fitView({ padding: 0.15, duration: 200, maxZoom: 1 }); setDialog("none"); } },
      { id: "zoom-in", label: "Zoom in", hint: "⌘+", run: () => { rf.zoomIn({ duration: 120 }); setDialog("none"); } },
      { id: "zoom-out", label: "Zoom out", hint: "⌘−", run: () => { rf.zoomOut({ duration: 120 }); setDialog("none"); } },
      { id: "zoom-100", label: "Zoom to 100%", run: () => { rf.zoomTo(1, { duration: 160 }); setDialog("none"); } },
      { id: "activity", label: "Show or hide activity", hint: "⌘J", run: () => { toggleActivity(); setDialog("none"); } },
      { id: "settings", label: "Settings", hint: "⌘,", run: () => setDialog("settings") },
      { id: "board", label: "Show or hide the goal board", hint: "⌘B", run: () => { toggleBoard(); setDialog("none"); } },
      { id: "notes", label: "Show or hide the shared notes card", hint: "⌘⇧N", run: () => { toggleNotes(); setDialog("none"); } },
      { id: "fanout", label: "Fan out a prompt to several agents…", run: () => openFanout([]) },
      { id: "inbox", label: "Approval inbox", hint: "⌘I", run: () => setDialog("inbox") },
      { id: "cost", label: "Cost and token usage", hint: "⌘⇧U", run: () => setDialog("cost") },
      { id: "doctor", label: "Doctor: check tmux, PATH, providers…", run: () => openSettings("doctor") },
      { id: "band", label: "New band (group cards under a title)…", hint: "⌘⇧G", run: () => { setDialog("none"); window.dispatchEvent(new CustomEvent("hubcodes:new-band")); } },
      { id: "jump", label: "Jump to the next agent that needs you", hint: "⌘⇧]", run: () => { setDialog("none"); window.dispatchEvent(new CustomEvent("hubcodes:jump-needs", { detail: 1 })); } },
      { id: "knowledge", label: "Search knowledge and memory…", hint: "⌘⇧K", run: () => setDialog("knowledge") },
      { id: "playbook", label: "Run a playbook…", hint: "plan → build → review → merge", run: () => setDialog("playbook") },
      { id: "restore", label: "Restore the saved session (agents not running)", run: () => { setDialog("none"); void backend.restoreSession(true).then((r) => notify(`Restored ${r.started.length} agents`)).catch((e) => notify(String(e), "error")); } },
      { id: "session-new", label: "New session (project)…", hint: "restarts the app into it", run: () => { setDialog("none"); void askText("New session", { placeholder: "project name, one word", hint: "A tmux session of its own: separate agents, board, layout and transcript. The app restarts into it." }).then((name) => { if (name) { notify(`Switching to ${name}…`); void backend.switchSession(name.replace(/\s+/g, "-")).catch((e) => notify(String(e), "error")); } }); } },
      { id: "team-save", label: "Save the running canvas as a team template…", run: () => { setDialog("none"); void askText("Save the running canvas as a team", { placeholder: "team name" }).then((name) => { if (name) void backend.teamSave(name).then(() => notify(`Saved team ${name}`)).catch((e) => notify(String(e), "error")); }); } },
    ];
    for (const a of agents) {
      list.push({ id: `open-${a.id}`, label: `Open ${a.name}`, hint: statusGlyph[a.status], run: () => { open(a.id); setDialog("none"); } });
    }
    if (sel) {
      for (const a of agents) {
        if (a.id === sel.id) continue;
        const linked = edges.some((e) => (e.from === sel.name && e.to === a.name) || (e.from === a.name && e.to === sel.name));
        list.push(
          linked
            ? { id: `dis-${a.id}`, label: `Disconnect ${sel.name} and ${a.name}`, run: () => { void disconnect(sel.name, a.name); setDialog("none"); } }
            : { id: `con-${a.id}`, label: `Connect ${sel.name} → ${a.name}`, run: () => { void connect(sel.name, a.name); setDialog("none"); } },
        );
        list.push({ id: `loop-${a.id}`, label: `Loop ${sel.name} ↔ ${a.name} with a goal…`, run: () => openLoopDialog(sel.name, a.name) });
      }
      list.push({ id: `kill-${sel.id}`, label: `Remove ${sel.name}`, hint: "stops it, closes its pane", run: () => { setDialog("none"); askStop(sel.id); } });
    }
    for (const a of agents) {
      list.push({ id: `sel-${a.id}`, label: `Select ${a.name}`, run: () => { select(a.id); setDialog("none"); } });
    }
    return list;
  }, [agents, edges, sel, rf, setDialog, open, select, askStop, connect, disconnect, toggleActivity, openLoopDialog, openFanout, toggleBoard, toggleNotes, notify, openSettings]);

  const shown = useMemo(() => {
    const t = q.trim().toLowerCase();
    return t ? cmds.filter((c) => c.label.toLowerCase().includes(t)) : cmds;
  }, [cmds, q]);

  useEffect(() => setIdx(0), [q]);
  useEffect(() => input.current?.focus(), []);

  return (
    <Dialog title="Commands" onClose={() => setDialog("none")} width={540}>
      <Input
        ref={input}
        className="palette-input"
        prefix={<SearchOutlined />}
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder="Type a command or an agent name"
        allowClear
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") { e.preventDefault(); setIdx((i) => Math.min(i + 1, shown.length - 1)); }
          if (e.key === "ArrowUp") { e.preventDefault(); setIdx((i) => Math.max(i - 1, 0)); }
          if (e.key === "Enter" && shown[idx]) { e.preventDefault(); shown[idx].run(); }
        }}
      />
      <ul className="palette-list" role="listbox">
        {shown.map((c, i) => (
          <li key={c.id}>
            <button role="option" aria-selected={i === idx} className={`palette-item${i === idx ? " is-on" : ""}`} onMouseEnter={() => setIdx(i)} onClick={c.run}>
              <span>{c.label}</span>
              {c.hint && <Text type="secondary">{c.hint}</Text>}
            </button>
          </li>
        ))}
        {shown.length === 0 && <li className="palette-empty"><Text type="secondary">Nothing matches “{q}”.</Text></li>}
      </ul>
    </Dialog>
  );
}
