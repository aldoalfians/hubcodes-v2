import { useEffect, useState } from "react";
import { Button, Dropdown, Input, InputNumber, Select, Space, Tag, Tooltip, Typography } from "antd";
import { ApiOutlined, CloudDownloadOutlined, PlusOutlined, SearchOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { backend } from "../../lib/backend";
import type { Choice, Connection, Tracker, TrackerOverlap } from "../../lib/types";
import { ChoiceSelect, ConnectionSelect, PasteLink, SecretInput, githubConnection, matchConnection, tokenRef, type Link } from "./connect";
import { McpServerFields, serverOf } from "./mcpAuth";
import { EditModal, Field, Fields, FormGroup, ItemList, ItemRow, Result, Section, ToolArgs, headersText, parseHeaders, type Check, ToggleField, Notice } from "./primitives";
import { code, hostOf, kindLabel, type Editing, type Opt } from "./shared";
import { useSettings } from "./context";

const { Text } = Typography;

const TRACKER_KINDS: { value: Tracker["kind"]; label: string }[] = [
  { value: "jira", label: "Jira" },
  { value: "plane", label: "Plane" },
  { value: "github", label: "GitHub issues" },
  { value: "github-review", label: "GitHub review requests" },
  { value: "github-feedback", label: "Feedback on my pull requests" },
  { value: "mcp", label: "MCP server" },
  { value: "command", label: "Command" },
];
const newTracker = (kind: Tracker["kind"]): Tracker => {
  const t: Tracker = { name: "", kind, url: kind === "plane" ? "https://api.plane.so" : "", email: "", token: "", api_key: "", workspace: "", project: "", repo: "", query: "", instruction: kind === "jira" ? "/jira-task {key}" : "", role: "software engineer", on_start: "", on_done: "", comment_on_done: true, limit: 2, poll_minutes: 2, run: "", command: "", args: [], headers: {}, list_tool: "", list_args: null, transition_tool: "", transition_args: null, comment_tool: "", comment_args: null, enabled: true };
  // Review requests: one PR per idle reviewer, the rest wait in Todo; the agent posts its own review.
  if (kind === "github-review") return { ...t, name: "reviews", role: "reviewer", comment_on_done: false, limit: 10 };
  // Feedback: only what arrives from today on, so old threads on open PRs do not all start at once.
  if (kind === "github-feedback") return { ...t, name: "feedback", role: "pr-fixer", comment_on_done: false, limit: 10, since: new Date().toLocaleDateString("sv") };
  return t;
};

/** "mini" or "mini (as pe, off)": where else the same issues are polled. */
const whereElse = (os: TrackerOverlap[]) => os.map((o) => `${o.device}${o.name !== o.tracker ? ` (as ${o.name}${o.enabled ? "" : ", off"})` : o.enabled ? "" : " (off)"}`).join(", ");

/**
 * Other devices that poll the same issues (same kind, place and query) as
 * these trackers. Asked over SSH, so debounced; devices that do not answer
 * are left out rather than reported.
 */
function useOverlaps(trackers: Tracker[], wait = 600): TrackerOverlap[] {
  const [found, setFound] = useState<TrackerOverlap[]>([]);
  const key = JSON.stringify(trackers.map((t) => [t.name, t.kind, t.url, t.workspace, t.project, t.repo, t.run, t.command, t.list_tool, t.query]));
  useEffect(() => {
    if (!trackers.length) return setFound([]);
    let live = true;
    const id = setTimeout(() => void backend.trackerOverlaps(trackers).then((o) => live && setFound(o)).catch(() => live && setFound([])), wait);
    return () => { live = false; clearTimeout(id); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return found;
}

export function TrackersPage() {
  const { cfg, dirty, setListItem, removeListItem, nameTaken, roleOptions } = useSettings();
  const notify = useStore((s) => s.notify);
  const [trResult, setTrResult] = useState<Record<string, Check & { issues?: { key: string; title: string }[] }>>({});
  const [trEdit, setTrEdit] = useState<Editing<Tracker>>(null);
  const overlaps = useOverlaps(cfg.trackers, 0);
  const draftOverlaps = useOverlaps(trEdit ? [trEdit.draft] : []);
  const testTracker = async (tr: Tracker) => {
    setTrResult((r) => ({ ...r, [tr.name]: { ok: true, text: "Testing…", busy: true } }));
    const c = await backend.trackerCheck(tr).catch((e) => ({ name: tr.name, ok: false, detail: String(e), ms: 0, issues: [] }));
    setTrResult((r) => ({ ...r, [tr.name]: { ok: c.ok, text: `${c.detail} (${c.ms} ms)`, issues: c.issues } }));
  };
  const applyTracker = () => {
    if (!cfg || !trEdit) return;
    const d = { ...trEdit.draft, name: trEdit.draft.name.trim() };
    if (!d.name) return notify("A tracker needs a name", "error");
    if (nameTaken(cfg.trackers, d.name, trEdit.index)) return notify(`A tracker called ${d.name} already exists`, "error");
    setListItem("trackers", trEdit.index, d);
    setTrEdit(null);
  };
  return (
    <Section
      title="Trackers"
      tagline="Tickets flow onto the board; their status flows back."
      help={<>Every poll, issues matching the query become board tasks (up to the limit); an idle agent with the role picks each up and is told the instruction, e.g. {code("/jira-task {key}")} so Claude Code runs the team's own skill with its gates. When the agent picks it up the issue moves to the start status; when the task is done or approved it moves to the done status and gets the result as a comment. Jira, Plane and GitHub are built in; any other tracker works through a command or an MCP server. {code("${ENV_VAR}")} keeps tokens out of the file.</>}
      extra={<Dropdown menu={{ items: TRACKER_KINDS.map((k) => ({ key: k.value, label: k.label })), onClick: ({ key }) => setTrEdit({ index: null, draft: newTracker(key as Tracker["kind"]) }) }}><Button type="primary" icon={<PlusOutlined />}>Add tracker</Button></Dropdown>}
    >
      <ItemList empty="No trackers yet. Add Jira, Plane, GitHub, or your own." onAdd={() => setTrEdit({ index: null, draft: newTracker("jira") })} addLabel="Add tracker">
        {cfg.trackers.map((tr, i) => (
          <ItemRow
            key={i}
            name={<span className="mono">{tr.name || "untitled"}</span>}
            meta={<>
              <Tag variant="filled">{kindLabel(TRACKER_KINDS, tr.kind)}</Tag>
              {(() => {
                const os = overlaps.filter((o) => o.tracker === tr.name && o.enabled);
                return os.length > 0 && tr.enabled ? <Tooltip title={`${whereElse(os)} polls the same issues, so every ticket lands on both boards. Keep it on one device: the board hands tasks to idle agents on every device anyway.`}><Tag color="warning" variant="filled">Also polled on {os.map((o) => o.device).join(", ")}</Tag></Tooltip> : null;
              })()}
            </>}
            summary={<><span className="mono">{tr.kind === "github-review" || tr.kind === "github-feedback" ? tr.repo || "every repository" : tr.kind === "github" ? tr.repo : tr.kind === "command" ? tr.run : tr.kind === "mcp" ? (tr.url ? hostOf(tr.url) : tr.command) : hostOf(tr.url)}</span>{tr.role ? <> · {tr.role}</> : null} · every {tr.poll_minutes} min · {tr.limit} at once</>}
            status={<Result r={trResult[tr.name]} />}
            enabled={tr.enabled}
            onEnable={(v) => setListItem("trackers", i, { ...tr, enabled: v })}
            actions={
              <Space size={4}>
                <Button size="small" icon={<ApiOutlined />} onClick={() => void testTracker(tr)} disabled={!tr.name.trim()}>Test</Button>
                <Tooltip title={dirty ? "Save first" : "Import matching issues now"}>
                  <Button size="small" icon={<CloudDownloadOutlined />} onClick={() => void backend.trackerPull(tr.name).then((lines) => notify(lines.length ? lines.join(" · ") : "Nothing new to import")).catch((e) => notify(String(e), "error"))} disabled={!tr.name.trim() || !tr.enabled || dirty}>Pull now</Button>
                </Tooltip>
              </Space>
            }
            onOpen={() => setTrEdit({ index: i, draft: structuredClone(tr) })}
            onRemove={() => removeListItem("trackers", i)}
            removeTitle={`Remove ${tr.name || "this tracker"}?`}
            off={!tr.enabled}
          />
        ))}
      </ItemList>
      <EditModal
        title={trEdit?.index === null ? "Add tracker" : `Edit ${trEdit?.draft.name ?? ""}`}
        open={!!trEdit}
        onClose={() => setTrEdit(null)}
        onApply={applyTracker}
        applyLabel={trEdit?.index === null ? "Add tracker" : "Apply"}
        width={640}
        extra={trEdit && <Space><Button icon={<ApiOutlined />} onClick={() => void testTracker(trEdit.draft)} disabled={!trEdit.draft.name.trim()}>Test</Button><Result r={trResult[trEdit.draft.name]} /></Space>}
      >
        {trEdit && draftOverlaps.length > 0 && (
          <Notice tone="warn">
            <b>{whereElse(draftOverlaps)} already polls these issues.</b>
            <span className="st-notice-quiet"> Every ticket would land on both boards; keep the tracker on one device (tasks still reach idle agents everywhere), or change the query.</span>
          </Notice>
        )}
        {trEdit && <TrackerForm draft={trEdit.draft} roleOptions={roleOptions} issues={trResult[trEdit.draft.name]?.issues} onChange={(patch) => setTrEdit({ ...trEdit, draft: { ...trEdit.draft, ...patch } })} />}
      </EditModal>
    </Section>
  );
}

/** The simple-picker JQL, and reading it back; anything else is custom JQL. */
export function jiraJql(project: string, statuses: string[], mine: boolean): string {
  if (!project) return "";
  const parts = [`project = ${project}`];
  if (statuses.length) parts.push(`status in (${statuses.map((s) => `"${s.replace(/"/g, '\\"')}"`).join(", ")})`);
  if (mine) parts.push("assignee = currentUser()");
  return `${parts.join(" AND ")} ORDER BY priority DESC, updated DESC`;
}
export function readJql(jql: string): { project: string; statuses: string[]; mine: boolean } | null {
  if (!jql.trim()) return { project: "", statuses: [], mine: true };
  // Also reads the common hand-written shape: status = "X", any ORDER BY.
  const m = jql.trim().match(/^project\s*=\s*"?([A-Z][A-Z0-9_]*)"?(?:\s+AND\s+status\s*(?:in\s*\((.*?)\)|=\s*("[^"]*"|\w+)))?(\s+AND\s+assignee\s*=\s*currentUser\(\))?(?:\s+ORDER BY\s+[\w\s,]+)?$/i);
  if (!m) return null;
  const list = m[2] ?? m[3] ?? "";
  const statuses = Array.from(list.matchAll(/"((?:[^"\\]|\\.)*)"|(\w+)/g)).map((x) => (x[1] ?? x[2]).replace(/\\"/g, '"'));
  return { project: m[1].toUpperCase(), statuses, mine: !!m[4] };
}
/** GitHub filters as `gh issue list` flags, and back. */
export function ghQuery(labels: string[], mine: boolean): string {
  return [...labels.map((l) => (/\s/.test(l) ? `--label "${l}"` : `--label ${l}`)), ...(mine ? ["--assignee @me"] : [])].join(" ");
}
export function readGhQuery(q: string): { labels: string[]; mine: boolean } | null {
  const rest = q.replace(/--label (?:"([^"]+)"|(\S+))/g, "").replace(/--assignee @me/g, "").trim();
  if (rest) return null;
  return { labels: Array.from(q.matchAll(/--label (?:"([^"]+)"|(\S+))/g)).map((m) => m[1] ?? m[2]), mine: q.includes("--assignee @me") };
}
const splitStates = (q: string) => q.split(",").map((x) => x.trim()).filter(Boolean);

/** What a tracker copies from the account it reads with. The token is the account's reference, so rotating it in one place updates all. */
function fromConnection(c: Connection): Partial<Tracker> {
  const ref = tokenRef(c);
  if (c.kind === "atlassian") return { url: c.url, email: c.email, token: ref };
  if (c.kind === "plane") return { url: c.url, workspace: c.workspace, api_key: ref };
  return {};
}

function TrackerForm({ draft: tr, roleOptions, issues, onChange }: { draft: Tracker; roleOptions: Opt[]; issues?: { key: string; title: string }[]; onChange: (p: Partial<Tracker>) => void }) {
  const { cfg } = useSettings();
  const notify = useStore((s) => s.notify);
  const [advanced, setAdvanced] = useState(false);
  const [finding, setFinding] = useState(false);
  const [signedIn, setSignedIn] = useState<string | null>(null);
  const feedback = tr.kind === "github-feedback";
  const review = tr.kind === "github-review" || feedback;
  const connKind = tr.kind === "jira" ? "atlassian" : tr.kind === "plane" ? "plane" : tr.kind === "github" || review ? "github" : null;
  const conn: Connection | undefined = connKind === "github" ? githubConnection(cfg) : connKind ? matchConnection(cfg, connKind, tr.url ?? "", connKind === "atlassian" ? tr.email : tr.workspace) : undefined;
  const pick = (c: Connection) => onChange(fromConnection(c));
  const jql = tr.kind === "jira" ? readJql(tr.query) : null;
  const gq = tr.kind === "github" ? readGhQuery(tr.query) : null;
  const statusArg = tr.kind === "jira" ? jql?.project ?? "" : tr.kind === "plane" ? tr.project : tr.repo;
  const named = (n: string) => (tr.name.trim() ? {} : { name: n.toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-|-$/g, "") });
  const onLink = (l: Link) => {
    if (l.kind === "jira") {
      const c = matchConnection(cfg, "atlassian", l.site);
      onChange({ ...(c ? fromConnection(c) : { url: l.site }), ...(l.project ? { query: jiraJql(l.project, jql?.statuses ?? [], jql?.mine ?? true) } : {}), ...named(l.project ? `jira-${l.project}` : "jira") });
      if (!c) notify(`No account for ${l.site.replace(/^https?:\/\//, "")} yet: pick “Connect an Atlassian account…”`);
    } else if (l.kind === "plane") {
      const c = cfg.connections.find((x) => x.kind === "plane" && x.workspace === l.workspace);
      onChange({ ...(c ? fromConnection(c) : { url: l.api, workspace: l.workspace }), ...(l.project ? { project: l.project } : {}), ...named(`plane-${l.workspace}`) });
      if (!c) notify(`No account for the ${l.workspace} workspace yet: pick “Connect a Plane account…”`);
    } else if (l.kind === "github") onChange({ repo: l.repo, ...named(`gh-${l.repo.split("/")[1]}`) });
  };
  const discover = async () => {
    setFinding(true);
    try {
      const d = await backend.mcpDiscover({ url: tr.url ?? "", command: tr.command ?? "", args: tr.args, headers: tr.headers, env: {} }, ["list", "transition", "comment"]);
      setSignedIn(`Signed in · ${d.tools.length} tool${d.tools.length === 1 ? "" : "s"} on the server`);
      const p = d.picks;
      onChange({ ...(p.list ? { list_tool: p.list[0], list_args: p.list[1] } : {}), ...(p.transition ? { transition_tool: p.transition[0], transition_args: p.transition[1] } : {}), ...(p.comment ? { comment_tool: p.comment[0], comment_args: p.comment[1] } : {}) });
      notify(p.list ? `Found ${d.tools.length} tools; picked ${[p.list?.[0], p.transition?.[0], p.comment?.[0]].filter(Boolean).join(", ")}` : `Found ${d.tools.length} tools, none that lists issues. Pick them under Advanced.`, p.list ? "info" : "error");
      if (!p.list) setAdvanced(true);
    } catch (e) {
      notify(String(e).replace(/^Error: /, ""), "error");
    } finally {
      setFinding(false);
    }
  };
  const statusPicker = (value: string, set: (v: string) => void, label: string, extra: Choice[] = []) =>
    tr.kind === "jira" || tr.kind === "plane" ? (
      <ChoiceSelect conn={conn} what="statuses" arg={statusArg} value={value} onChange={(v: string) => set(v)} label={label} placeholder="Leave as it is" />
    ) : tr.kind === "github" ? (
      <ChoiceSelect conn={conn} what="labels" arg={tr.repo} value={value} onChange={(v: string) => set(v)} label={label} placeholder="Nothing" extra={extra} />
    ) : (
      <Input className="mono" value={value} onChange={(e) => set(e.target.value)} spellCheck={false} aria-label={label} />
    );
  return (
    <Fields cols={1}>
      <Fields>
        <Field label="Name" required><Input className="mono" value={tr.name} onChange={(e) => onChange({ name: e.target.value })} placeholder={tr.kind} spellCheck={false} aria-label="Name" /></Field>
        {connKind && connKind !== "github" ? (
          <Field label="Account"><ConnectionSelect kind={connKind} value={conn} onPick={pick} auto={!tr.url} /></Field>
        ) : (
          <Field label="Kind"><Select className="st-wide" value={tr.kind} options={TRACKER_KINDS} onChange={(v) => onChange({ kind: v })} /></Field>
        )}
      </Fields>
      {tr.kind === "jira" && <PasteLink placeholder="https://yourteam.atlassian.net/jira/software/projects/PE/boards/1" accept={["jira"]} onLink={onLink} hint="A board, backlog or issue link fills the site and the project." />}
      {tr.kind === "plane" && <PasteLink placeholder="https://app.plane.so/your-workspace/projects/…/issues" accept={["plane"]} onLink={onLink} hint="A project link fills the workspace and the project." />}
      {tr.kind === "github" && <PasteLink placeholder="https://github.com/owner/repo" accept={["github"]} onLink={onLink} />}
      {tr.kind === "jira" && (
        <FormGroup title="Which issues">
          {jql ? (
            <Fields>
              <Field label="Project"><ChoiceSelect conn={conn} what="projects" value={jql.project} onChange={(v: string) => onChange({ query: jiraJql(v, [], jql.mine), ...named(v ? `jira-${v}` : "") })} label="Project" placeholder="Pick a project" /></Field>
              <Field label="In status" hint="Empty: every status."><ChoiceSelect conn={conn} what="statuses" arg={jql.project} multiple value={jql.statuses} onChange={(v: string[]) => onChange({ query: jiraJql(jql.project, v, jql.mine) })} label="In status" placeholder="Any status" /></Field>
              <ToggleField label="Only issues assigned to me" checked={jql.mine} onChange={(v) => onChange({ query: jiraJql(jql.project, jql.statuses, v) })} />
            </Fields>
          ) : (
            <div className="st-note">This tracker uses its own JQL (under Advanced). <a onClick={() => onChange({ query: "" })}>Use the pickers instead</a></div>
          )}
        </FormGroup>
      )}
      {tr.kind === "plane" && (
        <FormGroup title="Which issues">
          <Fields>
            <Field label="Project"><ChoiceSelect conn={conn} what="projects" value={tr.project} onChange={(v: string) => onChange({ project: v })} label="Project" placeholder="Pick a project" /></Field>
            <Field label="In state" hint="Empty: every state."><ChoiceSelect conn={conn} what="statuses" arg={tr.project} multiple value={splitStates(tr.query)} onChange={(v: string[]) => onChange({ query: v.join(", ") })} label="In state" placeholder="Any state" /></Field>
          </Fields>
        </FormGroup>
      )}
      {tr.kind === "github" && (
        <FormGroup title="Which issues">
          <Fields>
            <Field label="Repository" hint="Read with the gh sign-in on this device."><ChoiceSelect conn={conn} what="repos" value={tr.repo} onChange={(v: string) => onChange({ repo: v, ...named(v ? `gh-${v.split("/")[1]}` : "") })} label="Repository" placeholder="owner/repo" /></Field>
            {gq ? (
              <>
                <Field label="With label" hint="Empty: every open issue."><ChoiceSelect conn={conn} what="labels" arg={tr.repo} multiple value={gq.labels} onChange={(v: string[]) => onChange({ query: ghQuery(v, gq.mine) })} label="With label" placeholder="Any label" /></Field>
                <ToggleField label="Only issues assigned to me" checked={gq.mine} onChange={(v) => onChange({ query: ghQuery(gq.labels, v) })} />
              </>
            ) : (
              <div className="st-note">This tracker uses its own filters (under Advanced). <a onClick={() => onChange({ query: "" })}>Use the pickers instead</a></div>
            )}
          </Fields>
        </FormGroup>
      )}
      {feedback && (
        <FormGroup title="Which pull requests">
          <Fields>
            <Field label="Repository" hint="Your open pull requests (the gh sign-in on this device). Comments from other people, failed checks and a conflict with the base branch start a round; your comments and bots' do not.">
              <ChoiceSelect conn={conn} what="repos" value={tr.repo} onChange={(v: string) => onChange({ repo: v ?? "" })} label="Repository" placeholder="Every repository" />
            </Field>
            <Field label="Feedback from" hint="Older comments are left alone. Empty: every comment newer than the pull request's last commit.">
              <Input type="date" className="st-wide" value={tr.since ?? ""} onChange={(e) => onChange({ since: e.target.value })} aria-label="Feedback from" />
            </Field>
            <Field label="Ignore comments from" hint={<>GitHub logins, for automated reviewers that post from a person's account. Your own comments and bots are always ignored. Add {code("checks")} to leave failed checks alone, {code("conflicts")} for merge conflicts.</>} span>
              <Input className="mono" value={tr.ignore ?? ""} onChange={(e) => onChange({ ignore: e.target.value })} placeholder="auto-reviewer, other-login" spellCheck={false} aria-label="Ignore comments from" />
            </Field>
            <Field label="Rounds open at once" hint="One round per pull request at a time, one per idle agent with the role below; the rest wait in Todo. The agent fixes what is clear, pushes to the pull request's branch in its own checkout and answers every thread; the next comment starts the next round, once the comments have been quiet for 5 minutes. Two rounds in a row without a commit pause the pull request for you; a new commit resumes it." span>
              <InputNumber className="st-wide" min={1} max={50} value={tr.limit} onChange={(n) => onChange({ limit: Math.max(1, Number(n) || 1) })} aria-label="Rounds open at once" />
            </Field>
          </Fields>
        </FormGroup>
      )}
      {review && !feedback && (
        <FormGroup title="Which pull requests">
          <Fields>
            <Field label="Repository" hint="Open pull requests that ask you (the gh sign-in on this device) for a review. Drafts wait until they are ready.">
              <ChoiceSelect conn={conn} what="repos" value={tr.repo} onChange={(v: string) => onChange({ repo: v ?? "" })} label="Repository" placeholder="Every repository" />
            </Field>
            <Field label="Waiting or in review at once" hint={<>One pull request per idle agent with the role below, in parallel; the rest wait in Todo, oldest first. A request that is withdrawn leaves the queue; a new request after your review comes back. Each review runs in its own checkout, and the orchestrator is told when the queue changes.</>}>
              <InputNumber className="st-wide" min={1} max={50} value={tr.limit} onChange={(n) => onChange({ limit: Math.max(1, Number(n) || 1) })} aria-label="Waiting or in review at once" />
            </Field>
            <ToggleField label="Review again after a push" hint="When the author pushes after a review this tracker ran, the reviewer looks at what changed since and says which earlier points are addressed. Waits 5 minutes after the last commit." checked={tr.rereview !== false} onChange={(v) => onChange({ rereview: v })} />
            <ToggleField label="Approve when nothing blocks" hint="When the reviewer ends with VERDICT: approve, hubcodes approves the pull request on GitHub with the comment LGTM, as the gh sign-in on this device. Off: Pull requests shows an Approve on GitHub button and a person decides." checked={!!tr.auto_approve} onChange={(v) => onChange({ auto_approve: v })} />
          </Fields>
        </FormGroup>
      )}
      {tr.kind === "mcp" && (
        <FormGroup title="MCP server">
          <Fields>
            <McpServerFields target={tr} onChange={onChange} placeholder="https://tracker.example/mcp/" signIn={{ run: discover, busy: finding, done: signedIn }} />
            <Field label="Tools" hint={tr.list_tool ? <>Lists with <code>{tr.list_tool}</code>{tr.transition_tool && <>, moves with <code>{tr.transition_tool}</code></>}{tr.comment_tool && <>, comments with <code>{tr.comment_tool}</code></>}.</> : "Find them on the server; or set them under Advanced."} span>
              <Button icon={<SearchOutlined />} loading={finding} onClick={() => void discover()} disabled={!serverOf(tr) && !tr.command}>Find the tools</Button>
            </Field>
            <Field label="Query" hint="Passed to the list tool as {query}." span><Input className="mono" value={tr.query} onChange={(e) => onChange({ query: e.target.value })} spellCheck={false} /></Field>
          </Fields>
        </FormGroup>
      )}
      {tr.kind === "command" && (
        <Field label="Command" hint={<>Gets JSON on stdin: {code('{op: "list", query}')} prints a JSON array of {code("{key, title, url, description}")}; {code('{op: "transition", key, to}')}; {code('{op: "comment", key, text}')}.</>} span>
          <Input className="mono" value={tr.run} onChange={(e) => onChange({ run: e.target.value })} placeholder="my-tracker-cli" spellCheck={false} />
        </Field>
      )}
      <FormGroup title="Hand-off to agents">
        <Fields>
          <Field label="Who takes them" hint={review ? <ReviewerCount role={tr.role} what={feedback ? "round" : "review"} /> : "Idle agents with this role pick the tasks up; empty = any idle agent."}>
            <Select className="st-wide" showSearch allowClear value={tr.role || undefined} options={roleOptions} onChange={(v) => onChange({ role: v ?? "" })} placeholder="any" />
          </Field>
          {!review && <Field label="Open tasks at once"><InputNumber className="st-wide" min={1} max={20} value={tr.limit} onChange={(n) => onChange({ limit: Math.max(1, Number(n) || 1) })} /></Field>}
          {!review && <Field label="When picked up, move to" hint={tr.kind === "github" ? "A label to add." : "Empty: leave the status."}>{statusPicker(tr.on_start, (v) => onChange({ on_start: v }), "When picked up, move to")}</Field>}
          {!review && <Field label="When done, move to" hint={tr.kind === "github" ? "A label, or close the issue." : "Applied when the task is done or approved."}>{statusPicker(tr.on_done, (v) => onChange({ on_done: v }), "When done, move to", [{ value: "closed", label: "Close the issue" }])}</Field>}
          <Field label="What the agent is told" hint={feedback ? <>{code("{key} {title} {url} {description}")} are filled in; {code("{description}")} lists the new comments with the command that answers each. Empty: fix on the branch in its own checkout ({code("hubcodes review checkout {key} --branch")}), test, push, answer every thread.</> : review ? <>{code("{key} {title} {url} {description}")} are filled in. Empty: check the PR out in its own folder ({code("hubcodes review checkout {key}")}), read it, run the affected tests, post one comment review, never approve. {code("/pr-review {url}")} runs that skill instead.</> : <>{code("{key} {title} {url} {description}")} are filled in. {code("/jira-task {key}")} runs that Claude Code skill; empty = title, link and description.</>} span>
            <Input className="mono" value={tr.instruction} onChange={(e) => onChange({ instruction: e.target.value })} placeholder={feedback ? "" : review ? "/pr-review {url}" : "/jira-task {key}"} spellCheck={false} />
          </Field>
          <Field label="Check every"><InputNumber className="st-wide" min={1} max={120} value={tr.poll_minutes} onChange={(n) => onChange({ poll_minutes: Math.max(1, Number(n) || 1) })} suffix="min" /></Field>
          <ToggleField label="Post the result as a comment" hint={feedback ? "Off: the agent answers each thread itself." : review ? "Off: the agent posts its own review." : undefined} checked={tr.comment_on_done} onChange={(v) => onChange({ comment_on_done: v })} />
        </Fields>
      </FormGroup>
      {issues && issues.length > 0 && (
        <FormGroup title="Matching issues right now">
          <div className="st-issues">{issues.map((it) => <Tag key={it.key} variant="filled" className="mono">{it.key} <Text type="secondary">{it.title.slice(0, 60)}</Text></Tag>)}</div>
        </FormGroup>
      )}
      <button type="button" className="st-advanced-toggle" onClick={() => setAdvanced((a) => !a)} aria-expanded={advanced}>{advanced ? "Hide advanced" : "Advanced: raw fields"}</button>
      {advanced && <TrackerRaw tr={tr} onChange={onChange} />}
    </Fields>
  );
}

/** How many reviews run at once: one per live agent with the role (exact match, as dispatch does). */
function ReviewerCount({ role, what }: { role: string; what: "review" | "round" }) {
  const agents = useStore((s) => s.agents);
  if (!role.trim()) return <>Any idle agent takes a {what}.</>;
  const n = agents.filter((a) => !a.dead && a.role === role.trim()).length;
  if (n === 0) return <span className="st-bad">No agent has the role “{role}” yet: {what}s wait in Todo until one does.</span>;
  return <>{n} agent{n === 1 ? "" : "s"} with this role: {n === 1 ? `one ${what} at a time` : `up to ${n} ${what}s at once`}; the rest wait in Todo.</>;
}

/** Every field as stored, for what the pickers do not cover. */
function TrackerRaw({ tr, onChange }: { tr: Tracker; onChange: (p: Partial<Tracker>) => void }) {
  return (
    <FormGroup title="Advanced">
      <Fields>
        <Field label="Kind"><Select className="st-wide" value={tr.kind} options={TRACKER_KINDS} onChange={(v) => onChange({ kind: v })} /></Field>
        {tr.kind === "jira" && (
          <>
            <Field label="Site URL"><Input className="mono" value={tr.url ?? ""} onChange={(e) => onChange({ url: e.target.value })} placeholder="https://org.atlassian.net" spellCheck={false} /></Field>
            <Field label="Email"><Input className="mono" value={tr.email} onChange={(e) => onChange({ email: e.target.value })} spellCheck={false} /></Field>
            <Field label="API token"><SecretInput value={tr.token} onChange={(v) => onChange({ token: v })} label="API token" /></Field>
            <Field label="JQL" hint="Which issues to import. Empty: assigned to you and not done." span><Input.TextArea className="mono" autoSize value={tr.query} onChange={(e) => onChange({ query: e.target.value })} spellCheck={false} aria-label="JQL" /></Field>
          </>
        )}
        {tr.kind === "plane" && (
          <>
            <Field label="API URL"><Input className="mono" value={tr.url ?? ""} onChange={(e) => onChange({ url: e.target.value })} spellCheck={false} /></Field>
            <Field label="API key"><SecretInput value={tr.api_key} onChange={(v) => onChange({ api_key: v })} label="API key" /></Field>
            <Field label="Workspace slug"><Input className="mono" value={tr.workspace} onChange={(e) => onChange({ workspace: e.target.value })} spellCheck={false} /></Field>
            <Field label="Project id"><Input className="mono" value={tr.project} onChange={(e) => onChange({ project: e.target.value })} spellCheck={false} /></Field>
            <Field label="States to import" hint="State names, comma separated." span><Input className="mono" value={tr.query} onChange={(e) => onChange({ query: e.target.value })} spellCheck={false} /></Field>
          </>
        )}
        {tr.kind === "github" && (
          <>
            <Field label="Repository"><Input className="mono" value={tr.repo} onChange={(e) => onChange({ repo: e.target.value })} spellCheck={false} /></Field>
            <Field label="Filters" hint={<>Extra {code("gh issue list")} flags.</>}><Input className="mono" value={tr.query} onChange={(e) => onChange({ query: e.target.value })} spellCheck={false} aria-label="Filters" /></Field>
          </>
        )}
        {tr.kind === "mcp" && (
          <>
            <Field label="Command (stdio)"><Input className="mono" value={tr.command ?? ""} onChange={(e) => onChange({ command: e.target.value })} placeholder="npx -y some-tracker-mcp" spellCheck={false} /></Field>
            <Field label="Headers" hint={<>One per line, {code("Name: value")}.</>}><Input.TextArea rows={2} className="mono" value={headersText(tr.headers)} onChange={(e) => onChange({ headers: parseHeaders(e.target.value) })} spellCheck={false} /></Field>
            <Field label="List tool" hint={<>{code("{query}")} is replaced.</>} span>
              <ToolArgs tool={tr.list_tool} args={tr.list_args} fallback={{ query: "{query}" }} placeholder="list_issues" onTool={(v) => onChange({ list_tool: v })} onArgs={(v) => onChange({ list_args: v })} label="List tool" />
            </Field>
            <Field label="Transition tool" hint={<>{code("{key}")} and {code("{to}")} are replaced.</>} span>
              <ToolArgs tool={tr.transition_tool} args={tr.transition_args} fallback={{ key: "{key}", to: "{to}" }} placeholder="transition_issue" onTool={(v) => onChange({ transition_tool: v })} onArgs={(v) => onChange({ transition_args: v })} label="Transition tool" />
            </Field>
            <Field label="Comment tool" hint={<>{code("{key}")} and {code("{text}")} are replaced.</>} span>
              <ToolArgs tool={tr.comment_tool} args={tr.comment_args} fallback={{ key: "{key}", text: "{text}" }} placeholder="add_comment" onTool={(v) => onChange({ comment_tool: v })} onArgs={(v) => onChange({ comment_args: v })} label="Comment tool" />
            </Field>
          </>
        )}
        {(tr.kind === "jira" || tr.kind === "plane" || tr.kind === "github") && (
          <>
            <Field label="Status on pick-up (raw)"><Input className="mono" value={tr.on_start} onChange={(e) => onChange({ on_start: e.target.value })} spellCheck={false} /></Field>
            <Field label="Status when done (raw)"><Input className="mono" value={tr.on_done} onChange={(e) => onChange({ on_done: e.target.value })} spellCheck={false} /></Field>
          </>
        )}
      </Fields>
    </FormGroup>
  );
}
