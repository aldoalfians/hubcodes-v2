# Roadmap

Dates are targets, not commitments. "Done" means the acceptance criteria below pass on macOS and Linux.

## Phase 0 — Spike (done 2026-11-11)

Scope: prove that a full-TUI canvas with routed links and live previews is pleasant to use, on tmux, in Rust.

| Criterion | Status |
|---|---|
| canvas renders nodes, orthogonal links with labels, animated message pulses | done, `hubcodes --demo` |
| tmux backend: windows become nodes, previews from `capture-pane`, status heuristic | done |
| providers: claude, codex, opencode, pi, omp, gemini, shell; overridable in `hubcodes.toml` | done |
| `hubcodes spawn / send / connect / disconnect / read / ls / kill` | done |
| focus mode via `select-window`, return via `Alt+Esc` / `prefix g` | done; replaced 2026-11-11 by open / tile / back (`join-pane`) |
| mouse: select, drag, pan, right-click connect, wheel | done; exercised 2026-11-14 with SGR mouse sequences injected through tmux (click, drag, wheel, right-click all verified from captures) |
| unit tests for routing | 3 tests pass |
| docs with diagrams, REA-backed teardown of Maestri and T3 Code | done |
| `cargo clippy` | clean (0 warnings, 2026-11-11) |

## Phase 1b — Desktop app (started 2026-11-11)

Decision: the main client is a **Tauri 2 + React desktop app** (`desktop/`); tmux still owns every PTY, the TUI stays for SSH. Rationale and design: [desktop-design.md](desktop-design.md); UX references: [reference-ui-teardown.md](reference-ui-teardown.md).

| Criterion | Status |
|---|---|
| Cargo workspace: `hubcodes-core` shared by TUI and desktop | done |
| canvas with live xterm.js per agent, orthogonal links, message pulses | done (React Flow) |
| pane streaming via `pipe-pane` → FIFO → events; input via `send-keys -H`; size via `resize-window` | done, verified on tmux 3.8 |
| composer per node, open sheet (`Enter` / `Esc`), rail, activity, palette (`⌘K`), new-agent and settings dialogs | done |
| layout (positions + viewport) persisted per session in the app data dir | done |
| Nerd Font glyphs in terminals | done: `desktop/src/fonts/nerd-symbols-mono.woff2`, a 1.0 MB icon-range subset of Hack Nerd Font Mono, bundled with JetBrains Mono so every machine renders prompts the same |
| notifications when an agent needs you | done: OS notification on the idle→needs-you edge while the window is in the background, Dock badge with the count; toggle in Settings → General |
| `@agent` mentions in the composer | done: `@name` at the start of a composer routes the text to that agent along a link (`send_message`); popup with ↑↓ / Tab |
| model discovery and provider connect panel | done: `hubcodes-core::models` reads each CLI's own catalog and login state (`hubcodes providers`, desktop `provider_status`, 60 s cache); New agent shows the real list with a Connect panel when a provider is missing or signed out |
| role and model per agent | done: `hubcodes spawn -r/-m`, pane options `@hubcodes_role` / `@hubcodes_model`, provider `model_flag` / `role_flag` / `models`, role chip editable on the node |
| agent-to-agent loops (trailer + round limit + DONE + reminder) | done: `hubcodes-core::relay`, `hubcodes loop …`, link actions on the canvas; see [loops-and-devices.md](loops-and-devices.md) |
| agents on other devices over SSH/Tailscale (`[[peer]]`, `device/agent`, remote cards, ssh-attach open) | done: `hubcodes-core::remote`, `hubcodes ls --json`, `hubcodes peers`, Settings → Devices |
| CLI sidecar in the .app (`~/.local/bin/hubcodes`, PATH for agents) | done |
| status from provider hooks (Claude Code `--settings`, Codex `notify`), token usage from transcripts | done: `hooks.rs`, `hubcodes hook` |
| worktree per agent, Changes tab (diff, commit, PR) | done: `git.rs`, `--worktree`, Own branch |
| team templates (`[[template]]`, New agent → Team, save running canvas) | done: `team.rs` |
| goal board with dispatch to idle agents by role | done: `tasks.rs`, `tend.rs`, ⌘B |
| fan-out / fan-in with a collector | done: `fanout.rs` |
| saved prompts `/name` with variables | done: `prompts.rs` |
| shared notes card + `hubcodes note` | done: `notes.rs` |
| SQLite transcript, searchable Activity, `hubcodes log` | done: `db.rs` |
| approval inbox (⌘I), `hubcodes approve|deny`, per-provider keys | done |
| token budgets per agent / loop | done (Claude Code usage; others when they expose it) |
| triggers: cron, PR, webhook | done: `triggers.rs`, `tend.rs` |
| phone / teammate web page with tailnet roles (`hubcodes serve`) | done: `serve.rs` |
| headless `hubcodes watch` | done |
| team memory providers (builtin / Hindsight REST / MCP / command) with Test connection, auto recall + retain | done: `memory.rs`, `mcp.rs` |
| knowledge sources (MCP / git / folder / Confluence / Jira / command), ⌘⇧K search, `hubcodes kb` | done: `kb.rs` |
| repo settings `.hubcodes/team.toml` (teams, test, merge, board, kb) | done: `repo.rs`, `hubcodes team init` |
| test runner per agent (`hubcodes test`, Changes → Run tests, card badge) | done: `testrun.rs` |
| merge queue with tests and conflict hand-off | done: `merge.rs`, `hubcodes merge` |
| review gate (`[board] review_required`, approve / request changes) | done |
| restore after restart with provider resume flags | done: `sessiondef.rs`, `hubcodes restore` |
| alerts: idle with task, stale loop, repeated error, exited agent | done: `tend.rs` |
| sandbox per agent (sandbox-exec or wrapper command) | done: `sandbox.rs` |
| presence / keyboard holder, server refuses contested sends | done |
| full canvas in a browser via `hubcodes serve` (/canvas/) | done: `http.ts`, `serve.rs` |
| provider files (export / import / providers dir) | done |
| settings that write `hubcodes.toml` (providers, folders) | done: Settings → General / Providers edit and save `~/.config/hubcodes/hubcodes.toml` (atomic write, validated); the running app switches to the new config without restart |
| signed `.app` / `.dmg` | done: `pnpm build:mac` (`desktop/scripts/release-mac.sh`) picks Developer ID → Apple Development → ad-hoc; notarization needs a Developer ID certificate plus `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` |
| doctor + first-run banner, `hubcodes doctor`, Copy diagnostics, log viewer | done 2026-11-13: `doctor.rs`, Settings → This Mac → Doctor |
| delivery queue (`outbox`, queued/delivered/failed, `send --now`) | done 2026-11-13: `relay::flush_outbox`, hook + tick |
| the team as MCP tools (`hubcodes mcp`, auto-registered for Claude Code and Codex) | done 2026-11-13: `mcpserver.rs` |
| needs-you navigation (⌘⇧] / ⌘⇧[, edge pills, chime), multi-line composer, file drop | done 2026-11-13 |
| bands (⌘⇧G): group cards, move together, collapse, send to all, link a lead to all | done 2026-11-13: `BandNode.tsx`, layout `groups` |
| review comments on the diff → one message to the agent | done 2026-11-13: `DiffView.tsx` |
| cost dashboard (⌘⇧U): per day / agent / playbook run, `usage_log` | done 2026-11-13 |
| session switcher (top bar) + per-session layouts | done 2026-11-13: `sessions`, `switch_session`, `desktop-session` file |
| background watcher under launchd + one tender per session (`tend.lock`) | done 2026-11-13: `daemon.rs`, `hubcodes daemon` |
| sharing phase 3: contributor role with approval queue, audit log, presence cursors | done 2026-11-13: `pending`, `/api/audit`, `/api/presence` |
| updater (tauri-plugin-updater, signed artifacts, `latest.json`) + notarize/staple in the release script | done 2026-11-13: `desktop/scripts/release-github.sh` publishes the GitHub release and rewrites the feed to the API asset URL, so updates work on the private repo with a `gh` token; the app checks at launch and every 6 h and shows an update bar (`UpdateBar.tsx`), nothing installs without a click; verified 0.1.1 → 0.1.2 on two Macs. Notarization still needs a Developer ID certificate |
| Board UI (kanban, run strip, filter, assign) | done 2026-11-13 |
| install without building: `install.sh` (curl / `gh api`), Homebrew cask `Casks/hubcodes.rb`, CLI linked to `~/.local/bin` | done 2026-11-13; the cask tap waits for the repo to go public |
| opening screen, session switcher with per-session layout, `hubcodes spawn` creates a missing session | done 2026-11-13: `Boot.tsx`, `TopBar.tsx`, `Tmux::ensure` |
| links drawn by hubcodes (`canvas/Links.tsx`), not React Flow edges: survive resize, drag and re-mount; facing sides from the gap between cards | done 2026-11-13 |
| start brief per role at every spawn (orchestrator delegates with `hubcodes send` / `task add`, workers report `done:`), team roster in the brief | done 2026-11-13: `prompts::start_brief` |
| hubcodes role synced into every provider: claude `--agents/--agent hub-<role>`, opencode agent file `~/.config/opencode/agent/hub-<role>.md`, codex `developer_instructions`, pi/omp `--append-system-prompt` | done 2026-11-13: `Provider::role_part`; verified end to end human → lead → dev → reviewer (opencode) → lead |
| Claude idle notification maps to `idle`, not `working` (messages were queued for up to 15 min) | done 2026-11-13: `hooks.rs` |
| orchestrator delegation enforced: reminder on every human message, `hubcodes guard` refusals in the Claude / opencode / pi / omp hooks, brief version stamp + *Restart with the current brief* | done 2026-11-14: `guard.rs`, `prompts::delegation_reminder`, `sessiondef::restart`; verified live on claude, opencode, omp |

## Phase 1 — Daily driver (TUI, target: 2 weeks)

Make it usable for one person on one machine all day.

- ~~Persist node positions and camera per session (`~/.local/state/hubcodes/<session>.json`).~~ Done 2026-11-11: `src/layout.rs`, keyed by agent name, saved every 1 s when changed and on quit; verified with two shell agents across a canvas restart.
- ~~Edge-of-screen indicators and a terminal bell when an off-screen agent enters *needs you*.~~ Done in the desktop app 2026-11-13 (pills, ⌘⇧], chime) and in the TUI 2026-11-14 (edge markers, bell, `Ctrl+Y`).
- ~~Type-first instructing~~: done 2026-11-11; multi-line composer (`Alt+Enter` / `Ctrl+J` newline, grows to six rows) done 2026-11-14.
- ~~`hubcodes attach <agent>` to open a split pane with the agent next to the canvas instead of switching windows.~~ Done 2026-11-11 as *open / tile / back*: `Enter` opens, `Ctrl+T` tiles all, `Esc` / `Alt+Esc` / `hubcodes back` returns; agents are tmux panes now, the canvas becomes a sidebar while anything is open.
- ~~Worktree per agent: `hubcodes spawn --worktree feat/x` creates `git worktree add` and spawns inside it.~~ Done (`git.rs`, `--worktree`).
- ~~Obstacle-aware routing for compact zoom.~~ Done 2026-11-14: `routing::route_avoiding` (A* detour when the two-bend route crosses a node, both zooms).
- ~~Release build via `cargo install --path .`; Homebrew formula later.~~ Done 2026-11-13: GitHub releases, `install.sh`, cask `Casks/hubcodes.rb` (tap once the repo is public). The TUI ships inside the .app as the CLI sidecar.

Acceptance: a four-agent session survives a canvas restart with the same layout; no edge crosses a node in the default layout at either zoom level.

## Phase 2 — Daemon, hooks, roles (target: +4 weeks)

- ~~`hubcodesd` (unix socket); the canvas becomes a client.~~ Done 2026-11-14: `hubcodes daemon run` is one process per machine on `~/.local/state/hubcodes/hubcodesd.sock` (newline JSON, `ping / sessions / agents / send / hook / events / tend / subscribe`), tends every session, takes hook reports and pushes events; the app follows its events, the CLI and hooks use it when it answers. State stays in tmux + SQLite by design (see [architecture.md](architecture.md#8-the-daemon-hubcodesd-built-2026-11-14)).
- ~~Hook-based status for Claude Code and Codex; heuristic fallback for the rest.~~ Done (`hooks.rs`, `hubcodes hook`); opencode (plugin), pi and omp (extensions) report the same way since 2026-11-14 through managed shim files and `hubcodes hook generic`; gemini and shell keep the heuristic.
- ~~Message log with ids and delivery states; paste only when the recipient is at a prompt, queue otherwise.~~ Done 2026-11-13 (`outbox`, `relay::flush_outbox`, `send --now`).
- ~~Roles: a role is a Markdown file injected at spawn (`--role reviewer` → `roles/reviewer.md` appended to the agent's instructions); shown in the node title.~~ Done: the brief is generated per role (`prompts.rs`) and handed to each provider in its own form (2026-11-13); `<repo>/.hubcodes/roles/<role>.md` or `~/.config/hubcodes/roles/<role>.md` extends or replaces it (`hubcodes role init|show|list`, 2026-11-14).
- ~~Notes: sticky-note nodes agents write to with `hubcodes note "…"`; rendered on the canvas and linked to the agent.~~ Done (`notes.rs`, shared notes card).
- ~~MCP server exposing `send`, `read`, `note`, `ask_human`.~~ Done 2026-11-13 (`hubcodes mcp`, 13 tools).
- ~~Bands: group nodes into a collapsible box with a title; a *maestro* agent can be linked to the whole band.~~ Done in the desktop app 2026-11-13.

Acceptance: an agent's permission prompt shows as *needs you* within 1 s via hooks; a message sent to a busy agent is delivered after it returns to its prompt, with the delivery state visible in the activity panel.

## Phase 3 — Sharing over Tailscale (target: +4 weeks)

- ~~Topology A (Tailscale SSH, read-only guest canvas) as the first cut.~~ Superseded by B: viewers get the read-only canvas from `hubcodes serve` on the tailnet.
- Topology B: daemon on the tailnet IP, identity via `tailscale whois`, roles viewer/contributor/operator/owner, approval queue, audit log: done 2026-11-13; preview redaction and private agents done 2026-11-14 (`redact.rs`, `hubcodes private`).
- ~~Presence: guest selections and cursors on the owner's canvas.~~ Done 2026-11-13 (`/api/presence`).

Acceptance: see the permission table and threat model in [sharing-and-security.md](sharing-and-security.md); each row has a test. A viewer cannot cause any tmux write; a contributor's message reaches an agent only after owner approval; every write is attributed in the log.

## Open as of 2026-11-15

Closed in 0.1.5 (2026-11-14): preview redaction + private agents, `hubcodesd` on a unix socket, hooks for opencode / pi / omp, role files, the TUI items (multi-line composer, edge markers + bell + `Ctrl+Y`, obstacle-aware routing, mouse exercised).

Still open, in priority order:

1. Notarization (Developer ID certificate); until then the installer and cask clear quarantine.
2. Federation (phase 4). Done so far (0.1.7): agents on other devices are in the orchestrator's brief and the delegation guard; New agent / teams can start an agent on a device (*Runs on*, `hubcodes spawn --on`); one board across devices (To do tasks go to idle agents on other devices when none fits here, `hubcodes task done mac:3` forwarded back). Still open: peering daemons directly (today every hop is SSH from the device that acts), shared rooms, the HUB policy answer.
3. ~~Smaller: hooks for gemini; TUI multi-select / box select; omp named profiles; the phone page serves one session.~~ Done in 0.1.7: gemini hooks merged into `~/.gemini/settings.json` (guard + policy deny on `BeforeTool`), TUI marks (`Space`, `Shift+click`, `Shift+drag`) with group send, omp `--profile` / `OMP_PROFILE` shim dir, phone page session switcher (`?session=`, `/api/sessions`).

Accounts per agent (0.1.9): one login per panel for claude, codex, gemini, opencode, pi and omp through each tool's own isolation variable (`accounts.rs`); Settings → Providers → Accounts, New agent → Account, `hubcodes account …`, `spawn --account`, templates, restore, remote spawn, per-account status and shim/MCP files.

MCP parity (0.1.8): 26 tools (task show/review/edit/move/assign/columns, peers, loop_status, fanout, role_show, spawn, policy_test added), `agents` with other devices, resources (brief, notes, board, agents), prompts (the saved ones), registration for opencode / pi / omp / gemini next to the shims, stdio round-trip in `scripts/test-cli-local.sh`.

CLI parity (0.1.8): `--json` on every reader (`peers`, `loop status`, `usage`, `log`, `daemon status`, `task columns`, `team|prompt|kb|tracker|role list`, `merge --list`, `policy show`, `note show`, `doctor`), `task show|edit|place`, `peer add|rm|check|queue` with pairing and version checks (the far side's version, self name and peer list travel in `ls --json`; `doctor` and the Devices panel show the same), `board columns|add|rm|rename|wip`, `team start --only`, `log --agent|--task|--kind|--follow`, `completions <shell>`, `providers --on <device>`, exit code 1 when a batch (team start, restore, restart, peer check) had errors, SIGPIPE handled, a help-text pass. Per-device permissions: `[[peer]] allow` (send, task, spawn by default; approve, kill, restart only when listed), enforced where the SSH caller can be told apart. `scripts/test-cli-local.sh` covers it.

Also done the same day, after the local peer test: remote agents report real quiet time (`hubcodes ls --json`, pane-kept screen hash) and cross-device dispatch needs idle ≥ 5 s like local; a `hubcodes send device/agent` that cannot reach the device waits in the remote queue; SSH ControlMaster plus an adaptive device poll (3 s busy, 15 s quiet, 60 s away); Settings → Devices shows the queue, last poll, *Poll and retry now*; model pickers read another device's catalog (`hubcodes providers --json` over SSH); `--as` on forwarded reports is checked against `tailscale whois`; board: task detail, order within a column, WIP limits, owner/role/device filters.

Desktop look (0.1.10–0.1.11): floating panels on a themed backdrop, pill header, agents rail grouped by status or device, ten themes (Graphite plus nine Neovim colorschemes: tokyonight, kanagawa, kanagawa-dragon, rose-pine, catppuccin, everforest, oxocarbon, eldritch, nordic) that repaint antd and the terminals; settings drawers became modals; loops may run without a round limit (`--max 0`); a removed agent is dropped from the saved session and *Start fresh* forgets it.

Round 6 (in progress, 2026-11-15): Board, Inbox, Cost and New agent restyled to the floating look; unused CSS removed; Playwright smoke tests against the mock (`desktop/e2e`, `pnpm test:e2e`); `SettingsDialog.tsx` and the Tauri `lib.rs` split by page / command group; `ls --json --preview` (what peers read) redacts secrets and sends nothing for a private agent. Federation, first slice: `hubcodesd` takes requests from its peers over the tailnet (`[sharing] listen = true`, per-peer `transport = "auto"|"http"`, `hubcodes daemon listen`), same `allow` checks as SSH, SSH fallback; covered by `scripts/test-peers-local.sh`. The desktop follows such a device with a long poll (`/fed/v1/watch`) instead of the timed poll. Decided and built the same day: tailnet identity is enough (no shared token), someone else's device is view only until its `allow` says more (its messages wait for approval, `hubcodes pending`), rooms live on their creator's device (`hubcodes room …`, MCP `room_post`). The desktop has a Rooms view (rooms on this Mac: members, transcript, posting as you) and a quieter Inbox that tells a parked message's origin (another device or the shared page) and puts `y` / `n` on the buttons they press.

## Phase 4 — Federation (later)

Daemons peer over the tailnet; each person's agents run on their own machine with their own credentials; rooms span machines. Depends on the HUB policy answer in the sharing doc's open questions.

## Out of scope (for now)

- Portals (embedded browser / device simulators), computer use, on-device LLM assistant — Maestri features that need a GUI.
- Git workflow automation (one-button PR) — T3 Code's core; we rely on the agents' own git skills and on worktrees.
- Windows native (tmux is required; WSL works).
