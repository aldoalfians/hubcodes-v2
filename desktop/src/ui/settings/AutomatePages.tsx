import { useState } from "react";
import { Button, Input, InputNumber, Popconfirm, Select, Tag, Typography, Segmented } from "antd";
import { DeleteOutlined, PlusOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import type { Playbook, PlaybookStep, Prompt, Trigger } from "../../lib/types";
import { EditModal, Field, Fields, FormGroup, ItemList, ItemRow, Section } from "./primitives";
import { code, plural, type Editing, type Opt } from "./shared";
import { useSettings } from "./context";
import { TriggerForm, describeSchedule, readWhen } from "./TriggerForm";

/** The trigger's when, in words, for its row. */
const triggerWhen = (on: string) => {
  const w = readWhen(on);
  return w.kind === "schedule" ? describeSchedule(w.schedule) : w.kind === "pr" ? `New PR in ${w.repo}` : `Webhook ${w.name}`;
};

const { Text } = Typography;

const GATES = [{ value: "", label: "No gate: next step right away" }, { value: "human", label: "Human: pause until you approve" }, { value: "test", label: "Tests: the repo's tests must pass" }, { value: "review", label: "Review: the task must be approved" }];
const newStep = (): PlaybookStep => ({ name: "", agent: "", role: "", prompt: "", action: "", gate: "", retries: 0 });
const newPlaybook = (): Playbook => ({ name: "", description: "", step: [newStep()] });
const newPrompt = (): Prompt => ({ name: "", description: "", text: "" });
const newTrigger = (): Trigger => ({ name: "", on: "cron 0 9 * * 1-5", action: "", enabled: true });

export function PlaybooksPage() {
  const { cfg, setListItem, removeListItem, nameTaken, roleOptions } = useSettings();
  const notify = useStore((s) => s.notify);
  const [pbEdit, setPbEdit] = useState<Editing<Playbook>>(null);
  const applyPlaybook = () => {
    if (!cfg || !pbEdit) return;
    const d = { ...pbEdit.draft, name: pbEdit.draft.name.trim() };
    if (!d.name) return notify("A playbook needs a name", "error");
    if (d.step.some((s) => !s.name.trim())) return notify("Every step needs a name", "error");
    if (nameTaken(cfg.playbooks, d.name, pbEdit.index)) return notify(`A playbook called ${d.name} already exists`, "error");
    setListItem("playbooks", pbEdit.index, d);
    setPbEdit(null);
  };
  return (
    <Section
      title="Playbooks"
      tagline="A goal walked through named steps, each with a gate before the next."
      help={<>Each step is a prompt to an agent (by name or by role). Gates: <b>human</b> pauses until you approve, <b>tests</b> runs the repository's tests and hands failures back (up to the retries), <b>review</b> waits for the task to be approved. Steps become board tasks. A step can be an action instead: {code("merge {prev_agent}")}, {code('send agent "text"')}, {code('task add "title"')}. Prompt variables: {code("{goal} {prev_agent} {prev_result} {run} {playbook} {step}")}. Run one from ⌘K or the Board; a repository can carry its own in {code(".hubcodes/team.toml")}.</>}
      extra={<Button type="primary" icon={<PlusOutlined />} onClick={() => setPbEdit({ index: null, draft: newPlaybook() })}>Add playbook</Button>}
    >
      <ItemList empty="No playbooks yet." onAdd={() => setPbEdit({ index: null, draft: newPlaybook() })} addLabel="Add playbook">
        {cfg.playbooks.map((pb, i) => (
          <ItemRow
            key={i}
            name={<span className="mono">{pb.name || "untitled"}</span>}
            meta={pb.description ? <Text type="secondary">{pb.description}</Text> : null}
            summary={<>{plural(pb.step.length, "step")}: {pb.step.map((s) => s.name || "?").join(" → ")}</>}
            onOpen={() => setPbEdit({ index: i, draft: structuredClone(pb) })}
            onRemove={() => removeListItem("playbooks", i)}
            removeTitle={`Remove playbook ${pb.name}?`}
          />
        ))}
      </ItemList>
      <EditModal title={pbEdit?.index === null ? "Add playbook" : `Edit playbook ${pbEdit?.draft.name ?? ""}`} open={!!pbEdit} onClose={() => setPbEdit(null)} onApply={applyPlaybook} applyLabel={pbEdit?.index === null ? "Add playbook" : "Apply"} width={720}>
        {pbEdit && <PlaybookForm draft={pbEdit.draft} roleOptions={roleOptions} onChange={(patch) => setPbEdit({ ...pbEdit, draft: { ...pbEdit.draft, ...patch } })} />}
      </EditModal>
    </Section>
  );
}

export function PromptsPage() {
  const { cfg, setListItem, removeListItem, nameTaken } = useSettings();
  const notify = useStore((s) => s.notify);
  const [promptEdit, setPromptEdit] = useState<Editing<Prompt>>(null);
  const applyPrompt = () => {
    if (!cfg || !promptEdit) return;
    const d = { ...promptEdit.draft, name: promptEdit.draft.name.trim().replace(/^\//, "") };
    if (!d.name) return notify("A prompt needs a name", "error");
    if (!d.text.trim()) return notify("A prompt needs text", "error");
    if (nameTaken(cfg.prompts, d.name, promptEdit.index)) return notify(`/${d.name} already exists`, "error");
    setListItem("prompts", promptEdit.index, d);
    setPromptEdit(null);
  };
  return (
    <Section
      title="Prompts"
      tagline="Saved prompts: type /name in any composer."
      help={<>Words after the name are appended. Variables: {code("{agent} {role} {model} {cwd} {branch} {changes} {last_diff} {goal} {notes} {date}")}.</>}
      extra={<Button type="primary" icon={<PlusOutlined />} onClick={() => setPromptEdit({ index: null, draft: newPrompt() })}>Add prompt</Button>}
    >
      <ItemList empty="No prompts yet." onAdd={() => setPromptEdit({ index: null, draft: newPrompt() })} addLabel="Add prompt">
        {cfg.prompts.map((p, i) => (
          <ItemRow
            key={i}
            name={<span className="mono">/{p.name || "untitled"}</span>}
            meta={p.description ? <Text type="secondary">{p.description}</Text> : null}
            summary={<span className="st-clamp">{p.text}</span>}
            onOpen={() => setPromptEdit({ index: i, draft: { ...p } })}
            onRemove={() => removeListItem("prompts", i)}
            removeTitle={`Remove /${p.name}?`}
          />
        ))}
      </ItemList>
      <EditModal title={promptEdit?.index === null ? "Add prompt" : `Edit /${promptEdit?.draft.name ?? ""}`} open={!!promptEdit} onClose={() => setPromptEdit(null)} onApply={applyPrompt} applyLabel={promptEdit?.index === null ? "Add prompt" : "Apply"}>
        {promptEdit && (
          <Fields cols={1}>
            <Fields>
              <Field label="Name" hint="Typed as /name." required><Input className="mono" prefix="/" value={promptEdit.draft.name} onChange={(e) => setPromptEdit({ ...promptEdit, draft: { ...promptEdit.draft, name: e.target.value.replace(/^\//, "") } })} placeholder="review" spellCheck={false} /></Field>
              <Field label="Description" hint="Shown in the composer's suggestions."><Input value={promptEdit.draft.description} onChange={(e) => setPromptEdit({ ...promptEdit, draft: { ...promptEdit.draft, description: e.target.value } })} placeholder="Review the current changes" /></Field>
            </Fields>
            <Field label="Text" hint={<>Variables: {code("{agent} {role} {model} {cwd} {branch} {changes} {last_diff} {goal} {notes} {date}")}.</>} required>
              <Input.TextArea autoSize={{ minRows: 6, maxRows: 18 }} value={promptEdit.draft.text} onChange={(e) => setPromptEdit({ ...promptEdit, draft: { ...promptEdit.draft, text: e.target.value } })} placeholder="Review the changes on branch {branch} in {cwd}…" />
            </Field>
          </Fields>
        )}
      </EditModal>
    </Section>
  );
}

export function TriggersPage() {
  const { cfg, setListItem, removeListItem, nameTaken } = useSettings();
  const notify = useStore((s) => s.notify);
  const [trigEdit, setTrigEdit] = useState<Editing<Trigger>>(null);
  const applyTrigger = () => {
    if (!cfg || !trigEdit) return;
    const d = { ...trigEdit.draft, name: trigEdit.draft.name.trim(), on: trigEdit.draft.on.trim(), action: trigEdit.draft.action.trim() };
    if (!d.name) return notify("A trigger needs a name", "error");
    if (!d.on || !d.action) return notify("A trigger needs a when and an action", "error");
    if (nameTaken(cfg.triggers, d.name, trigEdit.index)) return notify(`A trigger called ${d.name} already exists`, "error");
    setListItem("triggers", trigEdit.index, d);
    setTrigEdit(null);
  };
  return (
    <Section
      title="Triggers"
      tagline="Run an action on a schedule, when a pull request appears, or from a webhook."
      help={<>When: {code("cron 0 9 * * 1-5")}, {code("pr owner/repo")} (needs {code("gh")}), {code("webhook name")}. Actions: {code('send agent "text"')}, {code('loop start a b "goal"')}, {code("team start name")}, {code('task add "title"')}. PR triggers may use {code("{number} {title} {url}")}, webhooks {code("{body}")}. They fire while this app, {code("hubcodes watch")} or the daemon runs.</>}
      extra={<Button type="primary" icon={<PlusOutlined />} onClick={() => setTrigEdit({ index: null, draft: newTrigger() })}>Add trigger</Button>}
    >
      <ItemList empty="No triggers yet." onAdd={() => setTrigEdit({ index: null, draft: newTrigger() })} addLabel="Add trigger">
        {cfg.triggers.map((t, i) => (
          <ItemRow
            key={i}
            name={<span className="mono">{t.name || "untitled"}</span>}
            meta={<Tag variant="filled">{triggerWhen(t.on)}</Tag>}
            summary={<span className="mono">{t.action}</span>}
            enabled={t.enabled}
            onEnable={(v) => setListItem("triggers", i, { ...t, enabled: v })}
            onOpen={() => setTrigEdit({ index: i, draft: { ...t } })}
            onRemove={() => removeListItem("triggers", i)}
            removeTitle={`Remove trigger ${t.name}?`}
            off={!t.enabled}
          />
        ))}
      </ItemList>
      <EditModal title={trigEdit?.index === null ? "Add trigger" : `Edit trigger ${trigEdit?.draft.name ?? ""}`} open={!!trigEdit} onClose={() => setTrigEdit(null)} onApply={applyTrigger} applyLabel={trigEdit?.index === null ? "Add trigger" : "Apply"}>
        {trigEdit && <TriggerForm draft={trigEdit.draft} onChange={(p) => setTrigEdit({ ...trigEdit, draft: { ...trigEdit.draft, ...p } })} />}
      </EditModal>
    </Section>
  );
}

function PlaybookForm({ draft: pb, roleOptions, onChange }: { draft: Playbook; roleOptions: Opt[]; onChange: (p: Partial<Playbook>) => void }) {
  const setStep = (k: number, patch: Partial<PlaybookStep>) => onChange({ step: pb.step.map((st, m) => (m === k ? { ...st, ...patch } : st)) });
  const move = (k: number, dir: -1 | 1) => {
    const s = [...pb.step];
    const j = k + dir;
    if (j < 0 || j >= s.length) return;
    [s[k], s[j]] = [s[j], s[k]];
    onChange({ step: s });
  };
  return (
    <Fields cols={1}>
      <Fields>
        <Field label="Name" required><Input className="mono" value={pb.name} onChange={(e) => onChange({ name: e.target.value })} placeholder="feature" spellCheck={false} /></Field>
        <Field label="Description"><Input value={pb.description} onChange={(e) => onChange({ description: e.target.value })} placeholder="Plan, build with tests, review, merge" /></Field>
      </Fields>
      <FormGroup title="Steps" extra={<Button size="small" icon={<PlusOutlined />} onClick={() => onChange({ step: [...pb.step, newStep()] })}>Step</Button>}>
        {pb.step.length === 0 && <Text type="secondary">No steps yet.</Text>}
        {pb.step.map((st, k) => {
          const target = st.action ? "action" : st.agent ? "agent" : "role";
          return (
            <div key={k} className="st-step">
              <div className="st-step-head">
                <Text type="secondary">Step {k + 1}</Text>
                <Input className="mono st-step-name" value={st.name} placeholder="name" onChange={(e) => setStep(k, { name: e.target.value })} spellCheck={false} aria-label="Step name" />
                <span className="grow" />
                <Button size="small" type="text" onClick={() => move(k, -1)} disabled={k === 0} aria-label="Move up">↑</Button>
                <Button size="small" type="text" onClick={() => move(k, 1)} disabled={k === pb.step.length - 1} aria-label="Move down">↓</Button>
                <Popconfirm title="Remove this step?" okText="Remove" okButtonProps={{ danger: true }} onConfirm={() => onChange({ step: pb.step.filter((_, m) => m !== k) })}><Button size="small" type="text" danger icon={<DeleteOutlined />} aria-label="Remove step" /></Popconfirm>
              </div>
              <Fields>
                <Field label="Who does it" span>
                  <div className="st-step-who">
                    <Segmented
                      size="small"
                      value={target}
                      onChange={(v) => setStep(k, v === "agent" ? { role: "", action: "", agent: st.agent || "lead" } : v === "action" ? { role: "", agent: "", action: st.action || "merge {prev_agent}" } : { agent: "", action: "", role: st.role })}
                      options={[{ value: "role", label: "An agent with a role" }, { value: "agent", label: "One named agent" }, { value: "action", label: "hubcodes itself" }]}
                      aria-label="Step target"
                    />
                    {target === "action" ? (
                      <Input className="mono" value={st.action} placeholder='merge {prev_agent} · send lead "…" · task add "…"' onChange={(e) => setStep(k, { action: e.target.value })} spellCheck={false} aria-label="Action" />
                    ) : target === "agent" ? (
                      <Input className="mono" value={st.agent} placeholder="agent name, e.g. lead" onChange={(e) => setStep(k, { agent: e.target.value })} spellCheck={false} aria-label="Agent" />
                    ) : (
                      <Select className="st-wide" showSearch allowClear value={st.role || undefined} options={roleOptions} placeholder="Any idle agent" onChange={(v) => setStep(k, { role: v ?? "" })} aria-label="Role" />
                    )}
                  </div>
                </Field>
                <Field label="Before the next step" span={!(st.gate === "test" && !st.action)}>
                  <Select className="st-wide" value={st.gate} options={GATES} onChange={(v) => setStep(k, { gate: v })} aria-label="Gate" disabled={!!st.action} />
                </Field>
                {st.gate === "test" && !st.action && (
                  <Field label="Retries" hint="How often failures are handed back before the run pauses.">
                    <InputNumber min={0} max={10} value={st.retries} onChange={(n) => setStep(k, { retries: Math.max(0, Number(n) || 0) })} style={{ width: 120 }} />
                  </Field>
                )}
                {target !== "action" && (
                  <Field label="Prompt" hint={<>{code("{goal} {prev_agent} {prev_result} {run} {playbook} {step}")} are replaced.</>} span>
                    <Input.TextArea className="mono" autoSize={{ minRows: 2, maxRows: 8 }} value={st.prompt} placeholder="What this step's agent is told." onChange={(e) => setStep(k, { prompt: e.target.value })} spellCheck={false} aria-label="Prompt" />
                  </Field>
                )}
              </Fields>
            </div>
          );
        })}
      </FormGroup>
    </Fields>
  );
}
