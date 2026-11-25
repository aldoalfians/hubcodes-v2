import { useState } from "react";
import { Button, Input, Segmented, Space, Switch, Typography } from "antd";
import { ApiOutlined, SearchOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { SecretInput } from "./connect";
import { backend } from "../../lib/backend";
import type { MemoryConfig } from "../../lib/types";
import { ChoiceCards, Group, Result, Row, Section, ToolArgs, type Check } from "./primitives";
import { code } from "./shared";
import { McpServerFields, bridgedUrl, serverOf } from "./mcpAuth";
import { useSettings } from "./context";
import { SECRET_STORE } from "../../lib/platform";

const { Text } = Typography;

export function MemoryPage() {
  const { cfg, editMemory } = useSettings();
  const [memPing, setMemPing] = useState<Check | null>(null);
  const notify = useStore((s) => s.notify);
  const [finding, setFinding] = useState(false);
  const [signedIn, setSignedIn] = useState<string | null>(null);
  const [transport, setTransport] = useState<"http" | "stdio">(() => (!cfg.memory.mcp.url && cfg.memory.mcp.command && !bridgedUrl(cfg.memory.mcp) ? "stdio" : "http"));
  const [toolsOpen, setToolsOpen] = useState(false);
  const hsUrl = cfg.memory.hindsight.url.trim();
  const hsBad = !!hsUrl && !/^(https?:\/\/|\$\{)/.test(hsUrl);
  // Ask the memory server for its tools; the remember and search tools are picked from them.
  const discover = async () => {
    setFinding(true);
    try {
      const m = cfg.memory.mcp;
      const d = await backend.mcpDiscover({ url: m.url, command: m.command, args: m.args, headers: m.headers, env: m.env }, ["retain", "recall"]);
      setSignedIn(`Signed in · ${d.tools.length} tool${d.tools.length === 1 ? "" : "s"} on the server`);
      const p = d.picks;
      editMemory({ mcp: { ...m, ...(p.retain ? { retain_tool: p.retain[0], retain_args: p.retain[1] } : {}), ...(p.recall ? { recall_tool: p.recall[0], recall_args: p.recall[1] } : {}) } });
      notify(p.retain || p.recall ? `Found ${d.tools.length} tools; remembering with ${p.retain?.[0] ?? "—"}, searching with ${p.recall?.[0] ?? "—"}` : `Found ${d.tools.length} tools, none for memory. Set them below.`, p.retain || p.recall ? "info" : "error");
    } catch (e) {
      notify(String(e).replace(/^Error: /, ""), "error");
    } finally {
      setFinding(false);
    }
  };
  const pingMemory = async () => {
    if (!cfg) return;
    setMemPing({ ok: true, text: "Testing…", busy: true });
    const p = await backend.memoryPing(cfg.memory).catch((e) => ({ ok: false, provider: cfg.memory.provider, detail: String(e), ms: 0 }));
    setMemPing({ ok: p.ok, text: `${p.detail} (${p.ms} ms)` });
  };
  return (
    <Section
      title="Memory"
      tagline="What the team remembers across sessions."
      help={<>Agents call {code("hubcodes memory recall")} and {code("hubcodes memory retain")}; hubcodes recalls for a new agent's role and repo at start and retains task and loop results. {code("${ENV_VAR}")} in any field reads an environment variable, so keys stay out of the file.</>}
    >
      <Group
        title="Where memories live"
        extra={<Space><Result r={memPing} /><Button size="small" icon={<ApiOutlined />} onClick={() => void pingMemory()}>Test connection</Button></Space>}
      >
        <div className="st-group-pad">
          <ChoiceCards
            label="Memory service"
            value={cfg.memory.provider}
            onChange={(p) => editMemory({ provider: p as MemoryConfig["provider"] })}
            options={[
              { value: "builtin", title: "Built-in", desc: "A searchable store on this device. Nothing to set up." },
              { value: "hindsight", title: "Hindsight", desc: "Your Hindsight server over REST, shared by the team." },
              { value: "mcp", title: "MCP server", desc: "Any memory server that speaks MCP." },
              { value: "command", title: "Your own command", desc: "Two commands that read JSON on stdin." },
            ]}
          />
        </div>
        {cfg.memory.provider === "hindsight" && (
          <>
            <Row label="Server URL" hint={hsBad ? <Text type="warning">That is not a web address; it starts with https://</Text> : "Its base address, e.g. https://memories.example.com."}><Input className="mono" status={hsBad ? "warning" : undefined} value={cfg.memory.hindsight.url} onChange={(e) => editMemory({ hindsight: { ...cfg.memory.hindsight, url: e.target.value } })} placeholder="https://memories.example.com" spellCheck={false} /></Row>
            <Row label="API key" hint={<>Kept in this device's {SECRET_STORE}. {code("${HINDSIGHT_API_KEY}")} reads it from the environment instead.</>}><SecretInput value={cfg.memory.hindsight.api_key} onChange={(v) => editMemory({ hindsight: { ...cfg.memory.hindsight, api_key: v } })} label="API key" /></Row>
            <Row label="Bank" hint="The memory bank to use; one per team or project."><Input className="mono" value={cfg.memory.hindsight.bank} onChange={(e) => editMemory({ hindsight: { ...cfg.memory.hindsight, bank: e.target.value } })} placeholder="team" spellCheck={false} /></Row>
          </>
        )}
        {cfg.memory.provider === "mcp" && (
          <>
            <Row label="Connects" hint={transport === "http" ? "A server on the web, by its address." : "A program started for each call; speaks MCP on stdin and stdout."}>
              <Segmented size="small" value={transport} options={[{ value: "http", label: "Over HTTP" }, { value: "stdio", label: "Run a command" }]} onChange={(v) => { setTransport(v as "http" | "stdio"); editMemory({ mcp: { ...cfg.memory.mcp, ...(v === "http" ? { command: "", args: [] } : { url: "" }) } }); }} aria-label="How it connects" />
            </Row>
            {transport === "http"
              ? <McpServerFields as="row" target={cfg.memory.mcp} onChange={(p) => editMemory({ mcp: { ...cfg.memory.mcp, ...p } })} placeholder="https://memories.example.com/mcp/you/" signIn={{ run: discover, busy: finding, done: signedIn }} />
              : <Row label="Command" stack><Input className="mono" value={cfg.memory.mcp.command ?? ""} onChange={(e) => editMemory({ mcp: { ...cfg.memory.mcp, command: e.target.value } })} placeholder="npx -y some-memory-mcp" spellCheck={false} aria-label="Command (stdio)" /></Row>}
            <Row label="Tools" hint={toolsOpen ? <>Name, then arguments as JSON: {code("{text}")} is the memory, {code("{query}")} the question.</> : "Which server tools remember and search."} stack>
              <div className="mem-tools">
                <div className="mem-tools-sum">
                  <span className="mem-tool"><Text type="secondary">Remember</Text> <code>{cfg.memory.mcp.retain_tool || "—"}</code></span>
                  <span className="mem-tool"><Text type="secondary">Search</Text> <code>{cfg.memory.mcp.recall_tool || "—"}</code></span>
                  <Button size="small" icon={<SearchOutlined />} loading={finding} onClick={() => void discover()} disabled={!serverOf(cfg.memory.mcp) && !cfg.memory.mcp.command}>Find the tools</Button>
                  <Button size="small" type="text" onClick={() => setToolsOpen((v) => !v)}>{toolsOpen ? "Hide arguments" : "Edit"}</Button>
                </div>
                {toolsOpen && (
                  <div className="mem-tools-edit">
                    <ToolArgs tool={cfg.memory.mcp.retain_tool} args={cfg.memory.mcp.retain_args} fallback={{ content: "{text}" }} placeholder="retain" onTool={(v) => editMemory({ mcp: { ...cfg.memory.mcp, retain_tool: v } })} onArgs={(v) => editMemory({ mcp: { ...cfg.memory.mcp, retain_args: v } })} label="Remember tool" />
                    <ToolArgs tool={cfg.memory.mcp.recall_tool} args={cfg.memory.mcp.recall_args} fallback={{ query: "{query}" }} placeholder="recall" onTool={(v) => editMemory({ mcp: { ...cfg.memory.mcp, recall_tool: v } })} onArgs={(v) => editMemory({ mcp: { ...cfg.memory.mcp, recall_args: v } })} label="Search tool" />
                  </div>
                )}
              </div>
            </Row>
          </>
        )}
        {cfg.memory.provider === "command" && (
          <>
            <Row label="Remember" hint={<>Gets {code("{text, by, tags, session}")} as JSON on stdin.</>}><Input className="mono" value={cfg.memory.command.retain} onChange={(e) => editMemory({ command: { ...cfg.memory.command, retain: e.target.value } })} placeholder="mytool remember" spellCheck={false} /></Row>
            <Row label="Search" hint={<>Gets {code("{query, limit}")} on stdin; prints a JSON list or plain text.</>}><Input className="mono" value={cfg.memory.command.recall} onChange={(e) => editMemory({ command: { ...cfg.memory.command, recall: e.target.value } })} placeholder="mytool search" spellCheck={false} /></Row>
          </>
        )}
      </Group>
      <Group title="Automatic">
        <Row label="Recall for every new agent" hint="Role, repo and name are the question; the answers go into its system prompt.">
          <Switch checked={cfg.memory.auto_recall_on_start} onChange={(v) => editMemory({ auto_recall_on_start: v })} aria-label="Recall for every new agent" />
        </Row>
        <Row label="Retain results" hint="When a task is done or a loop finishes.">
          <Switch checked={cfg.memory.auto_retain} onChange={(v) => editMemory({ auto_retain: v })} aria-label="Retain results automatically" />
        </Row>
      </Group>
    </Section>
  );
}
