import { useState } from "react";
import { Button, Dropdown, Input, Select, Space, Tag, Typography } from "antd";
import { ApiOutlined, PlusOutlined, SearchOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { backend } from "../../lib/backend";
import type { Connection, KbSource } from "../../lib/types";
import { ChoiceSelect, ConnectionSelect, PasteLink, SecretInput, matchConnection, tokenRef, type Link } from "./connect";
import { EditModal, Field, Fields, FormGroup, ItemList, ItemRow, Result, Section, ToggleField, ToolArgs, headersText, parseHeaders, type Check } from "./primitives";
import { code, hostOf, kindLabel, type Editing } from "./shared";
import { McpServerFields, bridgedUrl, serverOf } from "./mcpAuth";
import { useSettings } from "./context";

const { Text } = Typography;


const KB_KINDS: { value: KbSource["kind"]; label: string }[] = [
  { value: "mcp", label: "MCP server" },
  { value: "git", label: "Git repo of docs" },
  { value: "folder", label: "Folder" },
  { value: "confluence", label: "Confluence" },
  { value: "jira", label: "Jira" },
  { value: "command", label: "Command" },
];
const newKb = (kind: KbSource["kind"]): KbSource => ({ name: "", kind, url: "", command: "", args: [], headers: {}, search_tool: "", search_args: null, get_tool: "", get_args: null, path: "", repo: "", email: "", token: "", space: "", project: "", run: "", enabled: true });

export function KnowledgePage() {
  const { cfg, setListItem, removeListItem, nameTaken } = useSettings();
  const notify = useStore((s) => s.notify);
  const [kbResult, setKbResult] = useState<Record<string, Check>>({});
  const [kbEdit, setKbEdit] = useState<Editing<KbSource>>(null);
  const testKb = async (k: KbSource) => {
    setKbResult((r) => ({ ...r, [k.name]: { ok: true, text: "Testing…", busy: true } }));
    const c = await backend.kbCheck(k).catch((e) => ({ name: k.name, ok: false, detail: String(e), ms: 0 }));
    setKbResult((r) => ({ ...r, [k.name]: { ok: c.ok, text: `${c.detail} (${c.ms} ms)` } }));
  };
  const applyKb = () => {
    if (!cfg || !kbEdit) return;
    const d = { ...kbEdit.draft, name: kbEdit.draft.name.trim() };
    if (!d.name) return notify("A source needs a name", "error");
    if (nameTaken(cfg.kbs, d.name, kbEdit.index)) return notify(`A source called ${d.name} already exists`, "error");
    setListItem("kbs", kbEdit.index, d);
    setKbEdit(null);
  };
  return (
    <Section
      title="Knowledge"
      tagline="Where agents look things up: docs, wikis, tickets."
      help={<>{code("hubcodes kb search")} asks every enabled source; {code("hubcodes kb get")} reads one page. ⌘⇧K searches from here. Any MCP server with a search tool works, so does a docs repo, a folder, Confluence, Jira, or a command.</>}
      extra={<Dropdown menu={{ items: KB_KINDS.map((k) => ({ key: k.value, label: k.label })), onClick: ({ key }) => setKbEdit({ index: null, draft: newKb(key as KbSource["kind"]) }) }}><Button type="primary" icon={<PlusOutlined />}>Add source</Button></Dropdown>}
    >
      <ItemList empty="No sources yet. Add an MCP server, a docs repo, a folder, Confluence, Jira, or a command." onAdd={() => setKbEdit({ index: null, draft: newKb("mcp") })} addLabel="Add source">
        {cfg.kbs.map((k, i) => (
          <ItemRow
            key={i}
            name={<span className="mono">{k.name || "untitled"}</span>}
            meta={<Tag variant="filled">{kindLabel(KB_KINDS, k.kind)}</Tag>}
            summary={<span className="mono">{k.kind === "mcp" ? (bridgedUrl(k) ? `${hostOf(bridgedUrl(k)!)} · browser sign-in` : k.url ? hostOf(k.url) : k.command || "no server yet") : k.kind === "git" ? k.repo : k.kind === "folder" ? k.path : k.kind === "command" ? k.run : `${k.url ? hostOf(k.url) : ""}${k.kind === "confluence" && k.space ? ` · ${k.space}` : k.kind === "jira" && k.project ? ` · ${k.project}` : ""}`}</span>}
            status={<Result r={kbResult[k.name]} />}
            enabled={k.enabled}
            onEnable={(v) => setListItem("kbs", i, { ...k, enabled: v })}
            actions={<Button size="small" icon={<ApiOutlined />} onClick={() => void testKb(k)} disabled={!k.name.trim()}>Test</Button>}
            onOpen={() => setKbEdit({ index: i, draft: structuredClone(k) })}
            onRemove={() => removeListItem("kbs", i)}
            removeTitle={`Remove ${k.name || "this source"}?`}
            off={!k.enabled}
          />
        ))}
      </ItemList>
      <EditModal
        title={kbEdit?.index === null ? "Add source" : `Edit ${kbEdit?.draft.name ?? ""}`}
        open={!!kbEdit}
        onClose={() => setKbEdit(null)}
        onApply={applyKb}
        applyLabel={kbEdit?.index === null ? "Add source" : "Apply"}
        extra={kbEdit && <Space><Button icon={<ApiOutlined />} onClick={() => void testKb(kbEdit.draft)} disabled={!kbEdit.draft.name.trim()}>Test</Button><Result r={kbResult[kbEdit.draft.name]} /></Space>}
      >
        {kbEdit && <KbForm draft={kbEdit.draft} onChange={(patch) => setKbEdit({ ...kbEdit, draft: { ...kbEdit.draft, ...patch } })} />}
      </EditModal>
    </Section>
  );
}

function KbForm({ draft: k, onChange }: { draft: KbSource; onChange: (p: Partial<KbSource>) => void }) {
  const { cfg } = useSettings();
  const notify = useStore((s) => s.notify);
  const [advanced, setAdvanced] = useState(false);
  const [finding, setFinding] = useState(false);
  const [signedIn, setSignedIn] = useState<string | null>(null);
  const atl = k.kind === "jira" || k.kind === "confluence";
  const site = k.url.replace(/\/wiki\/?$/, "");
  const conn: Connection | undefined = atl ? matchConnection(cfg, "atlassian", site, k.email) : undefined;
  const named = (n: string) => (k.name.trim() ? {} : { name: n.toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-|-$/g, "") });
  const fromConn = (c: Connection): Partial<KbSource> => ({ url: k.kind === "confluence" ? `${c.url}/wiki` : c.url, email: c.email, token: tokenRef(c) });
  const pick = (c: Connection) => onChange(fromConn(c));
  // One patch per paste: the form's onChange merges into the draft it was rendered with.
  const onLink = (l: Link) => {
    if (l.kind === "jira" || l.kind === "confluence") {
      const c = matchConnection(cfg, "atlassian", l.site);
      if (!c) notify(`No account for ${l.site.replace(/^https?:\/\//, "")} yet: pick “Connect an Atlassian account…”`);
      onChange({
        ...(c ? fromConn(c) : { url: k.kind === "confluence" ? `${l.site}/wiki` : l.site }),
        ...(l.kind === "confluence" && l.space ? { space: l.space, ...named(`confluence-${l.space}`) } : {}),
        ...(l.kind === "jira" && l.project ? { project: l.project, ...named(`jira-${l.project}`) } : {}),
      });
    } else if (l.kind === "github" || l.kind === "git") onChange({ repo: l.kind === "github" ? `https://github.com/${l.repo}.git` : l.repo, ...named(l.kind === "github" ? l.repo.split("/")[1] : "docs") });
  };
  const serverUrl = serverOf(k);
  const discover = async () => {
    setFinding(true);
    try {
      const d = await backend.mcpDiscover({ url: k.url, command: k.command, args: k.args, headers: k.headers, env: {} }, ["search", "get"]);
      setSignedIn(`Signed in · ${d.tools.length} tool${d.tools.length === 1 ? "" : "s"} on the server`);
      const p = d.picks;
      onChange({ ...(p.search ? { search_tool: p.search[0], search_args: p.search[1] } : {}), ...(p.get ? { get_tool: p.get[0], get_args: p.get[1] } : {}), ...named(hostName(serverUrl)) });
      if (p.search) notify(`Found ${d.tools.length} tools; searching with ${p.search[0]}${p.get ? `, reading with ${p.get[0]}` : ""}`);
      else { notify(`Found ${d.tools.length} tools, none that searches. Pick one under Advanced.`, "error"); setAdvanced(true); }
    } catch (e) {
      notify(String(e).replace(/^Error: /, ""), "error");
    } finally {
      setFinding(false);
    }
  };
  return (
    <Fields cols={1}>
      <Fields>
        <Field label="Name" required><Input className="mono" value={k.name} onChange={(e) => onChange({ name: e.target.value })} placeholder="product-docs" spellCheck={false} aria-label="Name" /></Field>
        {atl ? <Field label="Account"><ConnectionSelect kind="atlassian" value={conn} onPick={pick} auto={!k.url} /></Field> : <Field label="Kind"><Select className="st-wide" value={k.kind} options={KB_KINDS} onChange={(v) => onChange({ kind: v })} /></Field>}
      </Fields>
      {atl && <PasteLink placeholder={k.kind === "confluence" ? "https://yourteam.atlassian.net/wiki/spaces/ENG/overview" : "https://yourteam.atlassian.net/jira/software/projects/PE/boards/1"} accept={["jira", "confluence"]} onLink={onLink} hint={k.kind === "confluence" ? "A space or page link fills the site and the space." : "A board or issue link fills the site and the project."} />}
      {k.kind === "git" && <PasteLink placeholder="https://github.com/org/docs" accept={["github", "git"]} onLink={onLink} />}
      {atl && (
        <Field label={k.kind === "confluence" ? "Space" : "Project"} hint="Optional. Every search stays inside it.">
          <ChoiceSelect conn={conn} what={k.kind === "confluence" ? "spaces" : "projects"} value={k.kind === "confluence" ? k.space : k.project} onChange={(v: string) => onChange(k.kind === "confluence" ? { space: v, ...named(v ? `confluence-${v}` : "") } : { project: v, ...named(v ? `jira-${v}` : "") })} label={k.kind === "confluence" ? "Space" : "Project"} placeholder={k.kind === "confluence" ? "Every space" : "Every project"} />
        </Field>
      )}
      {k.kind === "mcp" && (
        <FormGroup title="MCP server">
          <Fields>
            <McpServerFields target={k} onChange={onChange} placeholder="https://kb.example/mcp/" signIn={{ run: discover, busy: finding, done: signedIn }} />
            <Field label="Tools" hint={k.search_tool ? undefined : "Ask the server which tools it has; the search and read tools are picked for you."} span>
              <span className="st-tools-line">
                {k.search_tool && <span><Text type="secondary">Search</Text> <code>{k.search_tool}</code></span>}
                {k.get_tool && <span><Text type="secondary">Read</Text> <code>{k.get_tool}</code></span>}
                <Button size="small" icon={<SearchOutlined />} loading={finding} onClick={() => void discover()} disabled={!serverUrl && !k.command}>{k.search_tool ? "Find again" : "Find the tools"}</Button>
              </span>
            </Field>
          </Fields>
        </FormGroup>
      )}
      {k.kind === "git" && <Field label="Repository" hint="Cloned under ~/.local/state/hubcodes/kb/ and pulled every hour. Markdown and text files are searched."><Input className="mono" value={k.repo} onChange={(e) => onChange({ repo: e.target.value })} placeholder="git@github.com:org/docs.git" spellCheck={false} /></Field>}
      {k.kind === "folder" && <Field label="Folder" hint="Markdown and text files inside are searched."><Input className="mono" value={k.path} onChange={(e) => onChange({ path: e.target.value })} placeholder="~/docs" spellCheck={false} /></Field>}
      {k.kind === "command" && <Field label="Command" hint={<>Gets JSON on stdin: {code('{op: "search", query}')} or {code('{op: "get", id}')}. Prints JSON hits or text.</>}><Input className="mono" value={k.run} onChange={(e) => onChange({ run: e.target.value })} placeholder="mytool kb" spellCheck={false} /></Field>}
      <ToggleField label="Search this source" hint="Off keeps it here without searching it." checked={k.enabled} onChange={(v) => onChange({ enabled: v })} />
      {(k.kind === "mcp" || atl) && <button type="button" className="st-advanced-toggle" onClick={() => setAdvanced((a) => !a)} aria-expanded={advanced}>{advanced ? "Hide advanced" : "Advanced: raw fields"}</button>}
      {advanced && k.kind === "mcp" && (
        <FormGroup title="Advanced">
          <Fields>
            <Field label="Command (stdio)"><Input className="mono" value={k.command ?? ""} onChange={(e) => onChange({ command: e.target.value })} placeholder="npx -y @org/kb-mcp" spellCheck={false} /></Field>
            <Field label="Headers" hint={<>One per line, {code("Name: value")}.</>}><Input.TextArea rows={2} className="mono" value={headersText(k.headers)} onChange={(e) => onChange({ headers: parseHeaders(e.target.value) })} spellCheck={false} /></Field>
            <Field label="Search tool" hint={<>Name, then arguments as JSON; {code("{query}")} is the question.</>} span>
              <ToolArgs tool={k.search_tool} args={k.search_args} fallback={{ query: "{query}" }} placeholder="search" onTool={(v) => onChange({ search_tool: v })} onArgs={(v) => onChange({ search_args: v })} label="Search tool" />
            </Field>
            <Field label="Get tool" hint={<>Optional. {code("{id}")} is the reference from a search hit.</>} span>
              <ToolArgs tool={k.get_tool} args={k.get_args} fallback={{ id: "{id}" }} placeholder="get" onTool={(v) => onChange({ get_tool: v })} onArgs={(v) => onChange({ get_args: v })} label="Get tool" />
            </Field>
          </Fields>
        </FormGroup>
      )}
      {advanced && atl && (
        <FormGroup title="Advanced">
          <Fields>
            <Field label="Site URL" span><Input className="mono" value={k.url ?? ""} onChange={(e) => onChange({ url: e.target.value })} spellCheck={false} /></Field>
            <Field label="Email"><Input className="mono" value={k.email} onChange={(e) => onChange({ email: e.target.value })} spellCheck={false} /></Field>
            <Field label="API token"><SecretInput value={k.token} onChange={(v) => onChange({ token: v })} label="API token" /></Field>
          </Fields>
        </FormGroup>
      )}
    </Fields>
  );
}

const hostName = (u: string) => { try { return new URL(u).host.split(".")[0]; } catch { return ""; } };
