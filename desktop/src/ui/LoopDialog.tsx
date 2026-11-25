import { useEffect, useState } from "react";
import { Alert, Button, Form, Input, InputNumber, Segmented, Space, Typography } from "antd";
import { useStore } from "../store";
import { pairKey } from "../lib/types";
import { Dialog } from "./Dialog";
import { backend } from "../lib/backend";

/**
 * Start a loop between two linked agents: the goal goes to the first one,
 * each is told to hand its result to the other, and hubcodes counts rounds.
 */
export function LoopDialog() {
  const pair = useStore((s) => s.loopPair);
  const loops = useStore((s) => s.loops);
  const setDialog = useStore((s) => s.setDialog);
  const startLoop = useStore((s) => s.startLoop);
  const [first, setFirst] = useState(0);
  const existing = pair ? loops[pairKey(pair[0], pair[1])] : undefined;
  const [goal, setGoal] = useState(existing?.goal ?? "");
  const [max, setMax] = useState(existing?.max ?? 20);
  const [lastLimit, setLastLimit] = useState(max > 0 ? max : 20);
  // A new loop starts from the limit in Settings → Agents (0 = no limit).
  useEffect(() => {
    if (existing) return;
    backend.getConfig().then((c) => { setMax(c.maxRounds); if (c.maxRounds > 0) setLastLimit(c.maxRounds); }).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!pair) return null;
  const a = pair[first];
  const b = pair[1 - first];
  const close = () => setDialog("none");
  const submit = async () => {
    if (!goal.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await startLoop(a, b, goal.trim(), max);
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };
  return (
    <Dialog title={`Loop ${pair[0]} ↔ ${pair[1]}`} onClose={close} width={540}>
      <Form layout="vertical" onFinish={() => void submit()}>
        <Form.Item label="Starts with" extra={<>{a} gets the goal and reports to {b}; {b} answers {a}; and so on.</>}>
          <Segmented value={first} options={pair.map((n, i) => ({ value: i, label: n }))} onChange={(v) => setFirst(Number(v))} />
        </Form.Item>
        <Form.Item label="Goal">
          <Input.TextArea value={goal} onChange={(e) => setGoal(e.target.value)} rows={4} autoFocus placeholder="What the two of them should get done. Be concrete about what finished looks like." />
        </Form.Item>
        <Form.Item label="Rounds" extra={max > 0 ? "One round is one message along the loop. It pauses at the limit and asks you; either agent ends it early by answering DONE." : "Runs until an agent answers DONE, you pause it on the canvas, or a token budget in Settings → Agents is reached."}>
          <Space wrap>
            <Segmented value={max > 0 ? "limit" : "none"} options={[{ value: "limit", label: "Pause after" }, { value: "none", label: "No limit" }]} onChange={(v) => setMax(v === "none" ? 0 : lastLimit)} />
            {max > 0 && <InputNumber min={1} max={10000} value={max} onChange={(n) => { const v = Math.max(1, Number(n) || 1); setMax(v); setLastLimit(v); }} suffix="rounds" />}
          </Space>
        </Form.Item>
        {error && <Alert type="error" showIcon title={error} style={{ marginBottom: 12 }} />}
        <div className="dialog-actions">
          <Space>
            <Button onClick={close}>Cancel</Button>
            <Button type="primary" htmlType="submit" loading={busy} disabled={!goal.trim()}>Start loop</Button>
          </Space>
        </div>
      </Form>
      <Typography.Text type="secondary" />
    </Dialog>
  );
}
