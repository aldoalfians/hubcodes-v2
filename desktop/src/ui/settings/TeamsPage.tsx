import { useState } from "react";
import { Button, Input, Popconfirm, Select, Switch, Typography } from "antd";
import { ArrowRightOutlined, DeleteOutlined, PlusOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import type { ProviderEdit, Template, TemplateAgent } from "../../lib/types";
import { ModelPicker } from "../ModelPicker";
import { useProviderStatuses } from "../ConnectPanel";
import { EditModal, Field, Fields, FormGroup, ItemList, ItemRow, Section } from "./primitives";
import { code, plural, type Editing, type Opt } from "./shared";
import { useSettings } from "./context";

const { Text } = Typography;

const newTeam = (): Template => ({ name: "", description: "", agents: [], links: [] });

export function TeamsPage() {
  const { cfg, setListItem, removeListItem, nameTaken, providerOptions, roleOptions } = useSettings();
  const notify = useStore((s) => s.notify);
  const [teamEdit, setTeamEdit] = useState<Editing<Template>>(null);
  const applyTeam = () => {
    if (!cfg || !teamEdit) return;
    const d = { ...teamEdit.draft, name: teamEdit.draft.name.trim(), agents: teamEdit.draft.agents.filter((a) => a.name.trim()) };
    if (!d.name) return notify("A team needs a name", "error");
    if (nameTaken(cfg.templates, d.name, teamEdit.index)) return notify(`A team called ${d.name} already exists`, "error");
    setListItem("templates", teamEdit.index, d);
    setTeamEdit(null);
  };
  return (
    <Section
      title="Teams"
      tagline="Several agents with roles, links and loops, started in one go."
      help={<>Start a team from New agent → Team. Save the running canvas as a team from ⌘K. A repo can carry its own team in {code(".hubcodes/team.toml")}.</>}
      extra={<Button type="primary" icon={<PlusOutlined />} onClick={() => setTeamEdit({ index: null, draft: newTeam() })}>Add team</Button>}
    >
      <ItemList empty="No teams yet. Add one, or start agents on the canvas and save them as a team from ⌘K." onAdd={() => setTeamEdit({ index: null, draft: newTeam() })} addLabel="Add team">
        {cfg.templates.map((t, i) => (
          <ItemRow
            key={i}
            name={<span className="mono">{t.name || "untitled"}</span>}
            meta={t.description ? <Text type="secondary">{t.description}</Text> : null}
            summary={<>{plural(t.agents.length, "agent")} · {plural(t.links.length, "link")}{t.agents.length > 0 && <> · {t.agents.map((a) => a.name).join(", ")}</>}</>}
            onOpen={() => setTeamEdit({ index: i, draft: structuredClone(t) })}
            onRemove={() => removeListItem("templates", i)}
            removeTitle={`Remove team ${t.name}?`}
          />
        ))}
      </ItemList>
      <EditModal title={teamEdit?.index === null ? "Add team" : `Edit team ${teamEdit?.draft.name ?? ""}`} open={!!teamEdit} onClose={() => setTeamEdit(null)} onApply={applyTeam} applyLabel={teamEdit?.index === null ? "Add team" : "Apply"} width={720}>
        {teamEdit && <TeamForm isNew={teamEdit.index === null} starts={cfg.templates} draft={teamEdit.draft} providers={cfg.providers} providerOptions={providerOptions} roleOptions={roleOptions} peerOptions={(cfg.peers ?? []).map((p) => ({ value: p.name, label: p.name }))} onChange={(patch) => setTeamEdit({ ...teamEdit, draft: { ...teamEdit.draft, ...patch } })} />}
      </EditModal>
    </Section>
  );
}

const isLead = (r: string | null) => !!r && /orchestr|lead|manager|coordinator|planner/i.test(r);
/** `software engineer` → `software-engineer`, made unique among `taken`. */
function nameFor(role: string, taken: string[]): string {
  const base = isLead(role) ? "lead" : /review/i.test(role) ? "reviewer" : /test|qa/i.test(role) ? "tester" : /engineer|dev/i.test(role) ? "dev" : role.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "agent";
  if (!taken.includes(base)) return base;
  for (let i = 2; ; i++) if (!taken.includes(`${base}-${i}`)) return `${base}-${i}`;
}

function TeamForm({ draft, providers, providerOptions, roleOptions, peerOptions, onChange, isNew, starts }: { draft: Template; providers: ProviderEdit[]; providerOptions: Opt[]; roleOptions: Opt[]; peerOptions: Opt[]; onChange: (p: Partial<Template>) => void; isNew: boolean; starts: Template[] }) {
  const names = draft.agents.map((a) => a.name.trim()).filter(Boolean);
  const nameOpts = names.map((n) => ({ value: n, label: n }));
  const setLink = (i: number, l: string[]) => onChange({ links: draft.links.map((x, m) => (m === i ? l : x)) });
  const lead = draft.agents.find((a) => isLead(a.role))?.name.trim();
  const leadMissing = lead ? names.filter((n) => n !== lead && !draft.links.some((l) => (l[0] === lead && l[1] === n) || (l[1] === lead && l[0] === n))) : [];
  const setAgent = (k: number, patch: Partial<TemplateAgent>) => onChange({ agents: draft.agents.map((x, m) => (m === k ? { ...x, ...patch } : x)) });
  // Model lists come from each tool on this Mac (the same catalog New agent shows); the provider's configured list is the fallback.
  const { statuses, loading } = useProviderStatuses(draft.agents.map((a) => a.provider));
  const liveProviders = useStore((s) => s.providers);
  const catalog = (name: string) => {
    const st = statuses[name];
    if (st && st.models.length) return st.models;
    return (providers.find((p) => p.name === name)?.models ?? []).map((id) => ({ id, label: id, note: "", current: false }));
  };
  return (
    <Fields cols={1}>
      {isNew && draft.agents.length === 0 && starts.length > 0 && (
        <div className="tm-start">
          <Text type="secondary">Start from</Text>
          {starts.map((s) => (
            <Button key={s.name} size="small" onClick={() => onChange({ agents: structuredClone(s.agents), links: structuredClone(s.links), description: draft.description || s.description })}>
              {s.name} <Text type="secondary" className="tm-start-n">{s.agents.length}</Text>
            </Button>
          ))}
          <Text type="secondary">or add agents yourself.</Text>
        </div>
      )}
      <Fields>
        <Field label="Name" required><Input className="mono" value={draft.name} onChange={(e) => onChange({ name: e.target.value })} placeholder="full-stack" spellCheck={false} /></Field>
        <Field label="Description"><Input value={draft.description} onChange={(e) => onChange({ description: e.target.value })} placeholder="Orchestrator, software engineer, reviewer" /></Field>
      </Fields>
      <FormGroup title="Agents" extra={<Button size="small" icon={<PlusOutlined />} onClick={() => onChange({ agents: [...draft.agents, { name: "", provider: providers[0]?.name ?? "claude", role: null, model: null, worktree: null, device: null, account: null }] })}>Add agent</Button>}>
        {draft.agents.length === 0 ? (
          <Text type="secondary" className="tm-empty">No agents in this team yet.</Text>
        ) : (
          <div className="tm-agents">
            {draft.agents.map((a, k) => {
              const st = statuses[a.provider];
              const takes = providers.find((p) => p.name === a.provider)?.modelFlag;
              const accounts = liveProviders.find((p) => p.name === a.provider)?.accounts ?? ["default"];
              return (
                <div className="tm-agent" key={k}>
                  <div className="tm-agent-head">
                    <span className="tm-agent-n">{k + 1}</span>
                    <Input className="mono tm-agent-name" value={a.name} onChange={(e) => setAgent(k, { name: e.target.value })} placeholder="name, e.g. dev" spellCheck={false} aria-label="Agent name" status={!a.name.trim() ? "warning" : undefined} />
                    <Popconfirm title={`Remove ${a.name || "this agent"} from the team?`} okText="Remove" okButtonProps={{ danger: true }} onConfirm={() => onChange({ agents: draft.agents.filter((_, m) => m !== k) })}>
                      <Button size="small" type="text" className="tm-remove" icon={<DeleteOutlined />} aria-label="Remove agent" />
                    </Popconfirm>
                  </div>
                  <div className="tm-agent-grid">
                    <label className="tm-cell"><span>Provider</span><Select value={a.provider} options={providerOptions} onChange={(v) => setAgent(k, { provider: v, model: null, account: null })} /></label>
                    <label className="tm-cell"><span>Role</span><Select allowClear showSearch value={a.role ?? undefined} options={roleOptions} onChange={(v) => setAgent(k, { role: v || null, ...(v && !a.name.trim() ? { name: nameFor(v, names) } : {}) })} placeholder="any role" /></label>
                    <label className="tm-cell"><span>Model</span><ModelPicker value={a.model ?? ""} onChange={(v) => setAgent(k, { model: v.trim() || null })} models={catalog(a.provider)} loading={loading && !st} disabled={takes === null} size="small" compact placeholder={takes === null ? "takes no model" : st?.currentModel ? `${st.currentModel} (default)` : "default"} /></label>
                    <label className="tm-cell"><span>Runs on</span><Select value={a.device ?? ""} options={[{ value: "", label: "This device" }, ...peerOptions]} onChange={(v) => setAgent(k, { device: v || null, worktree: v ? null : a.worktree })} /></label>
                    <label className="tm-cell"><span>Account</span><Select value={a.account ?? "default"} options={accounts.map((x) => ({ value: x, label: x === "default" ? "Default" : x }))} onChange={(v) => setAgent(k, { account: v === "default" ? null : v })} /></label>
                    <label className="tm-cell"><span>Own branch</span><Input className="mono" value={a.worktree ?? ""} onChange={(e) => setAgent(k, { worktree: e.target.value || null })} placeholder={a.device ? "only on this device" : "shares the folder"} spellCheck={false} disabled={!!a.device} /></label>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </FormGroup>
      <FormGroup
        title="Links"
        extra={
          <>
            {lead && leadMissing.length > 0 && <Button size="small" type="text" onClick={() => onChange({ links: [...draft.links, ...leadMissing.map((n) => [lead, n])] })}>Link {lead} to everyone</Button>}
            <Button size="small" icon={<PlusOutlined />} disabled={names.length < 2} onClick={() => onChange({ links: [...draft.links, [names[0], names[1]]] })}>Add link</Button>
          </>
        }
      >
        {draft.links.length === 0 ? (
          <Text type="secondary" className="tm-empty">{names.length < 2 ? "Add two agents to link them." : "No links yet. A link lets two agents message each other; a loop makes them answer each other until the goal is met."}</Text>
        ) : (
          <div className="tm-links">
            {draft.links.map((l, i) => (
              <div className="tm-link" key={i}>
                <Select size="small" value={l[0]} options={nameOpts} onChange={(v) => setLink(i, [v, l[1], ...l.slice(2)])} aria-label="From" className="tm-link-end" />
                <ArrowRightOutlined className="tm-link-arrow" />
                <Select size="small" value={l[1]} options={nameOpts} onChange={(v) => setLink(i, [l[0], v, ...l.slice(2)])} aria-label="To" className="tm-link-end" />
                <label className="tm-link-loop"><Switch size="small" checked={l[2] === "loop"} onChange={(v) => setLink(i, v ? [l[0], l[1], "loop"] : [l[0], l[1]])} aria-label="Loop" /> loop</label>
                {l[0] === l[1] && <Text type="warning" className="tm-link-bad">pick two agents</Text>}
                <Button size="small" type="text" icon={<DeleteOutlined />} onClick={() => onChange({ links: draft.links.filter((_, m) => m !== i) })} aria-label="Remove link" />
              </div>
            ))}
          </div>
        )}
      </FormGroup>
    </Fields>
  );
}
