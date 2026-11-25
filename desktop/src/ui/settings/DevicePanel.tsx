import { useEffect, useState } from "react";
import { Badge, Button, Space, Tag, Typography } from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import { backend } from "../../lib/backend";
import { useStore } from "../../store";
import type { DeviceStatus } from "../../lib/types";
import { Group, Row } from "./primitives";

const { Text } = Typography;

const ago = (s: number | null) => (s === null ? "not yet" : s < 2 ? "just now" : s < 60 ? `${s} s ago` : `${Math.floor(s / 60)} min ago`);

/**
 * One device as this Mac sees it right now: reachable or not, when it was
 * last asked, how many agents it runs, and the reports waiting for it in
 * the remote queue (a `task done mac:3` sent while that Mac was off).
 * Refreshes every 5 s while open.
 */
export function DevicePanel({ name }: { name: string }) {
  const notify = useStore((s) => s.notify);
  const [st, setSt] = useState<DeviceStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = () => backend.deviceStatus(name).then((s) => { setSt(s); setError(null); }).catch((e) => setError(String(e)));
  useEffect(() => {
    void load();
    const t = window.setInterval(() => void load(), 5000);
    return () => window.clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name]);
  const retry = async () => {
    setBusy(true);
    try {
      const lines = await backend.deviceRetry();
      notify(lines.length ? lines.join(" · ") : "Polled; nothing was waiting");
      await load();
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(false);
    }
  };
  if (error) return <Text type="danger">{error}</Text>;
  if (!st) return <Text type="secondary">Checking…</Text>;
  return (
    <Group title="Right now" extra={<Button size="small" icon={<ReloadOutlined />} loading={busy} onClick={() => void retry()}>Poll and retry now</Button>}>
      <Row label="Reachable" hint={st.ok ? `Polled ${ago(st.polledSecsAgo)}; next in about ${st.nextPollSecs} s.` : `Last tried ${ago(st.polledSecsAgo)}; retried every ${st.nextPollSecs} s.`}>
        <Space><Badge status={st.ok ? "success" : "error"} />{st.ok ? `${st.agents} agent${st.agents === 1 ? "" : "s"} there` : st.error ?? "unreachable"}</Space>
      </Row>
      <Row label="Pairing" hint={st.issues.length ? "Fix these so replies and task reports find their way back." : st.ok ? `Same release (${st.version ?? "?"}), names match, it lists this device.` : "Checked once the device answers."}>
        {st.issues.length === 0 ? <Space><Badge status={st.ok ? "success" : "default"} />{st.ok ? "paired" : "unknown"}</Space> : (
          <div className="dev-issues">{st.issues.map((i, k) => <Text key={k} type="warning" className="dev-issue">{i}</Text>)}</div>
        )}
      </Row>
      <Row label="Waiting for it" hint={st.queued.length ? "Reports and messages kept while it was away; each tend pass retries them for two days." : "Nothing queued."}>
        {st.queued.length === 0 ? <Text type="secondary">none</Text> : (
          <div className="dev-queue">
            {st.queued.map((q) => (
              <div className="dev-queue-row" key={q.id}>
                <code className="mono">hubcodes {q.args.slice(0, 3).join(" ")}{q.args.length > 3 ? " …" : ""}</code>
                <Tag variant="filled" className="dev-queue-tag">{q.attempts} tr{q.attempts === 1 ? "y" : "ies"}</Tag>
                <Text type="secondary" className="dev-queue-err" title={q.error}>{q.error}</Text>
              </div>
            ))}
          </div>
        )}
      </Row>
    </Group>
  );
}
