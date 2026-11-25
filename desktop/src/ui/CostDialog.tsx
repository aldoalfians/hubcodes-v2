import { useEffect, useMemo, useState } from "react";
import { Segmented, Table, Tag, Tooltip, Typography } from "antd";
import { useStore } from "../store";
import { backend } from "../lib/backend";
import { costOf, fmtTok, fmtUsd } from "../lib/pricing";
import { Dialog } from "./Dialog";
import type { UsageRow } from "../lib/types";

const { Text } = Typography;

type Agg = { tokens: number; input: number; output: number; cached: number; cost: number; known: boolean; turns: number };
const empty = (): Agg => ({ tokens: 0, input: 0, output: 0, cached: 0, cost: 0, known: true, turns: 0 });
function add(a: Agg, r: UsageRow) {
  a.input += r.inputTokens;
  a.output += r.outputTokens;
  a.cached += r.cacheReadTokens;
  a.tokens += r.inputTokens + r.outputTokens;
  a.turns += 1;
  const c = costOf(r.model, r.inputTokens, r.outputTokens, r.cacheReadTokens, r.cacheWriteTokens);
  if (c === null) a.known = false;
  else a.cost += c;
}
// Local calendar day, so "today" means the user's today.
const dayKey = (at: number) => {
  const d = new Date(at * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
const dayDate = (k: string) => new Date(`${k}T00:00:00`);
const dayLong = (k: string) => dayDate(k).toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });
const dayWeekday = (k: string) => dayDate(k).toLocaleDateString(undefined, { weekday: "short" }).replace(/\.$/, "");
const dayNum = (k: string) => String(dayDate(k).getDate());

/** Tokens and estimated cost per agent, per day, per playbook run. */
export function CostDialog() {
  const setDialog = useStore((s) => s.setDialog);
  const runs = useStore((s) => s.runs);
  const [days, setDays] = useState<7 | 30 | 90>(7);
  const [metric, setMetric] = useState<"tokens" | "cost">("cost");
  const [rows, setRows] = useState<UsageRow[]>([]);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    setLoading(true);
    backend.usageLog(days).then(setRows).catch(() => setRows([])).finally(() => setLoading(false));
  }, [days]);

  const todayKey = dayKey(Date.now() / 1000);
  const total = useMemo(() => { const a = empty(); rows.forEach((r) => add(a, r)); return a; }, [rows]);
  const today = useMemo(() => { const a = empty(); rows.filter((r) => dayKey(r.at) === todayKey).forEach((r) => add(a, r)); return a; }, [rows, todayKey]);
  const byAgent = useMemo(() => {
    const m = new Map<string, Agg & { model: string }>();
    for (const r of rows) {
      const a = m.get(r.agent) ?? { ...empty(), model: r.model };
      add(a, r);
      if (r.model) a.model = r.model;
      m.set(r.agent, a);
    }
    return [...m.entries()].map(([agent, a]) => ({ agent, ...a })).sort((x, y) => y.cost - x.cost || y.tokens - x.tokens);
  }, [rows]);
  const byDay = useMemo(() => {
    const m = new Map<string, Agg>();
    const now = Date.now() / 1000;
    for (let d = days - 1; d >= 0; d--) m.set(dayKey(now - d * 86400), empty());
    for (const r of rows) { const k = dayKey(r.at); const a = m.get(k) ?? empty(); add(a, r); m.set(k, a); }
    return [...m.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [rows, days]);
  const byRun = useMemo(() => {
    const m = new Map<number, Agg>();
    for (const r of rows.filter((r) => r.run > 0)) { const a = m.get(r.run) ?? empty(); add(a, r); m.set(r.run, a); }
    return [...m.entries()].map(([run, a]) => ({ run, ...a, playbook: runs.find((x) => x.id === run)?.playbook ?? "", goal: runs.find((x) => x.id === run)?.goal ?? "" })).sort((x, y) => y.run - x.run);
  }, [rows, runs]);
  const value = (a: Agg) => (metric === "cost" ? a.cost : a.tokens);
  const maxDay = Math.max(metric === "cost" ? 0.000001 : 1, ...byDay.map(([, a]) => value(a)));
  const unknown = byAgent.filter((a) => !a.known).map((a) => a.agent);
  const activeDays = byDay.filter(([, a]) => a.turns > 0).length;
  const perDay = activeDays ? total.cost / activeDays : 0;
  // Label every bar at 7 days, every Monday at 30, the 1st of each month at 90.
  const labelFor = (k: string, i: number) => {
    if (days === 7) return dayWeekday(k);
    const d = dayDate(k);
    if (days === 30) return d.getDay() === 1 || i === 0 ? dayNum(k) : "";
    return d.getDate() === 1 || i === 0 ? d.toLocaleDateString(undefined, { month: "short" }) : "";
  };

  return (
    <Dialog
      title="Cost"
      onClose={() => setDialog("none")}
      width={880}
      className="cost-dialog"
      extra={<Segmented size="small" value={days} options={[{ value: 7, label: "7 days" }, { value: 30, label: "30 days" }, { value: 90, label: "90 days" }]} onChange={(v) => setDays(v as 7 | 30 | 90)} />}
    >
      <div className="cost-tiles">
        <Tile label="Today" value={fmtUsd(today.cost)} sub={today.turns ? `${fmtTok(today.tokens)} tokens in ${today.turns} turn${today.turns === 1 ? "" : "s"}` : "nothing yet today"} accent />
        <Tile label={`Last ${days} days`} value={fmtUsd(total.cost)} sub={total.turns ? `${fmtTok(total.tokens)} tokens in ${total.turns} turns` : "no usage recorded"} />
        <Tile label="Per active day" value={fmtUsd(perDay)} sub={activeDays ? `across ${activeDays} day${activeDays === 1 ? "" : "s"} with usage` : "no active day yet"} />
        <Tile label="Cached reads" value={fmtTok(total.cached)} sub={total.cached ? "billed at a tenth of input" : "no cache hits yet"} />
      </div>

      <section className="cost-section">
        <div className="cost-section-head">
          <Text strong>Per day</Text>
          <Text type="secondary">{metric === "cost" ? "estimated cost" : "tokens in + out"}</Text>
          <span className="grow" />
          <Segmented size="small" value={metric} options={[{ value: "cost", label: "Cost" }, { value: "tokens", label: "Tokens" }]} onChange={(v) => setMetric(v as "cost" | "tokens")} />
        </div>
        <div className="cost-chart">
          <div className="cost-axis"><span>{total.turns === 0 ? "" : metric === "cost" ? fmtUsd(maxDay) : fmtTok(maxDay)}</span><span>0</span></div>
          <div className="cost-bars" role="img" aria-label={metric === "cost" ? "Cost per day" : "Tokens per day"}>
            {byDay.map(([k, a], i) => {
              const v = value(a);
              const h = v > 0 ? Math.max(3, (v / maxDay) * 100) : 0;
              const isToday = k === todayKey;
              return (
                <Tooltip key={k} title={<span className="cost-tip"><b>{dayLong(k)}</b><br />{fmtUsd(a.cost)} · {fmtTok(a.tokens)} tokens · {a.turns} turn{a.turns === 1 ? "" : "s"}</span>}>
                  <div className={`cost-bar-col${isToday ? " is-today" : ""}${v === 0 ? " is-empty" : ""}`}>
                    <div className="cost-bar-track"><div className="cost-bar" style={{ height: `${h}%` }} /></div>
                    <span className="cost-bar-label">{labelFor(k, i)}</span>
                  </div>
                </Tooltip>
              );
            })}
          </div>
        </div>
      </section>

      <section className="cost-section">
        <div className="cost-section-head">
          <Text strong>Per agent</Text>
          {unknown.length > 0 && <Tooltip title={`No list price for: ${unknown.join(", ")}. Their cost shows as a lower bound.`}><Tag color="warning" variant="filled" className="cost-warn">{unknown.length} unpriced</Tag></Tooltip>}
        </div>
        <Table
          size="small"
          pagination={false}
          loading={loading}
          rowKey="agent"
          dataSource={byAgent}
          className="cost-table"
          locale={{ emptyText: <div className="cost-empty"><Text strong>No usage recorded yet</Text><Text type="secondary">Claude Code reports tokens through its hooks after each turn. Other providers appear when they expose usage.</Text></div> }}
          columns={[
            { title: "Agent", dataIndex: "agent", render: (v: string) => <span className="mono">{v}</span> },
            { title: "Model", dataIndex: "model", render: (v: string) => (v ? <Tag variant="filled" className="mono cost-model">{v}</Tag> : <Text type="secondary">unknown</Text>) },
            { title: "Turns", dataIndex: "turns", align: "right", width: 72 },
            { title: "In", dataIndex: "input", align: "right", width: 84, render: (v: number) => <span className="num">{fmtTok(v)}</span> },
            { title: "Out", dataIndex: "output", align: "right", width: 84, render: (v: number) => <span className="num">{fmtTok(v)}</span> },
            { title: "Cached", dataIndex: "cached", align: "right", width: 84, render: (v: number) => <span className="num">{fmtTok(v)}</span> },
            { title: "Cost", dataIndex: "cost", align: "right", width: 96, render: (v: number, r) => <Text strong={r.known} type={r.known ? undefined : "secondary"} className="num">{r.known ? fmtUsd(v) : `≥ ${fmtUsd(v)}`}</Text> },
          ]}
        />
      </section>

      {byRun.length > 0 && (
        <section className="cost-section">
          <div className="cost-section-head"><Text strong>Per playbook run</Text></div>
          <Table
            size="small"
            pagination={false}
            rowKey="run"
            dataSource={byRun}
            className="cost-table"
            columns={[
              { title: "Run", dataIndex: "run", width: 140, render: (v: number, r) => <span className="mono">#{v}{r.playbook ? ` ${r.playbook}` : ""}</span> },
              { title: "Goal", dataIndex: "goal", ellipsis: true },
              { title: "Turns", dataIndex: "turns", align: "right", width: 72 },
              { title: "Tokens", dataIndex: "tokens", align: "right", width: 96, render: (v: number) => <span className="num">{fmtTok(v)}</span> },
              { title: "Cost", dataIndex: "cost", align: "right", width: 96, render: (v: number) => <span className="num">{fmtUsd(v)}</span> },
            ]}
          />
        </section>
      )}
      <Text type="secondary" className="cost-foot">Estimates from list prices per model family. The provider's own dashboard is the bill. Budgets per agent and per loop live in Settings → Agents.</Text>
    </Dialog>
  );
}

function Tile({ label, value, sub, accent }: { label: string; value: string; sub: string; accent?: boolean }) {
  return (
    <div className={`cost-tile${accent ? " is-accent" : ""}`}>
      <Text type="secondary" className="cost-tile-label">{label}</Text>
      <div className="cost-tile-value">{value}</div>
      <Text type="secondary" className="cost-tile-sub">{sub}</Text>
    </div>
  );
}
