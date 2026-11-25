import { useState } from "react";
import { Input, Segmented, Space, Switch, Typography } from "antd";
import { Group, ListInput, Row, Section } from "./primitives";
import { Amount, OptionalNumber, code } from "./shared";
import { SecretInput } from "./connect";
import { useSettings } from "./context";
import { AutoTeamGroup, FailoverGroup } from "./AgentFeatures";
import { isMac } from "../../lib/platform";
import { SECRET_STORE } from "../../lib/platform";

const { Text } = Typography;

export function AgentsPage() {
  const { cfg, edit } = useSettings();
  return (
    <Section title="Agents" tagline="Limits that keep a team from running away, and when to alert you.">
      <Group title="Messages" desc="Keeps every role on its own work.">
        <Row label="Check with the orchestrator" hint={cfg.routeThroughOrchestrator ? "What you type to an agent linked to an orchestrator carries a note: check with that orchestrator before acting. Agents with no line to one, and ⌥Enter, get no note." : "What you type to an agent goes straight to it."}>
          <Switch checked={cfg.routeThroughOrchestrator} onChange={(v) => edit({ routeThroughOrchestrator: v })} aria-label="Ask agents to check with their orchestrator" />
        </Row>
      </Group>
      <Group title="Loops" desc="Two linked agents that hand their results to each other until the goal is met.">
        <Row label="Round limit" hint={cfg.maxRounds > 0 ? "One round is one message between the two agents. At the limit the loop pauses and asks you." : "Runs until an agent answers DONE, you pause it on the canvas, or a token budget below is reached."}>
          <RoundLimit value={cfg.maxRounds} onChange={(v) => edit({ maxRounds: v })} />
          {cfg.maxRounds === 0 && !cfg.maxTokensPerAgent && !cfg.maxTokensPerLoop && <Text type="warning" className="st-note">No token budget is set either, so only DONE or you stop a loop.</Text>}
        </Row>
      </Group>
      <Group title="Token budgets" desc="Read from each agent's own transcript, input and output together. Over budget, its loops pause and you get a notification.">
        <Row label="Per agent">
          <OptionalNumber value={cfg.maxTokensPerAgent} onChange={(v) => edit({ maxTokensPerAgent: v })} unit="tokens" step={10000} empty="No limit" thousandsSep />
        </Row>
        <Row label="Per loop" hint="Both agents of a loop added up.">
          <OptionalNumber value={cfg.maxTokensPerLoop} onChange={(v) => edit({ maxTokensPerLoop: v })} unit="tokens" step={10000} empty="No limit" thousandsSep />
        </Row>
      </Group>
      <Group title="Sandbox" desc="Keeps an agent from writing outside its own folders. Turned on per agent in New agent.">
        <Row label="Kind" hint={cfg.sandbox.kind === "macos" ? "Writes allowed only in the agent's folder, the repo's .git, temp, and the provider's own config and cache folders." : cfg.sandbox.kind === "command" ? <>{code("{cmd}")} is the agent command (quoted), {code("{cwd}")} its folder.</> : "Agents may write anywhere you can."}>
          <Segmented value={cfg.sandbox.kind} options={[{ value: "", label: "Off" }, ...(isMac || cfg.sandbox.kind === "macos" ? [{ value: "macos", label: "macOS sandbox" }] : []), { value: "command", label: "Wrapper command" }]} onChange={(k) => edit({ sandbox: { ...cfg.sandbox, kind: String(k) } })} />
        </Row>
        {cfg.sandbox.kind === "command" && (
          <Row label="Wrapper command" stack>
            <Input className="mono" value={cfg.sandbox.command} onChange={(e) => edit({ sandbox: { ...cfg.sandbox, command: e.target.value } })} placeholder="docker run --rm -it -v {cwd}:{cwd} -w {cwd} my/agents sh -lc {cmd}" spellCheck={false} />
          </Row>
        )}
        {cfg.sandbox.kind === "macos" && (
          <Row label="Extra writable folders" hint="Besides the agent's own folders listed above.">
            <ListInput label="Extra writable folders" mono value={cfg.sandbox.writable} onChange={(v) => edit({ sandbox: { ...cfg.sandbox, writable: v } })} placeholder="~/shared" emptyText="Only the defaults." />
          </Row>
        )}
      </Group>
      <AutoTeamGroup />
      <FailoverGroup />
      <Group title="Alerts" desc="A notification when something looks stuck. Empty turns that alert off.">
        <Row label="Idle with a task" hint="An agent that owns a task but has been quiet this long.">
          <OptionalNumber value={cfg.alerts.idle_minutes} onChange={(v) => edit({ alerts: { ...cfg.alerts, idle_minutes: v } })} unit="min" empty="Off" />
        </Row>
        <Row label="Loop changes no files" hint="Rounds in a row without a file change in either agent's folder.">
          <OptionalNumber value={cfg.alerts.stale_loop_rounds} onChange={(v) => edit({ alerts: { ...cfg.alerts, stale_loop_rounds: v } })} unit="rounds" empty="Off" />
        </Row>
        <Row label="Repeated error" hint="The same error line stays on screen for this many polls (one every 5 s).">
          <OptionalNumber value={cfg.alerts.repeated_error_polls} onChange={(v) => edit({ alerts: { ...cfg.alerts, repeated_error_polls: v } })} unit="polls" empty="Off" />
        </Row>
        <Row label="Also send to" hint={<>Every alert, including a pull request that needs you, posted to a Slack or Discord webhook or an ntfy topic ({code("https://ntfy.sh/your-topic")}) for your phone. Saved to the {SECRET_STORE}.</>} stack>
          <SecretInput value={cfg.alerts.webhook ?? ""} onChange={(v) => edit({ alerts: { ...cfg.alerts, webhook: v } })} placeholder={`Paste the webhook URL; it is saved to the ${SECRET_STORE}`} label="Alert webhook" />
        </Row>
      </Group>
    </Section>
  );
}

/** A loop's round limit: a number, or none at all (0). */
function RoundLimit({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const [last, setLast] = useState(value > 0 ? value : 20);
  return (
    <Space wrap>
      <Segmented value={value > 0 ? "limit" : "none"} options={[{ value: "limit", label: "Pause after" }, { value: "none", label: "No limit" }]} onChange={(v) => onChange(v === "none" ? 0 : last)} />
      {value > 0 && <Amount value={value} min={1} max={10000} onChange={(n) => { const v = Math.max(1, n || 1); setLast(v); onChange(v); }} unit="rounds" label="Rounds" />}
    </Space>
  );
}
