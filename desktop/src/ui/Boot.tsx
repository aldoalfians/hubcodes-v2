import { useEffect, useState } from "react";
import { useStore } from "../store";

const MIN_MS = 900;

/**
 * The opening screen: wordmark, a sweeping line and what the app is doing,
 * shown until the first snapshot is in (and for at least MIN_MS so it never
 * flashes). Fades out, then unmounts.
 */
export function Boot() {
  const ready = useStore((s) => s.ready);
  const session = useStore((s) => s.session);
  const agents = useStore((s) => s.agents);
  const [phase, setPhase] = useState<"in" | "out" | "gone">("in");
  const [t0] = useState(() => Date.now());
  useEffect(() => {
    if (!ready || phase !== "in") return;
    const wait = Math.max(0, MIN_MS - (Date.now() - t0));
    const a = window.setTimeout(() => setPhase("out"), wait);
    return () => window.clearTimeout(a);
  }, [ready, phase, t0]);
  useEffect(() => {
    if (phase !== "out") return;
    const a = window.setTimeout(() => setPhase("gone"), 420);
    return () => window.clearTimeout(a);
  }, [phase]);
  if (phase === "gone") return null;
  const step = !session ? "Connecting to tmux" : !ready ? `Opening ${session.session}` : agents.length ? `${agents.length} agent${agents.length === 1 ? "" : "s"} on the canvas` : "Canvas ready";
  return (
    <div className={`boot${phase === "out" ? " is-out" : ""}`} aria-live="polite" aria-busy={!ready}>
      <div className="boot-mark">
        <svg className="boot-glyph" viewBox="0 0 520 420" aria-hidden>
          <path d="M70 60 L318 234 L70 408" transform="translate(0 -24)" fill="none" stroke="currentColor" strokeWidth="44" strokeLinecap="round" strokeLinejoin="round" />
          <g fill="var(--bg, #0e1114)" stroke="currentColor" strokeWidth="22" transform="translate(0 -24)">
            <circle cx="70" cy="60" r="52" />
            <circle cx="318" cy="234" r="58" />
            <circle cx="70" cy="408" r="52" />
          </g>
          <rect x="368" y="350" width="148" height="48" rx="24" fill="#F5B13D" />
        </svg>
        <span className="boot-word">hubcodes</span>
      </div>
      <div className="boot-line"><span /></div>
      <div className="boot-step">{step}</div>
    </div>
  );
}
