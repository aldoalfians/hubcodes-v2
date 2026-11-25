import { useEffect, useMemo, useState } from "react";
import { Alert, AutoComplete, Badge, Button, Checkbox, Form, Input, Radio, Select, Space, Tabs, Tag, Typography } from "antd";
import { LoginOutlined, PlayCircleOutlined, PlusOutlined, SaveOutlined, SyncOutlined } from "@ant-design/icons";
import { askText } from "./ask";
import { useStore } from "../store";
import { color256 } from "../lib/colors";
import { Dialog } from "./Dialog";
import { ConnectPanel, useProviderStatus, useProviderStatuses } from "./ConnectPanel";
import { ModelPicker } from "./ModelPicker";
import { backend } from "../lib/backend";
import type { ProviderStatus, RoleBrief, Template } from "../lib/types";

const { Text } = Typography;

/** A team's agents on this machine: which providers are connected, which models their CLIs offer here. */
function TeamCard({ t, cwd, busy, statuses, loading, models, onModel, onStart }: {
  t: Template;
  cwd: string;
  busy: string | null;
  statuses: Record<string, ProviderStatus>;
  loading: boolean;
  models: Record<string, string>;
  onModel: (agent: string, model: string) => void;
  onStart: () => void;
}) {
  const providers = useStore((s) => s.providers);
  const missing = t.agents.filter((a) => !a.device).map((a) => a.provider).filter((p, i, all) => all.indexOf(p) === i && (statuses[p] ? !statuses[p].installed : providers.find((x) => x.name === p)?.installed === false));
  const glyph = (name: string) => providers.find((p) => p.name === name);
  return (
    <div className="team-card">
      <div className="team-head">
        <div className="team-title">
          <Text strong className="mono">{t.name}</Text>
          {t.description && <Text type="secondary">{t.description}</Text>}
        </div>
        <Button type="primary" icon={<PlayCircleOutlined />} loading={busy === t.name} disabled={(busy !== null && busy !== t.name) || missing.length > 0} onClick={onStart} title={missing.length ? `${missing.join(", ")} is not connected on this device` : cwd ? `Start in ${cwd}` : undefined}>
          Start team
        </Button>
      </div>
      <div className="team-agents">
        {t.agents.map((a) => {
          const st = a.device ? statuses[`${a.device}/${a.provider}`] : statuses[a.provider];
          const prov = glyph(a.provider);
          const takesModel = prov ? prov.takesModel : true;
          const value = a.name in models ? models[a.name] : a.model ?? "";
          const unknown = !!value.trim() && !!st && st.models.length > 0 && !st.models.some((m) => m.id === value.trim());
          const off = st ? !st.installed : prov?.installed === false;
          return (
            <div className="team-agent" key={a.name}>
              <span className="team-agent-name mono">{a.name}</span>
              <span className="team-agent-provider">
                {prov && <span style={{ color: color256(prov.color) }}>{prov.glyph}</span>} {a.provider}
                {a.device && <Tag variant="filled" className="team-agent-tag" title={`Started on ${a.device} by its own hubcodes`}>on {a.device}</Tag>}
                {!a.device && off && <Tag color="error" variant="outlined" className="team-agent-tag">not connected</Tag>}
              </span>
              <span className="team-agent-role">{a.role ?? <Text type="secondary">any role</Text>}{a.worktree && <Tag variant="filled" className="team-agent-tag">own branch</Tag>}</span>
              <span className="team-agent-model" title={unknown ? `${value.trim()} is not in ${a.provider}'s catalog on this device; it is passed as typed.` : st?.source ? `${st.models.length} models from ${st.source}` : undefined}>
                <ModelPicker
                  value={value}
                  onChange={(v) => onModel(a.name, v)}
                  models={st?.models ?? []}
                  loading={loading && !st}
                  disabled={!takesModel || (!a.device && off)}
                  size="small"
                  compact
                  placeholder={!takesModel ? "no model flag" : st?.currentModel ? `${st.currentModel} (default ${a.device ? `on ${a.device}` : "here"})` : a.device ? `${a.device}'s default` : "provider default"}
                />
                {unknown && <Text type="warning" className="team-agent-warn">not on this device</Text>}
              </span>
            </div>
          );
        })}
      </div>
      {t.links.length > 0 && (
        <div className="team-links">
          {t.links.map((l, i) => (
            <Tag key={i} variant="filled" className="mono team-link" title={l[2] === "loop" ? `${l[0]} and ${l[1]} answer each other` : `${l[0]} can send to ${l[1]}`}>
              {l[0]} {l[2] === "loop" ? "↻" : "→"} {l[1]}
            </Tag>
          ))}
        </div>
      )}
    </div>
  );
}

function TeamTab({ onDone }: { onDone: () => void }) {
  const notify = useStore((s) => s.notify);
  const session = useStore((s) => s.session);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [cwd, setCwd] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  // Models chosen here, per team and agent; the template keeps its own.
  const [models, setModels] = useState<Record<string, Record<string, string>>>({});
  useEffect(() => {
    backend.templates(cwd.trim() || null).then(setTemplates).catch(() => {});
  }, [cwd]);
  const providerNames = useMemo(() => templates.flatMap((t) => t.agents.filter((a) => !a.device).map((a) => a.provider)), [templates]);
  const { statuses, loading } = useProviderStatuses(providerNames);
  // Catalogs of the devices the templates start agents on, keyed `device/provider`.
  const deviceNames = useMemo(() => Array.from(new Set(templates.flatMap((t) => t.agents.map((a) => a.device ?? "").filter(Boolean)))), [templates]);
  const [remoteStatuses, setRemoteStatuses] = useState<Record<string, ProviderStatus>>({});
  useEffect(() => {
    let live = true;
    for (const d of deviceNames) {
      backend.deviceModels(d, false).then((all) => { if (live) setRemoteStatuses((cur) => ({ ...cur, ...Object.fromEntries(all.map((p) => [`${d}/${p.name}`, p])) })); }).catch(() => {});
    }
    return () => { live = false; };
  }, [deviceNames]);
  const saveToRepo = async () => {
    try {
      const p = await backend.teamInit(cwd.trim() || session?.defaultFolder || session?.home || "");
      setSaved(p);
      notify(`Wrote ${p}`);
      backend.templates(cwd.trim() || null).then(setTemplates).catch(() => {});
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const start = async (t: Template) => {
    setBusy(t.name);
    try {
      const chosen = models[t.name] ?? {};
      const r = await backend.teamStart(t.name, cwd.trim() || null, Object.keys(chosen).length ? chosen : undefined);
      notify(`Team ${t.name}: ${r.spawned.length} started${r.skipped.length ? `, ${r.skipped.length} already running` : ""}, ${r.links} links${r.errors.length ? ` · ${r.errors[0]}` : ""}`, r.errors.length ? "error" : "info");
      if (!r.errors.length) onDone();
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(null);
    }
  };
  const folder = cwd.trim() || session?.defaultFolder || session?.home || "";
  return (
    <div className="team-tab">
      <Form layout="vertical" className="na-form">
        <Form.Item label="Folder" extra={<>Every agent starts here. A repository's own team in <code>.hubcodes/team.toml</code> is listed first; Settings → Teams holds the global ones.</>}>
          <Input className="mono" value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder={session?.defaultFolder ?? session?.home ?? "home"} spellCheck={false} />
        </Form.Item>
      </Form>
      <div className="team-list-head">
        <Text type="secondary">Models are read from each tool on this device; pick one per agent or keep the default.</Text>
        <Button type="link" size="small" icon={<SaveOutlined />} onClick={() => void saveToRepo()} style={{ paddingInline: 0 }} title={folder ? `Writes ${folder}/.hubcodes/team.toml from the running canvas` : undefined}>Save the running canvas as this repo's team</Button>
      </div>
      {saved && <Text type="secondary" className="team-saved">Wrote <code>{saved}</code></Text>}
      {templates.length === 0 ? (
        <Text type="secondary" className="team-empty">No teams yet. Add one in Settings → Teams, or start agents on the canvas and save them as this repo's team.</Text>
      ) : (
        <div className="team-list">
          {templates.map((t) => (
            <TeamCard key={t.name} t={t} cwd={folder} busy={busy} statuses={{ ...statuses, ...remoteStatuses }} loading={loading} models={models[t.name] ?? {}} onModel={(agent, m) => setModels((cur) => ({ ...cur, [t.name]: { ...(cur[t.name] ?? {}), [agent]: m } }))} onStart={() => void start(t)} />
          ))}
        </div>
      )}
    </div>
  );
}

export function NewAgentDialog() {
  const [mode, setMode] = useState<"agent" | "team">("agent");
  const providers = useStore((s) => s.providers);
  const agents = useStore((s) => s.agents);
  const session = useStore((s) => s.session);
  const spawn = useStore((s) => s.spawn);
  const setDialog = useStore((s) => s.setDialog);
  const installed = useMemo(() => providers.filter((p) => p.installed), [providers]);
  const [provider, setProvider] = useState(installed[0]?.name ?? "shell");
  const [name, setName] = useState("");
  const [cwd, setCwd] = useState("");
  const [role, setRole] = useState("");
  const [model, setModel] = useState("");
  const [ownBranch, setOwnBranch] = useState(false);
  const [branch, setBranch] = useState("");
  const [sandboxed, setSandboxed] = useState(false);
  // Another device (Settings → Devices) can run it; its own hubcodes starts the agent and it shows here as device/name.
  const peers = useStore((s) => s.peers);
  const [device, setDevice] = useState<string>("");
  const remote = device !== "";
  const chosen = providers.find((p) => p.name === provider);
  // One login per account: three Claude agents can run on three subscriptions.
  const [account, setAccount] = useState<string>("default");
  const accounts = chosen?.accounts ?? ["default"];
  const local = useProviderStatus(provider, account);
  // On another device, its own tools decide which models exist: read its catalog instead.
  const [remoteStatus, setRemoteStatus] = useState<ProviderStatus | null>(null);
  const [remoteLoading, setRemoteLoading] = useState(false);
  // Bumped by the sync button: read the device's catalog again instead of the cached one.
  const [remoteReads, setRemoteReads] = useState(0);
  useEffect(() => {
    if (!remote) { setRemoteStatus(null); return; }
    let live = true;
    setRemoteLoading(true);
    setRemoteStatus(null);
    backend.deviceModels(device, remoteReads > 0).then((all) => { if (live) setRemoteStatus(all.find((p) => p.name === provider) ?? null); }).catch(() => {}).finally(() => { if (live) setRemoteLoading(false); });
    return () => { live = false; };
  }, [remote, device, provider, remoteReads]);
  const status = remote ? remoteStatus : local.status;
  const loading = remote ? remoteLoading : local.loading;
  const statusError = remote ? null : local.error;
  const refresh = remote ? () => setRemoteReads((n) => n + 1) : local.refresh;
  const ROLES = useMemo(() => {
    const list = [...(session?.roles ?? [])];
    for (const a of agents) if (a.role && !list.includes(a.role)) list.push(a.role);
    return list;
  }, [session, agents]);
  const readyHere = status ? status.installed && status.loggedIn !== false : chosen?.installed ?? false;
  const ready = remote || readyHere;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const suggested = useMemo(() => {
    const n = agents.filter((a) => a.provider === provider).length + 1;
    return `${provider}-${n}`;
  }, [agents, provider]);
  useEffect(() => setName(suggested), [suggested]);
  useEffect(() => { setModel(""); setAccount("default"); }, [provider]);
  const notify = useStore((s) => s.notify);
  const addAccount = async () => {
    const name = await askText(`New ${provider} account`, { placeholder: "work", hint: "A folder of its own for that login; you sign in once in a window that opens." });
    if (!name?.trim()) return;
    try {
      await backend.accountAdd(provider, name.trim());
      await useStore.getState().boot();
      setAccount(name.trim());
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const signIn = async () => {
    try {
      const agent = await backend.accountLogin(provider, account);
      notify(`Opened ${agent}: sign in there (for example /login), then close it and start the agent again`);
      setDialog("none");
    } catch (e) {
      notify(String(e), "error");
    }
  };

  // What this role is told at start: the generated brief with the role file (repo, then user) layered on.
  const [briefOpen, setBriefOpen] = useState(false);
  const [brief, setBrief] = useState<RoleBrief | null>(null);
  useEffect(() => {
    if (!briefOpen) return;
    let live = true;
    const t = setTimeout(() => {
      backend.roleBrief(role.trim(), cwd.trim() || null).then((b) => { if (live) setBrief(b); }).catch(() => { if (live) setBrief(null); });
    }, 250);
    return () => { live = false; clearTimeout(t); };
  }, [briefOpen, role, cwd]);

  const taken = agents.some((a) => a.name === name.trim());
  const submit = async () => {
    if (!name.trim() || taken) return;
    setBusy(true);
    setError(null);
    try {
      await spawn(name.trim(), provider, cwd.trim() || null, role.trim() || null, chosen?.takesModel ? model.trim() || null : null, !remote && ownBranch ? branch.trim() || `agent/${name.trim()}` : null, !remote && sandboxed, remote ? device : null, account !== "default" ? account : null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog title={mode === "agent" ? "New agent" : "Start a team"} onClose={() => setDialog("none")} width={600}>
      <Tabs activeKey={mode} onChange={(k) => setMode(k as "agent" | "team")} items={[{ key: "agent", label: "One agent" }, { key: "team", label: "Team" }]} size="small" />
      {mode === "team" && <TeamTab onDone={() => setDialog("none")} />}
      {mode === "agent" && (
        <Form layout="vertical" onFinish={() => void submit()} className="na-form">
          <Form.Item label="Runs">
            <Radio.Group value={provider} onChange={(e) => setProvider(e.target.value)} optionType="button" buttonStyle="solid" className="na-providers">
              {providers.map((p) => (
                <Radio.Button key={p.name} value={p.name} className={p.installed ? "" : "is-off"} title={p.installed ? p.command : `${p.command} is not on PATH · pick it to see how to connect`}>
                  <span style={{ color: color256(p.color) }}>{p.glyph}</span> {p.name}{!p.installed && <Text type="secondary"> · not connected</Text>}
                </Radio.Button>
              ))}
            </Radio.Group>
          </Form.Item>
          {peers.length > 0 && (
            <Form.Item label="Runs on" extra={remote ? `${device} starts it with its own ${provider} and credentials; it shows here as ${device}/${name.trim() || "name"}.` : "This device. Pick a device to run it there instead; the work and memory stay on that device."}>
              <Radio.Group value={device} onChange={(e) => setDevice(e.target.value)} optionType="button" buttonStyle="solid" className="na-providers">
                <Radio.Button value="">This device</Radio.Button>
                {peers.map((p) => (
                  <Radio.Button key={p.name} value={p.name} disabled={!p.ok} title={p.ok ? p.host : p.error ?? "unreachable"}>{p.name}{!p.ok && <Text type="secondary"> · unreachable</Text>}</Radio.Button>
                ))}
              </Radio.Group>
            </Form.Item>
          )}
          {!remote && accounts.length > 0 && provider !== "shell" && (
            <Form.Item label="Account" extra={account === "default" ? "The tool's own login. Add an account to run this agent on another subscription." : status?.loggedIn === false ? `${account} is not signed in yet.` : `Runs under the ${account} login (its own folder and credentials).`}>
              <Space>
                <Select value={account} onChange={setAccount} options={accounts.map((a) => ({ value: a, label: a === "default" ? "default (the tool's own)" : a }))} style={{ minWidth: 220 }} aria-label="Account" />
                <Button icon={<PlusOutlined />} onClick={() => void addAccount()}>Add account</Button>
                {account !== "default" && status?.loggedIn === false && <Button size="small" type="primary" icon={<LoginOutlined />} onClick={() => void signIn()}>Sign in</Button>}
              </Space>
            </Form.Item>
          )}
          <Form.Item label="Name" validateStatus={taken ? "error" : undefined} help={taken ? `An agent named ${name.trim()} is already running.` : undefined}>
            <Input className="mono" value={name} onChange={(e) => setName(e.target.value)} autoFocus spellCheck={false} />
          </Form.Item>
          <div className="na-row">
            <Form.Item label="Role" extra={chosen?.takesRole ? "Shown on the card and told to the agent as its job." : provider === "shell" ? "Shown on the card; a plain shell is not told." : "Shown on the card and sent as its first message once the tool is up (it has no system-prompt flag)."}>
              <AutoComplete value={role} onChange={setRole} options={ROLES.filter((r) => r.toLowerCase().includes(role.toLowerCase())).map((r) => ({ value: r }))} placeholder="orchestrator, software engineer, reviewer…" allowClear />
            </Form.Item>
            <Form.Item label={<span className="na-model-label">Model{status?.models.length ? <Text type="secondary">· {status.models.length} from {status.source}</Text> : null}{chosen?.takesModel && <Button type="text" size="small" icon={<SyncOutlined spin={loading} />} disabled={loading} onClick={(e) => { e.preventDefault(); refresh(); }} title={`Read ${chosen.name}'s models on ${remote ? device : "this device"} again`} aria-label="Sync models">Sync</Button>}</span>} extra={chosen?.takesModel ? `Passed to ${chosen.name} as its model flag.` : "Set a model flag for this provider in Settings → Providers."}>
              <ModelPicker value={model} onChange={setModel} models={status?.models ?? []} loading={loading} disabled={!chosen?.takesModel} placeholder={chosen?.takesModel ? (status?.currentModel ? `${status.currentModel} (provider default)` : "provider default") : "no model flag"} />
            </Form.Item>
          </div>
          {chosen?.takesRole && (
            <div style={{ margin: "-4px 0 14px" }}>
              <Button type="link" size="small" style={{ paddingInline: 0, height: "auto" }} onClick={() => setBriefOpen((v) => !v)}>{briefOpen ? "Hide what this role is told" : "What this role is told"}</Button>
              {briefOpen && (
                <>
                  <Text type="secondary" style={{ display: "block", maxHeight: 160, overflow: "auto", whiteSpace: "pre-wrap", fontSize: 12, lineHeight: 1.5 }}>{brief?.text || "…"}</Text>
                  <Text type="secondary" style={{ display: "block", fontSize: 11, marginTop: 4 }}>
                    {brief?.file ? <>from <code>{brief.file}</code> · <Button type="link" size="small" style={{ paddingInline: 0, height: "auto", fontSize: 11 }} onClick={() => void backend.openPath(brief.file!)}>Open</Button></> : <>generated · <code>hubcodes role init "{role.trim() || "role"}"</code> writes a file to change it</>}
                  </Text>
                </>
              )}
            </div>
          )}
          <Form.Item label="Folder" extra={remote ? `A folder on ${device}; empty means its default.` : "The agent starts here."}>
            <Input className="mono" value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder={session?.home ? `${session.home} (default)` : "current folder"} spellCheck={false} />
          </Form.Item>
          {!remote && <Form.Item>
            <Checkbox checked={ownBranch} onChange={(e) => setOwnBranch(e.target.checked)}>Own branch (git worktree)</Checkbox>
            <div className="na-hint"><Text type="secondary">A checkout under <code>.hubcodes/worktrees/{name.trim() || "name"}</code> on its own branch, so this agent's edits never collide with the others'. Needs the folder to be a git repository.</Text></div>
            {ownBranch && <Input className="mono na-branch" value={branch} onChange={(e) => setBranch(e.target.value)} placeholder={`agent/${name.trim() || "name"}`} spellCheck={false} aria-label="Branch" />}
          </Form.Item>}
          {!remote && <Form.Item>
            <Checkbox checked={sandboxed} onChange={(e) => setSandboxed(e.target.checked)}>Sandbox: writes only inside its folder</Checkbox>
            <div className="na-hint"><Text type="secondary">macOS <code>sandbox-exec</code> by default (Settings → Agents → Sandbox to use Docker or another wrapper). Reads and network stay open; the provider's own config folders stay writable.</Text></div>
          </Form.Item>}
          {!remote && (!ready || statusError) && (
            <Form.Item label={`Connect ${provider}`}>
              <ConnectPanel status={status} loading={loading} error={statusError} onRefresh={refresh} compact />
            </Form.Item>
          )}
          {error && <Alert type="error" showIcon title={error} style={{ marginBottom: 12 }} />}
          <div className="dialog-actions">
            {!remote && status && ready && <Button type="text" size="small" onClick={refresh} title={`${provider}: ${status.version ?? "installed"}${status.loggedIn ? " · signed in" : ""}`}><Badge status="success" /> {provider} connected</Button>}
            <span className="grow" />
            <Space>
              <Button onClick={() => setDialog("none")}>Cancel</Button>
              <Button type="primary" htmlType="submit" loading={busy} disabled={!name.trim() || taken || !ready}>Start agent</Button>
            </Space>
          </div>
        </Form>
      )}
    </Dialog>
  );
}
