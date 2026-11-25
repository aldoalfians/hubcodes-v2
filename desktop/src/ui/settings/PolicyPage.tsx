import { useState } from "react";
import { Button, Input, Space, Tag, Tooltip, Typography } from "antd";
import { useStore } from "../../store";
import { backend } from "../../lib/backend";
import type { PolicyDecision } from "../../lib/types";
import { Group, ListInput, Row, Section } from "./primitives";
import { code } from "./shared";
import { useSettings } from "./context";

const { Text } = Typography;

const SUGGESTED_POLICY = {
  allow: ["git status*", "git diff*", "git log*", "git branch*", "git show*", "ls*", "cat *", "head *", "tail *", "rg *", "grep *", "find *", "cargo test*", "cargo build*", "cargo check*", "cargo clippy*", "pnpm test*", "pnpm run build", "npm test*", "npm run build", "pytest*", "go test*", "make test"],
  deny: ["rm -rf *", "rm -r /*", "git push --force*", "git push -f*", "git reset --hard*", "git clean -f*", "sudo *", "* | sh", "* | bash", "curl * | *", "DROP TABLE*", "DROP DATABASE*", "mkfs*", "dd if=*"],
  allowTools: ["Read", "Glob", "Grep", "WebFetch", "WebSearch", "TodoWrite", "Task"],
  denyPaths: ["~/.ssh/*", "*/.ssh/*", "*.env", "*.env.*", "*/secrets/*", "*.pem", "*.key"],
};
const CLAUDE_TOOLS = ["Read", "Glob", "Grep", "LS", "WebFetch", "WebSearch", "TodoWrite", "Task", "NotebookRead", "Edit", "MultiEdit", "Write", "NotebookEdit", "Bash"];

export function PolicyPage() {
  const { cfg, editPolicy } = useSettings();
  const notify = useStore((s) => s.notify);
  const [policyCmd, setPolicyCmd] = useState("git status && cargo test");
  const [policyResult, setPolicyResult] = useState<PolicyDecision | null>(null);
  const testPolicy = async () => {
    if (!cfg) return;
    try {
      setPolicyResult(await backend.policyTest(cfg.policy, policyCmd));
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const missing = (["allow", "deny", "allowTools", "denyPaths"] as const).reduce((n, k) => n + SUGGESTED_POLICY[k].filter((r) => !cfg.policy[k].includes(r)).length, 0);
  const addSuggested = () => {
    const merge = (k: "allow" | "deny" | "allowTools" | "denyPaths") => Array.from(new Set([...cfg.policy[k], ...SUGGESTED_POLICY[k]]));
    editPolicy({ mode: "auto", allow: merge("allow"), deny: merge("deny"), allowTools: merge("allowTools"), denyPaths: merge("denyPaths") });
    notify(`Added ${missing} suggested rules. Save changes to keep them.`);
  };
  const empty = cfg.policy.allow.length + cfg.policy.deny.length + cfg.policy.allowTools.length + cfg.policy.denyPaths.length === 0;
  const verdict = policyResult && (
    <Tag variant="filled" color={policyResult.kind === "allow" ? "success" : policyResult.kind === "deny" ? "error" : "warning"} className="st-decision">
      {policyResult.kind === "allow" ? "allowed" : policyResult.kind === "deny" ? "denied" : "would ask you"}{policyResult.rule ? ` · ${policyResult.rule}` : ""}
    </Tag>
  );
  return (
    <Section
      title="Policy"
      tagline="Which agent prompts hubcodes answers by itself. Everything else still asks you."
      help={<>Claude Code is answered before it even asks (PreToolUse hook); every other agent gets its yes or no keys pressed when the question on screen matches a rule. Deny wins over allow. A repository can add rules in {code(".hubcodes/team.toml")} under {code("[policy]")}. Every decision is logged as a {code("policy")} event and shown as a toast; from a terminal: {code('hubcodes policy test "npm run deploy"')}.</>}
      extra={missing > 0 && !empty ? <Tooltip title={`Adds ${missing} common safe commands, risky commands to refuse, read-only Claude tools and protected paths`}><Button onClick={addSuggested}>Add {missing} suggested rules</Button></Tooltip> : null}
    >
      <Group
        title="Answer prompts by the rules"
        desc="Allowed commands run without asking, denied ones are refused, the rest wait for you."
        toggle={{ checked: cfg.policy.mode === "auto", onChange: (v) => editPolicy({ mode: v ? "auto" : "off" }), label: "Apply the rules" }}
        off="Off: every prompt waits for you. You can still prepare the rules below."
      >
        <Row label="Try a command" hint="What the rules answer now, unsaved changes included." stack>
          <Space.Compact className="st-wide">
            <Input className="mono" value={policyCmd} onChange={(e) => setPolicyCmd(e.target.value)} onPressEnter={() => void testPolicy()} spellCheck={false} aria-label="Command to try" />
            <Button onClick={() => void testPolicy()}>Test</Button>
          </Space.Compact>
          {verdict}
        </Row>
      </Group>
      {empty && (
        <div className="pol-start">
          <div>
            <div className="st-group-title">Start from the suggested rules</div>
            <Text type="secondary" className="st-group-desc">{missing} rules: common read-only and test commands allowed, force pushes, <code>sudo</code> and piping into a shell refused, read-only Claude tools, and secrets such as <code>~/.ssh</code> and <code>.env</code> protected. Edit them after.</Text>
          </div>
          <Button type="primary" onClick={addSuggested}>Add {missing} rules</Button>
        </div>
      )}
      <Group title="Shell commands" desc={<>{code("*")} is anything, {code("?")} one character. Without a wildcard a rule matches the first words ({code("git status")} also matches {code("git status -s")}). In {code("a && b")} every part must be allowed.</>}>
        <div className="pol-cols">
          <div className="pol-col">
            <div className="pol-col-head"><span className="pol-dot is-allow" />Allow <Text type="secondary">runs without asking</Text></div>
            <ListInput label="Allowed commands" mono value={cfg.policy.allow} onChange={(v) => editPolicy({ allow: v })} suggestions={SUGGESTED_POLICY.allow} separators={/\n/} placeholder="cargo test*" emptyText="Nothing yet." />
          </div>
          <div className="pol-col">
            <div className="pol-col-head"><span className="pol-dot is-deny" />Deny <Text type="secondary">refused, checked first</Text></div>
            <ListInput label="Denied commands" mono value={cfg.policy.deny} onChange={(v) => editPolicy({ deny: v })} suggestions={SUGGESTED_POLICY.deny} separators={/\n/} placeholder="git push --force*" emptyText="Nothing yet." />
          </div>
        </div>
      </Group>
      <Group title="Claude Code" desc="Its Bash calls follow the shell rules above; Write and Edit follow the protected paths.">
        <div className="pol-cols">
          <div className="pol-col">
            <div className="pol-col-head"><span className="pol-dot is-allow" />Tools that never ask <Text type="secondary">read-only ones are the usual pick</Text></div>
            <ListInput label="Tools that never ask" mono value={cfg.policy.allowTools} onChange={(v) => editPolicy({ allowTools: v })} suggestions={CLAUDE_TOOLS} placeholder="Read" emptyText="Every tool asks as usual." />
          </div>
          <div className="pol-col">
            <div className="pol-col-head"><span className="pol-dot is-deny" />Protected paths <Text type="secondary">writes refused; ~ is home</Text></div>
            <ListInput label="Protected paths" mono value={cfg.policy.denyPaths} onChange={(v) => editPolicy({ denyPaths: v })} suggestions={SUGGESTED_POLICY.denyPaths} separators={/\n/} placeholder="*.env" emptyText="No path is protected." />
          </div>
        </div>
      </Group>
    </Section>
  );
}
