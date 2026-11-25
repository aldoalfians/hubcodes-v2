import type { Config } from "./types";

/**
 * The core writes `hubcodes.toml` sparsely: empty strings, empty lists and
 * absent options are left out (`skip_serializing_if`), and `get_config`
 * passes that through. The page's types say every field is there, so fill
 * the gaps once, where the config arrives, instead of guarding every read.
 */
const fill = <T extends object>(value: T | undefined | null, defaults: Partial<T>): T => ({ ...defaults, ...(value ?? {}) }) as T;
const list = <T>(v: T[] | undefined | null): T[] => (Array.isArray(v) ? v : []);

const target = { url: "", command: "", args: [] as string[], headers: {} as Record<string, string>, env: {} as Record<string, string> };

export function normalizeConfig(c: Config): Config {
  return {
    ...c,
    routeThroughOrchestrator: c.routeThroughOrchestrator ?? true,
    providers: list(c.providers),
    roles: list(c.roles),
    peers: list(c.peers).map((p) => fill(p, { user: null, session: "hubcodes", hubcodes: "hubcodes", max_agents: 0, allow: [], transport: "", port: null })),
    prompts: list(c.prompts).map((p) => fill(p, { description: "", text: "" })),
    templates: list(c.templates).map((t) => ({
      ...fill(t, { description: "" }),
      agents: list(t.agents).map((a) => fill(a, { role: null, model: null, worktree: null, device: null, account: null })),
    })),
    triggers: list(c.triggers),
    memory: c.memory && {
      ...c.memory,
      hindsight: fill(c.memory.hindsight, { url: "", api_key: "", bank: "" }),
      mcp: fill(c.memory.mcp, { ...target, retain_tool: "", recall_tool: "" }),
      command: fill(c.memory.command, { retain: "", recall: "" }),
    },
    kbs: list(c.kbs).map((k) => fill(k, { ...target, search_tool: "", get_tool: "", path: "", repo: "", email: "", token: "", space: "", project: "", run: "", enabled: true })),
    sandbox: c.sandbox && fill(c.sandbox, { kind: "", command: "", writable: [] }),
    board: c.board && { ...c.board, columns: list(c.board.columns) },
    playbooks: list(c.playbooks).map((p) => ({
      ...fill(p, { description: "" }),
      step: list(p.step).map((s) => fill(s, { agent: "", role: "", prompt: "", action: "", gate: "", retries: 0 })),
    })),
    trackers: list(c.trackers).map((t) =>
      fill(t, {
        url: "", email: "", token: "", api_key: "", workspace: "", project: "", repo: "", query: "", instruction: "", role: "",
        on_start: "", on_done: "", run: "", command: "", args: [], headers: {}, list_tool: "", transition_tool: "", comment_tool: "", since: "", ignore: "",
      }),
    ),
    policy: c.policy && fill(c.policy, { mode: "off", allow: [], deny: [], allowTools: [], denyPaths: [] }),
    connections: list(c.connections).map((x) => fill(x, { url: "", email: "", token: "", workspace: "" })),
  };
}
