import { useEffect, useState } from "react";
import { Badge, Button, Space, Typography } from "antd";
import { LoginOutlined, ReloadOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { backend, isRemoteUi } from "../lib/backend";
import type { ProviderStatus } from "../lib/types";

const { Text } = Typography;

/**
 * One in-flight or settled read per provider, shared by every picker on
 * screen: the Team tab and the Settings team editor ask for the same few
 * providers at once, and the backend caches the answer for a minute anyway.
 */
const inflight = new Map<string, Promise<ProviderStatus>>();
export function readStatus(name: string, refresh: boolean, account?: string | null): Promise<ProviderStatus> {
  const key = account && account !== "default" ? `${name}@${account}` : name;
  if (refresh) inflight.delete(key);
  let p = inflight.get(key);
  if (!p) {
    p = backend.providerStatus(name, refresh, account ?? null);
    inflight.set(key, p);
    p.catch(() => inflight.delete(key));
  }
  return p;
}

/** Shared loader so the dialog and Settings show the same thing. */
export function useProviderStatus(name: string | null, account?: string | null) {
  const [status, setStatus] = useState<ProviderStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = (refresh: boolean) => {
    if (!name) return;
    setLoading(true);
    setError(null);
    readStatus(name, refresh, account)
      .then((s) => setStatus(s))
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  };
  useEffect(() => {
    setStatus(null);
    load(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name, account]);
  return { status, loading, error, refresh: () => load(true) };
}

/**
 * Status for several providers at once (a team's), read on this machine:
 * which are connected and which models their CLIs offer here.
 */
export function useProviderStatuses(names: string[]) {
  const key = Array.from(new Set(names)).sort().join(",");
  const [statuses, setStatuses] = useState<Record<string, ProviderStatus>>({});
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    const want = key ? key.split(",") : [];
    if (!want.length) return;
    let live = true;
    setLoading(true);
    Promise.all(want.map((n) => readStatus(n, false).then((s) => [n, s] as const).catch(() => null)))
      .then((rows) => { if (live) setStatuses((cur) => ({ ...cur, ...Object.fromEntries(rows.filter((r): r is readonly [string, ProviderStatus] => !!r)) })); })
      .finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [key]);
  return { statuses, loading };
}

function Row({ label, ok, children }: { label: string; ok: boolean | null; children: React.ReactNode }) {
  return (
    <div className="connect-row">
      <Badge status={ok === null ? "default" : ok ? "success" : "error"} />
      <span className="connect-label">{label}</span>
      <span className="connect-value">{children}</span>
    </div>
  );
}

function Cmd({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Typography.Text code className="cmd" title="Click to copy" onClick={() => { void navigator.clipboard?.writeText(text.split("   #")[0].trim()); setCopied(true); window.setTimeout(() => setCopied(false), 1200); }}>
      {text}{copied && <Text type="secondary"> · copied</Text>}
    </Typography.Text>
  );
}

/**
 * How a provider is connected: binary, login, where models come from, and
 * the commands to fix what is missing. Everything here is read from the
 * tool's own files; hubcodes never stores credentials.
 */
/** Tools whose sign-in hubcodes can open in a pane (the backend keeps the commands). */
const SIGN_IN_HERE = new Set(["hermes", "codex", "opencode", "claude", "gemini", "agy"]);

export function ConnectPanel({ status, loading, error, onRefresh, compact = false }: { status: ProviderStatus | null; loading: boolean; error: string | null; onRefresh: () => void; compact?: boolean }) {
  if (error) return <Text type="danger">{error}</Text>;
  if (!status) return <Text type="secondary">{loading ? "Checking…" : ""}</Text>;
  const s = status;
  const ready = s.installed && s.loggedIn !== false;
  const signIn = async (kind: `provider:${string}`) => {
    const notify = useStore.getState().notify;
    try {
      const pane = await backend.loginTerminal(kind);
      notify(`${pane} is open on the canvas: sign in there, then press Check again here.`);
    } catch (e) {
      notify(String(e).replace(/^Error: /, ""), "error");
    }
  };
  return (
    <div className={`connect${compact ? " compact" : ""}`}>
      <Row label="Command" ok={s.installed}>
        {s.installed ? <>{s.path}{s.version ? <Text type="secondary"> · {s.version}</Text> : null}</> : "not found on PATH"}
      </Row>
      <Row label="Login" ok={s.loggedIn}>
        {s.loggedIn === null ? "hubcodes cannot tell for this tool" : s.loggedIn ? "signed in" : "not signed in"}
        {s.configPath && <Text type="secondary"> · {s.configPath}</Text>}
      </Row>
      <Row label="Models" ok={s.models.length > 0 ? true : null}>
        {s.models.length > 0 ? `${s.models.length} from ${s.source}` : "none found"}
        {s.currentModel && <Text type="secondary"> · default {s.currentModel}</Text>}
      </Row>
      {!ready && (
        <div className="connect-steps">
          <Text strong>To connect {s.name}</Text>
          <ol>
            {!s.installed && s.setup.install && <li>Install: <Cmd text={s.setup.install} /></li>}
            {!s.installed && !s.setup.install && <li>Install {s.name} and make sure <code>{s.name}</code> is on your PATH.</li>}
            {s.loggedIn === false && s.setup.login && (
              <li>
                Sign in: <Cmd text={s.setup.login} />
                {s.installed && SIGN_IN_HERE.has(s.name) && !isRemoteUi && <Button size="small" type="primary" icon={<LoginOutlined />} className="connect-signin" onClick={() => void signIn(`provider:${s.name}`)}>Sign in</Button>}
              </li>
            )}
            <li>Come back and press Check again.</li>
          </ol>
        </div>
      )}
      <div className="connect-foot">
        <Text type="secondary">Models come from {s.setup.modelsFrom}.</Text>
        <span className="grow" />
        <Space size={4}>
          {s.setup.docs && <Button type="link" size="small" onClick={() => void backend.openPath(s.setup.docs!)}>Docs</Button>}
          <Button size="small" icon={<ReloadOutlined />} onClick={onRefresh} loading={loading}>Check again</Button>
        </Space>
      </div>
    </div>
  );
}
