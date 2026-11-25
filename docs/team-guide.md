# hubcodes for a team

What a team gets from hubcodes once the agents are running, and how each part is meant to be used. Everything here is built and runs through tmux plus a small SQLite transcript; nothing needs a server unless you want phones or teammates in (section 9).

## 1. The CLI is inside the app

Every command that reads state takes `--json` (`ls`, `task list|show|columns`, `peers`, `loop status`, `usage`, `log`, `daemon status`, lists); the writers end with exit code 1 when part of the work failed, so scripts and agents can tell. `hubcodes completions zsh > ~/.zfunc/_hubcodes` (or bash, fish) gives tab completion. `hubcodes.app` ships the `hubcodes` CLI as a sidecar. On start it links `~/.local/bin/hubcodes` to it and puts the bundled binary first on `PATH` for every agent it starts, so `hubcodes send`, `hubcodes task done` and the rest work on a machine that never ran `cargo`. Settings → Session shows where it is and whether your login shell sees it.

## 1b. Accounts: one login per panel

Three Claude agents on three subscriptions must not share one login. Every agent can run under an **account**: a folder of its own under `~/.local/state/hubcodes/accounts/<provider>/<name>/` that holds that login's credentials and settings, handed to the tool through the variable it already honours: `CLAUDE_CONFIG_DIR` (claude), `CODEX_HOME` (codex), `GEMINI_CLI_HOME` (gemini), `XDG_DATA_HOME` (opencode's auth), `PI_CODING_AGENT_DIR` (pi), `OMP_PROFILE` (omp's own profiles). `default` is the tool's own home and changes nothing.

Settings → Providers → **Accounts** lists them per provider with whether each is signed in; *Add* creates one, *Sign in* opens the tool under that account in its own window so you log in there once (`/login` and the like), then close it. New agent → **Account** picks the login for that agent (the Connect panel shows that account's state), team templates carry one per agent, `hubcodes spawn <name> -p claude --account work` and `hubcodes account list|add|rm|login <provider> [name]` do the same from a shell. The card shows the account next to the provider; `hubcodes ls --json`, session restore and spawning on another device carry it along. Hooks, shims and the MCP registration are written into the account's folders, so status and tools work the same under every login.

## 2. Status comes from the agents

Claude Code is started with `--settings <file>` that hooks `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Notification`, `Stop` and `SessionEnd` to `hubcodes hook claude`; Codex is started with `-c notify=[…, "hook", "codex"]`. The hook writes `@hubcodes_status`, `@hubcodes_status_at` and `@hubcodes_detail` on the agent's pane, and on `Stop` adds the turn's token usage from the transcript to the `usage` table. A report older than 10 minutes loses to the screen heuristic, so a crashed hook never freezes a card. The detail (tool being run, the permission question) is what the card footer, the inbox and the phone page show.

opencode, pi and omp report the same way (since 0.1.5). When hubcodes starts one of them it writes a small managed file into the tool's own config folder (`~/.config/opencode/plugins/hubcodes.js`, `~/.pi/agent/extensions/hubcodes.ts`, `~/.omp/agent/extensions/hubcodes.ts`), which the tool loads as a plugin or extension. The file is rewritten at every start (its first line says *managed by hubcodes*), so there is nothing to install or keep in sync, and it does nothing when you run the tool outside hubcodes. Inside hubcodes it calls `hubcodes hook <provider>` on the tool's own events: *working* with the tool name while it runs a tool, *needs you* with the question when it waits for a permission or an answer, *idle* the moment the turn ends. Messages to a busy agent are queued and delivered when it is really idle, and loop and task bookkeeping key off the real end of the turn. An omp started with `--profile <name>` (or `OMP_PROFILE`) gets the shim under `~/.omp/profiles/<name>/agent/extensions`; pi honours `PI_CODING_AGENT_DIR`. Gemini CLI (0.21+) reports through its own hooks: at start hubcodes merges one managed group per event (`SessionStart`, `BeforeAgent`, `BeforeTool`, `AfterTool`, `Notification`, `AfterAgent`, named `hubcodes`) into `~/.gemini/settings.json` next to whatever is there, idempotently; the command is inert outside hubcodes, and `BeforeTool` carries the orchestrator guard and the policy's deny rules (`{"decision":"deny"}`). Custom providers opt in with `hook_kind = "opencode" | "pi" | "omp" | "gemini"`; any other tool can report with `hubcodes hook generic` and `{"status":"working|waiting|idle","detail":"…","turn_done":true}` on stdin. When `hubcodesd` runs, every report goes through its socket (section 39).

## 3. One branch per agent

New agent → **Own branch** (or `hubcodes spawn api --worktree agent/api`) creates `<repo>/.hubcodes/worktrees/<agent>` with `git worktree add`, on that branch, and starts the agent there. The folder is excluded through `.git/info/exclude`. Cards show `⎇ branch` and `+ins −del`; Open → **Changes** shows the files and the diff, **Commit all** commits, **Open PR** pushes and runs `gh pr create --web`. Removing the agent removes the checkout; the branch stays.

## 4. Teams

`[[template]]` in `hubcodes.toml`, or Settings → Teams. New agent → **Team** starts every agent that is not already running, then the links and loops. ⌘K → *Save the running canvas as a team* writes what is on the canvas back as a template. Built-ins: `pair` (builder ↔ reviewer loop) and `full-stack` (lead, web, api, reviewer). CLI: `hubcodes team list|start|save`.

## 4b. Teammates check with their orchestrator

You talk to a local agent in its own terminal, on its card or in *Open*: there is no separate chat box, so pasting an image (⌃V) or dropping a file works the way the agent's own CLI does. An agent on another device has no live terminal here, so its card keeps a box to type into.

The **message dock** (⌘L, at the bottom of the canvas) is the one place to write as yourself to any agent, on this Mac or another device, without a loop. *To* follows the selected card (or the orchestrator) until you pick agents by hand; several at once get the same message. Paste a screenshot, drop files from Finder onto the dock or on empty canvas, or use the paper clip. Files are written to `~/.local/state/hubcodes/attachments/` and the agent gets their paths after the text and opens them with its own tools (Claude Code, Codex and opencode read images that way). For an agent on another device the files are copied there first over the same SSH connection (`scp`), so the paths are on its disk. ↩ sends, ⇧↩ adds a line, ⌥↩ skips the check-in with the lead, `/name` sends a saved prompt. From the CLI: `hubcodes send <agent> "text" --file shot.png` (repeat `--file`; works for `device/agent` too). Limit 100 MB a file; the shared phone page sends text only.

When an agent is connected to an orchestrator (a role such as *orchestrator*, *lead*, *planner*) by a line on the canvas, directly or through other agents, what you type still goes to that agent, and the agent checks with the orchestrator before acting: it tells the lead what was asked and what it plans (`hubcodes send <lead> …`), waits for the go-ahead, and hands over anything outside its role. A plain question about its own work it answers straight away and tells the lead in one line. So the two talk it over instead of the agent acting on its own, and a reviewer is not quietly turned into an engineer. *Open* says so in its footer.

- It is enforced, not only asked. A message without a `[from <name>]` tag (or `[from human:…]`) is yours; when one reaches a Claude Code teammate, its prompt hook marks it with its lead, and until the lead's next `[from <lead>]` message hubcodes refuses its file edits, builds, tests and git writes with a reason that says to ask the lead. Reads, searches and `hubcodes send` stay allowed. Removing the line to the lead lifts the mark. Other providers get the rule through their brief. An agent started before this version shows up in the "restart with the current brief" banner.
- `hubcodes send <agent> …` from a person, the TUI composer, the shared phone page and the box on a remote agent's card add the same note to the message itself; `--direct` (⌥Enter in the box) leaves it out.
- An agent with no line to an orchestrator is on its own: it acts on what you type, and its brief names no lead to report to. Draw a line to bring it into the team. Any orchestrator counts, whatever it is named (`lead`, `koordinator`…); the role decides, not the name.
- Switching an agent to another provider or model keeps its lines, so it stays in the team.
- Messages between agents, and anything sent to the orchestrator itself or to a plain shell, carry no note.
- Turn the note off in Settings → Agents → *Check with the orchestrator* (`[workspace] route_through_orchestrator = false`).

## 5. Goal board (⌘B)

Tasks have a title, detail, role, owner, status (`todo → doing → review → done`) and a result. Anyone adds them: you on the board, an orchestrator with `hubcodes task add "title" --role backend --detail "…"`. Every 5 s hubcodes hands each unowned `todo` task to an idle agent, same role first, then any idle agent, and pastes:

```
[hubcodes task #3] Login form uses the new store
When it is done, run:
  hubcodes task done 3 "<what you did and how to verify>"
If you are blocked, run:
  hubcodes task review 3 "<what you need>"
```

`task done` and `task review` also message the task's creator. Drag a card between columns to change its status by hand; *Assign to…* hands it to a specific agent. `hubcodes watch` does the same dispatching from a terminal when the app is closed.

The board opens as a modal (⌘B) and its columns are yours to shape (the gear next to the filter, or `[board] columns` in `hubcodes.toml`). To do, Doing, Review and Done are built in: they can be renamed and reordered, not removed, because the agents move tasks between them. A column you add (`Backlog`, `Blocked`, `QA`…) is a waiting place: a task there counts as open but idle agents only pick from To do, and it stays until someone drags it or runs `hubcodes task move <id> <column>` (`hubcodes task columns` lists them). Removing a column sends its tasks to To do.

Within a column cards keep an order: drop one onto another to put it before it (idle agents take the top To do task first), or drop on the column to append. A column can carry a WIP limit (gear → the number next to each column): the header shows `n/limit` and turns amber when full, and while Doing is full dispatch hands out nothing more. Click a card's title for the task in full: edit title, role and detail, move it to a column, see the result and what happened to it (messages and events naming `#id`), remove it. The toolbar narrows the board by owner, role and device.

## 6. Fan-out / fan-in

⌘K → *Fan out a prompt*, right-click → *Fan out a prompt from here…*, or `hubcodes fanout "prompt" --to a,b,c --collect judge`. Every target gets the prompt plus `[fanout #id] reply with: hubcodes send judge "[fanout #id] …"`. Replies carrying the tag are counted; when the last one lands the collector gets all answers in one message and is asked to pick or combine. Without a collector the answers come back to you.

## 7. Saved prompts

Type `/` in a composer: `/review`, `/test`, `/handoff`, `/status`, `/plan` are built in; Settings → Prompts adds yours. Variables are filled for the agent the prompt goes to: `{agent} {role} {model} {cwd} {branch} {changes} {last_diff} {goal} {notes} {date}`. Words after the name are appended (`/review focus on error handling`). The composer previews the expanded text before you send.

## 8. Shared notes (⌘⇧N)

One markdown file per session (`~/.local/state/hubcodes/<session>-notes.md`). The card on the canvas edits it in place; agents read it with `hubcodes note show` and append with `hubcodes note add "…"`; `{notes}` puts it into a prompt. Use it for decisions, paths and conventions instead of repeating them in messages.

## 9. Phones and teammates

`hubcodes serve` (or Settings → Remote access → Start server, or `hubcodes watch --serve`) serves one page on this Mac's Tailscale address, port 7433: who needs you, Approve / Deny, send a message, notes, activity. Identity is the tailnet login (`tailscale whois`); `[[share]]` decides the role:

| role | may |
|---|---|
| viewer | see agents, status, previews, activity, notes |
| operator | also send messages, approve / deny, add notes |
| owner | everything (you, from this Mac, always) |

A `token` lets a client outside the tailnet in as an operator (`?token=…`). `POST /webhook/<name>` fires webhook triggers.

## 10. Approval inbox (⌘I)

Every agent in *needs you*, on every device, with the question it asked. **Approve** presses the provider's yes keys in that terminal (Enter for Claude Code and OpenCode, `y` for Codex; Settings → Providers → Keys to change), **Deny** the no keys, **Open** for anything that is not yes or no. `y` / `n` answer the first item. CLI: `hubcodes approve <agent>`, `hubcodes deny <agent>`, also for `device/agent`.

Below them, **Pull requests**: one row per pull request a review or feedback tracker is handling, by its newest task. *Needs you* (the agent ended with `NEEDS HUMAN:`), *Paused* (two rounds without a commit) and *Ready to approve* come first and count toward the inbox total; then *In progress*, *Waiting* and *Done*. **Approve on GitHub** appears only on a finished review whose last line is `VERDICT: approve`: the agent recommends, you press, and `gh pr review --approve --body "LGTM"` runs as this Mac's `gh` account (with the tracker's `auto_approve` on, hubcodes presses it for you). **Run again** queues the last task once more, **One more round** lets a paused pull request have another round without a commit, **Skip** removes a queued one, **Ignore PR** stops every review and feedback round for that pull request for good (not while an agent is on it), and the arrow opens it on GitHub. A pull request leaves the list once it is merged or closed. The phone page (`hubcodes serve`) shows the ones that need you with the same buttons; approving there needs the `owner` role, ignoring `operator`.

## 11. Transcript

Every message, nudge, task hand-off and fan-out is a row in `~/.local/state/hubcodes/<session>.db`; status, approvals, spawns, budget hits are events. The Activity panel searches it; `hubcodes log --grep text -n 100` prints it; `hubcodes log --json` for scripts.

## 12. Budgets

`[budget] max_tokens_per_agent` and `max_tokens_per_loop` (Settings → General). Usage is what the agents' own transcripts report (Claude Code via hooks; other providers show nothing until they expose it). Over the limit: the agent's loops pause, you get a notification, an event is logged. `hubcodes usage` prints the table.

## 13. Triggers

`[[trigger]]` (Settings → Triggers): `on = "cron 0 9 * * 1-5"`, `"pr owner/repo"` (new pull requests, polled with `gh` once a minute), or `"webhook name"`. Actions: `send agent "text"`, `loop start a b "goal"`, `team start name`, `task add "title"`; PR triggers can use `{number} {title} {url}`, webhooks `{body}`. They fire while the app or `hubcodes watch` runs.


Settings → Triggers builds the lines for you: *When* is a schedule (every weekday, every day, once a week at a time, every hour, every few minutes, or custom cron), a new pull request in a repository picked from GitHub, or a webhook with its address to copy; *Then* tells an agent, adds a task, starts a loop or starts a team, with agents and teams picked from lists. A sentence under the form says what the trigger will do. The stored `on` and `action` lines are under *Advanced* for anything the pickers do not cover (`--max`, a team's folder).
## 14. Loops and devices

See [loops-and-devices.md](loops-and-devices.md). A loop between two agents keeps them answering each other until `DONE` or the round limit; agents on another machine join over SSH as `device/agent`.

A device that does not answer (asleep, off the network, Tailscale off on either side) is asked again every 60 s, since an SSH timeout is slow. Meanwhile the app reads `tailscale status` every 3 s: the moment Tailscale is back on this Mac or the device shows as online, it polls at once, its agents return to the canvas and the reports queued for it go out. Doctor runs its checks again every time it is opened, so a device that was away does not stay red there.

## 15. Team memory (Settings → Memory)

One interface, four providers, chosen in `[memory]`; **Test connection** checks the one you picked before you save.

| provider | what it is |
|---|---|
| `builtin` | SQLite full-text table in the session database; nothing to install |
| `hindsight` | Hindsight REST (`url`, `api_key`, `bank`): `POST /v1/default/banks/{bank}/memories` and `…/memories/recall` |
| `mcp` | any MCP server (HTTP or stdio) with a remember/search tool pair; tool names and argument JSON are configurable, defaults fit Hindsight's MCP (`retain` / `recall`) |
| `command` | a shell command that reads JSON on stdin |

Agents: `hubcodes memory recall "…"`, `hubcodes memory retain "…" --tags ops`. Automatic (each can be switched off): at start, a recall for the agent's role, repo and name goes into its system prompt under *What the team remembers*; on `task done` and loop `DONE` a one-line result is retained. `${NAME}` in any field reads an environment variable, so API keys need not sit in `hubcodes.toml`.

## 16. Knowledge sources (Settings → Knowledge, ⌘⇧K)

As many `[[kb]]` as the team has; each has **Test**. Jira and Confluence sources pick an account and a space or project from lists (or take a pasted link); an MCP source needs only the server URL and its token: *Find the tools* picks the search and read tools and fills their arguments from the server's schema. Settings → Memory has the same button for an MCP memory server. `hubcodes kb search "…"` searches all enabled sources at once, `hubcodes kb get <source> <reference>` reads one item, and every search is logged in the transcript so it is clear what an agent read.

| kind | config | how it searches |
|---|---|---|
| `mcp` | `url` or `command`, `headers`, `search_tool` + `search_args`, optional `get_tool` + `get_args` | calls the server's tools; `{query}`, `{limit}`, `{id}` are filled |
| `git` | `repo` | clones under `~/.local/state/hubcodes/kb/`, pulls hourly, searches `.md/.txt/.rst/.adoc` by term frequency |
| `folder` | `path` | same, on a local folder |
| `confluence` | `url`, `email`, `token`, optional `space` | CQL `text ~ "…"`, page body stripped to text |
| `jira` | `url`, `email`, `token`, optional `project` | JQL `text ~ "…"`, issue summary / status / description |
| `command` | `run` | JSON on stdin (`{op: "search", query}` / `{op: "get", id}`), JSON or text out |

⌘⇧K opens the search dialog: results from every source plus team memory, open one in place, or *Send to…* an agent as a pointer it can follow with `hubcodes kb get`.

## 17. Repository teams: `.hubcodes/team.toml`

A repository can carry its own hubcodes settings. `hubcodes team init` (or New agent → Team → *Save the running canvas as this repo's team*) writes the running agents and links there, with the detected test command. On any machine, `git clone` then `hubcodes team start <name>` (or New agent → Team with the folder set) gives the right team.

```toml
[[template]]
name = "hermina"
agents = [{ name = "lead", provider = "claude", role = "orchestrator" }, { name = "api", provider = "claude", role = "software engineer", worktree = "agent/api", sandbox = true }]
links = [["lead", "api", "loop"]]

[test]
command = "pnpm test"          # else detected: cargo test / pnpm|npm|yarn test / pytest / go test / make test

[merge]
into = "main"
test = true                    # run the tests before advancing main
push = false                   # push main after a merge

[board]
review_required = true         # an agent's `task done` lands in Review until approved

[[kb]]                         # docs that live in this repo (relative path)
name = "docs"
kind = "folder"
path = "docs"
```

## 18. Tests per agent

`hubcodes test <agent>` (or Open → Changes → **Run tests**) runs the repo's test command in the agent's folder, caps it at 15 minutes, and records the verdict: the card shows `✓ tests` / `✗ tests`, the transcript gets a line, `@hubcodes_test` on the pane keeps the summary. Agents can run `hubcodes test` on themselves and are expected to before `task done`.

## 19. Merge queue

Open → Changes → **Merge** (commit first), or `hubcodes merge <agent|branch>`. One merge at a time, in a separate checkout (`.hubcodes/worktrees/_merge`), so nobody's working tree moves:

1. `git merge --no-ff <branch>` onto the target; a conflict aborts and becomes a task for the branch's owner (*Resolve conflicts merging … into main*, with the conflicting files).
2. The tests run on the merge result; a failure resets and becomes a task (*Fix tests on … before merging*).
3. The target branch ref advances (and the main checkout follows when it has that branch out); `[merge] push = true` pushes.

`hubcodes merge --list` shows the queue; the app and `hubcodes watch` process it; `hubcodes merge --run` processes it now.

## 20. Review gate

With `[board] review_required = true`, an agent's `hubcodes task done` lands in **Review**. Approve on the board (or `hubcodes task approve <id>` by a human, or by a reviewer / lead agent that does not own the task); **Request changes** sends the note to the owner and moves the task back to Doing.

## 21. Restore after a restart

Every 15 s the running session (agents with provider, role, model, folder, worktree; links; loops) is saved to `~/.local/state/hubcodes/<session>-def.json`. When saved agents are not running the app shows a banner: **Restore and continue** starts them with the provider's resume flag (`claude -c`, `codex resume --last`, `opencode -c`, set per provider as `resume_flag`), **Start fresh** without it. If there is nothing to continue (the folder is new, the history is gone) the tool exits at once and hubcodes starts that agent fresh instead. CLI: `hubcodes restore [--fresh]`.

An agent whose process ended shows as **offline** (its pane is back at a shell prompt with `[hubcodes] agent exited`); messages, nudges and approvals to it are refused rather than typed into the shell, and Restore replaces it.

## 22. Alerts

Beyond *needs you*: an agent idle for 10 minutes while holding a task; a loop that went 6 rounds without any file changing; the same error line on screen three polls in a row; an agent that exited. Each fires once per episode as a notification and an event (`[alerts]` in Settings → Agents to tune or turn off). `webhook` also posts every alert, including a pull request that needs you, off the machine: a Slack or Discord incoming webhook gets JSON, any other URL (an ntfy topic such as `https://ntfy.sh/<topic>` for a phone) the text as a plain body. The URL is a secret: pasted in Settings it goes to the Keychain (`${keychain:alerts:webhook}`).

## 23. Sandbox

New agent → **Sandbox** (or `--sandbox`, or `sandbox = true` in a team): on macOS the agent runs under `sandbox-exec` with a generated profile: reads and network unrestricted, writes only inside its folder, the repo's `.git` and `.hubcodes`, temp, and the provider's own config and cache folders. `[sandbox] kind = "command"` with a wrapper (`docker run … {cwd} … {cmd}`) uses a container instead. Pair it with the provider's auto-approve flag to cut permission prompts without giving an agent the whole disk.

## 24. Presence

Whoever types into an agent (desktop or the web page) holds it for 45 s; cards and the web page show `⌨ name`, and another person's send is refused while it is held. Approvals, sends, tests, merges and alerts are events in the transcript (`/api/events`).

## 25. The canvas in a browser

`hubcodes serve` serves the same React canvas at `http://<tailscale-ip>:7433/canvas/` (the desktop app ships the built UI; from a terminal pass `--web desktop/dist`). Teammates get cards with previews, links, loops, the board, the inbox and the activity, gated by their tailnet role; what needs the agent's machine (live terminal, starting agents, settings) says so.

## 26. Provider files

`hubcodes provider export claude > claude.toml` writes everything about a provider (command, model and role flags, hook kind, resume flag, approve keys, model catalog command) as one file; `hubcodes provider import <file|url>` installs it under `~/.config/hubcodes/providers/`, where every file loads automatically. Settings → Providers has Export / Import. A custom provider can declare `hook_kind = "claude"` to keep hook status when it wraps Claude Code.

## 27. Headless

`hubcodes watch --serve` on a Mac mini does everything the app's background thread does (dispatch, reminders, budgets, triggers) plus the web page, with no window.

## 28. Auto-approve policy

`[policy]` in `hubcodes.toml` (Settings → Policy) decides which agent actions hubcodes answers by itself, so *needs you* is left for the real decisions:

```toml
[policy]
mode = "auto"                          # off = every prompt waits for you
allow = ["git status*", "git diff*", "cargo test*", "pnpm test", "ls*", "cat *", "rg *"]
deny = ["rm -rf *", "git push --force*", "git reset --hard*", "sudo *", "* | sh"]
allow_tools = ["Read", "Glob", "Grep", "WebFetch"]   # Claude Code tools that never ask
deny_paths = ["~/.ssh/*", "*.env", "*.env.*"]        # writes here are refused
```

Rules are globs (`*`, `?`); a rule without a wildcard also matches as the first words (`git status` covers `git status -s`). Compound commands (`a && b`, `a; b`, `a | b`) are judged part by part: one denied part denies the whole, every part must be allowed for an allow. Deny wins over allow; anything unmatched still asks you. A repository adds its own rules in `.hubcodes/team.toml` `[policy]`.

Two places apply it. Claude Code is answered before it even asks: the `PreToolUse` hook (`hubcodes hook claude`) prints `permissionDecision: allow|deny`. Every other agent (Codex, opencode, …) that is *waiting* gets its screen read: the command in the prompt (`` `cargo clippy` ``, `run rm -rf build?`) is matched and the provider's yes or no keys are pressed. Every decision is logged as a `policy` event and shown as a toast. Settings → Policy has **Add the suggested rules** and a **Try a command** box; `hubcodes policy test "npm run deploy"` does the same from a terminal.

## 29. Playbooks

A goal walked through named steps, each a prompt to an agent (by name or by role) with a gate before the next one. Steps become board tasks, so dispatch, `task done`, review and alerts all work unchanged. `[[playbook]]` in `hubcodes.toml` or `.hubcodes/team.toml`, Settings → Playbooks; the built-in `feature` playbook is plan → build (tests) → review → merge.

```toml
[[playbook]]
name = "feature"
[[playbook.step]]
name = "plan"
role = "orchestrator"
prompt = "Write a plan for: {goal}. Put it in the shared notes, then run the task done command."
gate = "human"                 # pause until you approve
[[playbook.step]]
name = "build"
role = "software engineer"
prompt = "Implement the plan for: {goal}. {prev_agent} said: {prev_result}"
gate = "test"                  # the repo's tests must pass; failures go back to the agent
retries = 2
[[playbook.step]]
name = "review"
role = "reviewer"
prompt = "Review what {prev_agent} built; approve the task or send changes back."
gate = "review"                # the task sits in Review until approved
[[playbook.step]]
name = "merge"
action = "merge {prev_agent}"  # an action instead of a prompt: merge <agent> | send <agent> "…" | task add "…"
```

Gates: `none` goes on as soon as the task is done; `human` pauses the run (Board → **Approve**, or `hubcodes playbook approve <run>`); `test` runs the repository's tests in the agent's folder off the poll thread and, on failure, hands the output back to the same agent as a new task up to `retries` times; `review` requires the task to be approved. `merge` waits for the merge queue. A task the agent sends to Review (`task review`) on a non-review gate pauses the run for you. Runs live in SQLite (`playbook_runs`) and show at the top of the Board with their current step and last log line; ⌘K → *Run a playbook…*. CLI: `hubcodes playbook list|start <name> "goal"|runs|show|approve|abort`.

## 30. Trackers (Jira, Plane, GitHub, anything)

`[[tracker]]` in `hubcodes.toml` (Settings → Trackers) connects an issue tracker to the board. Every poll (`poll_minutes`), issues matching `query` become board tasks up to `limit`; an idle agent with `role` picks each up and is told `instruction` with `{key} {title} {url} {description}` filled in. `/jira-task {key}` runs the team's own skill with its gates: an instruction whose first line is `/<skill> args` and names a skill installed here is spelled out in the assignment ("Use the `jira-task` skill on PE-1 …: load it with your skill tool"), because a slash command only counts as the very first thing typed and an assignment starts with the task line; every provider follows the words. A one-line `/<skill>` without `{url}` gets the URL as its argument on the same line. The skill has to be in the tool of the agent that picks the task up: see Settings → Skills (§44). When the agent picks a task up the issue moves to `on_start`; when the task is done (or approved, where the repo requires a review) it moves to `on_done` and gets the result as a comment. Tasks carry `external = "<tracker>:<key>"`, so nothing is imported twice; the card shows the key as a link.

| kind | fields | notes |
| --- | --- | --- |
| `jira` | `url`, `email`, `token`, `query` (JQL) | transitions by name; comments in ADF |
| `plane` | `url` (`https://api.plane.so`), `api_key`, `workspace`, `project`, `query` (state names) | `on_start`/`on_done` are state names |
| `github` | `repo`, `query` (`gh issue list` flags) | `on_start`/`on_done` are labels, or `closed`; needs `gh` |
| `github-feedback` | `repo` (as above), `since` (date), `ignore` (logins) | feedback from other people on your own open pull requests; one round per pull request at a time |
| `github-review` | `repo` (empty: every repository; `owner/*`: one owner), `query` (`gh search prs` flags), `rereview` (default on) | open, non-draft pull requests that ask the `gh` user for a review; key `owner/repo#N`; no status to move |
| `command` | `run` | JSON on stdin: `{"op":"list"}` → array of `{key,title,url,description}`; `{"op":"transition","key","to"}`; `{"op":"comment","key","text"}` |
| `mcp` | `url` or `command`, `list_tool`/`transition_tool`/`comment_tool` + args | `{query} {key} {to} {text}` are replaced |

Adding one is mostly picking: choose the account (Settings → Accounts), paste a board, backlog or issue link to fill the site and project, then pick the statuses to import and the statuses to move to; the JQL (or the `gh issue list` flags) is written from the picks, and the raw fields stay under *Advanced*. For an MCP tracker, *Find the tools* asks the server and picks the list, transition and comment tools with their arguments. CLI: `hubcodes tracker list|test [name]|pull [name]`.

### Review requests (`kind = "github-review"`)

When someone asks you for a review on GitHub, the pull request becomes a Todo task for `role` (default `reviewer`). Each idle agent with that role takes one, so three reviewer agents review three pull requests in parallel; the rest wait in Todo, oldest first, up to `limit` waiting or in review. The form shows how many agents have the role. A queued request that is withdrawn (someone else reviewed, the pull request was closed or merged, you were removed) leaves the queue. Your review takes the pull request off GitHub's list; if the author asks again, it comes back as a new task. Each review gets its own checkout: the agent runs `hubcodes review checkout owner/repo#N`, which keeps a partial clone per repository under `~/.local/state/hubcodes/reviews/` (cloned with the `gh` sign-in, so private repositories work) and puts the pull request's head in its own worktree, `pr-<N>/`. Parallel reviewers never share a working tree, and nobody's own checkout is touched; a new push moves the same folder, and the folder is removed once the review is finished and the pull request has left your list (`hubcodes review clean owner/repo#N` by hand). Orchestrators on this Mac are told when requests come or go, with the queue (waiting, and who reviews what), for their information: reviewers do not wait for them, and the order can be changed with `hubcodes task place`. The default instruction checks the pull request out, reads it, runs the affected tests and posts one comment review through `gh api …/pulls/N/reviews`: every finding is marked 🔴 (blocks merging), 🟡 (should be fixed) or `needs_validation` (names the fact it could not check), a finding about a changed line is an inline comment on that line, and the body lists what ran, a count line (`2 🔴, 4 🟡, 1 needs_validation`) and the numbered findings. It never approves or requests changes; its result ends with the count line and `VERDICT: approve` (no 🔴, 🟡 or `needs_validation`) or `VERDICT: changes`, and the Pull requests card shows the counts, and an approve shows a one-click **Approve on GitHub** in the inbox for you to press. `instruction = "/pr-review {url}"` runs that skill instead; an instruction that does not ask for a verdict gets the verdict rule appended, so the button works with any skill. **Approve when nothing blocks** (`auto_approve = true`) skips the button: when the review finishes with `VERDICT: approve`, hubcodes runs `gh pr review --approve --body "LGTM"` itself, once, as this Mac's `gh` account; if that fails (your own pull request, no rights) the error shows and the button stays. The button posts the same LGTM comment. `comment_on_done` is off, because the agent posts its own review.

When the author pushes after a review this tracker ran, the pull request comes back as *Re-review owner/repo#N*, once the commits have been quiet for five minutes: the task gives the commit you reviewed, so the agent looks at `git diff <that commit>..HEAD` and says which earlier points are addressed and what is new. Reviews you made by hand before the tracker existed are left alone. `rereview = false` turns this off.

### Feedback on your pull requests (`kind = "github-feedback"`)

Your open pull requests are read for feedback from other people: reviews with a body (not approvals), inline comments and conversation comments. Anything newer than both the pull request's last commit and its last round becomes one task, *Feedback round N on owner/repo#N*, for `role` (default `pr-fixer`, kept apart from engineers working on tickets). Only one round of a pull request is open at a time; comments that arrive meanwhile start the next one. The task lists each new point with the command that answers it (`gh api …/comments/<id>/replies` for an inline thread, `gh pr comment` otherwise). The agent runs `hubcodes review checkout owner/repo#N --branch`, which puts the pull request's branch, as it is on GitHub, in its own folder (`fix-<N>/`, next to the review checkouts); local commits that were not pushed are kept, not reset. The agent decides each point: it fixes it or explains why not, and when two reviewers disagree it picks one, applies it and says in both threads which it followed and why. It runs the affected tests, pushes with `git push origin HEAD` (never another branch, never `--force`) and answers every point in its own thread. Only a point that truly needs a person (a product decision, missing access, a merge order) is left; the agent says so in that thread and starts its task result with `NEEDS HUMAN:`, and you get one notification (and the orchestrator a note) with the points. The reviewer's next comment starts the next round, with no limit on rounds, once the comments have been quiet for five minutes (a review that arrives as many comments is one round). Your own comments and replies (the `gh` sign-in), bots (`…[bot]`) and the logins in `ignore` (automated reviewers posting from a person's account) never start one. Two rounds in a row that end without a commit mean the reviewer and the agent are going in circles: the pull request is paused, you get a notification and the orchestrator a note, and the next commit on it (yours, say) resumes the rounds. Merged or closed pull requests leave the queue and their folder is removed. `since` (set to the day the tracker is added) keeps old threads on open pull requests from all starting at once. Pull requests are re-read only when GitHub's `updatedAt` changes, a few at a time, so a poll stays quick. Pull requests from a fork are not checked out for pushing.

GitHub itself starts rounds too. A check that failed on the head commit (once every check has finished) is a point from `@checks`, with the command that prints its log (`gh run view --job <id> --log-failed`); the agent fixes the cause and pushes, and no thread reply is needed. A pull request that no longer merges into its base is a point from `@conflicts`: the agent merges the base in (`git fetch origin <base> && git merge origin/<base>`), resolves, tests and pushes, never rebasing; a conflict it cannot resolve safely ends in `NEEDS HUMAN:`. A conflict comes back only after the base branch moves again. `ignore = "checks"` or `"conflicts"` turns either off. Checks and mergeability are read on every poll, in the same single search as the pull requests.

### Accounts and secrets (Settings → Accounts)

Connect Atlassian (Jira and Confluence), Plane and GitHub once; trackers, knowledge sources and triggers pick the account instead of asking for the site and token again. GitHub uses the `gh` sign-in on the Mac. *Check* signs in and says as whom.

An Atlassian account signs in one of two ways. **twg login** (the default for a new account) uses the Atlassian sign-in of the TWG CLI on this computer: run `twg login` once, nothing is stored in hubcodes, and the settings file says `token = "twg"`. Jira trackers (list, move, comment) and Confluence or Jira knowledge sources then run through `twg … -o json`. **API token** is the email plus an API token from your Atlassian profile, kept in the Keychain. If *Check* fails on twg, run `twg whoami` in a terminal.

No token is written to `hubcodes.toml`. Saving Settings moves every plain token, API key and credential header (accounts, trackers, knowledge sources, team memory) into the system's secret store (the Keychain on macOS; on Linux the login keyring, GNOME Keyring or KWallet, through `secret-tool` from `libsecret-tools` / `libsecret`) and leaves `${keychain:NAME}` in the file; `${ENV_VAR}` still works and stays as it is. On macOS items are stored and read through `/usr/bin/security`, so the app, the CLI and the daemon read them without a Keychain prompt. Each device has its own store: connect the account (or `hubcodes secret set NAME`, value on stdin) on every device that polls with it. `hubcodes secret check NAME` says whether one is there; `hubcodes secret rm NAME` removes it.

Several trackers run side by side (Jira, Plane and GitHub at once), each with its own query, role and limit. The daemon reads `hubcodes.toml` again when it changes, so a tracker or trigger added in Settings polls without a restart.

With several devices, keep each tracker on one of them. The board already hands tasks to idle agents on every device, so the device that polls only needs to stay awake. Settings → Trackers asks the other devices what they poll and flags a tracker with the same kind, place and query (“Also polled on mini”), since both boards would get every ticket. `hubcodes tracker list --json` leaves secrets out (a `${ENV_VAR}` reference stays).

## 31. Doctor (Settings → This Mac → Doctor, `hubcodes doctor`)

Every "my agents do not show up" of the first weeks was an environment difference the UI could not see: no `LANG` under launchd, the CLI missing from the login PATH, an old tmux. Doctor checks tmux and its version, the locale, the state folder, the config file, which providers are installed and signed in, whether `hubcodes` on the login shell PATH is the same build as the app, the tmux session, Tailscale, and the code signature of the bundle. Each line is a fact plus what to do. **Copy diagnostics** puts the report and the last 80 lines of `~/.local/state/hubcodes/desktop.log` on the clipboard for a bug report; the **App log** group shows the same log live. At start the app runs the checks once and shows a banner when one fails; opening Doctor runs them again. From a terminal: `hubcodes doctor` (exit code 1 on a failed check).

## 32. Delivery queue

`hubcodes send` (and a `@name` in a composer, and the `send` MCP tool) no longer types into an agent that is mid-turn: text pasted into a running tool call lands inside it or is lost. When the receiver's own hooks report `working`, the message goes to the SQLite `outbox` and the transcript shows it as **queued**; it is delivered the moment that agent's turn ends (hook) or at the next tick, and the link pulses then. Providers without hooks never look busy and get the text at once. The card shows `N queued`; `hubcodes send --now` forces it in anyway. A message whose agent is gone is marked **failed** with a `delivery` event.

## 33. The team as MCP tools (`hubcodes mcp`)

Agents started by hubcodes get an MCP server registered automatically: Claude Code (`--mcp-config`) and Codex (`-c mcp_servers.hubcodes…`) on the command line; opencode (`mcp.hubcodes` in `opencode.json`), pi and omp (`mcpServers` in `<agent dir>/mcp.json`, under the omp profile when one is active) and Gemini CLI (`mcpServers` in `settings.json`) through an idempotent entry in their own config, written next to the status shim at every start. Tools: `send`, `read`, `agents` (this session plus other devices' agents, with a device column), `peers`, `messages`, `task_add`, `task_list`, `task_show`, `task_done`, `task_review`, `task_edit`, `task_move`, `task_assign`, `task_columns`, `loop_status`, `fanout`, `role_show`, `spawn` (here or on a device; `max_agents` and the device's `allow` apply), `policy_test`, `note_add`, `note_show`, `memory_recall`, `memory_retain`, `kb_search`, `ask_human`. Resources: `hubcodes://brief`, `hubcodes://notes`, `hubcodes://board`, `hubcodes://agents`. Prompts: the saved prompts, expanded for the calling agent. They are the same code paths as the CLI, so the transcript, loops, policy, the board and the delivery queue see no difference; the model just stops having to get a shell string right. `ask_human` marks the agent as *needs you* (notification, inbox) and the answer arrives as a message. Any other MCP client can run `HUBCODES_AGENT=<name> hubcodes mcp` over stdio. Without `HUBCODES_AGENT` (your own opencode, pi, omp or Gemini session, which reads the same config entry) the server lists no tools, resources or prompts and refuses every call, so a session hubcodes did not start never acts on the team.

## 34. Needs-you navigation, sound, multi-line composer

⌘⇧] / ⌘⇧[ jump to the next / previous agent that needs you (the arrow next to *N need you* in the top bar does the same). Agents that need you but are off-screen show as a pill at the edge of the canvas; click it to go there. Settings → General → *Play a sound* chimes on the idle → needs-you edge (per Mac, off by default). In a composer, Shift+Enter inserts a new line and Enter sends; drop files from Finder onto a card and their paths land in its composer (Claude Code takes image paths).

## 35. Bands (⌘⇧G)

A band is a titled box behind the cards. Cards whose centre lies inside it are its members: drag the band and they come along, collapse it to hide them (the header lists their names), **Send to all** fans a prompt out to every member, **Link from…** links a lead to every member, the swatch picks a colour, double-click the title to rename. Bands live in the layout file with positions and the camera.

## 36. Review comments (Changes tab)

In an agent's Changes tab, click a diff line to leave a comment; **Send to agent** turns all of them into one message (`file:line`, the quoted line, the comment) typed into the agent, which is asked to address each and reply.

## 37. Cost (⌘⇧U)

Every Claude Code turn is logged with its tokens, model and playbook run (`usage_log`). The Cost dialog shows today and the last 7/30/90 days, a bar per day, a table per agent and per playbook run, priced from the list prices in `desktop/src/lib/pricing.ts` (estimates: the provider's dashboard is the bill; unknown models show `≥`).

## 38. Sessions (projects)

The session name in the top bar is a menu: every tmux session with hubcodes agents plus every saved session definition, with agent counts; pick one or *New session…* and the app restarts into it (the choice is kept in `~/.local/state/hubcodes/desktop-session`; `HUBCODES_SESSION` still wins). Layouts, transcripts, boards and bands are per session.

## 39. The daemon: hubcodesd (launchd, systemd)

On Linux and in WSL the same switch writes a systemd user unit, `~/.config/systemd/user/hubcodesd.service` (`Restart=always`, log in `~/.local/state/hubcodes/hubcodesd.log`), and enables it with `systemctl --user enable --now`. WSL needs `systemd=true` under `[boot]` in `/etc/wsl.conf`; without systemd, run `hubcodes daemon run --serve` in a tmux window instead. To keep it running after you log out of a Linux server, `loginctl enable-linger $USER`. The rest of this section describes macOS.

Settings → This Mac → *Keep working when the app is closed* installs `~/Library/LaunchAgents/id.hubstudio.hubcodes.daemon.plist`, which keeps `hubcodes daemon run --serve` alive (`KeepAlive`, `RunAtLoad`, log in `~/.local/state/hubcodes/hubcodesd.log`). One daemon tends **every** tmux session that has hubcodes agents: triggers, trackers, loop reminders, task dispatch, the merge queue and queued deliveries run without the app, and the phone page is served for the session in `HUBCODES_SESSION`. Only one process tends a session at a time: whoever holds `<session>.tend.lock` (a live pid) does dispatch and triggers, the other stands by; the app shows *this session tended by hubcodesd* and still gets the daemon's housekeeping events in its activity panel and notifications, because it follows the daemon's socket (`~/.local/state/hubcodes/hubcodesd.sock`). Hook reports from the agents go through that socket too when it is up, so a turn ending in a terminal reaches queued messages and loops at once; when the socket is down the hook writes to tmux directly, as before. CLI: `hubcodes daemon install [--no-serve] | uninstall | status | run [--serve]`. Installing retires the old per-session `hubcodes watch` LaunchAgents; `hubcodes watch --serve` still works by hand for one session.

## 40. Sharing: contributors, audit, presence

A `[[share]]` role `contributor` sees everything and may write, but every message waits for an operator: it lands in the **inbox** (and the web page) as *From teammates, waiting for your approval* with **Approve and send** / **Deny**; approved text is delivered as `human:<their login>`. Every write through the web page (send, approve, deny, note, pending decisions) is an audit event, listed in Settings → Remote access → **Audit log** and `GET /api/audit`. Teammates on the web canvas post their pointer every second; everyone else sees it as a named cursor on the canvas (`/api/presence`, 12 s TTL).

**Previews are redacted for guests, and an agent can be private.** Viewers see what each agent printed, including a `.env` an agent cat'ed or a token it echoed. Two things limit that. First, every preview, last line, message and history row that goes to anyone below the owner passes through a redactor that replaces common secret shapes (`sk-…`, GitHub `ghp_…` / `github_pat_…`, AWS `AKIA…`, Slack `xox…`, Google `AIza…`, JWTs, `Bearer …`, PEM private-key blocks, and any `password=`, `secret:`, `token=`, `api_key=` style value) with `[redacted]`; the owner on their own machine always sees the raw screen. Second, you can mark an agent private: select its card and press **Private** in the toolbar, or run `hubcodes private <agent> on`. Guests then get `[private agent: working]` instead of its screen and only the status word as its detail; a small lock shows next to its name, `hubcodes ls` lists it as `(private)`, and **Make public** or `hubcodes private <agent> off` reverses it. Redaction is a safety net, not a guarantee: a secret with an unusual shape passes through, so make an agent private when it works with credentials. Tasks, notes and the event list are not redacted.

## 41. Releases: signing, notarization, updates

`pnpm build:mac` signs with the best identity in the keychain. With a **Developer ID Application** certificate and `APPLE_ID`, `APPLE_PASSWORD` (app-specific), `APPLE_TEAM_ID` set, the script also notarizes the `.dmg` with `notarytool` and staples both the app and the dmg, so Gatekeeper accepts it on other Macs. With `TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/hubcodes.key` the build also produces `hubcodes.app.tar.gz` + `.sig` and `target/release/bundle/latest.json`; `desktop/scripts/release-github.sh` then tags `v<version>`, uploads the dmg, the tarball and its `.sig`, rewrites `latest.json` so the download URL is the GitHub API asset URL (readable with a `gh` token while the repo is private, without one once it is public), uploads the feed and pins the dmg checksum in `Casks/hubcodes.rb`. Bump every version field first with `scripts/bump-version.sh 0.1.2`. On other Macs the app checks the feed a few seconds after launch and every 6 hours (through the GitHub API, with the token of a logged-in `gh` when the repo is private) and shows a bar at the bottom: *hubcodes 0.1.2 is ready · Update and restart / Later*. The update is downloaded, its minisign signature verified against the public key in `tauri.conf.json`, installed over the bundle, and the app restarts. Nothing installs without that click; Settings → General → **Check for updates** runs the same check by hand. Keep the private key out of the repo; losing it means a new key and a build that cannot auto-update from the old one.

## 42. Role files (`roles/<role>.md`)

Every agent is told at start what its role does, who is on the team right now and how to hand work over; hubcodes generates that brief from the role name. You can add to it, or write your own, with a role file: a Markdown file named after the role (`software-engineer.md` for "Software Engineer") in `<repo>/.hubcodes/roles/` for one repository, or in `~/.config/hubcodes/roles/` for every repository; the repository's file wins. By default the file is appended to the generated brief. If its first line is `<!-- hubcodes: replace -->`, the file is sent instead of the generated brief (you then also lose the live team roster, so mention `hubcodes ls` yourself). HTML comments in the file are for you and are never sent. `hubcodes role init "<role>"` (add `--repo` for the repository) writes a starting file in replace mode containing the generated text to edit; delete the first line to switch to extending. `hubcodes role show "<role>"` prints exactly what an agent with that role gets, `hubcodes role list` shows the files in both places, and the New agent dialog shows the same under *What this role is told*, with a link to the file in use. The file is read at every start, so restart an agent (`hubcodes kill <agent>` then `hubcodes restore`) after editing it.

## 43. The orchestrator delegates, enforced

A start brief is advice; a long-running or resumed orchestrator keeps the prompt it was started with, and a model under pressure "just does it". Since 0.1.6 the rule holds in three places, whatever prompt the agent carries:

1. **Every human message to an orchestrator carries the rule.** `hubcodes send`, the composer and the phone page append one line to a human's message when the recipient's role is an orchestrator kind (orchestrator, lead, manager, coordinator, planner): `[hubcodes] You are the orchestrator: do not do this yourself. Delegate with hubcodes send <name> "goal, files, done-when" to dev (software engineer), reviewer (reviewer); wait for their [from <name>] replies, then report back.` The names are the agents linked to it on the canvas when there are links, otherwise the whole team; with no teammate running it says how to start one.
2. **The tools refuse.** Before a tool runs, the provider hook asks `hubcodes guard` (Claude Code `PreToolUse`, the opencode plugin's `tool.execute.before`, the pi / omp extensions' `tool_call`). For an orchestrator the answer is *deny* for file edits (Write, Edit, MultiEdit, patch…), build and test runners (`cargo build|test`, `pnpm|npm|yarn|bun …`, `make`, `pytest`, `go build`…), git writes (commit, push, add, checkout, merge, rebase…), file changes from the shell (`sed -i`, `rm`, `mv`, `cp`, `mkdir`, `tee`, `> file`). Reads, searches, `git status|log|diff`, `cargo --version` and every `hubcodes …` command stay allowed. The model sees the reason as the tool's error: *hubcodes: you are the orchestrator; editing files is the engineers' job. Send it to dev with hubcodes send …*, and the refusal is logged as a `guard` event in the activity. The pane's current role decides, so changing the role chip applies at once. `[workspace] guard_orchestrator = false` in `hubcodes.toml` turns the guard off.
3. **Old briefs are flagged.** Every agent is stamped with the brief version it was started with (`@hubcodes_brief_v`). When the app sees a running agent with an older one it shows *N agents started with an older team brief* with **Restart with the current brief**: the agents are stopped and started again with their links, roles and folders kept and the conversation continued where the provider can (`claude -c`). From a shell: `hubcodes restart <name>... [--fresh]`. To change an agent's provider or model, open its panel and press **Switch** (`hubcodes switch <name> -p <provider> [-m <model>]`). The agent keeps its name, role, folder, worktree and links. With the same provider the conversation is continued on the new model; another provider cannot read the old conversation, so it starts with a hand-over note: its brief says who it replaces and lists its open board tasks, and points to a private file (`~/.local/state/hubcodes/handoff/`, mode 600) with its last messages and the end of the old terminal (secrets redacted). With the same provider, the conversation is only resumed when no other agent of that tool works in the same folder (the resume flag would pick up the folder's latest conversation); otherwise it hands over too. The account is reset to the new tool's default login.

**Auto team: extra agents when work piles up.** Settings → Agents → *Auto team* (or `[autoteam]`). When tasks or review requests wait for a role and every agent of that role is busy, hubcodes starts one more, copied from an agent with the role (provider, model, folder, account and links), named `<name>-N` and marked *extra* on its card; it takes the waiting task through the normal dispatch. The orchestrator is told it may ask for one with `hubcodes hire --role "<role>" --reason "…"`. Limits: `per_role` extra agents per role (default 2, a per-role override under *By role*; orchestrators get 0 unless set), `max_agents` on this device (8), `cooldown_minutes` between two hires of one role (2). An added software engineer works in its own worktree on `agent/<name>`; reviewers and testers share the folder. When nothing waits for its role and it has been idle, with its screen unchanged, for `idle_minutes` (10), it is removed (`hubcodes release <name>` does it at once). Work it left (uncommitted changes, or commits not on the main branch) stays on its branch and becomes a board task with a hand-over note for the agent it was copied from, so nothing is lost and no new extra agent is started for it; a clean worktree is removed. Agents you started yourself are never removed.

```toml
[autoteam]
enabled = true
per_role = 2
roles = { reviewer = 3 }
max_agents = 8
idle_minutes = 10
cooldown_minutes = 2
worktree = true
```

**Failover when a provider runs out.** Settings → Agents → *When a provider runs out* (or `[failover]` in `hubcodes.toml`) moves an agent automatically when the bottom of its screen shows the tool's own out-of-limit message (Claude's usage, 5-hour, weekly or Opus limit, Codex's usage limit, Claude's low credit balance), seen on two polls in a row with the screen unchanged. API error codes alone (a 429 the tool retries, `insufficient_quota` in code or test output) do not count. It switches the agent, as **Switch** does, to the next entry of `chain` (a provider or `provider:model`, wrapping round), skipping providers that are not installed and ones that ran out within `rest_minutes` (default 60). An Opus-only limit rests only that model; any other limit rests the whole provider. About 20 seconds after the move the agent gets a message telling it to carry on. When nothing in the chain is free, you get an alert and the agent waits.

```toml
[failover]
enabled = true
chain = ["claude:opus", "claude:sonnet", "codex", "opencode"]
rest_minutes = 60
``` Restart the orchestrator after you add or link teammates, too: the brief names the engineers that existed when it started.

Verified 2026-11-14 on a probe session: a Claude orchestrator with one haiku engineer delegated a file task within 6 s and reported back; when told to edit the file itself, Claude Code, opencode and omp orchestrators were each refused with the reason above and the file stayed unchanged.

## 44. Skills (Settings → Automate → Skills, `hubcodes skill`)

Claude Code, opencode, pi, omp, Codex and Gemini each load skills (`<name>/SKILL.md` folders) from their own folder: `~/.claude/skills`, `~/.config/opencode/skills`, `~/.pi/agent/skills`, `~/.omp/agent/skills`, `~/.codex/skills`, `~/.gemini/skills`. `~/.agents/skills` is the device's shared folder. A skill installed for one tool is missing for the others, so a tracker's `/pr-review {url}` would work for a Claude reviewer and not for an opencode one.

`[skills] sync = true` (**Keep every tool in sync**) fixes that for good. Each minute the daemon (or the app) links every skill it finds, in the shared folder, in any tool's folder, or in a `sources` folder, into every installed tool and the shared folder. A link points at the one real folder, so editing a skill, or a `git pull` of a source such as `~/.hub-skills/skills`, reaches every tool at once; a skill added later anywhere (Claude installing one, a download, a new folder in the source) is linked within a minute. Folders that are already there are never overwritten. A link hubcodes made is removed when its skill disappears (`~/.local/state/hubcodes/skill-links.json` lists them); nothing else is ever deleted. A tool counts as installed when its skill folder exists or its command is on the PATH.

**Add a skill** takes a file or folder dropped from Finder or a pasted path or link: a folder with SKILL.md (or holding several skills), a lone `SKILL.md` (named by its `name:` front matter), a `.zip` or `.skill` file downloaded from a browser, a download link to one of those, a GitHub link to one skill (`https://github.com/anthropics/skills/tree/main/skills/pdf`), or a whole repository (`https://github.com/org/skills`, `git@…`). One skill is copied into `~/.agents/skills`; a repository is cloned under `~/.local/share/hubcodes/skill-repos/` and stays a source. An existing skill of the same name is never replaced. A skill is instructions agents follow: add only ones you trust. **Pull sources** runs `git pull --ff-only` in every source that is a git checkout, then links what is new. Each device keeps its own copies: pick another device at the top of the list to see its skills, and **Copy to this Mac** fetches one (`hubcodes skill pack` there, over federation or SSH) into the shared folder and links it into every tool here. An agent whose tool still lacks a skill is told to run `hubcodes skill show <name>`, which prints the SKILL.md and the folder its files are in.

```toml
[skills]
sync = true
sources = ["~/.hub-skills/skills"]
```

**Set up and manage by asking.** `hubcodes setup [--source <folder>]… [--drop-source <folder>] [--no-sync]` turns `[skills] sync` on, adds folders, writes the built-in `hubcodes` skill to `~/.agents/skills/hubcodes` (linked into every tool), syncs once and prints the doctor report; it is safe to run again. The built-in skill teaches any agent the install, update, setup and `hubcodes skill` commands, so "add this skill: <link>" or "copy pr-review from mini" works in Claude Code, Codex, OpenCode, Pi, OMP or Gemini, inside hubcodes or not. The daemon refreshes that skill after an upgrade; a `hubcodes` skill you wrote yourself (one without the `hubcodes:builtin` marker) is never replaced.

CLI: `hubcodes skill list [--device d] [--json]`, `skill show <name>`, `skill sync`, `skill add <folder|SKILL.md|zip|link|git url>`, `skill pull`, `skill copy <name> [--from <device>]`. Over federation a device may read `skill list` and `skill pack`; copying always writes on the device that asks.
