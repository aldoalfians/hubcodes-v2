import { useState } from "react";
import { Button, Select, Switch, Tooltip, Typography } from "antd";
import { ArrowDownOutlined, ArrowUpOutlined, CloseOutlined, ThunderboltOutlined } from "@ant-design/icons";
import { Group, Row } from "./primitives";
import { Amount, code } from "./shared";
import { useSettings } from "./context";
import { useStore } from "../../store";
import { color256 } from "../../lib/colors";
import type { AutoTeamConfig, FailoverConfig, Provider } from "../../lib/types";

const { Text } = Typography;

const AUTOTEAM: AutoTeamConfig = { enabled: false, per_role: 2, roles: {}, max_agents: 8, idle_minutes: 10, cooldown_minutes: 2, worktree: true };
const FAILOVER: FailoverConfig = { enabled: false, chain: [], rest_minutes: 60 };
const isLead = (r: string) => /orchestr|lead|manager|coordinator|planner/i.test(r);

function Num({ value, onChange, min, max, unit, label }: { value: number; onChange: (n: number) => void; min: number; max: number; unit: string; label: string }) {
  return <Amount value={value} onChange={(n) => onChange(Math.max(min, n))} min={min} max={max} unit={unit} label={label} />;
}

/**
 * `[autoteam]`: one more agent for a role whose work waits while all its
 * agents are busy, removed again once it is idle with nothing waiting.
 */
export function AutoTeamGroup() {
  const { cfg, edit, roleOptions } = useSettings();
  const agents = useStore((s) => s.agents);
  const a = { ...AUTOTEAM, ...cfg.autoteam };
  const set = (p: Partial<AutoTeamConfig>) => edit({ autoteam: { ...a, ...p } });
  const [q, setQ] = useState("");
  const running = new Set(agents.filter((x) => !x.peer && x.role).map((x) => x.role as string));
  const roles = Array.from(new Set([...running, ...Object.keys(a.roles ?? {})]));
  const fallback = (r: string) => (isLead(r) ? 0 : a.per_role);
  const limitOf = (r: string) => a.roles?.[r] ?? fallback(r);
  const setRole = (r: string, n: number) => {
    const next = { ...(a.roles ?? {}) };
    if (n === fallback(r)) delete next[r];
    else next[r] = n;
    set({ roles: next });
  };
  return (
    <Group
      title="Auto team"
      desc="When work for a role piles up and all its agents are busy, a temporary agent joins; it leaves once idle. Agents you start yourself stay."
      toggle={{ checked: a.enabled, onChange: (v) => set({ enabled: v }), label: "Add agents when work piles up" }}
      off="Off: work waits for the agents you have."
    >
      <Row label="Extra agents per role" hint="How many copies of an agent with that role may join at once: same provider, model, folder and links." stack>
        <div className="ft-table" role="list" aria-label="Extra agents per role">
          {roles.map((r) => (
            <div key={r} className={`ft-trow${limitOf(r) === 0 ? " is-zero" : ""}`} role="listitem">
              <span className="ft-trow-name">{r}</span>
              <Text type="secondary" className="ft-trow-note">{!running.has(r) ? "no agent runs it yet" : isLead(r) && limitOf(r) === 0 ? "one lead per team" : limitOf(r) === 0 ? "never added" : ""}</Text>
              <Amount value={limitOf(r)} onChange={(n) => setRole(r, n)} max={10} unit="extra" label={`Extra ${r} agents`} />
              <span className="ft-trow-x">
                {a.roles?.[r] !== undefined && <Tooltip title="Use the default"><Button size="small" type="text" icon={<CloseOutlined />} onClick={() => { const next = { ...(a.roles ?? {}) }; delete next[r]; set({ roles: next }); }} aria-label={`Reset ${r}`} /></Tooltip>}
              </span>
            </div>
          ))}
          <div className="ft-trow is-other" role="listitem">
            <span className="ft-trow-name">{roles.length ? "Any other role" : "Every role"}</span>
            <Text type="secondary" className="ft-trow-note">the default</Text>
            <Amount value={a.per_role} onChange={(n) => set({ per_role: n })} max={10} unit="extra" label="Extra agents per role" />
            <span className="ft-trow-x" />
          </div>
          <Select
            className="ft-table-add"
            value={null as unknown as string}
            placeholder="+ A different limit for another role…"
            showSearch
            options={[
              ...roleOptions.filter((o) => !roles.includes(o.value)),
              ...(q.trim() && !roles.includes(q.trim()) && !roleOptions.some((o) => o.value === q.trim()) ? [{ value: q.trim(), label: `Use “${q.trim()}”` }] : []),
            ]}
            onSearch={setQ}
            onChange={(v: string) => { setQ(""); if (v) set({ roles: { ...(a.roles ?? {}), [v]: a.per_role } }); }}
            aria-label="Add a role limit"
            notFoundContent="Type a role name"
          />
        </div>
      </Row>
      <Row label="Agents on this device, at most" hint="All of them, added or not. Past it, no extra joins.">
        <Num value={a.max_agents} onChange={(n) => set({ max_agents: n })} min={1} max={50} unit="agents" label="Agents at most" />
      </Row>
      <Row label="Remove an extra after idle" hint="Its screen unchanged and nothing waiting for its role.">
        <Num value={a.idle_minutes} onChange={(n) => set({ idle_minutes: n })} min={1} max={240} unit="minutes" label="Remove after idle minutes" />
      </Row>
      <Row label="Own branch for engineers" hint={<>Each extra engineer works on {code("agent/<name>")}, so two never edit one folder. Left-over work stays there and goes to another engineer as a task.</>}>
        <Switch checked={a.worktree} onChange={(v) => set({ worktree: v })} aria-label="Own branch for added software engineers" />
      </Row>
      <div className="ft-foot"><Text type="secondary">The orchestrator can ask for one too: {code('hubcodes hire --role "software engineer"')}.</Text></div>
    </Group>
  );
}

/** `claude:sonnet` → provider and model. */
const split = (e: string) => {
  const i = e.indexOf(":");
  return i < 0 ? { p: e, m: "" } : { p: e.slice(0, i), m: e.slice(i + 1) };
};

/** A sensible order from what is installed: Claude's models first, then the other tools. */
function suggested(providers: Provider[]): string[] {
  const ok = providers.filter((p) => p.installed && p.name !== "shell");
  const out: string[] = [];
  const claude = ok.find((p) => p.name === "claude");
  if (claude) {
    for (const m of ["opus", "sonnet"]) if (claude.models.includes(m)) out.push(`claude:${m}`);
    if (!out.length) out.push("claude");
  }
  for (const n of ["codex", "opencode", "gemini", "pi", "omp"]) if (ok.some((p) => p.name === n)) out.push(n);
  return out;
}

/**
 * `[failover]`: when a tool says it ran out, the agent moves to the next
 * entry of the order, keeping its role, folder and links.
 */
export function FailoverGroup() {
  const { cfg, edit } = useSettings();
  const providers = useStore((s) => s.providers);
  const f = { ...FAILOVER, ...cfg.failover };
  const set = (p: Partial<FailoverConfig>) => edit({ failover: { ...f, ...p } });
  const chain = f.chain;
  const move = (i: number, d: number) => {
    const next = [...chain];
    const [x] = next.splice(i, 1);
    next.splice(i + d, 0, x);
    set({ chain: next });
  };
  const prov = (n: string) => providers.find((p) => p.name === n);
  const options = providers
    .filter((p) => p.installed && p.name !== "shell")
    .map((p) => ({
      label: p.name,
      options: [p.name, ...p.models.map((m) => `${p.name}:${m}`)].filter((e) => !chain.includes(e)).map((e) => ({ value: e, label: e.includes(":") ? `${p.name} · ${split(e).m}` : `${p.name} · its default model` })),
    }))
    .filter((g) => g.options.length);
  const pick = suggested(providers);
  return (
    <Group
      title="When a provider runs out"
      desc="A usage limit or no credit left: the agent moves to the next one below and is told to carry on. Same provider keeps the conversation; another one gets a hand-over note."
      toggle={{ checked: f.enabled, onChange: (v) => set({ enabled: v, chain: v && chain.length === 0 ? pick : chain }), label: "Move to the next provider when one runs out" }}
      off="Off: the agent waits for its limit to reset and you get an alert."
    >
      <Row label="Try in this order" hint="From the agent's own entry onward, then round from the top. Ones not installed are skipped." stack>
        <div className="ft-chain">
          {chain.length === 0 && (
            <div className="ft-chain-empty">
              <Text type="secondary">Nothing to move to yet.</Text>
              {pick.length > 0 && <Button size="small" icon={<ThunderboltOutlined />} onClick={() => set({ chain: pick })}>Use {pick.join(" → ")}</Button>}
            </div>
          )}
          {chain.map((e, i) => {
            const { p, m } = split(e);
            const pr = prov(p);
            return (
              <div key={e} className={`ft-step${pr && !pr.installed ? " is-missing" : ""}`}>
                <span className="ft-step-n">{i + 1}</span>
                <span className="ft-step-glyph" style={{ color: color256(pr?.color) }}>{pr?.glyph ?? "·"}</span>
                <span className="ft-step-name">{p}</span>
                <span className="ft-step-model">{m || "default model"}</span>
                {pr && !pr.installed && <Text type="warning" className="ft-step-note">not installed</Text>}
                <span className="ft-step-actions">
                  <Tooltip title="Earlier"><Button size="small" type="text" icon={<ArrowUpOutlined />} disabled={i === 0} onClick={() => move(i, -1)} aria-label={`Move ${e} earlier`} /></Tooltip>
                  <Tooltip title="Later"><Button size="small" type="text" icon={<ArrowDownOutlined />} disabled={i === chain.length - 1} onClick={() => move(i, 1)} aria-label={`Move ${e} later`} /></Tooltip>
                  <Tooltip title="Remove"><Button size="small" type="text" icon={<CloseOutlined />} onClick={() => set({ chain: chain.filter((x) => x !== e) })} aria-label={`Remove ${e}`} /></Tooltip>
                </span>
              </div>
            );
          })}
          {options.length > 0 && (
            <Select className="ft-chain-add" value={null as unknown as string} placeholder="Add a provider or model…" options={options} showSearch onChange={(v: string) => v && set({ chain: [...chain, v] })} aria-label="Add to the failover order" />
          )}
        </div>
      </Row>
      <Row label="Skip one that ran out for" hint="Then it is tried again; most limits reset within hours.">
        <Num value={f.rest_minutes} onChange={(n) => set({ rest_minutes: n })} min={5} max={1440} unit="minutes" label="Skip minutes" />
      </Row>
    </Group>
  );
}
