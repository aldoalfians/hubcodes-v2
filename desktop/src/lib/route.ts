import type { Agent, Edge } from "./types";

/** Same words as the core's `prompts::kind`: these roles coordinate. */
export const leads = (role: string | null | undefined) => !!role && /orchestr|lead|manager|coordinator|planner/i.test(role);

/**
 * The orchestrator a person's message to `target` goes through first, as the
 * core decides it (`relay::route_for`): the lead `target` is connected to by
 * lines on the canvas, through any chain of agents. None for an orchestrator,
 * a shell, an agent with no line to a lead, or when routing is off.
 */
export function routeLead(agents: Agent[], edges: Edge[], target: Agent, on = true): string | null {
  if (!on || leads(target.role) || target.provider === "shell") return null;
  const live = new Set(agents.filter((a) => !a.peer && !a.dead && leads(a.role)).map((a) => a.name));
  const seen = [target.name];
  for (let i = 0; i < seen.length; i++) {
    for (const e of edges) {
      const next = e.from === seen[i] ? e.to : e.to === seen[i] ? e.from : null;
      if (next && !seen.includes(next)) seen.push(next);
    }
  }
  return seen.slice(1).filter((n) => live.has(n)).sort()[0] ?? null;
}
