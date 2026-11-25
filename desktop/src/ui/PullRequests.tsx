import { useCallback, useEffect, useState } from "react";
import { Button, Tooltip } from "antd";
import { ExportOutlined, RedoOutlined, CaretRightOutlined, DeleteOutlined, CheckOutlined, StopOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { backend } from "../lib/backend";
import type { Task } from "../lib/types";

/** `reviews:owner/repo#12`: a review request or feedback round from GitHub. */
const PR_KEY = /^[^:]+:[\w.-]+\/[\w.-]+#\d+$/;

export type PrState = "asked" | "paused" | "ready" | "working" | "queued" | "done";

export interface PrRow {
  key: string;
  kind: "review" | "feedback";
  task: Task;
  state: PrState;
  rounds: number;
}

const asked = (t: Task) => t.synced === "asked" || /^\s*NEEDS HUMAN/i.test(t.result);
/** The reviewer's last line: `VERDICT: approve` (same rule as the core's `verdict_approve`). */
export const verdictApprove = (result: string) => {
  const last = result.split("\n").map((l) => l.trim()).filter(Boolean).pop() ?? "";
  return /^verdict:\s*approve$/i.test(last.replace(/[*`]/g, ""));
};
/** The reviewer's count line, e.g. `2 🔴, 4 🟡, 1 needs_validation`: what the review found, by severity. */
export const findingCounts = (result: string) => {
  const n = (re: RegExp) => Number(re.exec(result)?.[1] ?? 0);
  const red = n(/(\d+)\s*🔴/u);
  const yellow = n(/(\d+)\s*🟡/u);
  const check = n(/(\d+)\s*needs[_ ]validation/i);
  return { red, yellow, check, any: red + yellow + check > 0 };
};
const isReview = (t: Task) => t.title.startsWith("Review ") || t.title.startsWith("Re-review ");
const ready = (t: Task) => isReview(t) && t.status === "done" && ["", "started", "done"].includes(t.synced) && verdictApprove(t.result);

/**
 * One row per pull request: its newest task decides what it is waiting for.
 * Rows whose pull request was merged or closed (`released`) are left out.
 */
export function prRows(tasks: Task[]): PrRow[] {
  const by = new Map<string, Task[]>();
  for (const t of tasks) if (PR_KEY.test(t.external)) by.set(t.external, [...(by.get(t.external) ?? []), t]);
  const rows: PrRow[] = [];
  for (const [ext, list] of by) {
    list.sort((a, b) => a.at - b.at || a.id - b.id);
    const t = list[list.length - 1];
    if (list.some((x) => x.synced === "ignored") || (t.status === "done" && t.synced === "released")) continue;
    const state: PrState = t.status === "done" ? (t.synced === "stalled" ? "paused" : ready(t) ? "ready" : asked(t) ? "asked" : "done") : t.status === "todo" && !t.owner ? "queued" : "working";
    rows.push({ key: ext.slice(ext.indexOf(":") + 1), kind: t.title.startsWith("Feedback round") ? "feedback" : "review", task: t, state, rounds: Number(/^Feedback round (\d+)/.exec(t.title)?.[1] ?? list.length) });
  }
  const order: PrState[] = ["asked", "paused", "ready", "working", "queued", "done"];
  return rows.sort((a, b) => order.indexOf(a.state) - order.indexOf(b.state) || b.task.updated - a.task.updated);
}

const LABEL: Record<PrState, string> = {
  asked: "Needs you",
  paused: "Paused",
  ready: "Ready to approve",
  working: "In progress",
  queued: "Waiting",
  done: "Done",
};

/** Review requests and feedback on your pull requests, with what each is waiting for. */
export function usePullRequests() {
  const live = useStore((s) => s.tasks);
  const [rows, setRows] = useState<PrRow[]>([]);
  const load = useCallback(() => backend.tasksAll(true).then((all) => setRows(prRows(all))).catch(() => {}), []);
  useEffect(() => { void load(); }, [load, live]);
  useEffect(() => { const id = window.setInterval(() => void load(), 15000); return () => window.clearInterval(id); }, [load]);
  return { rows, reload: load };
}

function Findings({ result }: { result: string }) {
  const f = findingCounts(result);
  if (!f.any) return verdictApprove(result) ? <span className="pr-findings is-clean" title="The reviewer found nothing blocking">no findings</span> : null;
  const parts = [f.red && `${f.red} 🔴`, f.yellow && `${f.yellow} 🟡`, f.check && `${f.check} to check`].filter(Boolean).join(" ");
  return <span className={`pr-findings${f.red ? " is-red" : ""}`} title="🔴 blocks merging · 🟡 should be fixed · to check: needs_validation">{parts}</span>;
}

export function PullRequests({ rows, reload }: { rows: PrRow[]; reload: () => void }) {
  const agents = useStore((s) => s.agents);
  const open = useStore((s) => s.open);
  const setDialog = useStore((s) => s.setDialog);
  const notify = useStore((s) => s.notify);
  if (!rows.length) return null;
  const act = (p: Promise<unknown>, done: string) => p.then(() => { notify(done); reload(); }).catch((e) => notify(String(e), "error"));
  const agentOf = (name: string) => agents.find((a) => a.name === name);
  const what = (r: PrRow) => {
    const t = r.task;
    if (r.state === "asked") return t.result.replace(/^\s*NEEDS HUMAN:?\s*/i, "");
    if (r.state === "paused") return "Two rounds in a row ended without a commit: the reviewer and the agent disagree. A new commit resumes it, or let it try once more.";
    if (r.state === "working") return `${t.owner || "An agent"} is on it.`;
    if (r.state === "queued") return `Waiting for a free ${t.role || "agent"}.`;
    if (r.state === "ready") return t.result.split("\n").filter((l) => !/^\W*verdict:/i.test(l)).join("\n").trim() || "Nothing blocking.";
    if (t.synced === "approved") return "You approved it on GitHub.";
    return t.result || "Finished.";
  };
  return (
    <section className="ib-group" aria-label="Pull requests">
      <h3 className="ib-group-title">Pull requests <span>{rows.length}</span></h3>
      {rows.map((r) => {
        const t = r.task;
        const agent = t.owner ? agentOf(t.owner) : undefined;
        return (
          <article key={r.key} className={`ib-card pr-card is-${r.state}`}>
            <header className="ib-head">
              <span className={`pr-state is-${r.state}`}>{LABEL[r.state]}</span>
              <span className="ib-name mono">{r.key}</span>
              <span className="ib-meta">{r.kind === "feedback" ? `feedback, round ${r.rounds}` : t.title.startsWith("Re-review") ? "review again" : "review"}</span>
              {r.kind === "review" && t.status === "done" && <Findings result={t.result} />}
              {t.externalUrl && (
                <Tooltip title="Open on GitHub">
                  <Button size="small" type="text" icon={<ExportOutlined />} className="ib-open" aria-label={`Open ${r.key} on GitHub`} onClick={() => void backend.openPath(t.externalUrl)} />
                </Tooltip>
              )}
            </header>
            <p className="ib-text pr-what">{what(r)}</p>
            {r.state !== "working" && (
              <footer className="ib-actions">
                {r.state === "ready" && (
                  <Tooltip title="Approves on GitHub as the account signed in to gh on this device. The agent only recommended it.">
                    <Button size="small" type="primary" icon={<CheckOutlined />} onClick={() => void act(backend.prApprove(t.id), `${r.key}: approved on GitHub`)}>Approve on GitHub</Button>
                  </Tooltip>
                )}
                {r.state === "paused" && <Button size="small" type="primary" icon={<CaretRightOutlined />} onClick={() => void act(backend.taskResume(t.id), `${r.key}: one more round`)}>One more round</Button>}
                {(r.state === "asked" || r.state === "done" || r.state === "ready") && <Button size="small" type="text" icon={<RedoOutlined />} onClick={() => void act(backend.taskSet(t.id, "todo"), `${r.key}: queued again`)}>Run again</Button>}
                {r.state === "queued" && <Button size="small" type="text" icon={<DeleteOutlined />} onClick={() => void act(backend.taskRemove(t.id), `${r.key}: removed from the queue`)}>Skip</Button>}
                <Tooltip title="No more reviews or feedback rounds for this pull request">
                  <Button size="small" type="text" icon={<StopOutlined />} onClick={() => void act(backend.prIgnore(t.id), `${r.key}: ignored`)}>Ignore PR</Button>
                </Tooltip>
              </footer>
            )}
            {agent && r.state === "working" && (
              <footer className="ib-actions">
                <Button size="small" type="text" onClick={() => { setDialog("none"); open(agent.id); }}>Open {agent.name}</Button>
              </footer>
            )}
          </article>
        );
      })}
    </section>
  );
}
