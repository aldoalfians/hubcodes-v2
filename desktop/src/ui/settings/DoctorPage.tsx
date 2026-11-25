import { useEffect, useState } from "react";
import { FixActions } from "../FixActions";
import { Button, Space, Tag, Typography } from "antd";
import { CheckCircleFilled, CloseCircleFilled, CopyOutlined, ExclamationCircleFilled, ReloadOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { backend } from "../../lib/backend";
import type { DoctorReport } from "../../lib/types";
import { Group, Section } from "./primitives";

const { Text } = Typography;

const ICON = {
  ok: <CheckCircleFilled className="st-doc-ok" />,
  warn: <ExclamationCircleFilled className="st-doc-warn" />,
  fail: <CloseCircleFilled className="st-doc-fail" />,
};

/** Settings → This Mac → Doctor: the environment checks and the app log. */
export function DoctorPage() {
  const report = useStore((s) => s.doctor);
  const runDoctor = useStore((s) => s.runDoctor);
  const notify = useStore((s) => s.notify);
  const [busy, setBusy] = useState(false);
  const [log, setLog] = useState<string>("");
  const [copied, setCopied] = useState(false);
  const refreshLog = () => backend.logTail(120).then(setLog).catch(() => setLog(""));
  // Run on every open: a result from before (a device that was away, Tailscale off) is stale.
  useEffect(() => {
    void run();
    void refreshLog();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const run = async () => {
    setBusy(true);
    await runDoctor();
    await refreshLog();
    setBusy(false);
  };
  const copy = async () => {
    try {
      const text = await backend.diagnostics();
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const summary = (r: DoctorReport) => (r.fail ? `${r.fail} failed` : r.warn ? `${r.warn} to look at` : "everything looks fine");
  return (
    <Section
      title="Doctor"
      tagline="Why agents would not show up: tmux, locale, PATH, providers, Tailscale, signing."
      extra={
        <Space>
          <Button icon={<ReloadOutlined />} loading={busy} onClick={() => void run()}>Run again</Button>
          <Button icon={<CopyOutlined />} onClick={() => void copy()} disabled={!report}>{copied ? "Copied" : "Copy diagnostics"}</Button>
        </Space>
      }
    >
      <Group
        title="Checks"
        desc={report ? summary(report) : "Running…"}
        extra={report && <Space size={4}><Tag color="success" variant="filled">{report.ok} ok</Tag>{report.warn > 0 && <Tag color="warning" variant="filled">{report.warn} warn</Tag>}{report.fail > 0 && <Tag color="error" variant="filled">{report.fail} fail</Tag>}</Space>}
      >
        {(report?.checks ?? []).map((c) => (
          <div key={c.id} className={`st-doc st-doc-${c.level}`}>
            <span className="st-doc-icon">{ICON[c.level]}</span>
            <div className="st-doc-body">
              <div className="st-doc-head">
                <span className="st-doc-title">{c.title}</span>
                <Text type="secondary" className="st-doc-detail mono">{c.detail}</Text>
              </div>
              {c.fix && <Text className="st-doc-fix">{c.fix}</Text>}
              {c.level !== "ok" && <FixActions command={c.command} open={c.open} />}
            </div>
          </div>
        ))}
      </Group>
      <Group title="App log" desc="~/.local/state/hubcodes/desktop.log, last 120 lines. Errors from the poll thread and the terminal bridge land here." extra={<Button size="small" icon={<ReloadOutlined />} onClick={() => void refreshLog()}>Refresh</Button>}>
        <pre className="st-log">{log || "Nothing logged yet."}</pre>
      </Group>
    </Section>
  );
}
