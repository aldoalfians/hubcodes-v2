import { useMemo, useState } from "react";
import { Button, Popover, Tooltip, Typography } from "antd";
import { useReactFlow } from "@xyflow/react";
import { statusLabel, useStore } from "../store";
import { color256 } from "../lib/colors";
import { NODE_H, NODE_W } from "../canvas/Canvas";
import type { Agent, PeerInfo, Status } from "../lib/types";

const { Text } = Typography;

/** Status groups in the order they matter: what waits on you first. */
const GROUPS: { status: Status; label: string }[] = [
  { status: "waiting", label: "Waiting on you" },
  { status: "error", label: "Error" },
  { status: "working", label: "Working" },
  { status: "idle", label: "Idle" },
  { status: "offline", label: "Offline" },
];

/** Agents, grouped by status (or by device), one row each with a status dot. */
export function Rail() {
  const allAgents = useStore((s) => s.agents);
  const peers = useStore((s) => s.peers);
  const showRemote = useStore((s) => s.showRemote);
  const setShowRemote = useStore((s) => s.setShowRemote);
  const agents = useMemo(() => (showRemote ? allAgents : allAgents.filter((a) => !a.peer)), [allAgents, showRemote]);
  const providers = useStore((s) => s.providers);
  const selectedId = useStore((s) => s.selectedId);
  const positions = useStore((s) => s.positions);
  const select = useStore((s) => s.select);
  const openMenu = useStore((s) => s.openMenu);
  const rf = useReactFlow();
  const [by, setBy] = useState<"status" | "device">("status");

  const go = (id: string, name: string) => {
    select(id);
    const p = positions[name];
    const node = rf.getNode(id);
    const x = node?.position.x ?? p?.[0] ?? 0;
    const y = node?.position.y ?? p?.[1] ?? 0;
    rf.setCenter(x + NODE_W / 2, y + NODE_H / 2, { zoom: Math.max(rf.getZoom(), 0.8), duration: 240 });
  };

  const groups: { key: string; label: string; items: Agent[] }[] =
    by === "status" || !showRemote
      ? GROUPS.map((g) => ({ key: g.status, label: g.label, items: agents.filter((a) => a.status === g.status) })).filter((g) => g.items.length)
      : Array.from(new Set(agents.map((a) => a.peer ?? ""))).map((d) => ({ key: d || "local", label: d || "This device", items: agents.filter((a) => (a.peer ?? "") === d) }));

  const row = (a: Agent) => {
    const i = agents.indexOf(a);
    const p = providers.find((x) => x.name === a.provider);
    const name = a.peer ? a.name.slice(a.peer.length + 1) : a.name;
    const sub = [p?.name, a.role, by === "status" && a.peer ? `on ${a.peer}` : null].filter(Boolean).join(" · ");
    return (
      <li key={a.id}>
        <Tooltip title={`${statusLabel[a.status]} · ⌘${i + 1}${a.lastLine ? `\n${a.lastLine}` : ""}`} placement="right" mouseEnterDelay={0.6}>
          <button
            className={`rail-item st-${a.status}${a.id === selectedId ? " is-selected" : ""}${a.dead ? " is-dead" : ""}`}
            onClick={() => go(a.id, a.name)}
            onContextMenu={(e) => { e.preventDefault(); openMenu(e.clientX, e.clientY, a.id); }}
          >
            <span className="rail-glyph" style={{ color: color256(p?.color) }} aria-hidden>{p?.glyph || "◆"}</span>
            <span className="rail-text">
              <span className="rail-name">{name}</span>
              {sub && <span className="rail-sub">{sub}</span>}
            </span>
            <span className={`rail-dot d-${a.status}`} aria-label={statusLabel[a.status]} />
          </button>
        </Tooltip>
      </li>
    );
  };

  return (
    <nav className="rail" aria-label="Agents">
      {peers.length > 0 && (
        <div className="rail-devices" aria-label="Other devices">
          {peers.map((d) => <DeviceBadge key={d.name} device={d} agents={allAgents.filter((a) => a.peer === d.name)} shown={showRemote} onShow={setShowRemote} />)}
        </div>
      )}
      {showRemote && (
        <div className="rail-seg" role="tablist" aria-label="Group agents">
          <button type="button" role="tab" aria-selected={by === "status"} className={by === "status" ? "is-on" : ""} onClick={() => setBy("status")}>By status</button>
          <button type="button" role="tab" aria-selected={by === "device"} className={by === "device" ? "is-on" : ""} onClick={() => setBy("device")}>By device</button>
        </div>
      )}
      <div className="rail-scroll">
        {groups.map((g) => (
          <section key={g.key} className={`rail-group g-${g.key}`}>
            <h4>{g.label}<span>{g.items.length}</span></h4>
            <ul>{g.items.map(row)}</ul>
          </section>
        ))}
        {agents.length === 0 && <Text type="secondary" className="rail-empty">No agents yet. Start one with New agent.</Text>}
      </div>
    </nav>
  );
}

/**
 * Another device, as one badge: reachable or not, how many agents, and who
 * needs you there. Its agents are cards on that Mac; the popover lists
 * them, and "Show on this canvas" brings them here when wanted.
 */
function DeviceBadge({ device, agents, shown, onShow }: { device: PeerInfo; agents: Agent[]; shown: boolean; onShow: (on: boolean) => void }) {
  const waiting = agents.filter((a) => a.status === "waiting").length;
  const working = agents.filter((a) => a.status === "working").length;
  const state = !device.ok ? "away" : waiting ? "waiting" : working ? "working" : "idle";
  const summary = !device.ok ? "unreachable" : `${device.agents} agent${device.agents === 1 ? "" : "s"}${waiting ? ` · ${waiting} waiting` : working ? ` · ${working} working` : ""}`;
  const list = (
    <div className="device-pop">
      {!device.ok && <Text type="secondary" className="device-pop-error">{device.error ?? "Not reachable right now."}</Text>}
      {agents.length > 0 && (
        <ul>
          {agents.map((a) => (
            <li key={a.id}>
              <span className={`rail-dot d-${a.status}`} aria-hidden />
              <span className="device-pop-name">{a.name.slice(device.name.length + 1)}</span>
              <Text type="secondary">{[a.role, statusLabel[a.status]].filter(Boolean).join(" · ")}</Text>
            </li>
          ))}
        </ul>
      )}
      <Text type="secondary" className="device-pop-hint">Message them as {device.name}/name. Their terminals live on {device.name}.</Text>
      <Button size="small" block onClick={() => onShow(!shown)}>{shown ? "Hide other devices from this canvas" : "Show on this canvas"}</Button>
    </div>
  );
  return (
    <Popover content={list} title={`${device.name} · ${device.host}`} trigger="click" placement="rightTop">
      <button type="button" className={`device-badge s-${state}`} aria-label={`${device.name}: ${summary}`}>
        <span className={`rail-dot d-${state === "away" ? "offline" : state}`} aria-hidden />
        <span className="device-badge-name">{device.name}</span>
        <span className="device-badge-sub">{summary}</span>
      </button>
    </Popover>
  );
}
