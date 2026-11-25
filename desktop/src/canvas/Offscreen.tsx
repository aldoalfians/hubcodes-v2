import { useNodes, useReactFlow, useViewport } from "@xyflow/react";
import { useStore } from "../store";
import type { AgentNodeType } from "./AgentNode";

/**
 * Pills at the edge of the canvas for agents that need you but are out of
 * view; clicking one brings the card in. Lives inside <ReactFlow>, so the
 * node positions and the viewport are the live ones.
 */
export function OffscreenNeeds() {
  const nodes = useNodes<AgentNodeType>();
  const agents = useStore((s) => s.agents);
  const select = useStore((s) => s.select);
  const { x, y, zoom } = useViewport();
  const rf = useReactFlow();
  const stage = document.querySelector<HTMLElement>(".stage");
  if (!stage) return null;
  const W = stage.clientWidth;
  const H = stage.clientHeight;
  const waiting = agents.filter((a) => a.status === "waiting");
  const pills = waiting.flatMap((a) => {
    const n = nodes.find((n) => n.id === a.id);
    if (!n) return [];
    const w = (n.measured?.width ?? n.width ?? 440) * zoom;
    const h = (n.measured?.height ?? n.height ?? 320) * zoom;
    const left = n.position.x * zoom + x;
    const top = n.position.y * zoom + y;
    const visible = left + w > 0 && left < W && top + h > 0 && top < H;
    if (visible) return [];
    // Where the card's centre is, clamped to the stage with a margin.
    const cx = Math.min(Math.max(left + w / 2, 70), W - 70);
    const cy = Math.min(Math.max(top + h / 2, 92), H - 44);
    const dir = left + w / 2 < 0 ? "←" : left + w / 2 > W ? "→" : top + h / 2 < 0 ? "↑" : "↓";
    return [{ a, cx, cy, dir }];
  });
  if (pills.length === 0) return null;
  return (
    <div className="offscreen" aria-live="polite">
      {pills.map(({ a, cx, cy, dir }) => (
        <button
          key={a.id}
          type="button"
          className="offscreen-pill"
          style={{ left: cx, top: cy }}
          title={`${a.name} needs you · click to go there`}
          onClick={() => { select(a.id); void rf.fitView({ nodes: [{ id: a.id }], duration: 240, maxZoom: 1, padding: 0.3 }); }}
        >
          <span className="offscreen-dir">{dir}</span>
          <span className="mono">{a.name}</span>
          <span className="offscreen-what">needs you</span>
        </button>
      ))}
    </div>
  );
}
