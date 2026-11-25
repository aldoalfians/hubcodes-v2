import { useEffect, useState } from "react";
import { Button, Dropdown, Tag } from "antd";
import { ApiOutlined, PlusOutlined } from "@ant-design/icons";
import { backend } from "../../lib/backend";
import type { Config, Connection, TwgProfile } from "../../lib/types";
import { ConnectionForm, TWG, connectionKinds, newConnection, suggestName, tokenRef, usesTwg } from "./connect";
import { EditModal, ItemList, ItemRow, Result, Section, type Check, Notice } from "./primitives";
import { kindLabel, type Editing } from "./shared";
import { useSettings } from "./context";
import { useStore } from "../../store";
import { SECRET_STORE } from "../../lib/platform";

const plain = (v: string | undefined) => !!v && v.trim() !== "" && !v.includes("${") && !usesTwg(v);
const siteOf = (u: string) => u.trim().replace(/\/(wiki)?\/?$/, "").toLowerCase();

/**
 * What "Sync from twg" does: an account for each site twg is signed in to
 * that has none yet, and the sign-in of each API-token account on such a
 * site moved to twg, with the trackers and sources that copied its token.
 */
export function twgSync(cfg: Config, profiles: TwgProfile[]): { patch: Partial<Config>; added: Connection[]; switched: Connection[]; copies: number } {
  const on = (p: TwgProfile) => cfg.connections.filter((c) => c.kind === "atlassian" && siteOf(c.url) === siteOf(p.url));
  const switched = profiles.flatMap((p) => on(p).filter((c) => !usesTwg(c.token)));
  const taken = new Set(cfg.connections.map((c) => c.name));
  const added = profiles.filter((p) => on(p).length === 0).map((p) => {
    const base = suggestName(newConnection("atlassian", { url: p.url }));
    let name = base;
    for (let i = 2; taken.has(name); i++) name = `${base}-${i}`;
    taken.add(name);
    return newConnection("atlassian", { name, url: p.url });
  });
  const old = switched.map((c) => [siteOf(c.url), tokenRef(c)] as const);
  const copied = (url: string, token: string) => old.some(([s, r]) => s === siteOf(url) && r === token.trim());
  let copies = 0;
  const move = <T extends { url?: string; token: string; email: string }>(x: T, atl: boolean): T => (atl && copied(x.url ?? "", x.token) ? (copies++, { ...x, token: TWG, email: "" }) : x);
  const names = new Set(switched.map((c) => c.name));
  const patch: Partial<Config> = {
    connections: [...cfg.connections.map((c) => (names.has(c.name) ? { ...c, token: TWG, email: "" } : c)), ...added],
    trackers: cfg.trackers.map((tr) => move(tr, tr.kind === "jira")),
    kbs: cfg.kbs.map((k) => move(k, k.kind === "jira" || k.kind === "confluence")),
  };
  return { patch, added, switched, copies };
}
const secretHeader = (k: string) => /authorization|token|key|secret|cookie/i.test(k);

/** Places in the settings that still hold a token in plain text. */
export function plainSecrets(cfg: Config): string[] {
  const out: string[] = [];
  cfg.connections.forEach((c) => plain(c.token) && out.push(`account ${c.name}`));
  cfg.trackers.forEach((t) => (plain(t.token) || plain(t.api_key) || Object.entries(t.headers ?? {}).some(([k, v]) => secretHeader(k) && plain(v))) && out.push(`tracker ${t.name}`));
  cfg.kbs.forEach((k) => (plain(k.token) || Object.entries(k.headers ?? {}).some(([h, v]) => secretHeader(h) && plain(v))) && out.push(`knowledge source ${k.name}`));
  if (plain(cfg.memory?.hindsight?.api_key) || Object.entries(cfg.memory?.mcp?.headers ?? {}).some(([k, v]) => secretHeader(k) && plain(v))) out.push("team memory");
  return out;
}

/** Accounts connected once; trackers, knowledge sources and triggers pick from them. */
export function AccountsPage() {
  const { cfg, edit, setListItem, removeListItem, nameTaken } = useSettings();
  const [twg, setTwg] = useState<TwgProfile[]>([]);
  useEffect(() => { void backend.twgProfiles().then(setTwg).catch(() => setTwg([])); }, []);
  const plan = twgSync(cfg, twg);
  const notify = useStore((s) => s.notify);
  const [editing, setEditing] = useState<Editing<Connection>>(null);
  const [checks, setChecks] = useState<Record<string, Check>>({});
  const loose = plainSecrets(cfg);
  const used = (c: Connection) => {
    const n = cfg.trackers.filter((t) => (c.kind === "atlassian" && t.kind === "jira" && t.url === c.url) || (c.kind === "plane" && t.kind === "plane" && t.workspace === c.workspace) || (c.kind === "github" && t.kind === "github")).length
      + cfg.kbs.filter((k) => c.kind === "atlassian" && (k.kind === "jira" || k.kind === "confluence") && k.url.replace(/\/wiki\/?$/, "") === c.url).length;
    return n;
  };
  const check = async (c: Connection) => {
    setChecks((r) => ({ ...r, [c.name]: { ok: true, text: "Checking…", busy: true } }));
    try {
      const who = await backend.connectionCheck(c);
      setChecks((r) => ({ ...r, [c.name]: { ok: true, text: c.kind === "plane" ? who : `Signed in as ${who}` } }));
    } catch (e) {
      setChecks((r) => ({ ...r, [c.name]: { ok: false, text: String(e).replace(/^Error: /, "") } }));
    }
  };
  const sync = () => {
    edit(plan.patch);
    const done = [...plan.added.map((c) => `added ${c.name}`), ...plan.switched.map((c) => `${c.name} now signs in through twg`), ...(plan.copies ? [`${plan.copies} tracker${plan.copies === 1 ? "" : "s"} and source${plan.copies === 1 ? "" : "s"} follow`] : [])];
    notify(`Synced from twg: ${done.join("; ")}. Save changes to keep it.`);
    [...plan.added, ...plan.switched].forEach((c) => void check({ ...c, token: TWG, email: "" }));
  };
  const apply = () => {
    if (!editing) return;
    const d = { ...editing.draft, name: (editing.draft.name || suggestName(editing.draft)).trim() };
    if (nameTaken(cfg.connections, d.name, editing.index)) return notify(`An account called ${d.name} already exists`, "error");
    if (d.kind !== "github" && (!d.url || !d.token)) return notify(d.kind === "plane" ? "An account needs its address and an API key" : "An account needs the site and an API token", "error");
    if (d.kind === "atlassian" && !d.email && !usesTwg(d.token)) return notify("An Atlassian account needs the email you sign in with", "error");
    setListItem("connections", editing.index, d);
    setEditing(null);
  };
  return (
    <Section
      title="Accounts"
      tagline="Connect Jira, Confluence, Plane and GitHub once; trackers and sources pick from here."
      help={<>Tokens are kept in this device's {SECRET_STORE}; the settings file only says where (<code>{"${keychain:…}"}</code>). Each device has its own {SECRET_STORE}, so connect the account on every device that polls with it. From a terminal: <code>hubcodes secret set NAME</code>.</>}
      extra={<Dropdown menu={{ items: connectionKinds.map((k) => ({ key: k.value, label: k.label })), onClick: ({ key }) => setEditing({ index: null, draft: newConnection(key as Connection["kind"]) }) }}><Button type="primary" icon={<PlusOutlined />}>Connect an account</Button></Dropdown>}
    >
      {(plan.added.length > 0 || plan.switched.length > 0) && (
        <Notice action={<Button size="small" onClick={sync}>{plan.added.length ? "Add from twg" : "Use twg"}</Button>}>
          <b>twg</b> is signed in to {twg.filter((p) => [...plan.added, ...plan.switched].some((c) => siteOf(c.url) === siteOf(p.url))).map((p) => p.url.replace(/^https?:\/\//, "")).join(", ")}
          <span className="st-notice-quiet">
            {plan.added.length ? ", which has no account here yet." : `; ${plan.switched.map((c) => c.name).join(", ")} still sign${plan.switched.length === 1 ? "s" : ""} in with a token.`}
            {plan.copies ? ` ${plan.copies} tracker${plan.copies === 1 ? "" : "s"} or source${plan.copies === 1 ? "" : "s"} follow.` : ""}
          </span>
        </Notice>
      )}
      {loose.length > 0 && (
        <Notice tone="warn">
          <b>{loose.length === 1 ? "A token is" : `${loose.length} tokens are`} in the settings file as plain text</b>
          <span className="st-notice-quiet"> ({loose.join(", ")}). Save changes moves {loose.length === 1 ? "it" : "them"} into the {SECRET_STORE}.</span>
        </Notice>
      )}
      <ItemList empty="No accounts yet. Connect Atlassian, Plane or GitHub, then pick it when you add a tracker or a source.">
        {cfg.connections.map((c, i) => (
          <ItemRow
            key={i}
            name={<span className="mono">{c.name}</span>}
            meta={<Tag variant="filled">{kindLabel(connectionKinds, c.kind)}</Tag>}
            summary={<span className="mono">{c.kind === "github" ? "gh sign-in on this device" : `${c.url.replace(/^https?:\/\//, "")}${c.email ? ` · ${c.email}` : ""}${usesTwg(c.token) ? " · twg sign-in" : ""}${c.workspace ? ` · ${c.workspace}` : ""}`}{used(c) ? <span className="muted"> · used by {used(c)}</span> : null}</span>}
            status={<Result r={checks[c.name]} />}
            actions={<Button size="small" icon={<ApiOutlined />} onClick={() => void check(c)}>Check</Button>}
            onOpen={() => setEditing({ index: i, draft: { ...c } })}
            onRemove={() => removeListItem("connections", i)}
            removeTitle={`Remove the ${c.name} account? Trackers that use it keep their copy of the site and token.`}
          />
        ))}
      </ItemList>
      <EditModal title={editing?.index === null ? `Connect ${kindLabel(connectionKinds, editing?.draft.kind ?? "")}` : `Edit ${editing?.draft.name ?? ""}`} open={!!editing} onClose={() => setEditing(null)} onApply={apply} applyLabel={editing?.index === null ? "Connect" : "Apply"} width={640}>
        {editing && <ConnectionForm draft={editing.draft} onChange={(p) => setEditing({ ...editing, draft: { ...editing.draft, ...p } })} />}
      </EditModal>
    </Section>
  );
}
