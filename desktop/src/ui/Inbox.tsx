import { useEffect } from "react";
import { Button, Tooltip, Typography } from "antd";
import { CheckOutlined, CloseOutlined, ExportOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { Dialog } from "./Dialog";
import { backend } from "../lib/backend";
import { PullRequests, usePullRequests } from "./PullRequests";

const { Text } = Typography;

/** Every agent that needs a human, on every device, answered from one place. */
export function Inbox() {
  const agents = useStore((s) => s.agents);
  const setDialog = useStore((s) => s.setDialog);
  const answer = useStore((s) => s.answer);
  const open = useStore((s) => s.open);
  const waiting = agents.filter((a) => a.status === "waiting");
  const pending = useStore((s) => s.pending);
  const notify = useStore((s) => s.notify);
  const decide = (id: number, approve: boolean) => backend.pendingDecide(id, approve).then((r) => notify(r)).catch((e) => notify(String(e), "error"));
  const prs = usePullRequests();
  const prNeeds = prs.rows.filter((r) => r.state === "asked" || r.state === "paused" || r.state === "ready").length;
  const count = waiting.length + pending.length + prNeeds;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.closest?.("input, textarea")) return;
      const first = waiting[0];
      if (!first) return;
      if (e.key === "y" || e.key === "a") { e.preventDefault(); void answer(first.id, true); }
      if (e.key === "n" || e.key === "d") { e.preventDefault(); void answer(first.id, false); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [waiting, answer]);
  /** Where a parked message came from: another device, or the shared page. */
  const origin = (by: string) => (by.includes("/") ? `from ${by.split("/")[0]}` : "via the shared page");
  const target = (to: string) => (to.startsWith("room:") ? `room ${to.slice(5)}` : to);
  return (
    <Dialog title={count ? `${count} need${count === 1 ? "s" : ""} you` : "Inbox"} onClose={() => setDialog("none")} width={620} className="inbox-dialog">
      {count === 0 && prs.rows.length === 0 && (
        <div className="ib-empty">
          <Text strong>All clear</Text>
          <Text type="secondary">Permission prompts, questions, messages waiting for your approval and the pull requests your agents are handling land here, from every device.</Text>
        </div>
      )}

      {waiting.length > 0 && (
        <section className="ib-group" aria-label="Agents waiting for an answer">
          <h3 className="ib-group-title">Waiting for an answer <span>{waiting.length}</span></h3>
          {waiting.map((a, i) => (
            <article key={a.id} className="ib-card">
              <header className="ib-head">
                <span className="ib-dot" aria-hidden />
                <span className="ib-name mono">{a.peer ? a.name.slice(a.peer.length + 1) : a.name}</span>
                <span className="ib-meta">{[a.role, a.provider, a.peer && `on ${a.peer}`].filter(Boolean).join(" · ")}</span>
                <Tooltip title="Open its terminal when the question needs more than yes or no">
                  <Button size="small" type="text" icon={<ExportOutlined />} className="ib-open" onClick={() => { setDialog("none"); open(a.id); }}>Open</Button>
                </Tooltip>
              </header>
              <pre className="ib-ask">{a.lastLine || "waiting for input"}</pre>
              <footer className="ib-actions">
                <Tooltip title="Sends the provider's yes key to that terminal">
                  <Button type="primary" size="small" icon={<CheckOutlined />} onClick={() => void answer(a.id, true)}>Approve{i === 0 && <kbd className="ib-key">y</kbd>}</Button>
                </Tooltip>
                <Tooltip title="Sends the provider's no key to that terminal">
                  <Button size="small" type="text" icon={<CloseOutlined />} onClick={() => void answer(a.id, false)}>Deny{i === 0 && <kbd className="ib-key">n</kbd>}</Button>
                </Tooltip>
              </footer>
            </article>
          ))}
        </section>
      )}

      {pending.length > 0 && (
        <section className="ib-group" aria-label="Messages waiting for your approval">
          <h3 className="ib-group-title">Messages waiting for your approval <span>{pending.length}</span></h3>
          {pending.map((p) => (
            <article key={p.id} className="ib-card is-message">
              <header className="ib-head">
                <span className="ib-name">{p.by}</span>
                <span className="ib-to">to <b className="mono">{target(p.to)}</b></span>
                <span className="ib-meta">{origin(p.by)}</span>
              </header>
              <p className="ib-text">{p.text}</p>
              <footer className="ib-actions">
                <Button type="primary" size="small" icon={<CheckOutlined />} onClick={() => void decide(p.id, true)}>{p.to.startsWith("room:") ? "Approve and post" : "Approve and send"}</Button>
                <Button size="small" type="text" icon={<CloseOutlined />} onClick={() => void decide(p.id, false)}>Deny</Button>
              </footer>
            </article>
          ))}
        </section>
      )}

      <PullRequests rows={prs.rows} reload={prs.reload} />
    </Dialog>
  );
}
