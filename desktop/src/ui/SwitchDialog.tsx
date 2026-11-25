import { useState } from "react";
import { Alert, Button, Select, Space, Typography } from "antd";
import { SwapOutlined } from "@ant-design/icons";
import { Dialog } from "./Dialog";
import { ModelPicker } from "./ModelPicker";
import { useProviderStatus } from "./ConnectPanel";
import { backend } from "../lib/backend";
import { color256 } from "../lib/colors";
import { useStore } from "../store";
import type { Agent } from "../lib/types";

const { Text } = Typography;

/**
 * Run an agent on another provider or model. It keeps its name, role,
 * folder and links. The same provider continues the conversation on the new
 * model; another provider starts with a hand-over note.
 */
export function SwitchDialog({ agent, onClose }: { agent: Agent; onClose: () => void }) {
  const providers = useStore((s) => s.providers);
  const notify = useStore((s) => s.notify);
  const [provider, setProvider] = useState(agent.provider);
  const [model, setModel] = useState(agent.model ?? "");
  const [busy, setBusy] = useState(false);
  const chosen = providers.find((p) => p.name === provider);
  const status = useProviderStatus(provider, provider === agent.provider ? agent.account : null);
  const same = provider === agent.provider;
  const unchanged = same && model.trim() === (agent.model ?? "");
  const pick = (p: string) => {
    setProvider(p);
    setModel(p === agent.provider ? agent.model ?? "" : "");
  };
  const run = async () => {
    setBusy(true);
    try {
      useStore.getState().markStarting(agent.name, `${provider}${chosen?.takesModel && model.trim() ? ` · ${model.trim()}` : ""}`);
      const r = await backend.switchAgent(agent.name, provider, chosen?.takesModel ? model.trim() || null : null);
      if (r.restored.errors.length) notify(r.restored.errors.join("; "), "error");
      else notify(`${agent.name} now runs on ${provider}${model.trim() ? ` · ${model.trim()}` : ""}: ${r.kept === "conversation" ? "the conversation continues" : "it started with a hand-over note"}.`);
      onClose();
    } catch (e) {
      useStore.getState().doneStarting(agent.name);
      notify(String(e).replace(/^Error: /, ""), "error");
    }
    setBusy(false);
  };
  return (
    <Dialog title={`Switch ${agent.name}`} onClose={onClose} width={520}>
      <div className="switch-form">
        <Text type="secondary">
          {agent.name} keeps its name{agent.role ? <>, the <b>{agent.role}</b> role</> : null}, its folder and its links.
        </Text>
        <label className="switch-field">
          <span>Provider</span>
          <Select
            value={provider}
            onChange={pick}
            aria-label="Provider"
            options={providers.filter((p) => p.installed && p.name !== "shell").map((p) => ({ value: p.name, label: <span><span style={{ color: color256(p.color) }}>{p.glyph}</span> {p.name}</span> }))}
          />
        </label>
        {chosen?.takesModel && (
          <label className="switch-field">
            <span>Model</span>
            <ModelPicker value={model} onChange={setModel} models={status.status?.models ?? []} loading={status.loading} placeholder={status.status?.currentModel ? `${status.status.currentModel} (default)` : "provider default"} />
          </label>
        )}
        <Alert
          type={same ? "success" : "info"}
          showIcon
          title={same ? "The conversation continues" : "A new tool starts with a hand-over note"}
          description={same
            ? `${agent.provider} picks up the same conversation on the new model.`
            : `${provider} cannot read ${agent.provider}'s conversation, so its brief carries what matters: the role, open board tasks, recent messages and the end of this terminal.`}
        />
        <Space className="switch-actions">
          <Button onClick={onClose} disabled={busy}>Cancel</Button>
          <Button type="primary" icon={<SwapOutlined />} loading={busy} disabled={unchanged} onClick={() => void run()}>Switch</Button>
        </Space>
      </div>
    </Dialog>
  );
}
