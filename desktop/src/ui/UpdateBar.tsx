import { Button, Progress, Typography } from "antd";
import { CloudDownloadOutlined } from "@ant-design/icons";
import { useStore } from "../store";

const { Text } = Typography;

const mb = (n: number) => `${(n / 1_000_000).toFixed(1)} MB`;

/**
 * The card at the bottom when a newer release is out: what changed, one
 * button to download + verify + restart, one to put it off for this run.
 */
export function UpdateBar() {
  const update = useStore((s) => s.update);
  const state = useStore((s) => s.updateState);
  const progress = useStore((s) => s.updateProgress);
  const error = useStore((s) => s.updateError);
  const snoozed = useStore((s) => s.updateSnoozed);
  const install = useStore((s) => s.installUpdate);
  const snooze = useStore((s) => s.snoozeUpdate);
  const dialog = useStore((s) => s.dialog);
  if (!update || snoozed.includes(update.version) || dialog === "settings") return null;
  const busy = state === "downloading";
  const pct = progress && progress.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null;
  const note = update.notes.split("\n").map((l) => l.trim()).filter(Boolean)[0] ?? "";
  return (
    <aside className="updatebar" role="status" aria-live="polite">
      <span className="updatebar-icon"><CloudDownloadOutlined /></span>
      <div className="updatebar-body">
        <div className="updatebar-title">hubcodes {update.version} is ready{update.current ? <Text type="secondary"> · you have {update.current}</Text> : null}</div>
        {busy ? (
          <div className="updatebar-progress">
            <Progress percent={pct ?? 0} showInfo={false} size="small" strokeColor="var(--mint)" status={pct === null ? "active" : undefined} />
            <Text type="secondary" className="updatebar-note">{pct === null ? "Downloading…" : pct < 100 ? `Downloading ${mb(progress!.downloaded)} of ${mb(progress!.total!)}` : "Verifying and installing; the app restarts in a moment"}</Text>
          </div>
        ) : state === "failed" ? (
          <Text type="danger" className="updatebar-note">{error || "The update did not install."}</Text>
        ) : note ? (
          <Text type="secondary" className="updatebar-note" title={update.notes}>{note}</Text>
        ) : null}
      </div>
      <div className="updatebar-actions">
        {!busy && <Button size="small" type="text" onClick={snooze}>Later</Button>}
        <Button size="small" type="primary" loading={busy} onClick={() => void install()}>{state === "failed" ? "Try again" : busy ? "Updating" : "Update and restart"}</Button>
      </div>
    </aside>
  );
}
