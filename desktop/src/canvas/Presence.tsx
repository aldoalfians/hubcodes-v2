import { useEffect, useRef } from "react";
import { useReactFlow, useViewport } from "@xyflow/react";
import { useStore } from "../store";
import { backend, isRemoteUi } from "../lib/backend";

const COLORS = ["#22c7e8", "#8b6cff", "#2de1c2", "#d2a8ff", "#ffa657", "#79c0ff"];
const colorFor = (who: string) => COLORS[[...who].reduce((a, c) => a + c.charCodeAt(0), 0) % COLORS.length];

/**
 * Teammates' pointers on the canvas (flow coordinates from the server), and
 * this viewer's own pointer reported back when the canvas runs in a browser.
 */
export function PresenceLayer() {
  const presence = useStore((s) => s.presence);
  const selectedId = useStore((s) => s.selectedId);
  const agents = useStore((s) => s.agents);
  const { x, y, zoom } = useViewport();
  const rf = useReactFlow();
  const last = useRef(0);
  useEffect(() => {
    if (!isRemoteUi) return;
    const onMove = (e: MouseEvent) => {
      const now = Date.now();
      if (now - last.current < 1000) return;
      last.current = now;
      const p = rf.screenToFlowPosition({ x: e.clientX, y: e.clientY });
      const sel = agents.find((a) => a.id === selectedId)?.name ?? "";
      void backend.presence(Math.round(p.x), Math.round(p.y), sel);
    };
    window.addEventListener("mousemove", onMove);
    return () => window.removeEventListener("mousemove", onMove);
  }, [rf, agents, selectedId]);
  if (presence.length === 0) return null;
  return (
    <div className="presence" aria-hidden>
      {presence.map((p) => (
        <div key={p.who} className="presence-cursor" style={{ left: p.x * zoom + x, top: p.y * zoom + y, "--who": colorFor(p.who) } as React.CSSProperties}>
          <svg width="14" height="18" viewBox="0 0 14 18"><path d="M1 1l12 7-5 1.5L5 17z" fill="var(--who)" stroke="#000" strokeOpacity="0.4" /></svg>
          <span className="presence-label">{p.who.split("@")[0]}{p.selected ? ` · ${p.selected}` : ""}</span>
        </div>
      ))}
    </div>
  );
}
