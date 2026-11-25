import { useState } from "react";
import { Badge, Button, Dropdown, Input, InputNumber, Select, Space, Tooltip } from "antd";
import { ApiOutlined, ExportOutlined, PlusOutlined, UploadOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { color256 } from "../../lib/colors";
import { backend } from "../../lib/backend";
import type { ProviderEdit } from "../../lib/types";
import { ConnectPanel, useProviderStatus } from "../ConnectPanel";
import { askText } from "../ask";
import { ProviderAccounts } from "./ProviderAccounts";
import { ModelsOnDevice } from "./ModelsOnDevice";
import { EditModal, Field, Fields, FormGroup, Group, ItemList, ItemRow, ListInput, Section } from "./primitives";
import { code, type Editing } from "./shared";
import { useSettings } from "./context";

const newProvider = (): ProviderEdit => ({ name: "", command: "", glyph: "◆", color: null, waitingPattern: null, modelFlag: "--model {model}", roleFlag: null, models: [], modelsCommand: null, approveKeys: [], denyKeys: [], resumeFlag: null, hookKind: null, builtin: false, installed: false });

export function ProvidersPage() {
  const { cfg, setCfg, setListItem, removeListItem, nameTaken } = useSettings();
  const reload = useStore((s) => s.reload);
  const notify = useStore((s) => s.notify);
  const [checking, setChecking] = useState<string | null>(null);
  const check = useProviderStatus(checking);
  const [provEdit, setProvEdit] = useState<Editing<ProviderEdit>>(null);
  const applyProvider = () => {
    if (!cfg || !provEdit) return;
    const d = { ...provEdit.draft, name: provEdit.draft.name.trim(), command: provEdit.draft.command.trim() };
    if (!d.name) return notify("A provider needs a name", "error");
    if (!d.command) return notify("A provider needs a command", "error");
    if (nameTaken(cfg.providers, d.name, provEdit.index)) return notify(`A provider called ${d.name} already exists`, "error");
    setListItem("providers", provEdit.index, d);
    setProvEdit(null);
  };
  return (
    <Section
      title="Providers"
      tagline="The command hubcodes runs for a new agent. Add your own for any CLI agent."
      help={<>Built-in providers read their own model catalog; Check shows what is installed and logged in. Provider files are one TOML each (command, model and role flags, hooks kind, resume flag, approve keys, models). Files in {code("~/.config/hubcodes/providers/")} load automatically, so a team shares one file per tool.</>}
      extra={
        <Space>
          <Dropdown menu={{ items: cfg.providers.map((p) => ({ key: p.name, label: p.name })), onClick: ({ key }) => { void backend.providerExport(key).then((t) => { void navigator.clipboard?.writeText(t); notify(`Copied ${key}.toml to the clipboard; share it or save it under ~/.config/hubcodes/providers/`); }).catch((e) => notify(String(e), "error")); } }}>
            <Button icon={<ExportOutlined />}>Export</Button>
          </Dropdown>
          <Button icon={<UploadOutlined />} onClick={() => void askText("Import a provider file", { multiline: true, placeholder: "[provider]\nname = \"aider\"\ncommand = \"aider\"\n…", okText: "Install" }).then((t) => { if (t) void backend.providerImport(t).then((paths) => { notify(`Installed ${paths.join(", ")}`); void reload(); backend.getConfig().then(setCfg).catch(() => {}); }).catch((e) => notify(String(e), "error")); })}>Import</Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setProvEdit({ index: null, draft: newProvider() })}>Add provider</Button>
        </Space>
      }
    >
      <Group>
        <ItemList>
          {cfg.providers.map((p, i) => {
            const models = !p.modelFlag ? "takes no model" : p.builtin ? "models read from the tool (below)" : p.models.length ? p.models.join(", ") : "no models listed";
            return (
              <div key={p.name || `new-${i}`} className="st-item-wrap">
                <ItemRow
                  icon={<span className="st-glyph" style={{ color: color256(p.color) }}>{p.glyph || "◆"}</span>}
                  name={p.name}
                  meta={p.command !== p.name ? <span className="st-item-cmd mono">{p.command}</span> : undefined}
                  summary={<>{models}{p.roleFlag ? " · told its role" : ""}</>}
                  status={p.installed ? <Badge status="success" text="Installed" className="st-badge" /> : <Tooltip title="Not found on your PATH. Install it or fix the command."><Badge status="error" text="Not installed" className="st-badge" /></Tooltip>}
                  actions={<Button size="small" type="text" icon={<ApiOutlined />} onClick={() => setChecking(checking === p.name ? null : p.name)} disabled={!p.name.trim()}>{checking === p.name ? "Hide" : "Check"}</Button>}
                  onOpen={() => setProvEdit({ index: i, draft: { ...p } })}
                  onRemove={p.builtin ? undefined : () => removeListItem("providers", i)}
                  removeTitle={`Remove ${p.name}?`}
                />
                {checking === p.name && <div className="st-expand"><ConnectPanel status={check.status} loading={check.loading} error={check.error} onRefresh={check.refresh} /></div>}
              </div>
            );
          })}
        </ItemList>
      </Group>
      <EditModal title={provEdit?.index === null ? "Add provider" : `Edit ${provEdit?.draft.name ?? ""}`} open={!!provEdit} onClose={() => setProvEdit(null)} onApply={applyProvider} applyLabel={provEdit?.index === null ? "Add provider" : "Apply"}>
        {provEdit && <ProviderForm draft={provEdit.draft} onChange={(patch) => setProvEdit({ ...provEdit, draft: { ...provEdit.draft, ...patch } })} />}
      </EditModal>
      <ModelsOnDevice />
      <ProviderAccounts />
    </Section>
  );
}

function ProviderForm({ draft, onChange }: { draft: ProviderEdit; onChange: (p: Partial<ProviderEdit>) => void }) {
  return (
    <Fields cols={1}>
      <FormGroup title="Command">
        <Fields>
          <Field label="Name" hint="Shown on cards and used in hubcodes spawn -p." required><Input className="mono" value={draft.name} disabled={draft.builtin} onChange={(e) => onChange({ name: e.target.value })} placeholder="aider" spellCheck={false} /></Field>
          <Field label="Command" hint="Run in the agent's folder. Flags are fine; $SHELL works." required><Input className="mono" value={draft.command} onChange={(e) => onChange({ command: e.target.value })} placeholder="aider --no-auto-commits" spellCheck={false} /></Field>
          <Field label="Glyph" hint="One or two characters."><Input value={draft.glyph} maxLength={2} className="st-glyph-input" onChange={(e) => onChange({ glyph: e.target.value })} /></Field>
          <Field label="Colour" hint="xterm-256 index.">
            <Space><span className="st-swatch" style={{ background: color256(draft.color) }} /><InputNumber min={0} max={255} value={draft.color} onChange={(n) => onChange({ color: n === null || n === undefined ? null : Number(n) })} /></Space>
          </Field>
        </Fields>
      </FormGroup>
      <FormGroup title="Models">
        <Fields>
          <Field label="Model flag" hint={<>{code("{model}")} is replaced. Empty: the provider takes no model.</>}><Input className="mono" value={draft.modelFlag ?? ""} onChange={(e) => onChange({ modelFlag: e.target.value || null })} placeholder="--model {model}" spellCheck={false} /></Field>
          <Field label="Models command" hint="Optional. Prints one model id per line; read when New agent opens."><Input className="mono" value={draft.modelsCommand ?? ""} onChange={(e) => onChange({ modelsCommand: e.target.value || null })} placeholder="aider --list-models" spellCheck={false} /></Field>
          <Field label="Models" hint={draft.modelFlag ? "Suggestions in New agent. Built-in providers also read their own catalog." : "Set a model flag first."} span>
            <ListInput label="Models" mono value={draft.models} disabled={!draft.modelFlag} onChange={(v) => onChange({ models: v })} placeholder="model id" emptyText="No suggestions." />
          </Field>
        </Fields>
      </FormGroup>
      <FormGroup title="Behaviour">
        <Fields>
          <Field label="Role flag" hint={<>How the agent is told its role at start; {code("{role}")} is replaced.</>}><Input className="mono" value={draft.roleFlag ?? ""} onChange={(e) => onChange({ roleFlag: e.target.value || null })} placeholder="--system-prompt 'You are the {role}'" spellCheck={false} /></Field>
          <Field label="Resume flag" hint={<>Appended by Restore and continue. {code("-c")} for claude, {code("resume --last")} for codex.</>}><Input className="mono" value={draft.resumeFlag ?? ""} onChange={(e) => onChange({ resumeFlag: e.target.value || null })} placeholder="-c" spellCheck={false} /></Field>
          <Field label="Hooks" hint="Lets the tool report working / waiting / idle itself instead of the screen heuristic.">
            <Select className="st-wide" value={draft.hookKind ?? ""} onChange={(v) => onChange({ hookKind: v || null })} options={[{ value: "", label: "None (screen heuristic)" }, { value: "claude", label: "Claude Code hooks" }, { value: "codex", label: "Codex notify" }]} />
          </Field>
          <Field label="Waiting pattern" hint="Regex on the last screen line that marks the agent as needs you. Empty: the built-in pattern."><Input className="mono" value={draft.waitingPattern ?? ""} onChange={(e) => onChange({ waitingPattern: e.target.value || null })} placeholder="(y/n)|allow\\?" spellCheck={false} /></Field>
          <Field label="Approve keys" hint="tmux key names, space separated."><Input className="mono" value={draft.approveKeys.join(" ")} onChange={(e) => onChange({ approveKeys: e.target.value.split(/\s+/).filter(Boolean) })} placeholder={defaultKeys(draft.name)[0]} spellCheck={false} /></Field>
          <Field label="Deny keys"><Input className="mono" value={draft.denyKeys.join(" ")} onChange={(e) => onChange({ denyKeys: e.target.value.split(/\s+/).filter(Boolean) })} placeholder={defaultKeys(draft.name)[1]} spellCheck={false} /></Field>
        </Fields>
      </FormGroup>
    </Fields>
  );
}

function defaultKeys(provider: string): [string, string] {
  if (provider === "claude" || provider === "opencode") return ["Enter", "Escape"];
  if (provider === "codex") return ["y", "n"];
  return ["y Enter", "n Enter"];
}
