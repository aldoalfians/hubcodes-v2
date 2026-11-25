import { useEffect, useState } from "react";
import { Button, Input, Space, Tag, Typography } from "antd";
import { BranchesOutlined, CheckOutlined, ExperimentOutlined, MergeOutlined, PullRequestOutlined } from "@ant-design/icons";
import { backend } from "../lib/backend";
import type { Agent, DiffInfo, TestRun } from "../lib/types";
import { useStore } from "../store";
import { DiffView } from "./DiffView";

const { Text } = Typography;

/** The agent's branch, changed files and diff, with commit and PR. */
export function Changes({ agent }: { agent: Agent }) {
  const notify = useStore((s) => s.notify);
  const instruct = useStore((s) => s.instruct);
  const [info, setInfo] = useState<DiffInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState<"" | "commit" | "pr" | "test" | "merge">("");
  const [test, setTest] = useState<TestRun | null>(null);
  const [showTest, setShowTest] = useState(false);
  const allMerges = useStore((s) => s.merges);
  const merges = allMerges.filter((m) => m.agent === agent.name);
  const load = () => backend.agentDiff(agent.id).then((d) => { setInfo(d); setError(null); }).catch((e) => setError(String(e)));
  useEffect(() => {
    void load();
    const id = window.setInterval(() => void load(), 5000);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agent.id]);
  const commit = async () => {
    setBusy("commit");
    try {
      const sha = await backend.commitAgent(agent.id, message);
      notify(`Committed ${sha} on ${info?.changes.branch ?? "branch"}`);
      setMessage("");
      await load();
    } catch (e) {
      notify(`Commit failed: ${e}`, "error");
    } finally {
      setBusy("");
    }
  };
  const pr = async () => {
    setBusy("pr");
    try {
      const out = await backend.openPr(agent.id);
      notify(out ? `PR: ${out}` : "PR form opened in the browser");
    } catch (e) {
      notify(`Could not open a PR: ${e}`, "error");
    } finally {
      setBusy("");
    }
  };
  const runTests = async () => {
    setBusy("test");
    setShowTest(true);
    try {
      const r = await backend.runTests(agent.id);
      setTest(r);
      notify(r.ok ? `Tests passed: ${r.summary}` : `Tests failed: ${r.summary}`, r.ok ? "info" : "error");
    } catch (e) {
      notify(`Could not run tests: ${e}`, "error");
    } finally {
      setBusy("");
    }
  };
  const merge = async () => {
    setBusy("merge");
    try {
      const m = await backend.mergeEnqueue(agent.id, null);
      notify(`Queued: ${m.branch} → ${m.into}. Tests run first; conflicts come back as a task for ${agent.name}.`);
    } catch (e) {
      notify(`Could not queue the merge: ${e}`, "error");
    } finally {
      setBusy("");
    }
  };
  if (error) return <div className="changes"><Text type="secondary">{error}</Text></div>;
  if (!info) return <div className="changes"><Text type="secondary">Reading git…</Text></div>;
  const c = info.changes;
  const mergeColor = (s: string) => (s === "merged" ? "success" : s === "conflict" || s === "failed" || s === "tests_failed" ? "error" : "warning");
  return (
    <div className="changes">
      <div className="changes-head">
        <Tag icon={<BranchesOutlined />} variant="filled" className="mono">{c.branch}</Tag>
        <Text type="secondary">{c.files} changed · <span className="ins">+{c.insertions}</span> <span className="del">−{c.deletions}</span>{c.ahead ? ` · ${c.ahead} to push` : ""}{c.behind ? ` · ${c.behind} behind` : ""}</Text>
        <Text type="secondary" className="changes-dir mono">{info.dir}</Text>
      </div>
      {c.status.length > 0 && (
        <ul className="changes-files">
          {c.status.map((l) => <li key={l}><code>{l.slice(0, 2).trim() || "·"}</code> {l.slice(3)}</li>)}
        </ul>
      )}
      {merges.length > 0 && (
        <ul className="merge-list">
          {merges.slice(0, 3).map((m) => (
            <li key={m.id} className="merge-row" title={m.log}>
              <Tag color={mergeColor(m.status)} variant="filled">{m.status.replace("_", " ")}</Tag>
              <span className="mono">{m.branch} → {m.into}</span>
              <Text type="secondary">{m.log.split("\n").filter(Boolean).pop()?.slice(0, 90)}</Text>
            </li>
          ))}
        </ul>
      )}
      {showTest && (
        <div className="test-box">
          <div className="row">
            <Text strong>{busy === "test" ? "Running tests…" : test ? (test.ok ? `✓ ${test.summary}` : `✗ ${test.summary}`) : ""}</Text>
            {test && <Text type="secondary"> · {test.command} · {test.secs} s</Text>}
            <Button type="text" size="small" style={{ marginLeft: "auto" }} onClick={() => setShowTest(false)}>Hide</Button>
          </div>
          {test && <pre className="test-tail">{test.tail}</pre>}
        </div>
      )}
      <DiffView diff={info.diff} branch={c.branch} onSend={(text) => void instruct(agent.id, text).then(() => notify(`Review sent to ${agent.name}`)).catch((e) => notify(String(e), "error"))} />
      <div className="changes-actions">
        <Input value={message} onChange={(e) => setMessage(e.target.value)} placeholder="Commit message" spellCheck={false} disabled={c.files === 0} onPressEnter={() => { if (message.trim()) void commit(); }} onKeyDown={(e) => e.stopPropagation()} />
        <Space size={4}>
          <Button type="primary" icon={<CheckOutlined />} disabled={c.files === 0 || !message.trim() || busy !== ""} loading={busy === "commit"} onClick={() => void commit()}>Commit all</Button>
          <Button icon={<PullRequestOutlined />} disabled={busy !== ""} loading={busy === "pr"} onClick={() => void pr()} title="git push, then gh pr create --web">Open PR</Button>
          <Button icon={<ExperimentOutlined />} disabled={busy !== ""} loading={busy === "test"} onClick={() => void runTests()} title="Run the repo's tests in this folder ([test] command or detected)">Run tests</Button>
          <Button icon={<MergeOutlined />} disabled={busy !== "" || c.files > 0} loading={busy === "merge"} onClick={() => void merge()} title={c.files > 0 ? "Commit first" : "Queue a merge into the repo's target branch; tests run in a separate checkout"}>Merge</Button>
        </Space>
      </div>
    </div>
  );
}
