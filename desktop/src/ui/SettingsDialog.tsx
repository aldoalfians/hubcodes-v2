import { useEffect, useMemo, useState, type ComponentType } from "react";
import { Alert, Button, Menu, Space, Tag, Typography } from "antd";
import { LoadingOutlined, ReloadOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { Dialog } from "./Dialog";
import { APP_VERSION } from "../lib/platform";
import { backend } from "../lib/backend";
import type { Config, MemoryConfig, Share } from "../lib/types";
import { DoctorPage } from "./settings/DoctorPage";
import { SettingsProvider, type ListKey, type Settings } from "./settings/context";
import { GeneralPage } from "./settings/GeneralPage";
import { AgentsPage } from "./settings/AgentsPage";
import { PolicyPage } from "./settings/PolicyPage";
import { ProvidersPage } from "./settings/ProvidersPage";
import { TeamsPage } from "./settings/TeamsPage";
import { DevicesPage } from "./settings/DevicesPage";
import { AccountsPage } from "./settings/AccountsPage";
import { MemoryPage } from "./settings/MemoryPage";
import { KnowledgePage } from "./settings/KnowledgePage";
import { TrackersPage } from "./settings/TrackersPage";
import { RemotePage } from "./settings/RemotePage";
import { PlaybooksPage, PromptsPage, TriggersPage } from "./settings/AutomatePages";
import { SessionPage } from "./settings/SessionPage";
import { SkillsPage } from "./settings/SkillsPage";
import { ShortcutsPage } from "./settings/ShortcutsPage";
import { RolesPage } from "./settings/RolesPage";
import { SECRET_STORE } from "../lib/platform";

const { Text } = Typography;

type Tab = "general" | "agents" | "roles" | "policy" | "providers" | "teams" | "accounts" | "devices" | "memory" | "knowledge" | "trackers" | "remote" | "skills" | "prompts" | "playbooks" | "triggers" | "doctor" | "session" | "shortcuts";
const READ_ONLY: Tab[] = ["roles", "doctor", "session", "shortcuts"];
const TABS: Tab[] = ["general", "agents", "roles", "policy", "providers", "teams", "accounts", "devices", "memory", "knowledge", "trackers", "remote", "skills", "prompts", "playbooks", "triggers", "doctor", "session", "shortcuts"];
const NAV = [
  { type: "group" as const, label: "Workspace", children: [{ key: "general", label: "General" }, { key: "agents", label: "Agents" }, { key: "roles", label: "Roles" }, { key: "policy", label: "Policy" }, { key: "providers", label: "Providers" }, { key: "teams", label: "Teams" }] },
  { type: "group" as const, label: "Connect", children: [{ key: "accounts", label: "Accounts" }, { key: "devices", label: "Devices" }, { key: "memory", label: "Memory" }, { key: "knowledge", label: "Knowledge" }, { key: "trackers", label: "Trackers" }, { key: "remote", label: "Remote access" }] },
  { type: "group" as const, label: "Automate", children: [{ key: "playbooks", label: "Playbooks" }, { key: "skills", label: "Skills" }, { key: "prompts", label: "Prompts" }, { key: "triggers", label: "Triggers" }] },
  { type: "group" as const, label: "This device", children: [{ key: "doctor", label: "Doctor" }, { key: "session", label: "Session" }, { key: "shortcuts", label: "Shortcuts" }] },
];

/** The pages that edit the config; they render once it has loaded. */
const CONFIG_PAGES: Partial<Record<Tab, ComponentType>> = {
  general: GeneralPage,
  agents: AgentsPage,
  policy: PolicyPage,
  providers: ProvidersPage,
  teams: TeamsPage,
  accounts: AccountsPage,
  devices: DevicesPage,
  memory: MemoryPage,
  knowledge: KnowledgePage,
  trackers: TrackersPage,
  remote: RemotePage,
  playbooks: PlaybooksPage,
  skills: SkillsPage,
  prompts: PromptsPage,
  triggers: TriggersPage,
};

export function SettingsDialog() {
  const setDialog = useStore((s) => s.setDialog);
  const reload = useStore((s) => s.reload);
  const notify = useStore((s) => s.notify);
  const wanted = useStore((s) => s.settingsTab);

  const [tab, setTab] = useState<Tab>(wanted && TABS.includes(wanted as Tab) ? (wanted as Tab) : "general");
  const [cfg, setCfg] = useState<Config | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tsIp, setTsIp] = useState<string | null>(null);

  useEffect(() => {
    backend.getConfig().then(setCfg).catch((e) => setError(String(e)));
    backend.tailscaleInfo().then((i) => setTsIp(i.ip)).catch(() => {});
  }, []);

  const edit = (patch: Partial<Config>) => {
    setCfg((c) => (c ? { ...c, ...patch } : c));
    setDirty(true);
    setError(null);
  };
  const editMemory = (patch: Partial<MemoryConfig>) => cfg && edit({ memory: { ...cfg.memory, ...patch } });
  const editServe = (patch: Partial<Config["serve"]>) => cfg && edit({ serve: { ...cfg.serve, ...patch } });
  const editPolicy = (patch: Partial<Config["policy"]>) => cfg && edit({ policy: { ...cfg.policy, ...patch } });
  const editShare = (i: number, patch: Partial<Share>) => cfg && editServe({ shares: cfg.serve.shares.map((x, j) => (j === i ? { ...x, ...patch } : x)) });
  const setListItem = <K extends ListKey>(key: K, i: number | null, item: Config[K][number]) => {
    if (!cfg) return;
    const list = cfg[key] as Config[K][number][];
    edit({ [key]: i === null ? [...list, item] : list.map((x, j) => (j === i ? item : x)) } as Partial<Config>);
  };
  const removeListItem = (key: ListKey, i: number) => {
    if (!cfg) return;
    edit({ [key]: (cfg[key] as unknown[]).filter((_, j) => j !== i) } as Partial<Config>);
  };
  const nameTaken = (list: { name: string }[], name: string, index: number | null) => list.some((x, i) => x.name === name && i !== index);

  const save = async () => {
    if (!cfg) return;
    setSaving(true);
    setError(null);
    try {
      const { moved } = await backend.saveConfig(cfg);
      setDirty(false);
      await reload();
      setCfg(await backend.getConfig());
      notify(moved.length ? `Saved. Tokens moved to the ${SECRET_STORE}: ${moved.join(", ")}` : "Saved");
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };
  const discard = () => {
    backend.getConfig().then(setCfg).catch(() => {});
    setDirty(false);
  };
  const close = () => setDialog("none");
  const configDir = cfg?.path.replace(/\/[^/]+$/, "") ?? "";
  const providerOptions = useMemo(() => (cfg?.providers ?? []).map((p) => ({ value: p.name, label: p.name })), [cfg?.providers]);
  const roleOptions = useMemo(() => (cfg?.roles ?? []).map((r) => ({ value: r, label: r })), [cfg?.roles]);
  const settings: Settings = { cfg, setCfg, dirty, edit, editMemory, editServe, editPolicy, editShare, setListItem, removeListItem, nameTaken, providerOptions, roleOptions, configDir, tsIp };

  const editable = !READ_ONLY.includes(tab);
  const ConfigPage = CONFIG_PAGES[tab];

  return (
    <SettingsProvider value={settings}>
      <Dialog title="Settings" onClose={close} width={1140} className="settings-dialog" extra={<AppVersion onOpen={() => setTab("general")} />}>
        <div className="settings st-ant">
          <nav className="settings-nav" aria-label="Settings sections">
            <Menu mode="inline" selectedKeys={[tab]} items={NAV} onClick={(e) => setTab(e.key as Tab)} inlineIndent={12} />
          </nav>

          <div className="settings-page">
            <div className="st-scroll">
              {!cfg && !error && <div className="st-loading"><LoadingOutlined /> Loading…</div>}
              {error && <Alert type="error" showIcon title={error} className="st-alert" />}

              {ConfigPage && cfg && <ConfigPage />}
              {tab === "roles" && <RolesPage />}
              {tab === "doctor" && <DoctorPage />}
              {tab === "session" && <SessionPage />}
              {tab === "shortcuts" && <ShortcutsPage />}
            </div>

            {editable && (
              <footer className="settings-foot">
                <Text type="secondary" className="grow">{dirty && <Tag color="warning" variant="filled">Unsaved changes</Tag>}</Text>
                <Space>
                  {dirty ? (
                    <>
                      <Button icon={<ReloadOutlined />} onClick={discard}>Discard</Button>
                      <Button type="primary" disabled={!cfg} loading={saving} onClick={() => void save()}>Save changes</Button>
                    </>
                  ) : (
                    <Button onClick={close}>Close</Button>
                  )}
                </Space>
              </footer>
            )}
          </div>
        </div>
      </Dialog>
    </SettingsProvider>
  );
}


/** Next to the Settings title: the version running now, and a newer one when the update check found it. */
function AppVersion({ onOpen }: { onOpen: () => void }) {
  const update = useStore((s) => s.update);
  return (
    <div className="settings-version" aria-label="App version">
      <Text type="secondary">hubcodes {APP_VERSION}</Text>
      {update && <Tag color="processing" variant="filled" className="settings-version-new" onClick={onOpen} title="Open General → Updates to install it">{update.version} available</Tag>}
    </div>
  );
}
