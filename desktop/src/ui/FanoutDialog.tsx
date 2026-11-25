import { useState } from "react";
import { Alert, Button, Form, Input, Select, Space } from "antd";
import { useStore } from "../store";
import { backend } from "../lib/backend";
import { Dialog } from "./Dialog";

/** One prompt to several agents; a collector gets every answer plus a wrap-up. */
export function FanoutDialog() {
  const agents = useStore((s) => s.agents);
  const preset = useStore((s) => s.fanoutTargets);
  const setDialog = useStore((s) => s.setDialog);
  const notify = useStore((s) => s.notify);
  const logHuman = useStore((s) => s.logHuman);
  const [targets, setTargets] = useState<string[]>(preset.length ? preset : agents.slice(0, 2).map((a) => a.name));
  const [collector, setCollector] = useState<string>("");
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const f = await backend.fanoutStart(targets, collector || null, prompt.trim());
      logHuman(targets.join(", "), prompt.trim());
      notify(`Fan-out #${f.id} to ${f.expected} agents${collector ? `, answers go to ${collector}` : ""}`);
      setDialog("none");
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };
  const label = (name: string) => { const a = agents.find((x) => x.name === name); return a?.role ? `${name} · ${a.role}` : name; };
  return (
    <Dialog title="Fan out a prompt" onClose={() => setDialog("none")} width={580}>
      <Form layout="vertical" onFinish={() => void submit()}>
        <Form.Item label="Send to">
          <Select mode="multiple" value={targets} onChange={setTargets} options={agents.filter((a) => a.name !== collector).map((a) => ({ value: a.name, label: label(a.name) }))} placeholder="Pick the agents" />
        </Form.Item>
        <Form.Item label="Collector" extra="Each answer is sent to the collector; when all are in it gets a wrap-up and is asked to pick or combine.">
          <Select value={collector} onChange={(v) => { setCollector(v); setTargets((t) => t.filter((x) => x !== v)); }} options={[{ value: "", label: "Nobody: answers come back to you" }, ...agents.map((a) => ({ value: a.name, label: label(a.name) }))]} />
        </Form.Item>
        <Form.Item label="Prompt">
          <Input.TextArea value={prompt} onChange={(e) => setPrompt(e.target.value)} rows={4} autoFocus placeholder="Same question for everyone: a design, a name, a review of the same diff…" />
        </Form.Item>
        {error && <Alert type="error" showIcon title={error} style={{ marginBottom: 12 }} />}
        <div className="dialog-actions">
          <Space>
            <Button onClick={() => setDialog("none")}>Cancel</Button>
            <Button type="primary" htmlType="submit" loading={busy} disabled={targets.length === 0 || !prompt.trim()}>Send to {targets.length}</Button>
          </Space>
        </div>
      </Form>
    </Dialog>
  );
}
