import { useEffect, useState, type ReactNode } from "react";
import { Button, Input, Segmented, Select, Space, Tag, Tooltip } from "antd";
import { ExportOutlined, LinkOutlined, LockOutlined, LoginOutlined, ReloadOutlined } from "@ant-design/icons";
import { backend } from "../../lib/backend";
import type { Choice, Config, Connection, TwgProfile } from "../../lib/types";
import { useStore } from "../../store";
import { EditModal, Field, Fields, Result, type Check } from "./primitives";
import { useSettings } from "./context";
import { useSignIn } from "./mcpAuth";
import { SECRET_STORE } from "../../lib/platform";

/** What a pasted link points at. Pure, so forms can fill themselves from one paste. */
export type Link =
  | { kind: "jira"; site: string; project: string | null; issue: string | null }
  | { kind: "confluence"; site: string; space: string | null }
  | { kind: "plane"; api: string; workspace: string; project: string | null }
  | { kind: "github"; repo: string }
  | { kind: "git"; repo: string };

/**
 * Jira (board, backlog, issue, project, filter), Confluence (space, page),
 * Plane (project or issue, cloud or self-hosted) and GitHub links.
 */
export function parseLink(raw: string): Link | null {
  const text = raw.trim();
  if (/^git@[\w.-]+:[\w.-]+\/[\w.-]+(\.git)?$/.test(text)) {
    const m = text.match(/^git@github\.com:([\w.-]+\/[\w.-]+?)(\.git)?$/);
    return m ? { kind: "github", repo: m[1] } : { kind: "git", repo: text };
  }
  let u: URL;
  try {
    u = new URL(/^https?:\/\//.test(text) ? text : `https://${text}`);
  } catch {
    return null;
  }
  const parts = u.pathname.split("/").filter(Boolean);
  const site = `${u.protocol}//${u.host}`;
  if (u.host === "github.com" && parts.length >= 2) return { kind: "github", repo: `${parts[0]}/${parts[1].replace(/\.git$/, "")}` };
  if (parts[0] === "wiki") {
    const i = parts.indexOf("spaces");
    return { kind: "confluence", site, space: i >= 0 ? parts[i + 1] ?? null : null };
  }
  if (u.host.endsWith(".atlassian.net") || parts.includes("browse") || parts.includes("jira")) {
    const issue = parts[parts.indexOf("browse") + 1] ?? u.searchParams.get("selectedIssue");
    const pi = parts.indexOf("projects");
    const fromIssue = issue && /^[A-Z][A-Z0-9_]+-\d+$/.test(issue) ? issue : null;
    const project = pi >= 0 ? parts[pi + 1] ?? null : fromIssue ? fromIssue.split("-")[0] : u.searchParams.get("projectKey");
    return { kind: "jira", site, project: project ? project.toUpperCase() : null, issue: fromIssue };
  }
  const pi = parts.indexOf("projects");
  if (pi === 1 && (u.host.includes("plane") || /^[0-9a-f-]{36}$/i.test(parts[2] ?? ""))) {
    const api = u.host === "app.plane.so" ? "https://api.plane.so" : site;
    return { kind: "plane", api, workspace: parts[0], project: /^[0-9a-f-]{36}$/i.test(parts[2] ?? "") ? parts[2] : null };
  }
  return null;
}

/**
 * "Paste a link" shortcut: fills the form from what the link points at.
 * Folded away by default; the pickers are the main path.
 */
export function PasteLink({ placeholder, accept, onLink, hint, folded = true }: { placeholder: string; accept: Link["kind"][]; onLink: (l: Link) => void; hint?: ReactNode; folded?: boolean }) {
  const [open, setOpen] = useState(!folded);
  const [text, setText] = useState("");
  const [bad, setBad] = useState(false);
  const take = (v: string) => {
    setText(v);
    const l = parseLink(v);
    if (l && accept.includes(l.kind)) {
      setBad(false);
      onLink(l);
    } else setBad(v.trim().length > 8);
  };
  if (!open) return <button type="button" className="st-advanced-toggle" onClick={() => setOpen(true)}><LinkOutlined /> Have a link? Paste it instead</button>;
  return (
    <Field label="Paste a link" hint={bad ? <span className="st-bad">That link is not one this form reads. Fill the fields below instead.</span> : hint} span>
      <Input autoFocus={folded} prefix={<LinkOutlined />} className="mono" value={text} onChange={(e) => take(e.target.value)} placeholder={placeholder} spellCheck={false} allowClear aria-label="Paste a link" />
    </Field>
  );
}

const KIND_LABEL: Record<Connection["kind"], string> = { atlassian: "Atlassian (Jira, Confluence)", plane: "Plane", github: "GitHub" };
export const connectionKinds = (Object.keys(KIND_LABEL) as Connection["kind"][]).map((k) => ({ value: k, label: KIND_LABEL[k] }));
const TOKEN_PAGE: Record<Connection["kind"], string> = {
  atlassian: "https://id.atlassian.com/manage-profile/security/api-tokens",
  plane: "https://app.plane.so/profile/api-tokens",
  github: "",
};

/** A new Atlassian account starts on the twg sign-in: no token to make or paste. */
export const newConnection = (kind: Connection["kind"], seed: Partial<Connection> = {}): Connection => ({ name: "", kind, url: kind === "plane" ? "https://api.plane.so" : "", email: "", token: kind === "atlassian" ? "twg" : "", workspace: "", ...seed });

/** A name for a new connection from its site: hubstudio.atlassian.net → hubstudio. */
export const suggestName = (c: Connection) => {
  if (c.kind === "github") return "github";
  if (c.kind === "plane" && c.workspace) return c.workspace;
  try { return new URL(c.url).host.split(".")[0] || c.kind; } catch { return c.kind; }
};

/** The GitHub account needs no fields: `gh` is already signed in. */
export const githubConnection = (cfg: Config): Connection => cfg.connections.find((c) => c.kind === "github") ?? newConnection("github", { name: "github" });

const sameSite = (a: string, b: string) => a.trim().replace(/\/(wiki)?\/?$/, "").toLowerCase() === b.trim().replace(/\/(wiki)?\/?$/, "").toLowerCase();

/** The connection a form's own fields already match, if any. */
export function matchConnection(cfg: Config, kind: Connection["kind"], url: string, extra?: string): Connection | undefined {
  return cfg.connections.find((c) => c.kind === kind && (kind === "github" || (sameSite(c.url, url) && (!extra || c.email === extra || c.workspace === extra))));
}

/** `token = "twg"`: Atlassian through the twg CLI sign-in, no email or API token. */
export const TWG = "twg";
export const usesTwg = (token: string) => token.trim() === TWG;
/** What a tracker or source copies as its token: the account's reference, or "twg". */
export const tokenRef = (c: Connection) => (c.token.trim().startsWith("${") || usesTwg(c.token) ? c.token.trim() : `\${keychain:connection:${c.name}}`);

/** Token field: a pasted token is stored in the Keychain on save; a reference shows as stored. */
export function SecretInput({ value, onChange, placeholder, label }: { value: string; onChange: (v: string) => void; placeholder?: string; label: string }) {
  if (usesTwg(value)) {
    return (
      <Space.Compact className="st-wide">
        <Input prefix={<LockOutlined />} value="Signs in through twg" disabled aria-label={label} />
        <Button onClick={() => onChange("")}>Use a token</Button>
      </Space.Compact>
    );
  }
  const stored = /^\$\{keychain:[^}]+\}$/.test(value.trim());
  const env = !stored && /^\$\{[A-Z0-9_]+\}$/.test(value.trim());
  if (stored) {
    return (
      <Space.Compact className="st-wide">
        <Input prefix={<LockOutlined />} value={`Stored in the ${SECRET_STORE}`} disabled aria-label={label} />
        <Button onClick={() => onChange("")}>Replace</Button>
      </Space.Compact>
    );
  }
  return (
    <Input.Password
      className="mono"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder ?? "Paste a token"}
      aria-label={label}
      suffix={env ? <Tag variant="filled">from the environment</Tag> : undefined}
    />
  );
}

/** Choices read from the account (projects, statuses, …), cached per key while the dialog is open. */
const choiceCache = new Map<string, Promise<{ ok: boolean; choices: Choice[]; error?: string }>>();
export function useChoices(conn: Connection | null | undefined, what: string, arg = "", enabled = true) {
  const [state, setState] = useState<{ loading: boolean; choices: Choice[]; error: string | null }>({ loading: false, choices: [], error: null });
  const [nonce, setNonce] = useState(0);
  const ready = !!conn && enabled && (conn.kind === "github" || (!!conn.url && !!conn.token)) && (what !== "statuses" && what !== "labels" ? true : !!arg);
  const key = conn ? JSON.stringify([conn.kind, conn.url, conn.email, conn.token, conn.workspace, what, arg]) : "";
  useEffect(() => {
    if (!ready || !conn) { setState({ loading: false, choices: [], error: null }); return; }
    let live = true;
    if (nonce) choiceCache.delete(key);
    if (!choiceCache.has(key)) choiceCache.set(key, backend.connectionChoices(conn, what, arg).catch((e) => ({ ok: false, choices: [], error: String(e) })));
    setState((s) => ({ ...s, loading: true, error: null }));
    void choiceCache.get(key)!.then((r) => {
      if (!live) return;
      if (!r.ok) choiceCache.delete(key);
      setState({ loading: false, choices: r.choices, error: r.ok ? null : r.error ?? "could not read from the account" });
    });
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, ready, nonce]);
  return { ...state, ready, reload: () => setNonce((n) => n + 1) };
}

/**
 * A select filled from the account. Typing a value that is not listed is
 * still allowed (a status the API did not return, an account without the
 * permission to list), so the picker never blocks the form.
 */
export function ChoiceSelect({ conn, what, arg, value, onChange, multiple, placeholder, label, extra = [] }: { conn: Connection | null | undefined; what: string; arg?: string; value: string | string[]; onChange: (v: never) => void; multiple?: boolean; placeholder?: string; label: string; extra?: Choice[] }) {
  const c = useChoices(conn, what, arg);
  const [typed, setTyped] = useState("");
  const values = Array.isArray(value) ? value : value ? [value] : [];
  const known = [...extra, ...c.choices];
  const options = [
    ...known.map((x) => ({ value: x.value, label: x.label })),
    ...values.filter((v) => !known.some((k) => k.value === v)).map((v) => ({ value: v, label: v })),
    ...(typed.trim() && !known.some((k) => k.value.toLowerCase() === typed.trim().toLowerCase()) ? [{ value: typed.trim(), label: `Use “${typed.trim()}”` }] : []),
  ];
  const note = !c.ready ? (conn ? null : "Pick an account first") : c.error ? c.error : null;
  return (
    <div className="st-choice">
      <Space.Compact className="st-wide">
        <Select
          className="st-wide"
          mode={multiple ? "multiple" : undefined}
          showSearch
          allowClear
          value={multiple ? values : values[0]}
          onChange={(v) => { setTyped(""); (onChange as (v: unknown) => void)(v ?? (multiple ? [] : "")); }}
          onSearch={setTyped}
          options={options}
          loading={c.loading}
          placeholder={c.loading ? "Reading from the account…" : placeholder}
          optionFilterProp="label"
          notFoundContent={c.loading ? "Reading…" : "Type to use a value"}
          aria-label={label}
        />
        {c.ready && <Tooltip title="Read again"><Button icon={<ReloadOutlined />} onClick={c.reload} aria-label={`Reload ${label}`} /></Tooltip>}
      </Space.Compact>
      {note && <div className={`st-choice-note${c.error ? " is-bad" : ""}`}>{note}</div>}
    </div>
  );
}

/** Add or edit one connection; Check signs in with what is typed. */
export function ConnectionForm({ draft: c, onChange }: { draft: Connection; onChange: (p: Partial<Connection>) => void }) {
  const [check, setCheck] = useState<Check | null>(null);
  const signIn = useSignIn();
  // Editing an account that has its address: the paste shortcut is folded.
  const [existing] = useState(() => !!c.url.trim() && c.kind !== "plane");
  const run = async () => {
    setCheck({ ok: true, text: "Checking…", busy: true });
    try {
      const who = await backend.connectionCheck(c);
      setCheck({ ok: true, text: c.kind === "plane" ? `Connected: ${who}` : `Signed in as ${who}` });
    } catch (e) {
      setCheck({ ok: false, text: String(e).replace(/^Error: /, "") });
    }
  };
  const page = TOKEN_PAGE[c.kind];
  const twg = c.kind === "atlassian" && usesTwg(c.token);
  const [profiles, setProfiles] = useState<TwgProfile[]>([]);
  useEffect(() => { if (twg) void backend.twgProfiles().then(setProfiles).catch(() => setProfiles([])); }, [twg]);
  const signedIn = profiles.filter((p) => !sameSite(p.url, c.url));
  return (
    <Fields cols={1}>
      {c.kind === "atlassian" && (
        <PasteLink folded={existing} placeholder="https://yourteam.atlassian.net/jira/software/projects/PE/boards/1" accept={["jira", "confluence"]} onLink={(l) => "site" in l && onChange({ url: l.site, name: c.name || suggestName({ ...c, url: l.site }) })} hint="Any Jira or Confluence link from your site fills the address." />
      )}
      {c.kind === "plane" && (
        <PasteLink folded={false} placeholder="https://app.plane.so/your-workspace/projects/…" accept={["plane"]} onLink={(l) => l.kind === "plane" && onChange({ url: l.api, workspace: l.workspace, name: c.name || l.workspace })} hint="A project link from Plane fills the address and workspace." />
      )}
      {c.kind === "atlassian" && (
        <Field label="Sign in with" hint={twg ? <>Uses the Atlassian sign-in of the <code>twg</code> CLI on this device; nothing is stored here. Not signed in yet? <a onClick={() => void signIn("twg")}>Sign in to twg</a>.</> : "An API token from your Atlassian profile, with the email it belongs to."}>
          <Segmented
            value={twg ? "twg" : "token"}
            onChange={(v) => { setCheck(null); onChange(v === "twg" ? { token: TWG, email: "" } : { token: "" }); }}
            options={[{ value: "twg", label: "twg login" }, { value: "token", label: "API token" }]}
            aria-label="Sign in with"
          />
        </Field>
      )}
      <Fields>
        <Field label="Name" required hint="How trackers and sources refer to it."><Input className="mono" value={c.name} onChange={(e) => onChange({ name: e.target.value })} placeholder={suggestName(c)} spellCheck={false} /></Field>
        {c.kind === "atlassian" && <Field label="Site" hint={twg && signedIn.length > 0 ? <>twg is signed in to {signedIn.map((p, i) => <span key={p.url}>{i > 0 && ", "}<a onClick={() => onChange({ url: p.url, name: c.name || suggestName({ ...c, url: p.url }) })}>{p.url.replace(/^https?:\/\//, "")}</a></span>)}</> : undefined}><Input className="mono" value={c.url} onChange={(e) => onChange({ url: e.target.value })} placeholder="https://yourteam.atlassian.net" spellCheck={false} /></Field>}
        {c.kind === "plane" && <Field label="Address" hint="Plane Cloud is https://api.plane.so; self-hosted is your own address."><Input className="mono" value={c.url} onChange={(e) => onChange({ url: e.target.value })} spellCheck={false} /></Field>}
        {c.kind === "plane" && <Field label="Workspace"><Input className="mono" value={c.workspace} onChange={(e) => onChange({ workspace: e.target.value })} placeholder="your-workspace" spellCheck={false} /></Field>}
      </Fields>
      <Fields>
        {c.kind === "atlassian" && !twg && <Field span label="Email" hint="The one you sign in to Atlassian with."><Input className="mono" value={c.email} onChange={(e) => onChange({ email: e.target.value })} placeholder="you@company.com" spellCheck={false} /></Field>}
        {c.kind !== "github" && !twg && (
          <Field span label={c.kind === "plane" ? "API key" : "API token"} hint={<>Saved to this device's {SECRET_STORE}, not to the settings file. {page && <a onClick={() => void backend.openPath(page)}>Create one <ExportOutlined /></a>}</>}>
            <SecretInput value={c.token} onChange={(v) => onChange({ token: v })} label={c.kind === "plane" ? "API key" : "API token"} />
          </Field>
        )}
      </Fields>
      {c.kind === "github" && <div className="st-note st-note-action"><span>GitHub uses the <code>gh</code> sign-in on this device.</span><Button size="small" icon={<LoginOutlined />} onClick={() => void signIn("github")}>Sign in to GitHub</Button></div>}
      <Space><Button onClick={() => void run()} disabled={c.kind !== "github" && (!c.url || !c.token)}>Check</Button><Result r={check} /></Space>
    </Fields>
  );
}

/**
 * Pick the account a form reads from. "Connect an account…" opens the
 * connection form in place; the new account is added to the settings and
 * picked at once.
 */
export function ConnectionSelect({ kind, value, onPick, auto }: { kind: Connection["kind"]; value: Connection | undefined; onPick: (c: Connection) => void; auto?: boolean }) {
  const { cfg, edit } = useSettings();
  const notify = useStore((s) => s.notify);
  const [adding, setAdding] = useState<Connection | null>(null);
  const list = cfg.connections.filter((c) => c.kind === kind);
  // A new form with one account of this kind needs no choice: take it.
  const only = auto && !value && list.length === 1 ? list[0] : undefined;
  useEffect(() => {
    if (only) onPick(only);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [only?.name]);
  const add = () => {
    if (!adding) return;
    const c = { ...adding, name: (adding.name || suggestName(adding)).trim() };
    if (cfg.connections.some((x) => x.name === c.name)) return notify(`An account called ${c.name} already exists`, "error");
    edit({ connections: [...cfg.connections, c] });
    onPick(c);
    setAdding(null);
  };
  return (
    <>
      <Select
        className="st-wide"
        value={value?.name}
        placeholder="Pick an account"
        aria-label="Account"
        options={[
          ...list.map((c) => ({ value: c.name, label: <span>{c.name} <span className="muted">{c.kind === "github" ? "gh" : c.url.replace(/^https?:\/\//, "")}{c.email && !c.email.startsWith("${") ? ` · ${c.email}` : ""}{usesTwg(c.token) ? " · twg" : ""}</span></span> })),
          { value: "__new", label: <span className="st-add-opt">+ Connect {kind === "atlassian" ? "an Atlassian" : kind === "plane" ? "a Plane" : "a GitHub"} account…</span> },
        ]}
        onChange={(v: string) => (v === "__new" ? setAdding(newConnection(kind)) : onPick(list.find((c) => c.name === v)!))}
      />
      <EditModal title={`Connect ${KIND_LABEL[kind]}`} open={!!adding} onClose={() => setAdding(null)} onApply={add} applyLabel="Connect" width={560}>
        {adding && <ConnectionForm draft={adding} onChange={(p) => setAdding({ ...adding, ...p })} />}
      </EditModal>
    </>
  );
}
