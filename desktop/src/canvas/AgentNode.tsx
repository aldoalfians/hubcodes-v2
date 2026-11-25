import { createPortal } from "react-dom";
import { memo, useEffect, useMemo, useState, type ChangeEvent, type KeyboardEvent } from "react";
import { Handle, NodeResizer, NodeToolbar, Position, useViewport, type NodeProps, type Node } from "@xyflow/react";
import { Badge, Button, Input, Select, Space, Spin, Tag, Tooltip, Typography } from "antd";
import { DeleteOutlined, ExportOutlined, LinkOutlined, LockOutlined, SwapOutlined, UnlockOutlined } from "@ant-design/icons";
import { SwitchDialog } from "../ui/SwitchDialog";
import { resizeEnd } from "./Canvas";
import { modelMatches } from "../lib/platform";
import type { Agent } from "../lib/types";
import { statusGlyph, statusLabel, useStore } from "../store";
import type { Status } from "../lib/types";

const BADGE: Record<Status, "success" | "warning" | "error" | "default" | "processing"> = { working: "processing", waiting: "warning", error: "error", idle: "default", offline: "default" };
import { backend, isRemoteUi } from "../lib/backend";
import type { Prompt } from "../lib/types";
import { color256 } from "../lib/colors";
import { routeLead } from "../lib/route";
import { PaneTerminal } from "../terminal/PaneTerminal";


export type AgentNodeType = Node<{ agent: Agent; links: number; slot: number }, "agent">;

/** `@name` token the caret is in, if any: `@rev|` → { q: "rev", start: 0 }. */
function mentionAtCaret(text: string, caret: number): { q: string; start: number } | null {
  const before = text.slice(0, caret);
  const m = /(^|\s)@([\w-]*)$/.exec(before);
  return m ? { q: m[2], start: before.length - m[2].length - 1 } : null;
}

/** Leading `@name` tokens that match live agents become recipients. */
export function splitMentions(text: string, names: string[]): { to: string[]; body: string } {
  const to: string[] = [];
  let rest = text.trim();
  for (;;) {
    const m = /^@([\w-]+)\s*/.exec(rest);
    if (!m || !names.includes(m[1])) break;
    if (!to.includes(m[1])) to.push(m[1]);
    rest = rest.slice(m[0].length);
  }
  return { to, body: rest.trim() };
}

/** Saved prompts, loaded once per app run. */
let promptCache: Prompt[] | null = null;
function usePrompts() {
  const [list, setList] = useState<Prompt[]>(promptCache ?? []);
  useEffect(() => {
    if (promptCache) return;
    backend.promptList().then((p) => { promptCache = p; setList(p); }).catch(() => {});
  }, []);
  return list;
}

function Composer({ agent }: { agent: Agent }) {
  const [text, setText] = useState("");
  const [caret, setCaret] = useState(0);
  const [idx, setIdx] = useState(0);
  const prompts = usePrompts();
  const [preview, setPreview] = useState<string | null>(null);
  const agents = useStore((s) => s.agents);
  const instruct = useStore((s) => s.instruct);
  const message = useStore((s) => s.message);
  const notify = useStore((s) => s.notify);
  const open = useStore((s) => s.open);

  const others = useMemo(() => agents.filter((a) => a.id !== agent.id), [agents, agent.id]);
  const routeOn = useStore((s) => s.routeThroughOrchestrator);
  const edges = useStore((s) => s.edges);
  const mention = mentionAtCaret(text, caret);
  const candidates = useMemo(
    () => (mention ? others.filter((a) => a.name.toLowerCase().startsWith(mention.q.toLowerCase())) : []),
    [mention, others],
  );
  const menuOpen = mention !== null && candidates.length > 0;
  // `/name` at the start: the saved prompts menu.
  const slash = /^\/(\w*)$/.exec(text);
  const slashItems = slash ? prompts.filter((p) => p.name.startsWith(slash[1].toLowerCase())) : [];
  const slashOpen = slash !== null && slashItems.length > 0;
  useEffect(() => {
    const m = /^\/(\w+)(\s|$)/.exec(text);
    if (!m || !prompts.some((p) => p.name === m[1])) { setPreview(null); return; }
    let alive = true;
    const t = window.setTimeout(() => backend.promptPreview(agent.id, text).then((v) => alive && setPreview(v)).catch(() => {}), 150);
    return () => { alive = false; window.clearTimeout(t); };
  }, [text, prompts, agent.id]);
  const names = useMemo(() => agents.map((a) => a.name), [agents]);
  const { to } = splitMentions(text, names);

  const pick = (name: string) => {
    if (!mention) return;
    const next = `${text.slice(0, mention.start)}@${name} ${text.slice(caret)}`;
    setText(next);
    setCaret(mention.start + name.length + 2);
    setIdx(0);
  };

  const send = (direct = false) => {
    const parsed = splitMentions(text, names);
    if (parsed.to.length) {
      if (!parsed.body) {
        notify(`Write what to tell ${parsed.to.map((n) => `@${n}`).join(" ")}`);
        return;
      }
      for (const name of parsed.to) void message(agent.name, name, parsed.body);
    } else {
      void instruct(agent.id, text.trim(), direct);
    }
    setText("");
    setCaret(0);
  };

  // Paths dropped on this card (Tauri drag-drop) land in the composer.
  useEffect(() => {
    const onDrop = (e: Event) => {
      const d = (e as CustomEvent<{ agentId: string; paths: string[] }>).detail;
      if (d.agentId !== agent.id) return;
      const quoted = d.paths.map((p) => (/[\s"']/.test(p) ? `"${p.replace(/"/g, '\\"')}"` : p)).join(" ");
      setText((t) => (t.trim() ? `${t.replace(/\s+$/, "")} ${quoted}` : quoted));
    };
    window.addEventListener("hubcodes:drop-paths", onDrop);
    return () => window.removeEventListener("hubcodes:drop-paths", onDrop);
  }, [agent.id]);
  const onChange = (e: ChangeEvent<HTMLTextAreaElement>) => {
    setText(e.target.value);
    setCaret(e.target.selectionStart ?? e.target.value.length);
    setIdx(0);
  };
  const pickSlash = (p: Prompt) => {
    setText(`/${p.name} `);
    setCaret(p.name.length + 2);
    setIdx(0);
  };
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.target as HTMLTextAreaElement;
    if (slashOpen) {
      if (e.key === "ArrowDown") { e.preventDefault(); setIdx((i) => (i + 1) % slashItems.length); e.stopPropagation(); return; }
      if (e.key === "ArrowUp") { e.preventDefault(); setIdx((i) => (i - 1 + slashItems.length) % slashItems.length); e.stopPropagation(); return; }
      if (e.key === "Tab" || (e.key === "Enter" && !text.includes(" "))) { e.preventDefault(); pickSlash(slashItems[idx]); e.stopPropagation(); return; }
    }
    if (menuOpen) {
      if (e.key === "ArrowDown") { e.preventDefault(); setIdx((i) => (i + 1) % candidates.length); e.stopPropagation(); return; }
      if (e.key === "ArrowUp") { e.preventDefault(); setIdx((i) => (i - 1 + candidates.length) % candidates.length); e.stopPropagation(); return; }
      if (e.key === "Tab" || e.key === "Enter") { e.preventDefault(); pick(candidates[idx].name); e.stopPropagation(); return; }
      if (e.key === "Escape") { e.preventDefault(); setCaret(0); e.stopPropagation(); return; }
    }
    if (e.key === "Enter" && e.shiftKey) {
      // Shift+Enter: a new line; Enter alone sends.
      e.stopPropagation();
      return;
    }
    if (e.key === "Enter" && text.trim()) {
      e.preventDefault();
      // ⌥Enter: straight to this agent, not through the orchestrator.
      send(e.altKey);
    } else if (e.key === "Enter" && !text.trim()) {
      e.preventDefault();
      open(agent.id);
    } else if (e.key === "Escape") {
      setText("");
      el.blur();
    }
    e.stopPropagation();
  };

  const lead = routeLead(agents, edges, agent, routeOn);
  const placeholder = lead
    ? `Tell ${agent.name} something · it checks with ${lead} first · ⌥↩ skips that · / prompts`
    : others.length ? `Tell ${agent.name} something · @name sends to another agent · / prompts` : `Tell ${agent.name} something · / prompts`;
  return (
    <div className="composer nodrag">
      {slashOpen && (
        <ul className="mention-menu" role="listbox" aria-label="Saved prompts">
          <li className="mention-hint">Saved prompts · Settings → Prompts</li>
          {slashItems.map((p, i) => (
            <li key={p.name}>
              <button role="option" aria-selected={i === idx} className={`mention-item${i === idx ? " is-on" : ""}`} onMouseDown={(e) => { e.preventDefault(); pickSlash(p); }} onMouseEnter={() => setIdx(i)}>
                <span>/{p.name}</span>
                <span className="muted">{p.description}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {preview && !slashOpen && (
        <div className="prompt-preview" title="What will be sent">
          <div className="prompt-preview-text">{preview.length > 700 ? preview.slice(0, 700) + "…" : preview}</div>
        </div>
      )}
      {menuOpen && (
        <ul className="mention-menu" role="listbox" aria-label="Agents">
          <li className="mention-hint">Send from {agent.name} to…</li>
          {candidates.map((a, i) => (
            <li key={a.id}>
              <button
                role="option"
                aria-selected={i === idx}
                className={`mention-item${i === idx ? " is-on" : ""}`}
                onMouseDown={(e) => { e.preventDefault(); pick(a.name); }}
                onMouseEnter={() => setIdx(i)}
              >
                <span className={`glyph st-${a.status}`}>{statusGlyph[a.status]}</span>
                <span>{a.name}</span>
                <span className="muted">{a.provider}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="composer-row">
        <span className="composer-mark" title={to.length ? `Goes to ${to.join(", ")}` : undefined}>{to.length ? "→" : "›"}</span>
        <Input.TextArea
          variant="borderless"
          autoSize={{ minRows: 1, maxRows: 6 }}
          value={text}
          onChange={onChange}
          onKeyDown={onKey}
          onKeyUp={(e) => setCaret((e.target as HTMLTextAreaElement).selectionStart ?? 0)}
          onClick={(e) => setCaret((e.target as HTMLTextAreaElement).selectionStart ?? 0)}
          placeholder={placeholder}
          spellCheck={false}
          aria-label={`Instruct ${agent.name}`}
          aria-autocomplete="list"
          aria-expanded={menuOpen}
        />
      </div>
    </div>
  );
}

/** Role label on the node head; click to edit, Enter saves, empty clears. */
function RoleChip({ agent }: { agent: Agent }) {
  const setRole = useStore((s) => s.setRole);
  const roles = useStore((s) => s.session?.roles ?? []);
  const agents = useStore((s) => s.agents);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(agent.role ?? "");
  if (agent.peer) {
    return agent.role ? <Tag variant="filled" color="processing" title="Set on its own device">{agent.role}</Tag> : null;
  }
  if (!editing) {
    return (
      <Tag
        variant="filled"
        color={agent.role ? "processing" : undefined}
        className={`role${agent.role ? "" : " is-empty"} nodrag`}
        onClick={(e) => { e.stopPropagation(); setDraft(agent.role ?? ""); setEditing(true); }}
        onDoubleClick={(e) => e.stopPropagation()}
        title={agent.role ? "Role · click to change" : "Give this agent a role"}
      >
        {agent.role ?? "+ role"}
      </Tag>
    );
  }
  const done = (save: boolean, value = draft) => {
    setEditing(false);
    if (save && value.trim() !== (agent.role ?? "")) void setRole(agent.id, value.trim());
  };
  // The session's roles plus what other agents already use; a new name can still be typed.
  const options = Array.from(new Set([...roles, ...agents.map((a) => a.role ?? "").filter(Boolean), ...(agent.role ? [agent.role] : [])])).map((r) => ({ value: r, label: r }));
  const typed = draft.trim();
  const list = typed && !options.some((o) => o.value.toLowerCase() === typed.toLowerCase()) ? [{ value: typed, label: `Use “${typed}”` }, ...options] : options;
  return (
    <Select
      className="role-input nodrag"
      size="small"
      autoFocus
      defaultOpen
      showSearch
      allowClear
      value={agent.role || undefined}
      options={list}
      filterOption={(input, o) => String(o?.value ?? "").toLowerCase().includes(input.toLowerCase())}
      onSearch={setDraft}
      onChange={(v) => done(true, v ? String(v) : "")}
      onBlur={() => done(false)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") done(false);
      }}
      placeholder="role"
      aria-label="Role"
      popupMatchSelectWidth={200}
    />
  );
}

/** Remote agents have no live PTY here: the last screen lines, refreshed by the peer poll. */
function RemotePreview({ agent }: { agent: Agent }) {
  return (
    <pre className="pane-box remote-preview nodrag nowheel" aria-label={`${agent.name} screen`}>
      {agent.preview.length ? agent.preview.join("\n") : "waiting for the first screen from " + (agent.peer ?? "the device") + "…"}
    </pre>
  );
}

/** Below this zoom the terminal is unreadable: show name and status big instead. */
export const COMPACT_ZOOM = 0.55;
export const MIN_W = 320;
export const MIN_H = 220;

/** Ports plus a card-sized drop target, so a dragged link lands anywhere on the card. */
function Ports() {
  return (
    <>
      <Handle type="source" position={Position.Left} id="l" className="port" />
      <Handle type="source" position={Position.Right} id="r" className="port" />
      <Handle type="source" position={Position.Top} id="t" className="port" />
      <Handle type="source" position={Position.Bottom} id="b" className="port" />
      <Handle type="target" position={Position.Left} id="card" className="drop-zone" isConnectableStart={false} />
    </>
  );
}

/**
 * Over the terminal while an agent's tool starts (after Start agent or
 * Switch): a spinner instead of a blank or half-drawn screen. It lifts once
 * the tool has drawn something, and after 15 seconds at most.
 */
export function StartingVeil({ agent, className = "" }: { agent: Agent; className?: string }) {
  const mark = useStore((s) => s.starting[agent.name]);
  const since = mark?.at;
  const [, tick] = useState(0);
  useEffect(() => {
    if (!since) return;
    const t = window.setInterval(() => tick((n) => n + 1), 500);
    return () => window.clearInterval(t);
  }, [since]);
  if (!since) return null;
  const age = Date.now() - since;
  // The old card stays up while a switch replaces its window: only the new pane counts as drawn.
  const replaced = mark.from === agent.id;
  const drawn = !replaced && age > 1500 && agent.lastLine.trim() !== "" && agent.quietSecs < 60;
  if ((!replaced && age > 15000) || drawn) return null;
  return (
    <div className={`node-starting ${className}`} role="status" aria-live="polite">
      <Spin size="small" />
      <span>{replaced ? `Switching to ${mark.to || agent.provider}…` : `Starting ${agent.provider}${agent.model ? ` · ${agent.model}` : ""}…`}</span>
    </div>
  );
}

/** Small lock by the name when guests of the shared canvas see only the status. */
function PrivateMark({ agent }: { agent: Agent }) {
  if (!agent.private) return null;
  return (
    <Tooltip title="Private: guests of the shared canvas see its status, not its screen">
      <LockOutlined className="node-lock" aria-label="private" style={{ color: "var(--muted)", fontSize: 11 }} />
    </Tooltip>
  );
}

/** Resize handles and the floating toolbar, only for the selected card. */
function Chrome({ agent, selected }: { agent: Agent; selected: boolean }) {
  const open = useStore((s) => s.open);
  const startLink = useStore((s) => s.startLink);
  const askStop = useStore((s) => s.askStop);
  const linkingFrom = useStore((s) => s.linkingFrom);
  const notify = useStore((s) => s.notify);
  const [switching, setSwitching] = useState(false);
  const togglePrivate = () =>
    backend.setPrivate(agent.id, !agent.private).catch((e) => notify(`Could not change privacy: ${e}`, "error"));
  return (
    <>
      <NodeResizer
        isVisible={selected}
        minWidth={MIN_W}
        minHeight={MIN_H}
        maxWidth={1400}
        maxHeight={1000}
        lineClassName="resize-line"
        handleClassName="resize-handle"
        onResizeEnd={(_, p) => resizeEnd.current?.(agent.name, p.x, p.y, p.width, p.height)}
      />
      <NodeToolbar isVisible={selected} position={Position.Top} offset={8} className="node-tools">
        <Space.Compact size="small">
          <Tooltip title="Open full size · Enter">
            <Button icon={<ExportOutlined />} onClick={() => open(agent.id)} aria-label="Open" />
          </Tooltip>
          <Tooltip title={linkingFrom === agent.id ? "Click a target… (click again to cancel)" : "Link to another agent: click this, then the other card"}>
            <Button icon={<LinkOutlined />} type={linkingFrom === agent.id ? "primary" : "default"} onClick={() => startLink(linkingFrom === agent.id ? null : agent.id)} aria-label={linkingFrom === agent.id ? "Cancel link" : "Link to…"} />
          </Tooltip>
          {!agent.peer && !isRemoteUi && (
            <Tooltip title={agent.private ? "Private: guests see its status only. Click to show its screen again." : "Make private: hide its screen from guests of the shared canvas"}>
              <Button icon={agent.private ? <LockOutlined /> : <UnlockOutlined />} type={agent.private ? "primary" : "default"} onClick={togglePrivate} aria-label={agent.private ? "Make public" : "Private"} />
            </Tooltip>
          )}
          {!agent.peer && !isRemoteUi && (
            <Tooltip title="Switch provider or model; its role, folder and links stay">
              <Button icon={<SwapOutlined />} onClick={() => setSwitching(true)} aria-label="Switch" />
            </Tooltip>
          )}
          {!agent.peer && (
            <Tooltip title="Remove: stops it and closes its pane · ⌫">
              <Button danger icon={<DeleteOutlined />} onClick={() => askStop(agent.id)} aria-label="Remove" />
            </Tooltip>
          )}
        </Space.Compact>
      </NodeToolbar>
      {switching && createPortal(<SwitchDialog agent={agent} onClose={() => setSwitching(false)} />, document.body)}
    </>
  );
}

export const AgentNode = memo(function AgentNode({ data, selected }: NodeProps<AgentNodeType>) {
  const { agent, links, slot } = data;
  const providers = useStore((s) => s.providers);
  const open = useStore((s) => s.open);
  const { zoom } = useViewport();
  const p = providers.find((x) => x.name === agent.provider);
  const tint = color256(p?.color);
  if (zoom < COMPACT_ZOOM) {
    return (
      <div
        className={`node compact status-${agent.status}${selected ? " is-selected" : ""}`}
        style={{ ["--tint" as string]: tint }}
        onDoubleClick={() => open(agent.id)}
        title="Zoom in to see the terminal · double-click opens"
      >
        <Ports />
        <Chrome agent={agent} selected={!!selected} />
        <div className="compact-head">
          <span className={`glyph st-${agent.status}`}>{statusGlyph[agent.status]}</span>
          <span className="node-name">{agent.name}</span>
          <PrivateMark agent={agent} />
          <span className="node-provider"><span style={{ color: tint }}>{p?.glyph ?? ""}</span> {agent.provider}{agent.model ? ` · ${agent.model}` : ""}{agent.account ? ` · ${agent.account}` : ""}</span>
        </div>
        {agent.role && <div className="compact-role">{agent.role}</div>}
        <div className={`compact-status st-${agent.status}`}>{statusLabel[agent.status]}</div>
        <div className="compact-line">{agent.lastLine || shortPath(agent.cwd)}</div>
        <div className="compact-foot">{links > 0 ? `⇄ ${links}` : ""}</div>
      </div>
    );
  }
  return (
    <div
      className={`node status-${agent.status}${selected ? " is-selected" : ""}`}
      style={{ ["--tint" as string]: tint }}
      onDoubleClick={() => open(agent.id)}
    >
      <Ports />
      <Chrome agent={agent} selected={!!selected} />
      <header className="node-head">
        <Tooltip title={statusLabel[agent.status]}><Badge status={BADGE[agent.status]} className="node-badge" /></Tooltip>
        <span className="node-name">{agent.peer ? agent.name.slice(agent.peer.length + 1) : agent.name}</span>
        <PrivateMark agent={agent} />
        {agent.peer &&<Tag variant="filled" title={`Runs on ${agent.peer}; preview refreshes every few seconds over SSH`}>on {agent.peer}</Tag>}
        {agent.holder && <Tag variant="filled" color="warning" title={`${agent.holder} is typing into this agent`}>⌨ {agent.holder}</Tag>}
        <RoleChip agent={agent} />
        {agent.model && agent.modelSeen && !modelMatches(agent.model, agent.modelSeen) && (
          <Tag variant="filled" color="error" title={`Picked ${agent.model}, but it answers with ${agent.modelSeen}. Switch it (or restart it) to run the picked model.`}>runs {agent.modelSeen}</Tag>
        )}
        {agent.extra && <Tag variant="filled" color="purple" title="Added by auto team while the queue for its role was long; removed again once it is idle with nothing waiting">extra</Tag>}
        {slot <= 9 && <kbd className="slot" title={`⌘${slot} selects this agent`}>⌘{slot}</kbd>}
        <Typography.Text type="secondary" className="node-provider" title={`${agent.provider}${agent.model ? ` · model ${agent.model}` : ""}${agent.account ? ` · account ${agent.account}` : ""}`}>
          <span className="provider-glyph" style={{ color: tint }}>{p?.glyph ?? ""}</span>
          {agent.provider}
          {agent.model && <span className="node-model">{agent.model.split("/").pop()}</span>}
          {agent.account && <Tag variant="filled" className="node-account" title={`Runs under the ${agent.account} login`}>{agent.account}</Tag>}
        </Typography.Text>
      </header>
      {agent.peer || isRemoteUi ? <RemotePreview agent={agent} /> : <PaneTerminal id={agent.id} />}
      <StartingVeil agent={agent} />
      {/* A local agent is typed to in its own terminal; the box is for agents without one here. */}
      {(agent.peer || isRemoteUi) && <Composer agent={agent} />}
      <footer className="node-foot">
        <span className="node-cwd" title={agent.worktree ?? agent.cwd}>{shortPath(agent.worktree ?? agent.cwd)}</span>
        {agent.changes && (
          <span className="node-git" title={`${agent.changes.branch}: ${agent.changes.files} changed file${agent.changes.files === 1 ? "" : "s"}${agent.changes.ahead ? `, ${agent.changes.ahead} to push` : ""}`}>
            <span className="git-branch">⎇ {agent.changes.branch}</span>
            {agent.changes.files > 0 && <span className="git-counts"><span className="ins">+{agent.changes.insertions}</span> <span className="del">−{agent.changes.deletions}</span></span>}
          </span>
        )}
        {agent.test && <Tag variant="filled" color={agent.test[0] ? "success" : "error"} className="node-test" title={`Tests ${agent.test[0] ? "passed" : "failed"}: ${agent.test[2]} · ${new Date(agent.test[1] * 1000).toLocaleTimeString()}`}>{agent.test[0] ? "✓" : "✗"} tests</Tag>}
        {agent.usage && <span className="node-usage" title={`${agent.usage.inputTokens.toLocaleString()} in · ${agent.usage.outputTokens.toLocaleString()} out · ${agent.usage.cacheReadTokens.toLocaleString()} cached · ${agent.usage.turns} turns`}>{fmtTokens(agent.usage.inputTokens + agent.usage.outputTokens)} tok</span>}
        {(agent.queued ?? 0) > 0 && <Tag variant="filled" color="processing" className="node-queued" title="Waiting for this agent to finish its turn; delivered at its next prompt">{agent.queued} queued</Tag>}
        <span className="node-links">{links > 0 ? `⇄ ${links}` : ""}</span>
        <span className={`node-status st-${agent.status}`} title={agent.lastLine || undefined}>{statusLabel[agent.status]}</span>
      </footer>
    </div>
  );
});

export function fmtTokens(n: number) {
  return n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${Math.round(n / 1000)}k` : String(n);
}

export function shortPath(p: string) {
  if (!p) return "";
  const home = p.replace(/^\/Users\/[^/]+/, "~").replace(/^\/home\/[^/]+/, "~");
  const parts = home.split("/");
  return parts.length > 3 ? `…/${parts.slice(-2).join("/")}` : home;
}
