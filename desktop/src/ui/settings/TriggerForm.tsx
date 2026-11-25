import { useState } from "react";
import { Button, Input, InputNumber, Segmented, Select } from "antd";
import { CopyOutlined } from "@ant-design/icons";
import type { Trigger } from "../../lib/types";
import { useStore } from "../../store";
import { ChoiceSelect, githubConnection } from "./connect";
import { Field, Fields, FormGroup, ToggleField } from "./primitives";
import { useSettings } from "./context";

/* ── When ─────────────────────────────────────────────────────────── */

export type Schedule =
  | { every: "weekdays" | "day"; hour: number; minute: number }
  | { every: "week"; day: number; hour: number; minute: number }
  | { every: "hour"; minute: number }
  | { every: "minutes"; n: number }
  | { every: "cron"; cron: string };

const DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const pad = (n: number) => String(n).padStart(2, "0");

export function cronOf(s: Schedule): string {
  switch (s.every) {
    case "weekdays": return `${s.minute} ${s.hour} * * 1-5`;
    case "day": return `${s.minute} ${s.hour} * * *`;
    case "week": return `${s.minute} ${s.hour} * * ${s.day}`;
    case "hour": return `${s.minute} * * * *`;
    case "minutes": return `*/${s.n} * * * *`;
    case "cron": return s.cron.trim();
  }
}

/** A cron line as one of the simple schedules, or a custom one. */
export function readCron(cron: string): Schedule {
  const f = cron.trim().split(/\s+/);
  const num = (x: string | undefined, max: number) => (x !== undefined && /^\d+$/.test(x) && Number(x) <= max ? Number(x) : null);
  if (f.length === 5 && f[2] === "*" && f[3] === "*") {
    const [mi, h, , , wd] = f;
    const m = num(mi, 59);
    const hr = num(h, 23);
    if (m !== null && hr !== null && wd === "1-5") return { every: "weekdays", hour: hr, minute: m };
    if (m !== null && hr !== null && wd === "*") return { every: "day", hour: hr, minute: m };
    if (m !== null && hr !== null && num(wd, 6) !== null) return { every: "week", day: Number(wd), hour: hr, minute: m };
    if (m !== null && h === "*" && wd === "*") return { every: "hour", minute: m };
    const step = mi.match(/^\*\/(\d+)$/);
    if (step && h === "*" && wd === "*") return { every: "minutes", n: Number(step[1]) };
  }
  return { every: "cron", cron };
}

export function describeSchedule(s: Schedule): string {
  const at = "hour" in s ? `${pad(s.hour)}:${pad(s.minute)}` : "";
  switch (s.every) {
    case "weekdays": return `Every weekday at ${at}`;
    case "day": return `Every day at ${at}`;
    case "week": return `Every ${DAYS[s.day]} at ${at}`;
    case "hour": return s.minute ? `Every hour at :${pad(s.minute)}` : "Every hour";
    case "minutes": return `Every ${s.n} minutes`;
    case "cron": return `On cron ${s.cron || "…"}`;
  }
}

type When = { kind: "schedule"; schedule: Schedule } | { kind: "pr"; repo: string } | { kind: "webhook"; name: string };

export function readWhen(on: string): When {
  const [head, ...rest] = on.trim().split(/\s+/);
  const arg = rest.join(" ");
  if (head === "pr") return { kind: "pr", repo: arg };
  if (head === "webhook") return { kind: "webhook", name: arg };
  return { kind: "schedule", schedule: readCron(head === "cron" ? arg : "0 9 * * 1-5") };
}
export function whenOf(w: When): string {
  return w.kind === "pr" ? `pr ${w.repo.trim()}` : w.kind === "webhook" ? `webhook ${w.name.trim()}` : `cron ${cronOf(w.schedule)}`;
}

/* ── Action ───────────────────────────────────────────────────────── */

type Action =
  | { kind: "send"; to: string; text: string }
  | { kind: "task"; title: string; detail: string }
  | { kind: "loop"; a: string; b: string; goal: string }
  | { kind: "team"; template: string }
  | { kind: "raw"; text: string };

/** The action line keeps double quotes as delimiters, so a quote typed in the text becomes a typographic one. */
const q = (s: string) => `"${s.replace(/"/g, "”")}"`;

/** Same splitting as the core's `triggers::words`. */
function words(action: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quoted = false;
  let had = false;
  for (const c of action) {
    if (c === '"') { quoted = !quoted; had = true; } else if (/\s/.test(c) && !quoted) { if (cur || had) out.push(cur); cur = ""; had = false; } else cur += c;
  }
  if (cur || had) out.push(cur);
  return out;
}

export function readAction(action: string): Action {
  const w = words(action);
  if (!action.trim()) return { kind: "send", to: "", text: "" };
  if (w[0] === "send" && w.length >= 2) return { kind: "send", to: w[1], text: w.slice(2).join(" ") };
  if (w[0] === "task" && w[1] === "add" && w.length >= 3) return { kind: "task", title: w[2], detail: w.slice(3).join(" ") };
  if (w[0] === "loop" && w[1] === "start" && w.length >= 4 && !w.slice(4).some((x) => x.startsWith("--"))) return { kind: "loop", a: w[2], b: w[3], goal: w.slice(4).join(" ") };
  if (w[0] === "team" && w[1] === "start" && w.length === 3) return { kind: "team", template: w[2] };
  return { kind: "raw", text: action };
}
export function actionOf(a: Action): string {
  switch (a.kind) {
    case "send": return `send ${a.to} ${q(a.text)}`;
    case "task": return `task add ${q(a.title)}${a.detail.trim() ? ` ${q(a.detail)}` : ""}`;
    case "loop": return `loop start ${a.a} ${a.b} ${q(a.goal)}`;
    case "team": return `team start ${a.template}`;
    case "raw": return a.text;
  }
}

function describeAction(a: Action): string {
  switch (a.kind) {
    case "send": return a.to ? `tell ${a.to}: “${a.text || "…"}”` : "tell an agent something";
    case "task": return `add the task “${a.title || "…"}” to the board`;
    case "loop": return a.a && a.b ? `start a loop between ${a.a} and ${a.b}: “${a.goal || "…"}”` : "start a loop";
    case "team": return a.template ? `start the ${a.template} team` : "start a team";
    case "raw": return `run ${a.text || "…"}`;
  }
}

/* ── Form ─────────────────────────────────────────────────────────── */

/**
 * When and what, picked rather than typed; the stored `on` / `action`
 * strings are built from the picks. A line the pickers cannot show stays
 * editable as text under Advanced.
 */
export function TriggerForm({ draft, onChange }: { draft: Trigger; onChange: (p: Partial<Trigger>) => void }) {
  const { cfg, tsIp } = useSettings();
  const agents = useStore((s) => s.agents);
  const notify = useStore((s) => s.notify);
  const [advanced, setAdvanced] = useState(false);
  const when = readWhen(draft.on);
  const act = readAction(draft.action);
  const setWhen = (w: When) => onChange({ on: whenOf(w) });
  const setAct = (a: Action) => onChange({ action: actionOf(a) });
  const agentOptions = agents.filter((a) => !a.dead).map((a) => ({ value: a.name, label: `${a.name}${a.role ? ` · ${a.role}` : ""}` }));
  const templates = cfg.templates.map((t) => ({ value: t.name, label: t.description ? `${t.name} · ${t.description}` : t.name }));
  const sched = when.kind === "schedule" ? when.schedule : null;
  const hookUrl = when.kind === "webhook" ? `http://${tsIp ?? "this-mac"}:${cfg.serve.port}/webhook/${when.name || "name"}` : "";
  const fill = when.kind === "pr" ? "{number} {title} {url}" : when.kind === "webhook" ? "{body}" : "";
  return (
    <Fields cols={1}>
      <Field label="Name" required><Input className="mono" value={draft.name} onChange={(e) => onChange({ name: e.target.value })} placeholder="morning-review" spellCheck={false} aria-label="Name" /></Field>
      <FormGroup title="When">
        <Segmented
          block
          value={when.kind}
          onChange={(v) => setWhen(v === "pr" ? { kind: "pr", repo: "" } : v === "webhook" ? { kind: "webhook", name: draft.name || "deploy" } : { kind: "schedule", schedule: { every: "weekdays", hour: 9, minute: 0 } })}
          options={[{ value: "schedule", label: "On a schedule" }, { value: "pr", label: "A pull request opens" }, { value: "webhook", label: "A webhook is called" }]}
        />
        {sched && (
          <Fields>
            <Field label="Repeat">
              <Select
                className="st-wide"
                value={sched.every}
                aria-label="Repeat"
                onChange={(every: Schedule["every"]) => {
                  const at = "hour" in sched ? { hour: sched.hour, minute: sched.minute } : { hour: 9, minute: 0 };
                  setWhen({ kind: "schedule", schedule: every === "week" ? { every, day: 1, ...at } : every === "hour" ? { every, minute: 0 } : every === "minutes" ? { every, n: 30 } : every === "cron" ? { every, cron: cronOf(sched) } : { every, ...at } });
                }}
                options={[{ value: "weekdays", label: "Every weekday (Mon–Fri)" }, { value: "day", label: "Every day" }, { value: "week", label: "Once a week" }, { value: "hour", label: "Every hour" }, { value: "minutes", label: "Every few minutes" }, { value: "cron", label: "Custom (cron)" }]}
              />
            </Field>
            {sched.every === "week" && <Field label="On"><Select className="st-wide" value={sched.day} onChange={(day: number) => setWhen({ kind: "schedule", schedule: { ...sched, day } })} options={DAYS.map((d, i) => ({ value: i, label: d }))} aria-label="Day" /></Field>}
            {"hour" in sched && <Field label="At" hint="This device's local time."><Input type="time" className="st-wide mono" value={`${pad(sched.hour)}:${pad(sched.minute)}`} onChange={(e) => { const [h, m] = e.target.value.split(":").map(Number); if (Number.isFinite(h) && Number.isFinite(m)) setWhen({ kind: "schedule", schedule: { ...sched, hour: h, minute: m } }); }} aria-label="At" /></Field>}
            {sched.every === "hour" && <Field label="At minute"><InputNumber className="st-wide" min={0} max={59} value={sched.minute} onChange={(n) => setWhen({ kind: "schedule", schedule: { every: "hour", minute: Math.max(0, Math.min(59, Number(n) || 0)) } })} /></Field>}
            {sched.every === "minutes" && <Field label="Every"><InputNumber className="st-wide" min={5} max={720} step={5} value={sched.n} suffix="minutes" onChange={(n) => setWhen({ kind: "schedule", schedule: { every: "minutes", n: Math.max(5, Number(n) || 5) } })} /></Field>}
            {sched.every === "cron" && <Field label="Cron" hint="minute hour day month weekday, local time"><Input className="mono" value={sched.cron} onChange={(e) => setWhen({ kind: "schedule", schedule: { every: "cron", cron: e.target.value } })} placeholder="0 9 * * 1-5" spellCheck={false} /></Field>}
          </Fields>
        )}
        {when.kind === "pr" && (
          <Field label="Repository" hint="New pull requests are found with the gh sign-in on this device.">
            <ChoiceSelect conn={githubConnection(cfg)} what="repos" value={when.repo} onChange={(repo: string) => setWhen({ kind: "pr", repo })} label="Repository" placeholder="owner/repo" />
          </Field>
        )}
        {when.kind === "webhook" && (
          <Fields>
            <Field label="Webhook name"><Input className="mono" value={when.name} onChange={(e) => setWhen({ kind: "webhook", name: e.target.value.replace(/[^\w-]/g, "") })} spellCheck={false} /></Field>
            <Field label="Address to call" hint={cfg.serve.token ? "Add ?token=… (Settings → Remote access) when calling from outside your tailnet." : "Needs Remote access to be on. POST to it; the body fills {body}."}>
              <Input className="mono" readOnly value={hookUrl} suffix={<Button type="text" size="small" icon={<CopyOutlined />} aria-label="Copy the address" onClick={() => void navigator.clipboard.writeText(hookUrl).then(() => notify("Copied"), () => notify("Could not copy", "error"))} />} />
            </Field>
          </Fields>
        )}
      </FormGroup>
      <FormGroup title="Then">
        {act.kind === "raw" ? (
          <div className="st-note">This trigger runs its own command line (under Advanced). <a onClick={() => setAct({ kind: "send", to: "", text: "" })}>Use the pickers instead</a></div>
        ) : (
          <>
            <Segmented
              block
              value={act.kind}
              onChange={(v) => setAct(v === "task" ? { kind: "task", title: "", detail: "" } : v === "loop" ? { kind: "loop", a: "", b: "", goal: "" } : v === "team" ? { kind: "team", template: "" } : { kind: "send", to: "", text: "" })}
              options={[{ value: "send", label: "Tell an agent" }, { value: "task", label: "Add a task" }, { value: "loop", label: "Start a loop" }, { value: "team", label: "Start a team" }]}
            />
            <Fields>
              {act.kind === "send" && (
                <>
                  <Field label="Agent"><Select className="st-wide" showSearch value={act.to || undefined} options={agentOptions} onChange={(to: string) => setAct({ ...act, to })} placeholder="Pick an agent" aria-label="Agent" /></Field>
                  <Field label="Message" hint={fill ? `${fill} are filled in.` : undefined} span><Input.TextArea autoSize={{ minRows: 2, maxRows: 6 }} value={act.text} onChange={(e) => setAct({ ...act, text: e.target.value })} placeholder={when.kind === "pr" ? "Review PR #{number}: {title} {url}" : "Review every open PR and summarise"} aria-label="Message" /></Field>
                </>
              )}
              {act.kind === "task" && (
                <>
                  <Field label="Task" span><Input value={act.title} onChange={(e) => setAct({ ...act, title: e.target.value })} placeholder={when.kind === "pr" ? "Review PR #{number}" : "Weekly dependency update"} aria-label="Task" /></Field>
                  <Field label="Details" hint={fill ? `${fill} are filled in. An idle agent picks it up.` : "An idle agent picks it up."} span><Input.TextArea autoSize={{ minRows: 2, maxRows: 6 }} value={act.detail} onChange={(e) => setAct({ ...act, detail: e.target.value })} aria-label="Details" /></Field>
                </>
              )}
              {act.kind === "loop" && (
                <>
                  <Field label="Between"><Select className="st-wide" showSearch value={act.a || undefined} options={agentOptions} onChange={(a: string) => setAct({ ...act, a })} placeholder="First agent" aria-label="First agent" /></Field>
                  <Field label="And"><Select className="st-wide" showSearch value={act.b || undefined} options={agentOptions.filter((o) => o.value !== act.a)} onChange={(b: string) => setAct({ ...act, b })} placeholder="Second agent" aria-label="Second agent" /></Field>
                  <Field label="Goal" span><Input.TextArea autoSize={{ minRows: 2, maxRows: 6 }} value={act.goal} onChange={(e) => setAct({ ...act, goal: e.target.value })} aria-label="Goal" /></Field>
                </>
              )}
              {act.kind === "team" && <Field label="Team" span><Select className="st-wide" value={act.template || undefined} options={templates} onChange={(template: string) => setAct({ kind: "team", template })} placeholder="Pick a team" aria-label="Team" /></Field>}
            </Fields>
          </>
        )}
      </FormGroup>
      <div className="st-trigger-preview" aria-label="What this trigger does">
        {when.kind === "schedule" ? describeSchedule(when.schedule) : when.kind === "pr" ? `When a pull request opens in ${when.repo || "a repository"}` : `When ${hookUrl.replace(/^http:\/\/[^/]+/, "")} is called`}, {describeAction(act)}.
      </div>
      <ToggleField label="Enabled" hint="Off keeps the trigger without firing it." checked={draft.enabled} onChange={(v) => onChange({ enabled: v })} />
      <button type="button" className="st-advanced-toggle" onClick={() => setAdvanced((a) => !a)} aria-expanded={advanced}>{advanced ? "Hide advanced" : "Advanced: the lines as stored"}</button>
      {advanced && (
        <FormGroup title="Advanced">
          <Fields cols={1}>
            <Field label="When"><Input className="mono" value={draft.on} onChange={(e) => onChange({ on: e.target.value })} spellCheck={false} aria-label="When (raw)" /></Field>
            <Field label="Action" hint={<>Also <code>loop start a b "goal" --max 5</code>, <code>team start name ~/code/app</code>.</>}><Input className="mono" value={draft.action} onChange={(e) => onChange({ action: e.target.value })} spellCheck={false} aria-label="Action (raw)" /></Field>
          </Fields>
        </FormGroup>
      )}
    </Fields>
  );
}
