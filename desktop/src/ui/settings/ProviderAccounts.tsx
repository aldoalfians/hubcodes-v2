import { useEffect, useState } from "react";
import { Badge, Button, Input, Popconfirm, Tooltip, Typography } from "antd";
import { DeleteOutlined, InfoCircleOutlined, LoginOutlined, PlusOutlined, ReloadOutlined } from "@ant-design/icons";
import { backend } from "../../lib/backend";
import { useStore } from "../../store";
import type { ProviderStatus } from "../../lib/types";
import { Group } from "./primitives";

const { Text } = Typography;

/** Providers whose sign-in hubcodes can keep apart per account, and how. */
const ISOLATION: Record<string, string> = {
  claude: "CLAUDE_CONFIG_DIR",
  codex: "CODEX_HOME",
  gemini: "GEMINI_CLI_HOME",
  opencode: "XDG_DATA_HOME",
  pi: "PI_CODING_AGENT_DIR",
  omp: "omp --profile",
};
const VALID = /^[A-Za-z0-9_-]{1,32}$/;
const key = (provider: string, account: string) => `${provider}@${account}`;

/**
 * Several subscriptions of one tool side by side: each account is its own
 * sign-in folder, so three Claude agents can run on three plans. Add an
 * account, sign in once in the window that opens, pick it in New agent.
 */
export function ProviderAccounts() {
  const providers = useStore((s) => s.providers);
  const reload = useStore((s) => s.boot);
  const notify = useStore((s) => s.notify);
  const [statuses, setStatuses] = useState<Record<string, ProviderStatus | "error">>({});
  const [adding, setAdding] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const list = providers.filter((p) => p.name in ISOLATION);
  const installed = list.filter((p) => p.installed);
  const missing = list.filter((p) => !p.installed);

  const load = (refresh: boolean) => {
    for (const p of installed) {
      for (const a of p.accounts ?? ["default"]) {
        backend
          .providerStatus(p.name, refresh, a === "default" ? null : a)
          .then((st) => setStatuses((cur) => ({ ...cur, [key(p.name, a)]: st })))
          .catch(() => setStatuses((cur) => ({ ...cur, [key(p.name, a)]: "error" })));
      }
    }
  };
  useEffect(() => { load(false); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [providers]);

  const startAdd = (provider: string) => { setAdding(provider); setDraft(""); };
  const name = draft.trim();
  const taken = (provider: string) => (providers.find((p) => p.name === provider)?.accounts ?? []).includes(name);
  const problem = (provider: string) => !name ? null : !VALID.test(name) ? "Letters, digits, - and _ only, up to 32." : taken(provider) ? "This account already exists." : null;

  const add = async (provider: string) => {
    if (!name || problem(provider)) return;
    try {
      await backend.accountAdd(provider, name);
      setAdding(null);
      notify(`Added ${provider} account “${name}”. Sign in to use it.`);
      await reload();
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const login = async (provider: string, account: string) => {
    try {
      const agent = await backend.accountLogin(provider, account);
      notify(`Opened ${agent}. Sign in there (in Claude: /login), then close it.`);
      useStore.getState().setDialog("none");
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const remove = async (provider: string, account: string) => {
    try {
      await backend.accountRemove(provider, account);
      notify(`Removed ${provider} account “${account}”`);
      await reload();
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const state = (provider: string, account: string) => {
    const st = statuses[key(provider, account)];
    if (st === undefined) return { badge: "processing" as const, text: "Checking…", signedIn: null };
    if (st === "error" || st.loggedIn === null) return { badge: "default" as const, text: "Cannot tell", signedIn: null };
    if (st.loggedIn) return { badge: "success" as const, text: st.currentModel ? `Signed in · ${st.currentModel}` : "Signed in", signedIn: true };
    return { badge: "warning" as const, text: "Not signed in", signedIn: false };
  };

  const anyExtra = installed.some((p) => (p.accounts ?? []).some((a) => a !== "default"));
  return (
    <Group
      title="Accounts"
      desc="More than one subscription of the same tool. Each account keeps its own sign-in, so agents on different plans never share one."
      extra={<Button size="small" icon={<ReloadOutlined />} onClick={() => { setStatuses({}); load(true); }}>Refresh</Button>}
    >
      {!anyExtra && (
        <ol className="acct-steps">
          <li><span className="acct-step-n">1</span><span>Add an account under a tool</span></li>
          <li><span className="acct-step-n">2</span><span>Sign in once in the window that opens</span></li>
          <li><span className="acct-step-n">3</span><span>Pick it under <b>Account</b> when you start an agent</span></li>
        </ol>
      )}
      <div className="acct-table" role="table" aria-label="Accounts per tool">
        <div className="acct-tr is-head" role="row">
          <span role="columnheader">Tool</span><span role="columnheader">Account</span><span role="columnheader">Status</span><span role="columnheader" />
        </div>
        {installed.map((p) => {
          const extra = (p.accounts ?? []).filter((a) => a !== "default");
          const err = adding === p.name ? problem(p.name) : null;
          const rows = ["default", ...extra];
          return (
            <section className="acct-tbody" key={p.name} aria-label={`${p.name} accounts`} role="rowgroup">
              {rows.map((a, i) => {
                const s = state(p.name, a);
                return (
                  <div className="acct-tr" role="row" key={a}>
                    <span className="acct-td-tool" role="cell">
                      {i === 0 && (
                        <>
                          <span className="acct-name">{p.name}</span>
                          <Tooltip title={`Each account gets its own folder through ${ISOLATION[p.name]}.`}><InfoCircleOutlined className="acct-info" aria-label={`How ${p.name} accounts are kept apart`} /></Tooltip>
                        </>
                      )}
                    </span>
                    <span className="acct-td-acct" role="cell">{a === "default" ? <>Default <Text type="secondary" className="acct-sub">usual sign-in</Text></> : a}</span>
                    <span className="acct-td-state" role="cell"><Badge status={s.badge} /><Text type={s.signedIn === false ? "warning" : "secondary"} className="acct-state-text" ellipsis={{ tooltip: s.text }}>{s.text}</Text></span>
                    <span className="acct-td-act" role="cell">
                      {a !== "default" && (
                        <>
                          <Button size="small" type={s.signedIn === false ? "primary" : "text"} icon={<LoginOutlined />} onClick={() => void login(p.name, a)}>{s.signedIn === false ? "Sign in" : "Sign in again"}</Button>
                          <Popconfirm title={`Remove ${p.name} account “${a}”?`} description="Its sign-in is deleted from this device." okText="Remove" okButtonProps={{ danger: true }} onConfirm={() => void remove(p.name, a)}>
                            <Button size="small" type="text" danger icon={<DeleteOutlined />} aria-label={`Remove ${a}`} />
                          </Popconfirm>
                        </>
                      )}
                    </span>
                  </div>
                );
              })}
              <div className="acct-tr is-add" role="row">
                <span role="cell" />
                <span className="acct-td-add" role="cell">
                  {adding === p.name ? (
                    <span className="acct-add">
                      <Input
                        autoFocus
                        size="small"
                        value={draft}
                        status={err ? "error" : undefined}
                        onChange={(e) => setDraft(e.target.value)}
                        onPressEnter={() => void add(p.name)}
                        onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); setAdding(null); } }}
                        placeholder="Name, e.g. work"
                        aria-label={`New ${p.name} account name`}
                        spellCheck={false}
                        className="acct-add-input"
                      />
                      <Button size="small" type="primary" disabled={!name || !!err} onClick={() => void add(p.name)}>Add</Button>
                      <Button size="small" type="text" onClick={() => setAdding(null)}>Cancel</Button>
                      {err && <Text type="danger" className="acct-err">{err}</Text>}
                    </span>
                  ) : (
                    <Button size="small" type="link" icon={<PlusOutlined />} className="acct-add-link" onClick={() => startAdd(p.name)}>Add a {p.name} account</Button>
                  )}
                </span>
              </div>
            </section>
          );
        })}
      </div>
      {missing.length > 0 && (
        <div className="acct-missing">
          <Text type="secondary">Not installed on this device: {missing.map((p) => p.name).join(", ")}. Install one to give it accounts.</Text>
        </div>
      )}
    </Group>
  );
}
