import { Button, Space } from "antd";
import { ExpandOutlined, MinusOutlined, PlusOutlined } from "@ant-design/icons";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ReactFlow,
  Background,
  BackgroundVariant,
  useReactFlow,
  useViewport,
  type Connection,
  type Node,
  type NodeChange,
  type OnConnect,
  type Viewport,
  applyNodeChanges,
  ConnectionMode,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useStore } from "../store";
import { AgentNode, type AgentNodeType } from "./AgentNode";
import { Links } from "./Links";
import { NoteNode, type NoteNodeType } from "./NoteNode";
import { BandNode, type BandNodeType } from "./BandNode";
import { OffscreenNeeds } from "./Offscreen";
import { PresenceLayer } from "./Presence";
import { pruneHosts } from "../terminal/host";

const nodeTypes = { agent: AgentNode, note: NoteNode, band: BandNode };
type AnyNode = AgentNodeType | NoteNodeType | BandNodeType;
/** Cards whose centre lies inside the band's box. */
export function bandMembers(g: { x: number; y: number; w: number; h: number }, agents: { name: string }[], positions: Record<string, [number, number]>, sizes: Record<string, [number, number]>, fallback: (i: number) => [number, number]) {
  return agents.filter((a, i) => {
    const pos = positions[a.name] ?? fallback(i);
    const size = sizes[a.name] ?? [NODE_W, NODE_H];
    const cx = pos[0] + size[0] / 2;
    const cy = pos[1] + size[1] / 2;
    return cx >= g.x && cx <= g.x + g.w && cy >= g.y && cy <= g.y + g.h;
  }).map((a) => a.name);
}
const NOTE_ID = "note:shared";
export const NODE_W = 440;
export const NODE_H = 320;
const GAP_X = 120;
const GAP_Y = 80;

function autoSlot(index: number): [number, number] {
  const cols = 2;
  const col = index % cols;
  const row = Math.floor(index / cols);
  return [80 + col * (NODE_W + GAP_X), 80 + row * (NODE_H + GAP_Y)];
}

export function Canvas() {
  const allAgents = useStore((s) => s.agents);
  const showRemote = useStore((s) => s.showRemote);
  // Other devices' agents are cards on their own Mac; here only on request.
  const agents = useMemo(() => (showRemote ? allAgents : allAgents.filter((a) => !a.peer)), [allAgents, showRemote]);
  const edges = useStore((s) => s.edges);
  const positions = useStore((s) => s.positions);
  const selectedId = useStore((s) => s.selectedId);
  const select = useStore((s) => s.select);
  const setPosition = useStore((s) => s.setPosition);
  const setViewport = useStore((s) => s.setViewport);
  const connect = useStore((s) => s.connect);
  const initialViewport = useStore((s) => s.viewport);
  const rf = useReactFlow();
  const [localPos, setLocalPos] = useState<Record<string, [number, number]>>({});
  const [localSize, setLocalSize] = useState<Record<string, [number, number]>>({});
  const [connecting, setConnecting] = useState(false);
  const sizes = useStore((s) => s.sizes);
  const setSize = useStore((s) => s.setSize);
  const selectedEdge = useStore((s) => s.selectedEdge);
  const setSelectedEdge = useStore((s) => s.selectEdge);
  const linkingFrom = useStore((s) => s.linkingFrom);
  const startLink = useStore((s) => s.startLink);
  const openMenu = useStore((s) => s.openMenu);
  const disconnect = useStore((s) => s.disconnect);
  const groups = useStore((s) => s.groups);
  const setGroup = useStore((s) => s.setGroup);
  const moveGroupMembers = useStore((s) => s.moveGroupMembers);
  const dragStart = useRef<Record<string, [number, number]>>({});

  useEffect(() => {
    pruneHosts(new Set(agents.map((a) => a.id)));
  }, [agents]);

  const notesOpen = useStore((s) => s.notesOpen);
  const notesText = useStore((s) => s.notes);
  const nodes: AnyNode[] = useMemo(() => {
    const byName = new Map(agents.map((a) => [a.name, a]));
    const mergedPos = { ...positions, ...localPos };
    const mergedSize = { ...sizes, ...localSize };
    const bandNodes: BandNodeType[] = Object.entries(groups).map(([name, g]) => {
      const pos = localPos[`band:${name}`] ?? [g.x, g.y];
      const size = localSize[`band:${name}`] ?? [g.w, g.h];
      const members = bandMembers({ x: pos[0], y: pos[1], w: size[0], h: size[1] }, agents, mergedPos, mergedSize, autoSlot);
      const h = g.collapsed ? 40 : size[1];
      return { id: `band:${name}`, type: "band", position: { x: pos[0], y: pos[1] }, data: { name, members, collapsed: !!g.collapsed, color: g.color ?? null }, width: size[0], height: h, style: { width: size[0], height: h }, zIndex: -1, selectable: true };
    });
    const hidden = new Set(bandNodes.filter((b) => b.data.collapsed).flatMap((b) => b.data.members));
    const noteNode: NoteNodeType[] = notesOpen
      ? [{
          id: NOTE_ID,
          type: "note",
          position: { x: (localPos["note:shared"] ?? positions["note:shared"] ?? [80 - NODE_W - GAP_X, 80])[0], y: (localPos["note:shared"] ?? positions["note:shared"] ?? [80 - NODE_W - GAP_X, 80])[1] },
          data: { text: notesText },
          width: (localSize["note:shared"] ?? sizes["note:shared"] ?? [360, 320])[0],
          height: (localSize["note:shared"] ?? sizes["note:shared"] ?? [360, 320])[1],
          style: { width: (localSize["note:shared"] ?? sizes["note:shared"] ?? [360, 320])[0], height: (localSize["note:shared"] ?? sizes["note:shared"] ?? [360, 320])[1] },
          selectable: true,
        }]
      : [];
    const agentNodes: AgentNodeType[] = agents.filter((a) => !hidden.has(a.name)).map((a) => {
      const i = agents.indexOf(a);
      const pos = localPos[a.name] ?? positions[a.name] ?? autoSlot(i);
      const size = localSize[a.name] ?? sizes[a.name] ?? [NODE_W, NODE_H];
      const links = edges.filter((e) => (e.from === a.name || e.to === a.name) && byName.has(e.from) && byName.has(e.to)).length;
      return {
        id: a.id,
        type: "agent",
        position: { x: pos[0], y: pos[1] },
        data: { agent: a, links, slot: i + 1 },
        selected: a.id === selectedId,
        width: size[0],
        height: size[1],
        style: { width: size[0], height: size[1] },
      };
    });
    return [...bandNodes, ...noteNode, ...agentNodes];
  }, [agents, edges, positions, sizes, selectedId, localPos, localSize, notesOpen, notesText, groups]);


  useEffect(() => {
    if (!selectedEdge) return;
    const onKey = (e: KeyboardEvent) => {
      const typing = (e.target as HTMLElement)?.closest?.("input, textarea, .xterm");
      if ((e.key === "Backspace" || e.key === "Delete") && !typing) {
        const [from, to] = selectedEdge.split(">");
        void disconnect(from, to);
        setSelectedEdge(null);
      }
      if (e.key === "Escape") setSelectedEdge(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedEdge, disconnect]);

  const onNodesChange = useCallback(
    (changes: NodeChange<AnyNode>[]) => {
      const next = applyNodeChanges(changes, nodes);
      const moved: Record<string, [number, number]> = {};
      const resized: Record<string, [number, number]> = {};
      const keyOf = (n: Node) => (n.type === "note" ? "note:shared" : n.type === "band" ? n.id : (n as AgentNodeType).data.agent.name);
      for (const c of changes) {
        if (c.type === "add" || c.type === "replace") continue;
        const n = next.find((x) => x.id === c.id);
        if (c.type === "position" && c.position && n) moved[keyOf(n)] = [c.position.x, c.position.y];
        if (c.type === "dimensions" && c.dimensions && c.resizing && n) resized[keyOf(n)] = [c.dimensions.width, c.dimensions.height];
        if (c.type === "select" && c.selected && n?.type === "agent") select(c.id);
      }
      for (const c of changes) {
        if (c.type === "position" && c.position && c.dragging && c.id.startsWith("band:")) {
          const b = next.find((x) => x.id === c.id) as BandNodeType | undefined;
          const from = dragStart.current[c.id];
          if (b && from) {
            // Members are offset by the band's total displacement from the drag start.
            const dx = c.position.x - from[0];
            const dy = c.position.y - from[1];
            const base = positions;
            for (const m of b.data.members) {
              const cur = base[m] ?? autoSlot(agents.findIndex((a) => a.name === m));
              moved[m] = [cur[0] + dx, cur[1] + dy];
            }
          }
        }
      }
      if (Object.keys(moved).length) setLocalPos((p) => ({ ...p, ...moved }));
      if (Object.keys(resized).length) setLocalSize((p) => ({ ...p, ...resized }));
    },
    [nodes, select, positions, agents],
  );

  /** NodeResizer end: persist size, and position when a left/top handle moved it. */
  const onResizeEnd = useCallback(
    (name: string, x: number, y: number, w: number, h: number) => {
      setLocalSize((p) => ({ ...p, [name]: [w, h] }));
      setLocalPos((p) => ({ ...p, [name]: [x, y] }));
      if (name.startsWith("band:")) {
        setGroup(name.slice(5), { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) });
        return;
      }
      setSize(name, w, h);
      setPosition(name, Math.round(x), Math.round(y));
    },
    [setSize, setPosition, setGroup],
  );

  const onDragStart = useCallback((_: unknown, n: Node) => {
    if (n.type === "band") dragStart.current[n.id] = [n.position.x, n.position.y];
  }, []);
  const onDragStop = useCallback(
    (_: unknown, n: Node) => {
      if (n.type === "band") {
        const b = n as BandNodeType;
        const from = dragStart.current[n.id] ?? [n.position.x, n.position.y];
        const dx = n.position.x - from[0];
        const dy = n.position.y - from[1];
        delete dragStart.current[n.id];
        setGroup(b.data.name, { x: Math.round(n.position.x), y: Math.round(n.position.y) });
        if (dx || dy) {
          setLocalPos((p) => { const next = { ...p }; for (const m of b.data.members) { const cur = next[m] ?? positions[m]; if (cur) next[m] = [cur[0] + dx, cur[1] + dy]; } return next; });
          moveGroupMembers(b.data.name, dx, dy, b.data.members);
        }
        return;
      }
      const name = n.type === "note" ? "note:shared" : (n as AgentNodeType).data.agent.name;
      setPosition(name, Math.round(n.position.x), Math.round(n.position.y));
    },
    [setPosition, setGroup, moveGroupMembers, positions],
  );

  const onConnect: OnConnect = useCallback(
    (c: Connection) => {
      const from = agents.find((a) => a.id === c.source)?.name;
      const to = agents.find((a) => a.id === c.target)?.name;
      if (from && to && from !== to) void connect(from, to);
    },
    [agents, connect],
  );

  const onNodeClick = useCallback(
    (id: string) => {
      if (linkingFrom && linkingFrom !== id) {
        const from = agents.find((a) => a.id === linkingFrom)?.name;
        const to = agents.find((a) => a.id === id)?.name;
        if (from && to) void connect(from, to);
        startLink(null);
        return;
      }
      select(id);
      setSelectedEdge(null);
    },
    [linkingFrom, agents, connect, startLink, select, setSelectedEdge],
  );

  const onMoveEnd = useCallback((_: unknown, v: Viewport) => setViewport([v.x, v.y, v.zoom]), [setViewport]);

  useEffect(() => {
    if (initialViewport) rf.setViewport({ x: initialViewport[0], y: initialViewport[1], zoom: initialViewport[2] });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // A restored camera that shows no agent (window got smaller, agents moved,
  // different machine) is worse than none: fit once when agents first arrive.
  const checkedRestore = useRef(false);
  useEffect(() => {
    if (checkedRestore.current || agents.length === 0) return;
    checkedRestore.current = true;
    if (!initialViewport) return;
    window.setTimeout(() => {
      const stage = document.querySelector<HTMLElement>(".stage");
      if (!stage) return;
      const v = rf.getViewport();
      const rect = { x: -v.x / v.zoom, y: -v.y / v.zoom, width: stage.clientWidth / v.zoom, height: stage.clientHeight / v.zoom };
      if (rf.getIntersectingNodes(rect).filter((n) => n.type !== "note").length === 0 || v.zoom < 0.2) rf.fitView({ padding: 0.15, maxZoom: 1 });
    }, 50);
  }, [agents.length, initialViewport, rf]);

  return (
    <ReactFlow
      nodes={nodes}
      edges={[]}
      nodeTypes={nodeTypes}
      onNodesChange={onNodesChange}
      onNodeDragStart={onDragStart}
      onNodeDragStop={onDragStop}
      onNodeClick={(_, n) => { if (n.type === "agent") onNodeClick(n.id); }}
      onNodeContextMenu={(e, n) => { e.preventDefault(); if (n.type === "agent") openMenu(e.clientX, e.clientY, n.id); }}
      onPaneClick={() => { select(null); setSelectedEdge(null); if (linkingFrom) startLink(null); }}
      onConnectStart={() => setConnecting(true)}
      onConnectEnd={() => setConnecting(false)}
      connectionRadius={40}
      className={connecting ? "is-connecting" : undefined}
      onConnect={onConnect}
      onMoveEnd={onMoveEnd}
      connectionMode={ConnectionMode.Loose}
      minZoom={0.1}
      maxZoom={2}
      panOnScroll
      zoomOnScroll={false}
      zoomActivationKeyCode="Meta"
      zoomOnPinch
      selectionOnDrag={false}
      panOnDrag
      nodesConnectable
      elevateNodesOnSelect
      deleteKeyCode={null}
      fitView={!initialViewport}
      fitViewOptions={{ padding: 0.2, maxZoom: 1 }}
    >
      <Background variant={BackgroundVariant.Dots} gap={28} size={1.2} color="#303136" />
      <Links />
      <ZoomPill />
      <OffscreenNeeds />
      <PresenceLayer />
      <ResizeBridge onResizeEnd={onResizeEnd} />
    </ReactFlow>
  );
}

/** Lets AgentNode's resizer reach the canvas callbacks without prop drilling. */
export const resizeEnd: { current: ((name: string, x: number, y: number, w: number, h: number) => void) | null } = { current: null };
function ResizeBridge({ onResizeEnd }: { onResizeEnd: (name: string, x: number, y: number, w: number, h: number) => void }) {
  useEffect(() => {
    resizeEnd.current = onResizeEnd;
    return () => {
      resizeEnd.current = null;
    };
  }, [onResizeEnd]);
  return null;
}

/** Bottom-right zoom readout: − 100% + and fit, like a design canvas. */
function ZoomPill() {
  const { zoom } = useViewport();
  const rf = useReactFlow();
  return (
    <div className="zoom-pill" role="group" aria-label="Zoom">
      <Space.Compact size="small">
        <Button icon={<MinusOutlined />} onClick={() => rf.zoomOut({ duration: 120 })} title="Zoom out · ⌘−" aria-label="Zoom out" />
        <Button className="zoom-value" onClick={() => rf.zoomTo(1, { duration: 160 })} title="Reset to 100%">{Math.round(zoom * 100)}%</Button>
        <Button icon={<PlusOutlined />} onClick={() => rf.zoomIn({ duration: 120 })} title="Zoom in · ⌘+" aria-label="Zoom in" />
        <Button icon={<ExpandOutlined />} onClick={() => rf.fitView({ padding: 0.15, duration: 200, maxZoom: 1 })} title="Fit all agents · ⌘0">Fit</Button>
      </Space.Compact>
    </div>
  );
}
