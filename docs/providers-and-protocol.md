# Providers and protocol

How hubcodes starts an agent, reads its state, and lets agents talk. Status: everything in sections 1–5 is **observed** in the spike; section 6 is **designed**.

## 1. Provider adapter

A provider is a TOML table. The built-in set lives in `src/providers.rs`; a `[[provider]]` with the same `name` in `hubcodes.toml` overrides it field by field.

```toml
[[provider]]
name = "claude"              # used in `hubcodes spawn -p claude` and in the node title
command = "claude"           # shell command run inside the tmux window; may contain flags
glyph = "✦"                  # one character shown before the provider name
color = 215                  # xterm-256 index for the node accent
waiting_pattern = "(?i)(allow|y/n|press enter)"   # optional; see section 3
```

Built-in providers:

| name | command | glyph | colour | notes |
|---|---|---|---|---|
| claude | `claude` | ✦ | 215 | Claude Code |
| codex | `codex` | ◇ | 252 | Codex CLI |
| opencode | `opencode` | ◈ | 80 | |
| pi | `pi` | π | 176 | |
| omp | `omp` | ◆ | 114 | |
| gemini | `gemini` | ✧ | 75 | Gemini CLI |
| agy | `agy` | ◎ | 69 | Antigravity CLI: `--model`, resume with `-c`, models from `agy models`; no system-prompt flag, so the role and brief arrive as its first message |
| hermes | `hermes chat` | ☤ | 178 | Hermes Agent (Nous Research): `--model`, resume with `-c`; sign in with `hermes model` (secrets in `~/.hermes/.env`); role and brief arrive as its first message |
| shell | `$SHELL` | $ | 245 | plain shell; messages are sent without the `[from …]` prefix |

The command is wrapped at spawn time:

```sh
HUBCODES_AGENT='<name>' HUBCODES_SESSION='<session>' <command>; echo; echo '[hubcodes] agent exited'; exec $SHELL
```

so the window survives the agent exiting (the error stays visible) and the agent can attribute its own `hubcodes send` calls.

Adding a provider that hubcodes has never heard of needs no code: a `[[provider]]` table with `name` and `command` is enough.

## 2. Environment contract

| Variable | Set by | Used for |
|---|---|---|
| `HUBCODES_AGENT` | `hubcodes spawn` | default `--from` of `hubcodes send` |
| `HUBCODES_SESSION` | `hubcodes spawn` | default session of every subcommand (overridable with `-s`) |
| `TMUX` | tmux | `hubcodes` with no arguments opens the canvas in place when set |

## 3. Status heuristics

Observed every 500 ms per window:

| Signal | Status |
|---|---|
| `#{pane_dead}` = 1 | `Offline` |
| captured lines changed within the last 2.5 s | `Working` |
| no change, last non-empty line matches the waiting pattern, and the foreground command is not a shell | `Waiting` (shown as "needs you") |
| otherwise | `Idle` |

Default waiting pattern (used when the provider sets none):

```
(?i)(\(y/n\)|\[y/n\]|\by/n\b|allow|approve|permission|press enter|continue\?|\?\s*$|❯\s*$|>\s*$)
```

This is a heuristic and is documented as one in the UI (`?` help). It is good enough to see "reviewer is blocked on a permission prompt" from across the canvas; it is not good enough to automate on. Phase 2 adds hook-based status (section 6) and keeps this as the fallback.

## 4. Message protocol v0 (tmux options)

State shared between the CLI and the canvas, all stored as tmux **session** options so any process with access to the tmux socket can read and write them:

| Option | Scope | Format | Written by |
|---|---|---|---|
| `@hubcodes_provider` | window | provider name | `hubcodes spawn`, canvas `n` |
| `@hubcodes_edges` | session | `from>to>label,from>to>label,…` (names, not ids) | `hubcodes connect/disconnect`, canvas `c` |
| `@hubcodes_last_msg` | session | `seq\tfrom\tto\ttext` (tabs and newlines in text become spaces) | `hubcodes send`, canvas `m` |

Delivery of a message is a paste into the recipient's pane:

```
tmux load-buffer -b hubcodes -            # stdin = "[from builder] <text>"
tmux paste-buffer -d -p -b hubcodes -t @9 # bracketed paste
tmux send-keys -t @9 Enter
```

Guarantees and non-guarantees:

- At-most-once delivery. If the recipient is mid-prompt or in a pager, the text lands wherever the cursor is. v0 does not queue.
- Only the last message is retained in tmux; the canvas keeps a log for its lifetime.
- Names are the identity. Renaming a tmux window renames the agent and breaks edges that referenced the old name.
- `seq` is monotonic per session and lets the canvas de-duplicate its own sends.

## 5. CLI reference

```
hubcodes                              open the canvas (creates/attaches tmux session `hubcodes`)
hubcodes --demo | hubcodes demo       scripted demo, no tmux
hubcodes [-s SESSION] canvas          canvas in the current pane
hubcodes [-s SESSION] spawn NAME -p PROVIDER [--cwd DIR]
hubcodes [-s SESSION] send TO TEXT... [--from NAME]
hubcodes [-s SESSION] connect FROM TO [--label L]
hubcodes [-s SESSION] disconnect A B
hubcodes [-s SESSION] read AGENT [--lines N]     last lines, ANSI stripped
hubcodes [-s SESSION] ls                          agents, window ids, providers, links
hubcodes [-s SESSION] kill NAME
```

Exit codes: 0 on success, 1 with a message on stderr otherwise (unknown agent, unknown provider, tmux missing, session missing).

### Prompt snippet for agents

Paste into the agent's system prompt / `CLAUDE.md` / `AGENTS.md` so it knows the tools exist:

```
You are one agent on a shared hubcodes canvas. Other agents are tmux windows.
- `hubcodes ls` lists agents and links.
- `hubcodes send <agent> "<message>"` delivers a message into that agent's terminal. Keep it to one paragraph; say what you need and where the artefact is (branch, path).
- `hubcodes read <agent> --lines 40` shows what another agent printed last.
- Messages you receive start with `[from <agent>]`. Reply with `hubcodes send <agent> "..."` when the sender needs an answer.
- Do not send a message to an agent that is not linked to you unless a human told you to.
```

## 6. Phase 2 (designed): hooks and MCP

Status from the agent instead of from the screen:

| Provider | Mechanism | Event → status |
|---|---|---|
| Claude Code | hooks (`Notification`, `Stop`, `PreToolUse` permission prompts) run `hubcodes hook --event …` | `Notification` with permission → Waiting; `Stop` → Idle; any tool use → Working |
| Codex CLI | `notify` command in config | `agent-turn-complete` → Idle |
| OpenCode | plugin calling the CLI | tool events → Working, completion → Idle |
| pi, omp, gemini, agy, hermes, shell | heuristic from section 3 | fallback |

An MCP server exposed by the daemon gives agents typed tools (`send`, `read`, `note`, `ask_human`) with the same semantics as the CLI. The CLI stays, because a shell command is the only interface every provider shares.

Message protocol v1 moves from tmux options to the daemon: messages get ids, a delivery state (`queued`, `pasted`, `acked`), and the daemon pastes only when the recipient is at a prompt (hook-reported or heuristic), queuing otherwise. That removes the "text landed in a pager" failure of v0.
