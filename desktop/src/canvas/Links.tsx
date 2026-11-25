import { memo, useMemo } from "react";
import { Position, ViewportPortal, getBezierPath, useStore as useFlow, type InternalNode, type Node } from "@xyflow/react";
import { useStore } from "../store";
import { pairKey } from "../lib/types";
import type { AgentNodeType } from "./AgentNode";

/**
 * The links between cards, drawn by hubcodes itself rather than as React Flow
 * edges. React Flow only draws an edge once it has measured a handle element
 * on both cards; a card whose handles were measured at the wrong moment (a
 * re-mount, a resize mid-drag) keeps empty handle bounds and its links simply
 * vanish. The geometry here needs nothing but the cards' positions and sizes,
 * which the canvas already knows, so a link is on screen whenever both cards
 * are.
 */

type Box = { x: number; y: number; w: number; h: number };
const FALLBACK_W = 440;
const FALLBACK_H = 320;

function sideMid(b: Box, side: Position): [number, number] {
  switch (side) {
    case Position.Left: return [b.x, b.y + b.h / 2];
    case Position.Right: return [b.x + b.w, b.y + b.h / 2];
    case Position.Top: return [b.x + b.w / 2, b.y];
    default: return [b.x + b.w / 2, b.y + b.h];
  }
}

/**
 * A link leaves and enters on the sides that face each other, so cards can
 * sit anywhere (left, right, above, below) and the curve still reads. Sides
 * come from the free gap between the boxes, not their centres, so a card made
 * much taller than its neighbour still links through the facing left/right
 * sides while there is horizontal room. Overlapping boxes have no free side;
 * then the curve joins the sides facing each other's centre and the link is
 * drawn above the cards so it stays visible.
 */
export function floatingEnds(a: Box, b: Box) {
  const dx = b.x + b.w / 2 - (a.x + a.w / 2);
  const dy = b.y + b.h / 2 - (a.y + a.h / 2);
  const gapX = Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w));
  const gapY = Math.max(b.y - (a.y + a.h), a.y - (b.y + b.h));
  const horizontal = gapX >= 0 || gapY >= 0 ? gapX >= gapY : Math.abs(dx) * a.h >= Math.abs(dy) * a.w;
  const sourcePosition = horizontal ? (dx >= 0 ? Position.Right : Position.Left) : (dy >= 0 ? Position.Bottom : Position.Top);
  const targetPosition = horizontal ? (dx >= 0 ? Position.Left : Position.Right) : (dy >= 0 ? Position.Top : Position.Bottom);
  const [sourceX, sourceY] = sideMid(a, sourcePosition);
  const [targetX, targetY] = sideMid(b, targetPosition);
  return { sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition };
}

export function boxesOverlap(a: Box, b: Box): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

type Item = { key: string; from: string; to: string; label: string; path: string; lx: number; ly: number; over: boolean; involved: boolean; selected: boolean };

export const Links = memo(function Links() {
  // The geometry React Flow itself drew: absolute position and the measured
  // size of each card. Stored sizes and positions can lag the card on screen
  // (a resize, a restore, a card taller than its stored height), and a link
  // computed from them ends in the air beside the card.
  // A string signature, so a measure pass (which mutates the lookup in place)
  // still re-renders the links, and an unrelated store update does not.
  const lookup = useFlow((s) => s.nodeLookup);
  const sig = useFlow((s) => {
    let out = "";
    for (const n of s.nodeLookup.values()) {
      if (n.type !== "agent") continue;
      const p = n.internals.positionAbsolute;
      out += `${n.id}:${p.x},${p.y},${n.measured?.width ?? n.width},${n.measured?.height ?? n.height},${n.hidden ? 1 : 0};`;
    }
    return out;
  });
  const edges = useStore((s) => s.edges);
  const agents = useStore((s) => s.agents);
  const selectedId = useStore((s) => s.selectedId);
  const selectedEdge = useStore((s) => s.selectedEdge);
  const boxes = useMemo(() => {
    const m = new Map<string, Box>();
    for (const n of lookup.values() as IterableIterator<InternalNode<Node>>) {
      if (n.type !== "agent" || n.hidden) continue;
      const a = (n as unknown as InternalNode<AgentNodeType>).data.agent;
      const p = n.internals.positionAbsolute;
      m.set(a.name, { x: p.x, y: p.y, w: n.measured?.width ?? n.width ?? FALLBACK_W, h: n.measured?.height ?? n.height ?? FALLBACK_H });
    }
    return m;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lookup, sig]);
  const selName = agents.find((a) => a.id === selectedId)?.name;
  const items: Item[] = useMemo(
    () =>
      edges.flatMap((e) => {
        const a = boxes.get(e.from);
        const b = boxes.get(e.to);
        if (!a || !b) return [];
        const [path, lx, ly] = getBezierPath(floatingEnds(a, b));
        const key = `${e.from}>${e.to}`;
        return [{ key, from: e.from, to: e.to, label: e.label, path, lx, ly, over: boxesOverlap(a, b), involved: e.from === selName || e.to === selName, selected: selectedEdge === key }];
      }),
    [edges, boxes, selName, selectedEdge],
  );
  const below = items.filter((i) => !i.over);
  const above = items.filter((i) => i.over);
  return (
    <ViewportPortal>
      <Layer items={below} top={false} />
      {above.length > 0 && <Layer items={above} top />}
    </ViewportPortal>
  );
});

/** One stacking layer: the curves in an SVG, then the label chips over them. */
function Layer({ items, top }: { items: Item[]; top: boolean }) {
  return (
    <div className={`links-layer${top ? " is-top" : ""}`}>
      <svg className="links-svg" width="1" height="1" aria-hidden>
        {items.map((i) => <Curve key={i.key} item={i} />)}
      </svg>
      {items.map((i) => <Label key={i.key} item={i} />)}
    </div>
  );
}

function Curve({ item }: { item: Item }) {
  const [from, to] = [item.from, item.to];
  const loop = useStore((s) => s.loops[pairKey(from, to)]);
  const pulses = useStore((s) => s.pulses).filter((p) => p.key === item.key);
  const selectEdge = useStore((s) => s.selectEdge);
  const loopState = loop ? (loop.done ? "done" : loop.paused ? "paused" : "running") : null;
  const cls = `link${item.involved ? " is-involved" : ""}${item.selected ? " is-selected" : ""}${loop ? ` is-loop loop-${loopState}` : ""}`;
  return (
    <g>
      <path d={item.path} className={cls} />
      <path d={item.path} className="link-hit" onClick={(e) => { e.stopPropagation(); selectEdge(item.key); }} />
      {pulses.map((p) => (
        <circle key={p.at} r="4" className="pulse">
          <animateMotion dur="0.9s" repeatCount="1" fill="freeze" path={item.path} keyPoints={p.reverse ? "1;0" : "0;1"} keyTimes="0;1" calcMode="linear" />
        </circle>
      ))}
    </g>
  );
}

function Label({ item }: { item: Item }) {
  const [from, to] = [item.from, item.to];
  const loop = useStore((s) => s.loops[pairKey(from, to)]);
  const disconnect = useStore((s) => s.disconnect);
  const setLoop = useStore((s) => s.setLoop);
  const resetLoop = useStore((s) => s.resetLoop);
  const openLoopDialog = useStore((s) => s.openLoopDialog);
  const selectEdge = useStore((s) => s.selectEdge);
  const loopState = loop ? (loop.done ? "done" : loop.paused ? "paused" : "running") : null;
  const text = loop ? `↻ ${loop.max > 0 ? `${Math.min(loop.rounds, loop.max)}/${loop.max}` : `${loop.rounds}`}${loop.done ? " done" : loop.paused ? " paused" : ""}` : item.label;
  if (!text && !item.selected) return null;
  return (
    <div
      className={`edge-label${item.selected ? " is-selected" : ""}${loop ? ` is-loop loop-${loopState}` : ""}`}
      style={{ transform: `translate(-50%, -50%) translate(${item.lx}px, ${item.ly}px)` }}
      title={loop?.goal ? `Goal: ${loop.goal}` : undefined}
      onClick={(e) => { e.stopPropagation(); selectEdge(item.key); }}
    >
      {text}
      {item.selected && (
        <span className="edge-actions">
          {loop ? (
            <>
              {(loop.paused || loop.done) && <button onClick={(e) => { e.stopPropagation(); void resetLoop(from, to); }} title="Fresh round counter, keeps the goal">Resume</button>}
              <button onClick={(e) => { e.stopPropagation(); openLoopDialog(from, to); }} title="Send a (new) goal">Goal…</button>
              <button onClick={(e) => { e.stopPropagation(); void setLoop(from, to, false); }} title="Back to a plain link">Loop off</button>
            </>
          ) : (
            <button onClick={(e) => { e.stopPropagation(); openLoopDialog(from, to); }} title="They answer each other until DONE or the round limit">Loop…</button>
          )}
          <button className="edge-remove" onClick={(e) => { e.stopPropagation(); void disconnect(from, to); }} title="Remove this link · ⌫">Remove</button>
        </span>
      )}
    </div>
  );
}
