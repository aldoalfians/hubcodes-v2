import { useEffect, useMemo, useState } from "react";
import { Button, Input, Segmented, Tag, Tooltip, Typography } from "antd";
import { CloseOutlined, SearchOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { backend } from "../lib/backend";
import type { HistoryRow } from "../lib/types";

const { Text } = Typography;

function age(at: number) {
  const s = Math.max(0, Math.round(Date.now() / 1000 - at));
  if (s < 60) return "now";
  if (s < 3600) return `${Math.round(s / 60)}m`;
  if (s < 86400) return `${Math.round(s / 3600)}h`;
  return `${Math.round(s / 86400)}d`;
}

const dayOf = (at: number) => new Date(at * 1000).toDateString();
const dayTitle = (at: number) => {
  const d = new Date(at * 1000);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return "Today";
  const y = new Date(today);
  y.setDate(today.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });
};

type Filter = "all" | "you" | "agents" | "queued";

/** The session transcript (SQLite), live-updated, searchable, grouped by day. */
export function Activity() {
  const lastSeq = useStore((s) => s.lastSeq);
  const toggleActivity = useStore((s) => s.toggleActivity);
  const agents = useStore((s) => s.agents);
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [rows, setRows] = useState<HistoryRow[]>([]);
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setTick((t) => t + 1), 30000);
    return () => window.clearInterval(id);
  }, []);
  useEffect(() => {
    let alive = true;
    backend
      .history(300, q)
      .then((r) => alive && setRows(r))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [q, lastSeq]);

  const shown = useMemo(() => {
    const list = rows.filter((m) => {
      if (filter === "you") return m.from.startsWith("human") || m.to.startsWith("human");
      if (filter === "agents") return !m.from.startsWith("human") && !m.to.startsWith("human");
      if (filter === "queued") return m.kind === "queued" || m.kind === "failed";
      return true;
    });
    // Newest first, grouped by calendar day.
    const groups: { day: string; title: string; items: HistoryRow[] }[] = [];
    for (const m of list) {
      const day = dayOf(m.at);
      const g = groups[groups.length - 1];
      if (g && g.day === day) g.items.push(m);
      else groups.push({ day, title: dayTitle(m.at), items: [m] });
    }
    return groups;
  }, [rows, filter]);
  const names = new Set(agents.map((a) => a.name));
  const who = (s: string) => {
    const name = s.replace(/^human:?/, "") || "you";
    const isYou = s.startsWith("human");
    const live = names.has(name);
    return (
      <button type="button" className={`act-chip${isYou ? " is-you" : ""}${live ? " is-live" : ""}`} disabled={!live} onClick={() => { const a = agents.find((x) => x.name === name); if (a) window.dispatchEvent(new CustomEvent("hubcodes:focus-agent", { detail: a.id })); }} title={live ? `Show ${name}` : undefined}>
        {isYou ? "you" : name}
      </button>
    );
  };

  return (
    <aside className="activity" aria-label="Activity">
      <header className="activity-head">
        <Typography.Title level={5} style={{ margin: 0 }}>Activity</Typography.Title>
        <Text type="secondary" className="activity-count">{rows.length ? `${rows.length} message${rows.length === 1 ? "" : "s"}` : ""}</Text>
        <Button type="text" size="small" icon={<CloseOutlined />} aria-label="Close activity" title="Close · ⌘J" onClick={toggleActivity} />
      </header>
      <div className="activity-tools">
        <Input size="small" className="activity-search" prefix={<SearchOutlined />} allowClear value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search" spellCheck={false} aria-label="Search activity" />
        <Segmented size="small" value={filter} onChange={(v) => setFilter(v as Filter)} options={[{ value: "all", label: "All" }, { value: "you", label: "You" }, { value: "agents", label: "Agents" }, { value: "queued", label: "Queued" }]} />
      </div>
      {shown.length === 0 ? (
        <div className="activity-empty">
          <Text strong>{q ? `Nothing matches “${q}”` : filter !== "all" ? "Nothing here yet" : "No messages yet"}</Text>
          <Text type="secondary">{q || filter !== "all" ? "Try another word or filter." : "Everything you send to an agent, and everything agents send each other, lands here."}</Text>
        </div>
      ) : (
        <div className="activity-list">
          {shown.map((g) => (
            <section key={g.day} className="act-day">
              <div className="act-day-title"><span>{g.title}</span><span className="act-day-line" /></div>
              <ol>
                {g.items.map((m) => (
                  <li key={m.seq} className={`act kind-${m.kind}`}>
                    <Tooltip title={new Date(m.at * 1000).toLocaleString()} placement="left"><span className="act-age">{age(m.at)}</span></Tooltip>
                    <div className="act-main">
                      <div className="act-head">
                        {who(m.from)}
                        <span className="act-arrow" aria-hidden>›</span>
                        {who(m.to)}
                        {m.kind !== "message" && (
                          <Tag variant="filled" className="act-kind" color={m.kind === "queued" ? "processing" : m.kind === "failed" ? "error" : undefined} title={m.kind === "queued" ? "Waiting for the agent to finish its turn" : m.kind === "failed" ? "Could not be delivered; the agent is gone" : undefined}>{m.kind}</Tag>
                        )}
                      </div>
                      <div className="act-text">{m.text}</div>
                    </div>
                  </li>
                ))}
              </ol>
            </section>
          ))}
        </div>
      )}
    </aside>
  );
}
