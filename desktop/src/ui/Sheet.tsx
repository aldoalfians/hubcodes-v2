import { useEffect, useState } from "react";
import { Badge, Button, Segmented, Space, Tag, Typography } from "antd";
import { ArrowLeftOutlined, DeleteOutlined, SwapOutlined } from "@ant-design/icons";
import { Changes } from "./Changes";
import { SwitchDialog } from "./SwitchDialog";
import { isRemoteUi } from "../lib/backend";
import { modelMatches } from "../lib/platform";
import { statusLabel, useStore } from "../store";
import { PaneTerminal } from "../terminal/PaneTerminal";
import { StartingVeil } from "../canvas/AgentNode";
import { color256 } from "../lib/colors";
import { routeLead } from "../lib/route";
import type { Status } from "../lib/types";

const { Text } = Typography;
const BADGE: Record<Status, "success" | "warning" | "error" | "default" | "processing"> = { working: "processing", waiting: "warning", error: "error", idle: "default", offline: "default" };

/** The selected agent, full size, same terminal instance as its node. */
export function Sheet() {
  const openId = useStore((s) => s.openId);
  const openPane = useStore((s) => s.openPane);
  const agent = useStore((s) => s.agents.find((a) => a.id === openId));
  const providers = useStore((s) => s.providers);
  const open = useStore((s) => s.open);
  const askStop = useStore((s) => s.askStop);
  const [tab, setTab] = useState<"terminal" | "changes">("terminal");
  const [switching, setSwitching] = useState(false);
  const agents = useStore((s) => s.agents);
  const edges = useStore((s) => s.edges);
  const routeOn = useStore((s) => s.routeThroughOrchestrator);
  const lead = agent ? routeLead(agents, edges, agent, routeOn) : null;
  useEffect(() => {
    if (!agent || switching) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        open(null);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [agent, open, switching]);
  if (!agent) return null;
  const p = providers.find((x) => x.name === agent.provider);
  return (
    <section className="sheet" aria-label={`${agent.name} terminal`}>
      <header className="sheet-head">
        <Button type="text" size="small" icon={<ArrowLeftOutlined />} onClick={() => open(null)} title="Back to canvas · Esc">Back</Button>
        <Badge status={BADGE[agent.status]} />
        <Text strong className="mono node-name">{agent.name}</Text>
        {agent.peer && <Tag variant="filled">on {agent.peer} · ssh</Tag>}
        {agent.role && <Tag variant="filled" color="processing">{agent.role}</Tag>}
        <Text type="secondary"><span style={{ color: color256(p?.color) }}>{p?.glyph}</span> {agent.provider}{agent.model ? <span className="mono"> · {agent.model}</span> : null}</Text>
        {agent.model && agent.modelSeen && !modelMatches(agent.model, agent.modelSeen) && <Tag variant="filled" color="error" title={`Picked ${agent.model}, but it answers with ${agent.modelSeen}. Use Switch to restart it on the picked model.`}>runs {agent.modelSeen}</Tag>}
        <Text type="secondary" className="sheet-cwd mono">{agent.cwd}</Text>
        <Text type="secondary" className={`node-status st-${agent.status}`}>{statusLabel[agent.status]}</Text>
        {!agent.peer && (
          <Segmented size="small" value={tab} onChange={(v) => setTab(v as "terminal" | "changes")} options={[{ value: "terminal", label: "Terminal" }, { value: "changes", label: agent.changes && agent.changes.files > 0 ? `Changes · ${agent.changes.files}` : "Changes" }]} />
        )}
        <Space size={4} style={{ marginLeft: "auto" }}>
          {!agent.peer && !isRemoteUi && <Button type="text" size="small" icon={<SwapOutlined />} onClick={() => setSwitching(true)} title="Run this agent on another provider or model; its role, folder and links stay">Switch</Button>}
          {!agent.peer && <Button type="text" size="small" danger icon={<DeleteOutlined />} onClick={() => askStop(agent.id)} title="Remove this agent: stops it and closes its pane">Remove</Button>}
        </Space>
      </header>
      {tab === "changes" && !agent.peer ? <Changes agent={agent} /> : isRemoteUi ? <pre className="pane-box remote-preview sheet-preview">{agent.preview.join("\n") || "No screen yet."}</pre> : openPane ? <PaneTerminal id={openPane} autoFocus /> : <div className="pane-box muted">Connecting…</div>}
      {!isRemoteUi && tab !== "changes" && <StartingVeil agent={agent} className="sheet-starting" />}
      {switching && <SwitchDialog agent={agent} onClose={() => setSwitching(false)} />}
      <footer className="sheet-foot"><Text type="secondary">{isRemoteUi ? "Preview from the machine that runs the agents, refreshed every few seconds. Use the composer to send; the live terminal is in the desktop app. Esc returns to the canvas." : agent.peer ? `Attached over SSH to ${agent.peer}. Typing goes to the agent there. Esc returns to the canvas.` : lead ? `Typing goes straight to ${agent.name}, which checks with ${lead} before acting on it. Esc returns to the canvas.` : "Typing goes straight to the agent. Esc returns to the canvas."}</Text></footer>
    </section>
  );
}
