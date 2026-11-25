import { useEffect, useState } from "react";
import { Button, Space, Switch, Tag, Typography } from "antd";
import { CloudDownloadOutlined, ReloadOutlined } from "@ant-design/icons";
import { backend, isTauri } from "../../lib/backend";
import { useStore } from "../../store";
import type { AuditRow, DaemonStatus } from "../../lib/types";
import { Group, Row } from "./primitives";
import { APP_VERSION, SERVICE_MANAGER, isMac } from "../../lib/platform";

const { Text } = Typography;

/** Settings → General → Background: keep watching under launchd when the app is closed. */
export function BackgroundGroup() {
  const notify = useStore((s) => s.notify);
  const [st, setSt] = useState<DaemonStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const load = () => backend.daemonStatus().then(setSt).catch(() => setSt(null));
  useEffect(() => { void load(); }, []);
  const toggle = async (on: boolean) => {
    setBusy(true);
    try {
      setSt(await backend.daemonSet(on, true));
      notify(on ? `hubcodesd installed (${SERVICE_MANAGER}). It tends every session and keeps triggers, trackers, queued deliveries and the phone page running.` : "hubcodesd removed");
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <Group title="Background" desc={`A ${SERVICE_MANAGER} ${isMac ? "job" : "user service"} runs hubcodesd, one daemon for every session on this device: it tends dispatch, loops, triggers, trackers and queued deliveries, serves the phone page, and pushes changes to this app over its socket. Only one process tends a session at a time; the app stands by while the daemon runs.`}>
      <Row label="Keep working when the app is closed" hint={st ? <>{st.installed ? `Installed · ${st.running ? `running${st.pid ? ` (pid ${st.pid})` : ""}` : "not running"}` : "Not installed"}{st.socket ? ` · socket up${st.version ? ` (${st.version})` : ""}${st.sessions.length ? ` · tending ${st.sessions.join(", ")}` : ""}` : " · socket down"} · this session tended by {st.tender ?? "nobody"}</> : "Checking…"}>
        <Space>
          <Switch checked={!!st?.installed} loading={busy} onChange={(v) => void toggle(v)} aria-label="Keep working when the app is closed" />
          <Button size="small" icon={<ReloadOutlined />} onClick={() => void load()}>Refresh</Button>
          {st?.installed && <Tag variant="filled" color={st.running ? "success" : "warning"}>{st.running ? "running" : "stopped"}</Tag>}
        </Space>
      </Row>
      {st?.installed && <Row label="Log" hint="Where the daemon writes."><Text className="mono" type="secondary">{st.log}</Text></Row>}
    </Group>
  );
}

/** Settings → General → Updates: same check as the bar at the bottom. */
export function UpdatesGroup() {
  const update = useStore((s) => s.update);
  const state = useStore((s) => s.updateState);
  const checkUpdate = useStore((s) => s.checkUpdate);
  const install = useStore((s) => s.installUpdate);
  const [checked, setChecked] = useState(false);
  const hint = state === "downloading"
    ? "Downloading and installing; the app restarts when done."
    : update
      ? `Version ${update.version} is available.${update.notes ? ` ${update.notes.split("\n")[0]}` : ""}`
      : checked
        ? "You are on the latest version."
        : "Checked a few seconds after launch and every 6 hours; a bar at the bottom offers the update. Nothing installs on its own.";
  const running = `This is version ${APP_VERSION}.`;
  return (
    <Group title="Updates" desc="Releases are signed; the signature is verified before anything installs.">
      <Row label="App updates" hint={<>{running} {hint}</>}>
        <Space>
          <Button icon={<CloudDownloadOutlined />} loading={state === "checking" || state === "downloading"} disabled={!isTauri} onClick={() => void checkUpdate(true).then(() => setChecked(true))}>Check for updates</Button>
          {update && state !== "downloading" && <Button type="primary" onClick={() => void install()}>Install {update.version}</Button>}
        </Space>
      </Row>
    </Group>
  );
}


const ago = (t: number) => {
  const s = Math.max(0, Date.now() / 1000 - t);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
};

/** Settings → Remote access → Audit: every write that came through the web page, by whom. */
export function AuditGroup() {
  const [rows, setRows] = useState<AuditRow[]>([]);
  const load = () => backend.auditLog(100).then(setRows).catch(() => setRows([]));
  useEffect(() => { void load(); }, []);
  return (
    <Group title="Audit log" desc="Who sent, approved, denied or noted what through the web page. Kept in the session transcript." extra={<Button size="small" icon={<ReloadOutlined />} onClick={() => void load()}>Refresh</Button>}>
      {rows.length === 0 ? (
        <div className="st-empty"><Text type="secondary">Nothing yet.</Text></div>
      ) : (
        <div className="st-audit">
          {rows.map((r) => (
            <div key={r.id} className="st-audit-row">
              <Text type="secondary" className="st-audit-when">{ago(r.at)}</Text>
              <Text strong className="st-audit-who">{r.agent}</Text>
              <Text className="st-audit-what">{r.detail}</Text>
            </div>
          ))}
        </div>
      )}
    </Group>
  );
}