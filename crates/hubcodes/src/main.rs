mod app;
mod canvas;
mod demo;
mod layout;
mod routing;
mod theme;

use anyhow::{anyhow, Context, Result};
use app::App;
use clap::{Parser, Subcommand};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use hubcodes_core::providers::Registry;
use hubcodes_core::relay;
use hubcodes_core::tmux::{self, Tmux};

#[derive(Parser, Debug)]
#[command(name = "hubcodes", version, about = "Full-TUI canvas for orchestrating AI coding agents on tmux")]
struct Cli {
    /// tmux session to use (default: the current one, or `hubcodes`).
    #[arg(short, long, global = true)]
    session: Option<String>,
    /// Run the scripted demo (no tmux, fake agents).
    #[arg(long)]
    demo: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum ProviderCmd {
    List,
    Export {
        name: String,
        #[arg(long)]
        out: Option<String>,
    },
    Import { file: String },
}

#[derive(Subcommand, Debug)]
enum MemoryCmd {
    Recall {
        query: Vec<String>,
        #[arg(short = 'n', long, default_value = "8")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Retain {
        text: Vec<String>,
        #[arg(long, default_value = "")]
        tags: String,
    },
    /// Test the configured provider.
    Ping,
}

#[derive(Subcommand, Debug)]
enum KbCmd {
    /// Knowledge sources ([[kb]]).
    List {
        #[arg(long)]
        json: bool,
    },
    Search {
        query: Vec<String>,
        /// Only this source.
        #[arg(long = "in")]
        source: Option<String>,
        #[arg(short = 'n', long, default_value = "8")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// One document by the reference a search hit gave.
    Get { source: String, reference: String },
    /// Reach the source and run a sample search.
    Test { name: Option<String> },
}

#[derive(Subcommand, Debug)]
enum TaskCmd {
    Add {
        title: String,
        #[arg(long, default_value = "")]
        detail: String,
        /// Role that should take it (software engineer, reviewer…); empty = anyone idle.
        #[arg(long, default_value = "")]
        role: String,
        /// Hand it to this agent now instead of waiting for an idle one.
        #[arg(long, default_value = "")]
        owner: String,
    },
    List {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// `<id>` here, or `<device>:<id>` for a task handed over from another device's board.
    Done {
        id: String,
        result: Vec<String>,
        /// Who reports, as the other device knows it (`mini/reviewer`); set by the forwarding hop.
        #[arg(long = "as", value_name = "DEVICE/AGENT")]
        r#as: Option<String>,
    },
    /// Approve a task in Review (humans, or reviewer/lead agents that do not own it).
    Approve { id: i64 },
    /// `<id>` here, or `<device>:<id>` for a task from another device's board.
    Review {
        id: String,
        note: Vec<String>,
        #[arg(long = "as", value_name = "DEVICE/AGENT")]
        r#as: Option<String>,
    },
    /// Back to To do (unowned again; an idle agent may pick it up).
    Todo { id: i64 },
    /// Mark it Doing.
    Doing { id: i64 },
    /// Move a task to any board column, built in or your own (`hubcodes task columns` lists them).
    Move { id: i64, column: String },
    /// Put a task in a column at a position: before another task of that column, or last.
    Place {
        id: i64,
        column: String,
        /// The task to go before.
        #[arg(long)]
        before: Option<i64>,
    },
    /// The board's columns, in order.
    Columns {
        #[arg(long)]
        json: bool,
    },
    /// One task in full: fields, result and what happened to it.
    Show {
        id: i64,
        #[arg(long)]
        json: bool,
    },
    /// Change a task's title, detail or role.
    Edit {
        id: i64,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        detail: Option<String>,
        /// Empty string clears it.
        #[arg(long)]
        role: Option<String>,
    },
    /// Hand it to an agent now.
    Assign { id: i64, agent: String },
    /// Delete it.
    Rm { id: i64 },
}

#[derive(Subcommand, Debug)]
enum AccountCmd {
    /// Accounts known for a provider, with whether each is signed in.
    List {
        provider: String,
        #[arg(long)]
        json: bool,
    },
    /// Create an account folder; sign in with `account login`.
    Add { provider: String, name: String },
    /// Delete an account folder and its credentials.
    Rm { provider: String, name: String },
    /// Open the tool under that account in a new window so you can sign in (`/login` and the like).
    Login { provider: String, name: String },
}

#[derive(Subcommand, Debug)]
enum RoomCmd {
    /// Create a room on this device: `room create design lead mini/dev`.
    Create {
        name: String,
        /// Members: agents here (`lead`) or on a device (`mini/dev`).
        members: Vec<String>,
    },
    /// Add members to a room on this device.
    Add { name: String, members: Vec<String> },
    /// Take members out of a room on this device.
    Rm { name: String, members: Vec<String> },
    /// Delete a room and its transcript on this device.
    Delete { name: String },
    /// Rooms on this device with their members.
    List {
        #[arg(long)]
        json: bool,
    },
    /// The transcript of a room on this device.
    Show {
        name: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Post to a room: `room post design "…"` here, `room post mini:design "…"` on device mini.
    Post {
        room: String,
        text: Vec<String>,
        #[arg(long)]
        from: Option<String>,
        /// Set by the device that forwards a post: who sent it (`device/agent`).
        #[arg(long = "as", hide = true)]
        r#as: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum PeerCmd {
    /// Add a device ([[peer]] in hubcodes.toml).
    Add {
        /// Short name; its agents show here as name/agent. Must equal that device's own `self_name`.
        name: String,
        /// SSH host: a Tailscale name, an IP, a Host from ~/.ssh/config, or `local`.
        host: String,
        #[arg(long)]
        user: Option<String>,
        /// tmux session on that device.
        #[arg(long, default_value = "hubcodes")]
        session: String,
        /// Path of hubcodes there, when it is not on the login shell's PATH.
        #[arg(long, default_value = "hubcodes")]
        hubcodes: String,
        /// Refuse to start more agents than this there (0 = no limit).
        #[arg(long, default_value_t = 0)]
        max_agents: u32,
        /// What that device may do here: send, task, spawn, approve, kill, restart (default: send, task, spawn).
        #[arg(long, value_delimiter = ',')]
        allow: Vec<String>,
    },
    /// Remove a device.
    Rm { name: String },
    /// What another device needs to reach this one: the host and user to type there, whether Remote Login is on, and the line that lets this device's SSH key in there.
    Me {
        #[arg(long)]
        json: bool,
        /// Create ~/.ssh/id_ed25519 first when this device has no SSH key.
        #[arg(long)]
        create_key: bool,
    },
    /// Reachability, version and pairing of one device or all: does it run the same release, call itself by this name, and list this device back?
    Check {
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Commands waiting for devices that were unreachable (`--retry` tries them now).
    Queue {
        #[arg(long)]
        retry: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum BoardCmd {
    /// The columns, in order, with WIP limits.
    Columns {
        #[arg(long)]
        json: bool,
    },
    /// Add a column of your own (a waiting place; agents pick from To do only).
    Add {
        label: String,
        /// Put it before this column id instead of last.
        #[arg(long)]
        before: Option<String>,
    },
    /// Remove one of your columns; its tasks go to To do.
    Rm { id: String },
    /// Rename a column (built-ins too).
    Rename { id: String, label: String },
    /// WIP limit for a column (0 = none); dispatch stops while Doing is full.
    Wip { id: String, limit: u32 },
}

#[derive(Subcommand, Debug)]
enum NoteCmd {
    /// Print the shared notes.
    Show {
        #[arg(long)]
        json: bool,
    },
    /// Append a line, signed with your name (or the agent's).
    Add { text: Vec<String> },
    /// Empty the notes.
    Clear,
}

#[derive(Subcommand, Debug)]
enum TeamCmd {
    /// Templates known here: the repository's (.hubcodes/team.toml) first, then hubcodes.toml's.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Start every agent of a template that is not running, then its links and loops.
    Start {
        name: String,
        #[arg(long)]
        cwd: Option<String>,
        /// Model for one agent on this machine, `agent=model` (repeatable); overrides the template.
        #[arg(long = "model", value_name = "AGENT=MODEL")]
        model: Vec<String>,
        /// Only these agents of the template (comma-separated), with the links between them.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },
    /// Write the running agents and links to hubcodes.toml as a template.
    Save { name: String },
    /// Write the running canvas as this repository's team: <repo>/.hubcodes/team.toml.
    Init {
        #[arg(long)]
        cwd: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum PromptCmd {
    /// Saved prompts (`/name` in a composer).
    List {
        #[arg(long)]
        json: bool,
    },
    /// The prompt's text, expanded for an agent when one is named.
    Show { name: String, agent: Option<String> },
}

#[derive(Subcommand, Debug)]
enum SkillCmd {
    /// Skills on this device (or on `--device`) and the providers that have each.
    List {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        device: Option<String>,
    },
    /// Print a skill's SKILL.md (for an agent whose provider does not have it).
    Show { name: String },
    /// Add a skill to providers that lack it: `--to all` (default) or `--to pi --to omp`; `--from <device>` fetches it from another device first.
    Copy {
        name: String,
        #[arg(long, default_value = "all")]
        to: Vec<String>,
        #[arg(long)]
        from: Option<String>,
    },
    /// Link every skill into every installed tool now (what `[skills] sync` does each minute).
    Sync,
    /// Add a skill to the shared folder (~/.agents/skills) from a folder, a .zip/.skill file, or a git URL.
    Add { source: String },
    /// `git pull --ff-only` in every skill source that is a git checkout, then sync.
    Pull,
    /// A skill as base64 tar text, for `skill copy --from` on another device.
    #[command(hide = true)]
    Pack { name: String },
}

#[derive(Subcommand, Debug)]
enum RoleCmd {
    /// Print what an agent with this role is told at start: the generated brief with the role file layered on.
    Show {
        role: String,
        /// The agent's folder, for the repository's .hubcodes/roles/ (default: current folder).
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Write a role file to edit: ~/.config/hubcodes/roles/<slug>.md, or <repo>/.hubcodes/roles/<slug>.md with --repo.
    Init {
        role: String,
        /// Write into this repository's .hubcodes/roles/ instead of the user folder.
        #[arg(long)]
        repo: bool,
        /// The repository folder for --repo (default: current folder).
        #[arg(long)]
        cwd: Option<String>,
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// List the role files in the repository's and the user's roles folders.
    List {
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum PolicyCmd {
    /// Show the merged policy (user + this repository).
    Show {
        #[arg(long)]
        json: bool,
    },
    /// What the policy would answer for a shell command.
    Test { command: Vec<String> },
}

#[derive(Subcommand, Debug)]
enum PlaybookCmd {
    List,
    /// Start a run: the first step's task is created at once.
    Start {
        name: String,
        goal: Vec<String>,
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Open runs (`--all` for finished ones too).
    Runs {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// The log of one run.
    Show { id: i64 },
    /// Continue a run paused at a human gate.
    Approve { id: i64 },
    Abort { id: i64 },
}

#[derive(Subcommand, Debug)]
enum TrackerCmd {
    /// Issue trackers ([[tracker]]).
    List {
        #[arg(long)]
        json: bool,
    },
    /// Reach the tracker and show what its query returns.
    Test { name: Option<String> },
    /// Import new issues now (instead of waiting for the poll).
    Pull { name: Option<String> },
}

#[derive(Subcommand, Debug)]
enum LoopCmd {
    /// Link a and b as a loop and send the goal to a; they alternate until DONE or the round limit.
    Start {
        a: String,
        b: String,
        goal: Vec<String>,
        /// Rounds before it pauses; 0 runs until DONE (default: [loop] max_rounds).
        #[arg(long)]
        max: Option<u32>,
    },
    /// Mark the link a↔b as a loop (messages along it carry reply instructions).
    On { a: String, b: String, #[arg(long)] max: Option<u32> },
    /// Plain link again.
    Off { a: String, b: String },
    /// Resume a paused loop with a fresh round counter.
    Reset { a: String, b: String },
    /// Every loop with its state and round, plus who owes a reply.
    Status {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Open the TUI canvas in the current pane (what `hubcodes` with no command does inside tmux).
    Canvas,
    /// Run the scripted demo in the TUI (no tmux, fake agents).
    Demo,
    /// Show every provider: installed, logged in, models it knows.
    Providers {
        /// Machine-readable: every provider's status and model catalog (what another device reads over SSH).
        #[arg(long)]
        json: bool,
        /// Ask another device ([[peer]] name) for its providers and models instead of this device.
        #[arg(long = "on", value_name = "DEVICE")]
        on: Option<String>,
        /// Status as seen from this account (its own login and settings).
        #[arg(long)]
        account: Option<String>,
    },
    /// Serve the team as MCP tools over stdio (registered automatically for agents hubcodes starts).
    Mcp,
    /// hubcodesd: `daemon install [--no-serve]`, `daemon uninstall`, `daemon status`, `daemon run [--serve]` (tends every session, takes hook reports on its socket, pushes events; with `[sharing] listen = true` also the federation listener), `daemon listen` (that listener alone).
    Daemon {
        /// install | uninstall | status | run
        action: String,
        /// Install without the phone / teammate web page.
        #[arg(long)]
        no_serve: bool,
        /// With `run`: also serve the phone / teammate web page for the session in HUBCODES_SESSION.
        #[arg(long)]
        serve: bool,
        /// With `status`: machine-readable.
        #[arg(long)]
        json: bool,
    },
    /// Check for a newer release (patch releases included) and install it:
    /// replaces the app and the CLI. `--check` only reports.
    Update {
        #[arg(long)]
        check: bool,
    },
    /// Set this device up in one step: skills sync on, the `hubcodes` skill in every
    /// tool (so any agent can install, update and manage hubcodes when asked),
    /// one sync, then a health check. Safe to run again.
    Setup {
        /// A folder of skills to link into every tool too (repeatable), e.g. ~/.hub-skills/skills.
        #[arg(long = "source")]
        sources: Vec<String>,
        /// Stop linking a folder added with --source.
        #[arg(long = "drop-source")]
        drop: Vec<String>,
        /// Leave skills sync off (only install the skill and check).
        #[arg(long)]
        no_sync: bool,
    },
    /// Check tmux, locale, PATH, providers, Tailscale, devices; print a diagnostics text.
    Doctor {
        /// Include the last lines of the desktop log.
        #[arg(long, default_value_t = 40)]
        log_lines: usize,
        #[arg(long)]
        json: bool,
    },
    /// Spawn an agent window.
    Spawn {
        name: String,
        #[arg(short, long, default_value = "shell")]
        provider: String,
        #[arg(long)]
        cwd: Option<String>,
        /// Job in the team, shown on the node and told to the agent: orchestrator, frontend…
        #[arg(short, long)]
        role: Option<String>,
        /// Model passed to the provider's model flag (`claude --model opus`).
        #[arg(short, long)]
        model: Option<String>,
        /// Give the agent its own git worktree on this branch (created from HEAD if new), under <repo>/.hubcodes/worktrees/<name>.
        #[arg(short, long)]
        worktree: Option<String>,
        /// Confine its writes to its folder ([sandbox] in hubcodes.toml: macOS sandbox-exec or a wrapper command).
        #[arg(long)]
        sandbox: bool,
        /// Start it on another device (a [[peer]] name); it runs there and shows here as device/name.
        #[arg(long = "on", value_name = "DEVICE")]
        on: Option<String>,
        /// Run it under this account (a separate login of the tool; `hubcodes account list <provider>`).
        #[arg(long)]
        account: Option<String>,
    },
    /// Logins per provider, so several agents can run on different subscriptions: `account list|add|rm|login <provider> [name]`.
    Account {
        #[command(subcommand)]
        cmd: AccountCmd,
    },
    /// Send text to an agent (used by agents to talk to each other).
    Send {
        /// Target agent name (tmux window name).
        to: String,
        /// Message text.
        text: Vec<String>,
        /// Sender name; defaults to $HUBCODES_AGENT or human:$USER.
        #[arg(long)]
        from: Option<String>,
        /// Type it in even if the agent is mid-turn (default: wait for its prompt).
        #[arg(long)]
        now: bool,
        /// From a person: straight to the agent, not through the session's orchestrator.
        #[arg(long)]
        direct: bool,
        /// Attach a file (repeat for more); copied to the agent's device when it runs on another one.
        #[arg(long = "file", short = 'f')]
        files: Vec<std::path::PathBuf>,
    },
    /// Draw a link between two agents on the canvas (messages travel along links; `loop on` makes one answer back and forth).
    Connect {
        from: String,
        to: String,
        /// Shown on the edge; `loop` is set by `loop on`.
        #[arg(long, default_value = "link")]
        label: String,
    },
    /// Remove the link between two agents.
    Disconnect { a: String, b: String },
    /// Print the last lines of an agent's terminal.
    Read {
        agent: String,
        #[arg(long, default_value_t = 40)]
        lines: usize,
    },
    /// List agents in the session.
    Ls {
        /// Machine-readable output (used by other devices over SSH).
        #[arg(long)]
        json: bool,
        /// Include the last screen lines of every agent (with --json).
        #[arg(long)]
        preview: bool,
    },
    /// Agent-to-agent loops: `loop start a b "goal"`, `loop on|off|reset a b`, `loop status`.
    Loop {
        #[command(subcommand)]
        cmd: LoopCmd,
    },
    /// Other devices from [[peer]] in hubcodes.toml: reachability, version, pairing and their agents (same as `peer check`).
    Peers {
        #[arg(long)]
        json: bool,
    },
    /// Called by the agents' own hooks (Claude Code `--settings`, Codex `notify`, the managed opencode/pi/omp shims); reads the event and records status.
    Hook {
        /// `claude` (payload on stdin), `codex` (payload as the next argument), or `generic` / `opencode` / `pi` / `omp` (`{"status":…,"detail":…,"turn_done":…}` on stdin)
        provider: String,
        payload: Vec<String>,
    },
    /// Orchestrator guard, asked by the provider hooks before a tool runs: `guard --tool <name> [--input <json>]` prints `{"allow":bool,"reason":…}` for the agent in HUBCODES_AGENT.
    Guard {
        #[arg(long)]
        tool: String,
        #[arg(long, default_value = "{}")]
        input: String,
    },
    /// Restart agents with the current brief (links, roles and folders kept; the conversation continues where the provider can): `restart <name>... [--fresh]`.
    Restart {
        names: Vec<String>,
        #[arg(long)]
        fresh: bool,
    },
    /// Auto team: start one more agent of a role, copied from one that has it (provider, model, folder, links; a software engineer gets its own worktree). Refused past the limits in [autoteam]. `hire --role "software engineer" --reason "4 tasks waiting"`.
    Hire {
        #[arg(long)]
        role: String,
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Auto team: remove an extra agent now. Unfinished work in its worktree stays on its branch and goes to another agent of the role as a board task.
    Release { name: String },
    /// Run an agent on another provider or model, keeping its name, role, folder and links: `switch <name> -p codex [-m model]`. The same provider continues the conversation; a new one starts with a hand-over note (open tasks, recent messages, the end of the old terminal).
    Switch {
        name: String,
        #[arg(short, long)]
        provider: Option<String>,
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Goal board: `task add "title" [--detail …] [--role r]`, `task list`, `task done <id> "result"`, `task review|todo|doing <id>`, `task assign <id> <agent>`, `task rm <id>`.
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    /// One prompt to several agents, answers gathered by a collector: `fanout "prompt" --to a,b,c --collect judge`.
    Fanout {
        prompt: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        to: Vec<String>,
        #[arg(long)]
        collect: Option<String>,
        #[arg(long)]
        from: Option<String>,
    },
    /// Shared notes everyone reads: `note show`, `note add "text"`, `note clear`.
    Note {
        #[command(subcommand)]
        cmd: NoteCmd,
    },
    /// Team templates: `team list`, `team start <name> [--cwd dir]`, `team save <name>`.
    Team {
        #[command(subcommand)]
        cmd: TeamCmd,
    },
    /// Agent skills per provider: `skill list [--device d]`, `skill show <name>`, `skill copy <name> [--to all|provider…] [--from device]`.
    Skill {
        #[command(subcommand)]
        cmd: SkillCmd,
    },
    /// Saved prompts: `prompt list`, `prompt show <name> [agent]` (expanded for that agent).
    Prompt {
        #[command(subcommand)]
        cmd: PromptCmd,
    },
    /// Role files (what a role is told at start): `role show <role>`, `role init <role> [--repo]`, `role list`.
    Role {
        #[command(subcommand)]
        cmd: RoleCmd,
    },
    /// Team memory ([memory] in hubcodes.toml): `memory recall "query"`, `memory retain "fact"`, `memory ping`.
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },
    /// Knowledge sources ([[kb]]): `kb list`, `kb search "query" [--in name]`, `kb get <source> <reference>`, `kb test [name]`.
    Kb {
        #[command(subcommand)]
        cmd: KbCmd,
    },
    /// Run the repository's tests for an agent (or in the current folder) and record the verdict.
    Test {
        agent: Option<String>,
        #[arg(long)]
        cmd: Option<String>,
    },
    /// Merge queue: `merge <agent|branch>` queues a merge into the repo's target branch; `merge list`; `merge run` processes the queue now.
    Merge {
        /// Agent (its worktree branch) or branch name to queue; none with --list shows the queue.
        target: Option<String>,
        /// Target branch (default: the repository's [merge] into).
        #[arg(long)]
        into: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        list: bool,
        /// Process the queue now.
        #[arg(long)]
        run: bool,
        #[arg(long)]
        json: bool,
    },
    /// Start the agents, links and loops saved from the last run of this session (after a reboot).
    Restore {
        /// Start fresh conversations instead of continuing (`claude -c`, `codex resume --last`…).
        #[arg(long)]
        fresh: bool,
    },
    /// Shareable provider files: `provider list`, `provider export <name> [--out file]`, `provider import <file>`.
    Provider {
        #[command(subcommand)]
        cmd: ProviderCmd,
    },
    /// Token usage per agent (from the agents' own transcripts) and budget limits.
    Usage {
        #[arg(long)]
        json: bool,
    },
    /// Keep the session tended from a terminal: task dispatch, reminders, budgets, triggers. Runs until Ctrl-C.
    Watch {
        /// Also start the HTTP server (phones, teammates, webhooks).
        #[arg(long)]
        serve: bool,
        /// One tend pass (dispatch, deliveries, queued reports, alerts), then exit. For scripts and tests.
        #[arg(long)]
        once: bool,
    },
    /// HTTP server for phones and teammates (`[serve]` and `[[share]]` in hubcodes.toml). Runs until Ctrl-C.
    Serve {
        /// Folder with the built desktop UI (desktop/dist) to serve the full canvas at /canvas/.
        #[arg(long)]
        web: Option<String>,
    },
    /// Auto-approve policy ([policy] in hubcodes.toml): `policy show`, `policy test "git status"`.
    Policy {
        #[command(subcommand)]
        cmd: PolicyCmd,
    },
    /// Playbooks ([[playbook]]): `playbook list`, `playbook start <name> "goal"`, `playbook runs`, `playbook approve|abort <run>`.
    Playbook {
        #[command(subcommand)]
        cmd: PlaybookCmd,
    },
    /// Tokens in the system secret store (macOS Keychain, Linux keyring), referred to as ${keychain:NAME} in hubcodes.toml: `secret set NAME` (value on stdin), `secret rm NAME`, `secret check NAME`.
    Secret {
        /// set | rm | check
        action: String,
        name: String,
    },
    /// Pull requests under review: `review checkout owner/repo#N` prints a folder with that pull request checked out on its own (one per pull request); `--branch` puts your own pull request on its branch to push fixes from; `review clean owner/repo#N [--branch]` removes it.
    Review {
        /// checkout | clean
        action: String,
        key: String,
        /// Your own pull request on its branch (`fix-<N>`), to push fixes from.
        #[arg(long)]
        branch: bool,
    },
    /// Issue trackers ([[tracker]]): `tracker list`, `tracker test [name]`, `tracker pull [name]`.
    Tracker {
        #[command(subcommand)]
        cmd: TrackerCmd,
    },
    /// Answer an agent's permission prompt with yes (its provider's approve keys).
    Approve { agent: String },
    /// Answer an agent's permission prompt with no.
    Deny { agent: String },
    /// The session transcript: every message, newest first (oldest first with --follow).
    Log {
        #[arg(short = 'n', long, default_value = "50")]
        limit: usize,
        /// Only messages whose sender, receiver or text contains this.
        #[arg(long)]
        grep: Option<String>,
        /// Only messages sent or received by this agent.
        #[arg(long)]
        agent: Option<String>,
        /// Only messages that name this task (`#id`).
        #[arg(long)]
        task: Option<i64>,
        /// Only this kind: message, queued, nudge, loop, task, fanout.
        #[arg(long)]
        kind: Option<String>,
        /// Keep printing new messages as they arrive (Ctrl-C stops).
        #[arg(long)]
        follow: bool,
        #[arg(long)]
        json: bool,
    },
    /// Messages waiting for your approval (from someone else's device or a contributor): `pending`, `pending approve <id>`, `pending deny <id>`.
    Pending {
        /// `approve` or `deny`; empty = list.
        action: Option<String>,
        id: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Rooms: agents on any device hearing each other's posts (`room create|add|rm|delete|list|show|post`).
    Room {
        #[command(subcommand)]
        cmd: RoomCmd,
    },
    /// Devices: `peer add|rm|check|queue` ([[peer]] in hubcodes.toml).
    Peer {
        #[command(subcommand)]
        cmd: PeerCmd,
    },
    /// The board's columns: `board columns`, `board add|rm|rename|wip`.
    Board {
        #[command(subcommand)]
        cmd: BoardCmd,
    },
    /// Shell completions: `hubcodes completions zsh > ~/.zfunc/_hubcodes` (bash, fish, elvish, powershell too).
    Completions {
        shell: clap_complete::Shell,
    },
    /// Kill an agent pane.
    Kill { name: String },
    /// Hide an agent's screen from guests of the shared canvas (they see its status only): `private <agent> on|off`.
    Private {
        agent: String,
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Show an agent's terminal next to the canvas (tiled) and focus it.
    Open { agent: String },
    /// Tile every agent next to the canvas.
    Tile,
    /// Return every tiled agent to its own window and focus the canvas.
    Back,
}

fn main() -> Result<()> {
    // `hubcodes log | head` closes our stdout early: end quietly like any
    // Unix tool instead of panicking on the broken pipe.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    // A launchd job or an app-started shell may have no locale; tmux output
    // is only parseable under UTF-8.
    hubcodes_core::tmux::ensure_utf8_locale();
    let cli = Cli::parse();
    let reg = Registry::load()?;

    let cmd = match (cli.cmd, cli.demo) {
        (Some(c), _) => c,
        (None, true) => Cmd::Demo,
        (None, false) => return launch(&cli.session, reg),
    };

    match cmd {
        Cmd::Demo => run_tui(App::demo(reg)),
        Cmd::Canvas => {
            let t = session(&cli.session)?;
            if !t.has_session() {
                return Err(anyhow!("tmux session '{}' does not exist; run `hubcodes` without arguments", t.session));
            }
            run_tui(App::tmux(reg, t)?)
        }
        Cmd::Daemon { action, no_serve, serve, json } => {
            let session_name = cli.session.clone().or_else(|| std::env::var("HUBCODES_SESSION").ok()).unwrap_or_else(|| "hubcodes".into());
            match action.as_str() {
                "install" => {
                    let exe = hubcodes_core::hooks::self_exe()?;
                    let p = hubcodes_core::daemon::install(&session_name, &exe, !no_serve)?;
                    println!("installed {} (runs `hubcodes daemon run{}`; phone page for session {session_name}; log {})", p.display(), if no_serve { "" } else { " --serve" }, hubcodes_core::daemon::log_path().display());
                }
                "uninstall" => {
                    hubcodes_core::daemon::uninstall(&session_name)?;
                    println!("removed the hubcodesd LaunchAgent");
                }
                // The federation listener alone, in the foreground: no tend
                // pass, no socket (debugging a device link; the peer tests).
                "listen" => {
                    hubcodes_core::fed::listen(&reg.sharing.listen_bind, reg.sharing.listen_port)?;
                }
                "run" => {
                    let t = Tmux::named(&session_name);
                    let (t3, reg3) = (t.clone(), reg.clone());
                    let sink: hubcodes_core::serve::WebhookSink = std::sync::Arc::new(move |name: &str, body: &str| {
                        // Read the config per hit, so a webhook trigger added in Settings works without a restart.
                        let reg3 = Registry::load().unwrap_or_else(|_| reg3.clone());
                        for tr in reg3.triggers.iter().filter(|x| x.enabled) {
                            if hubcodes_core::triggers::kind(tr) == Some(hubcodes_core::triggers::Kind::Webhook(name.to_string())) {
                                let action = hubcodes_core::triggers::fill(&tr.action, &[("body", body.trim())]);
                                let ev = hubcodes_core::tend::run_action(&t3, &reg3, tr, &action);
                                println!("{}", ev.text);
                            }
                        }
                    });
                    hubcodes_core::daemon::run(reg.clone(), &session_name, serve, Some(sink))?;
                }
                _ => {
                    let st = hubcodes_core::daemon::status(&session_name);
                    if json {
                        println!("{}", serde_json::to_string(&st)?);
                        return Ok(());
                    }
                    println!("installed: {}  running: {}{}  socket: {}{}", st.installed, st.running, st.pid.map(|p| format!(" (pid {p})")).unwrap_or_default(), if st.socket { "up" } else { "down" }, st.version.map(|v| format!(" (hubcodesd {v})")).unwrap_or_default());
                    println!("tending: {}  subscribers: {}  this session ({session_name}) tended by: {}", if st.sessions.is_empty() { "-".to_string() } else { st.sessions.join(", ") }, st.subscribers, st.tender.unwrap_or_else(|| "none".into()));
                    println!("plist: {}\nlog: {}\nsocket path: {}", st.plist, st.log, hubcodes_core::ipc::socket_path().display());
                }
            }
            Ok(())
        }
        Cmd::Mcp => {
            let session_name = cli.session.clone().or_else(|| std::env::var("HUBCODES_SESSION").ok()).unwrap_or_else(|| "hubcodes".into());
            // Empty outside a pane hubcodes started: the server then offers no tools.
            let agent = std::env::var("HUBCODES_AGENT").unwrap_or_default();
            let mut server = hubcodes_core::mcpserver::Server::new(&session_name, &agent)?;
            server.run()
        }
        Cmd::Update { check } => {
            use hubcodes_core::update;
            let now = update::current();
            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
            println!("installed: hubcodes {now} ({}: {exe})", update::install_kind());
            let latest = update::latest()?;
            println!("latest:    hubcodes {latest}");
            if !update::newer(&latest, now) {
                println!("{}", if update::newer(now, &latest) { "this build is newer than the release" } else { "up to date" });
                return Ok(());
            }
            if check {
                println!("update with: hubcodes update");
                std::process::exit(10);
            }
            if update::install_kind() == "build" {
                println!("this hubcodes runs from a build tree; the installer puts the release in its usual place and links ~/.local/bin/hubcodes there");
            }
            update::run_installer()?;
            println!("updated to {latest}; restart the app and run hubcodes daemon install if the daemon runs");
            Ok(())
        }
        Cmd::Setup { sources, drop, no_sync } => {
            let mut reg = reg;
            let mut changes = hubcodes_core::skills::configure(&mut reg.skills, !no_sync, &sources, &drop);
            if !changes.is_empty() {
                let path = reg.save()?;
                changes.push(format!("saved {}", path.display()));
            }
            changes.extend(hubcodes_core::skills::ensure_builtin());
            if reg.skills.sync {
                changes.extend(hubcodes_core::skills::sync(&reg.skills));
            }
            println!("{}", if changes.is_empty() { "already set up".to_string() } else { changes.join("\n") });
            println!();
            let ctx = hubcodes_core::doctor::Context { cli: std::env::current_exe().ok(), bundle: None, session: Some(cli.session.clone().unwrap_or_else(|| "hubcodes".into())), peers: reg.peers.clone(), self_name: reg.sharing.name.clone() };
            let r = hubcodes_core::doctor::run(&ctx);
            print!("{}", hubcodes_core::doctor::render(&r, 0));
            if r.fail > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Doctor { log_lines, json } => {
            let ctx = hubcodes_core::doctor::Context { cli: std::env::current_exe().ok(), bundle: None, session: Some(cli.session.clone().unwrap_or_else(|| "hubcodes".into())), peers: reg.peers.clone(), self_name: reg.sharing.name.clone() };
            let r = hubcodes_core::doctor::run(&ctx);
            if json {
                println!("{}", serde_json::to_string(&r)?);
                return Ok(());
            }
            print!("{}", hubcodes_core::doctor::render(&r, log_lines));
            if r.fail > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Providers { json, on, account } => {
            let account = account.unwrap_or_else(|| hubcodes_core::accounts::DEFAULT.into());
            let all: Vec<hubcodes_core::models::ProviderStatus> = match on.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
                Some(device) => {
                    let peer = reg.peers.iter().find(|p| p.name == device).ok_or_else(|| anyhow!("unknown device '{device}'; see `hubcodes peer check`"))?;
                    hubcodes_core::remote::provider_statuses(peer)?
                }
                None => reg.providers.iter().map(|p| hubcodes_core::models::status_for(p, &account)).collect(),
            };
            if json {
                println!("{}", serde_json::to_string(&all)?);
                return Ok(());
            }
            for st in all {
                let login = match st.logged_in { Some(true) => "logged in", Some(false) => "not logged in", None => "login unknown" };
                println!(
                    "{:<10} {:<13} {:<16} {}{}",
                    st.name,
                    if st.installed { "installed" } else { "not installed" },
                    login,
                    st.version.clone().unwrap_or_default(),
                    st.current_model.as_ref().map(|m| format!("  default: {m}")).unwrap_or_default()
                );
                for m in &st.models {
                    println!("    {:<40} {}{}", m.id, m.label, if m.note.is_empty() { String::new() } else { format!("  · {}", m.note) });
                }
                if !st.source.is_empty() {
                    println!("    from {}", st.source);
                }
                if !st.installed {
                    if let Some(i) = &st.setup.install {
                        println!("    install: {i}");
                    }
                }
            }
            Ok(())
        }
        Cmd::Spawn { name, provider, cwd, role, model, worktree, sandbox, on, account } => {
            hubcodes_core::remote::check_allowed(&reg.peers, "spawn")?;
            let account = account.map(|a| a.trim().to_string()).filter(|a| !a.is_empty() && a != hubcodes_core::accounts::DEFAULT);
            if let Some(a) = &account {
                if !hubcodes_core::accounts::valid(a) {
                    return Err(anyhow!("bad account name '{a}'"));
                }
            }
            if let Some(device) = on.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
                let peer = reg.peers.iter().find(|p| p.name == device).ok_or_else(|| anyhow!("unknown device '{device}'; see `hubcodes peers`"))?;
                let full = hubcodes_core::remote::spawn_account(peer, &name, &provider, cwd.as_deref(), role.as_deref(), model.as_deref(), account.as_deref())?;
                println!("started {full} on {} (session {})", peer.host, peer.session);
                return Ok(());
            }
            let t = session(&cli.session)?;
            let mut cwd = cwd;
            let mut wt_path: Option<String> = None;
            if let Some(branch) = &worktree {
                let base = cwd.clone().map(std::path::PathBuf::from).unwrap_or(std::env::current_dir()?);
                let dir = hubcodes_core::git::add_worktree(&base, &name, branch)?;
                wt_path = Some(dir.display().to_string());
                cwd = wt_path.clone();
            }
            let p = reg
                .get(&provider)
                .ok_or_else(|| anyhow!("unknown provider '{provider}'; known: {}", reg.names().join(", ")))?;
            let exe = hubcodes_core::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into());
            // The same launch command the app and `team start` build (hooks, MCP, account env).
            let tpl = hubcodes_core::team::TemplateAgent { name: name.clone(), provider: provider.clone(), role: role.clone(), model: model.clone(), worktree: None, sandbox: false, device: None, account: account.clone() };
            let cmd = hubcodes_core::team::launch_command_in(&reg, &t.session, &tpl, &exe, None, cwd.as_deref())?;
            let _ = p;
            let cmd = if sandbox {
                let dir = cwd.clone().map(std::path::PathBuf::from).unwrap_or(std::env::current_dir()?);
                hubcodes_core::sandbox::wrap(&reg.sandbox, &cmd, &dir)
            } else {
                cmd
            };
            let id = t.spawn_with_account(&name, &provider, &cmd, cwd.as_deref(), role.as_deref(), model.as_deref(), account.as_deref())?;
            if let Some(w) = &wt_path {
                t.set_pane_option(&id, hubcodes_core::tmux::OPT_WORKTREE, w)?;
            }
            if sandbox {
                t.set_pane_option(&id, "@hubcodes_sandbox", "1")?;
            }
            println!(
                "{name} {id} ({provider}{}{}{}{})",
                role.map(|r| format!(", {r}")).unwrap_or_default(),
                model.map(|m| format!(", {m}")).unwrap_or_default(),
                account.map(|a| format!(", account {a}")).unwrap_or_default(),
                wt_path.map(|w| format!(", worktree {w}")).unwrap_or_default()
            );
            Ok(())
        }
        Cmd::Account { cmd } => {
            use hubcodes_core::accounts;
            match cmd {
                AccountCmd::List { provider, json } => {
                    let p = reg.get(&provider).ok_or_else(|| anyhow!("unknown provider '{provider}'; known: {}", reg.names().join(", ")))?;
                    let rows: Vec<serde_json::Value> = accounts::list(&provider)
                        .into_iter()
                        .map(|a| {
                            let st = hubcodes_core::models::status_for(p, &a);
                            serde_json::json!({ "name": a, "loggedIn": st.logged_in, "currentModel": st.current_model, "dir": accounts::dir(&provider, &a).map(|d| d.display().to_string()), "env": accounts::env(&provider, p.hook_kind.as_deref(), &a) })
                        })
                        .collect();
                    if json {
                        println!("{}", serde_json::Value::Array(rows));
                        return Ok(());
                    }
                    for r in rows {
                        println!("{:<14} {:<14} {}", r["name"].as_str().unwrap_or(""), match r["loggedIn"] { serde_json::Value::Bool(true) => "signed in", serde_json::Value::Bool(false) => "not signed in", _ => "unknown" }, r["dir"].as_str().unwrap_or("(the tool's own home)"));
                    }
                    if accounts::env(&provider, p.hook_kind.as_deref(), "x").is_empty() && provider != "shell" {
                        println!("note: {provider} has no isolation variable hubcodes knows; every account shares its login");
                    }
                }
                AccountCmd::Add { provider, name } => {
                    let p = reg.get(&provider).ok_or_else(|| anyhow!("unknown provider '{provider}'"))?;
                    let dir = accounts::add(&provider, &name)?;
                    accounts::prepare(&provider, p.hook_kind.as_deref(), &name)?;
                    println!("added {provider}/{name} at {} · sign in with: hubcodes account login {provider} {name}", dir.display());
                }
                AccountCmd::Rm { provider, name } => {
                    accounts::remove(&provider, &name)?;
                    println!("removed {provider}/{name} (its login is gone)");
                }
                AccountCmd::Login { provider, name } => {
                    let p = reg.get(&provider).ok_or_else(|| anyhow!("unknown provider '{provider}'"))?;
                    if accounts::dir(&provider, &name).is_none() {
                        return Err(anyhow!("name the account to sign in to (not default): hubcodes account add {provider} <name> first"));
                    }
                    accounts::prepare(&provider, p.hook_kind.as_deref(), &name)?;
                    let t = session(&cli.session)?;
                    let agent = format!("{provider}-{name}-login");
                    if t.pane_by_name(&agent).is_ok() {
                        return Err(anyhow!("{agent} is already open; sign in there"));
                    }
                    let cmd = format!("{}{}", accounts::env_prefix(&provider, p.hook_kind.as_deref(), &name), p.command.trim());
                    let id = t.spawn_with_account(&agent, &provider, &cmd, None, None, None, Some(&name))?;
                    println!("opened {agent} ({id}) running `{}` under account {name}: sign in there (for example /login), then close it. New agents with --account {name} use that login.", p.command.trim());
                }
            }
            Ok(())
        }
        Cmd::Send { to, text, from, now, direct, files } => {
            let gate = hubcodes_core::remote::send_gate(&reg.peers)?;
            let t = session(&cli.session)?;
            let text = text.join(" ");
            let text = if files.is_empty() { text } else { hubcodes_core::attach::with_files(&text, &hubcodes_core::attach::for_agent(&reg.peers, &to, &files)?) };
            if text.trim().is_empty() {
                return Err(anyhow!("empty message"));
            }
            let from = from
                .or_else(|| std::env::var("HUBCODES_AGENT").ok())
                .unwrap_or_else(|| format!("human:{}", std::env::var("USER").unwrap_or_else(|_| "me".into())));
            // Someone else's device: the owner approves it in the Inbox first.
            if let hubcodes_core::remote::SendGate::Approve(device) = gate {
                let by = hubcodes_core::remote::attribute(&device, &from);
                let id = hubcodes_core::db::Db::open(&t.session)?.pending_add(&by, &to, &hubcodes_core::redact::redact(&text))?;
                println!("waiting for approval on {} (#{id}): {to} gets it once the owner approves", reg.sharing.name);
                return Ok(());
            }
            // A person's message asks the agent to check with its lead first (unless --direct).
            let (d, routed) = if from.starts_with("human:") {
                relay::deliver_from_person(&t, &reg, &from, &to, &text, direct, now)?
            } else {
                (relay::deliver_with(&t, &reg, &from, &to, &text, now)?, None)
            };
            if let Some(lead) = routed {
                println!("sent to {to}; it checks with {lead} before acting (`--direct` skips that)");
                return Ok(());
            }
            let round = d.loop_round.map(|(r, m)| format!(" loop {}", relay::round_text(r, m))).unwrap_or_default();
            let event = match d.loop_event {
                Some(relay::LoopEvent::Done) => " (loop done)",
                Some(relay::LoopEvent::Paused) => " (loop paused: round limit, `hubcodes loop reset` to continue)",
                _ => "",
            };
            if d.queued && d.remote {
                println!("queued for {to}: its device is unreachable now; hubcodes retries for two days and delivers when it is back{round}{event}");
            } else if d.queued {
                println!("queued for {to}: it is mid-turn, delivered when it is back at its prompt{round}{event}");
            } else {
                println!("sent to {to} ({}){round}{event}", d.target);
            }
            Ok(())
        }
        Cmd::Connect { from, to, label } => {
            let t = session(&cli.session)?;
            // `device/agent` names belong to a peer; only check that the device exists.
            for name in [&from, &to] {
                match name.split_once('/') {
                    Some((peer, _)) if reg.peers.iter().any(|p| p.name == peer) => {}
                    Some((peer, _)) => return Err(anyhow!("unknown device '{peer}'; add a [[peer]] to hubcodes.toml")),
                    None => {
                        t.pane_by_name(name)?;
                    }
                }
            }
            t.add_edge(&from, &to, &label)?;
            println!("{from} → {to} ({label})");
            Ok(())
        }
        Cmd::Disconnect { a, b } => {
            let t = session(&cli.session)?;
            t.remove_edge(&a, &b)?;
            println!("removed {a} ↔ {b}");
            Ok(())
        }
        Cmd::Read { agent, lines } => {
            let t = session(&cli.session)?;
            let w = t.pane_by_name(&agent)?;
            for l in t.capture(&w.id, lines)? {
                println!("{}", canvas::strip_ansi(&l));
            }
            Ok(())
        }
        Cmd::Ls { json, preview } => {
            let t = session(&cli.session)?;
            let canvas = t.canvas_pane()?;
            let canvas_win = canvas.as_deref().and_then(|c| t.window_of(c).ok());
            if json {
                let mut agents = Vec::new();
                for w in t.panes()? {
                    if canvas.as_deref() == Some(w.id.as_str()) || w.current_command == "hubcodes" || w.provider.is_none() || w.provider.as_deref() == Some("proxy") {
                        continue;
                    }
                    // The screen hash lives on the pane, so each call (the peers
                    // poll this every few seconds) can say how long it sat still.
                    let screen: Vec<String> = t.capture(&w.id, 12).unwrap_or_default().iter().map(|l| canvas::strip_ansi(l)).collect();
                    let quiet_secs = {
                        use std::hash::{Hash, Hasher};
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        screen.hash(&mut h);
                        let hash = format!("{:x}", h.finish());
                        let now = hubcodes_core::db::now();
                        let prev = t.pane_option(&w.id, "@hubcodes_screen_hash");
                        let at: i64 = t.pane_option(&w.id, "@hubcodes_screen_at").parse().unwrap_or(0);
                        if prev != hash || at == 0 {
                            let _ = t.set_pane_option(&w.id, "@hubcodes_screen_hash", &hash);
                            let _ = t.set_pane_option(&w.id, "@hubcodes_screen_at", &now.to_string());
                            0.0
                        } else {
                            (now - at).max(0) as f32
                        }
                    };
                    // The snapshot leaves this device (peers poll it over SSH):
                    // a private agent sends no screen, the rest go redacted.
                    let lines = if preview && !w.private { hubcodes_core::redact::redact_lines(&screen) } else { Vec::new() };
                    let fresh = w.hook_at.map(|at| hubcodes_core::db::now() - at < hubcodes_core::hooks::FRESH_SECS).unwrap_or(false);
                    let cwd = std::process::Command::new("tmux")
                        .args(["display-message", "-p", "-t", &w.id, "#{pane_current_path}"])
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_default();
                    agents.push(hubcodes_core::remote::RemoteAgent {
                        name: w.name,
                        id: w.id,
                        provider: w.provider.unwrap_or_else(|| "shell".into()),
                        role: w.role,
                        model: w.model,
                        command: w.current_command,
                        cwd,
                        dead: w.dead || w.exited,
                        preview: lines,
                        private: w.private,
                        status: if fresh { w.hook_status.clone() } else { None },
                        quiet_secs,
                        account: w.account.clone(),
                    });
                }
                let snap = hubcodes_core::remote::RemoteSnapshot {
                    session: t.session.clone(),
                    agents,
                    edges: t.edges()?,
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                    self_name: Some(reg.sharing.name.clone()),
                    peers: reg.peers.iter().map(|p| (p.name.clone(), p.host.clone())).collect(),
                };
                println!("{}", serde_json::to_string(&snap)?);
                return Ok(());
            }
            for w in t.panes()? {
                if canvas.as_deref() == Some(w.id.as_str()) {
                    continue;
                }
                let open = canvas_win.as_deref() == Some(w.window_id.as_str());
                println!(
                    "{:<16} {:<5} {:<10} {:<14} {:<14} {}{}{}{}",
                    w.name,
                    w.id,
                    w.provider.unwrap_or_else(|| "shell".into()),
                    w.role.unwrap_or_else(|| "-".into()),
                    w.model.unwrap_or_else(|| "-".into()),
                    w.current_command,
                    if w.dead || w.exited { " (exited)" } else { "" },
                    if open { " (open)" } else { "" },
                    if w.private { " (private)" } else { "" }
                );
            }
            for (a, b, l) in t.edges()? {
                println!("link {a} → {b} ({l})");
            }
            Ok(())
        }
        Cmd::Loop { cmd } => {
            let t = session(&cli.session)?;
            let max_default = reg.loop_cfg.max_rounds;
            match cmd {
                LoopCmd::Start { a, b, goal, max } => {
                    let goal = goal.join(" ");
                    if goal.trim().is_empty() {
                        return Err(anyhow!("give the loop a goal"));
                    }
                    let me = format!("human:{}", std::env::var("USER").unwrap_or_else(|_| "me".into()));
                    let d = relay::kickoff(&t, &reg, &me, &a, &b, &goal, max.unwrap_or(max_default))?;
                    let m = d.loop_round.map(|(_, m)| m).unwrap_or(max_default);
                    let limit = if m == 0 { "no round limit".to_string() } else { format!("max {m} rounds") };
                    println!("loop {a} ↔ {b} started ({limit}); goal sent to {a}");
                }
                LoopCmd::On { a, b, max } => {
                    relay::set_loop(&t, &a, &b, true, max.unwrap_or(max_default))?;
                    println!("loop on: {a} ↔ {b}");
                }
                LoopCmd::Off { a, b } => {
                    relay::set_loop(&t, &a, &b, false, max_default)?;
                    println!("loop off: {a} ↔ {b} (link kept)");
                }
                LoopCmd::Reset { a, b } => {
                    relay::reset_loop(&t, &a, &b)?;
                    println!("loop reset: {a} ↔ {b}");
                }
                LoopCmd::Status { json } => {
                    let m = relay::loops(&t);
                    if json {
                        println!("{}", serde_json::json!({ "loops": m, "awaiting": relay::awaiting(&t) }));
                        return Ok(());
                    }
                    if m.is_empty() {
                        println!("no loops");
                    }
                    for (k, st) in m {
                        let state = if st.done { "done" } else if st.paused { "paused" } else { "running" };
                        println!("{:<30} {:<8} round {:<7} goal: {}", k.replace('|', " ↔ "), state, relay::round_text(if st.max > 0 { st.rounds.min(st.max) } else { st.rounds }, st.max), st.goal);
                    }
                    for (who, aw) in relay::awaiting(&t) {
                        println!("waiting: {who} owes a reply to {}{}", aw.to, if aw.nudged { " (nudged)" } else { "" });
                    }
                }
            }
            Ok(())
        }
        Cmd::Hook { provider, payload } => {
            use hubcodes_core::hooks;
            let agent = std::env::var("HUBCODES_AGENT").ok().filter(|s| !s.is_empty());
            let Some(agent) = agent else {
                // Not started by hubcodes: nothing to report, never block the tool.
                return Ok(());
            };
            let sess = std::env::var("HUBCODES_SESSION").ok().filter(|s| !s.is_empty()).or(cli.session.clone()).unwrap_or_else(|| tmux::DEFAULT_SESSION.into());
            let t = Tmux::named(&sess);
            let raw = if payload.is_empty() {
                let mut s = String::new();
                let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut s);
                s
            } else {
                payload.join(" ")
            };
            let value: serde_json::Value = serde_json::from_str(raw.trim()).unwrap_or(serde_json::Value::Null);
            // Gemini asks before a tool too: the guard and the policy's deny rules answer.
            if provider == "gemini" && value.get("hook_event_name").and_then(|v| v.as_str()) == Some("BeforeTool") {
                let tool = value.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let input = value.get("tool_input").cloned().unwrap_or(serde_json::Value::Null);
                if let Some(reason) = guard_reason(&reg, &t, &agent, tool, &input) {
                    let _ = hubcodes_core::db::Db::open(&sess).and_then(|db| db.add_event(&agent, "guard", &format!("refused {tool}: {reason}")));
                    println!("{}", serde_json::json!({ "decision": "deny", "reason": reason }));
                    return Ok(());
                }
                let cwd = value.get("cwd").and_then(|v| v.as_str()).map(std::path::PathBuf::from).or_else(|| std::env::current_dir().ok());
                let policy = cwd.as_deref().map(|d| reg.for_dir(d).policy).unwrap_or_else(|| reg.policy.clone());
                if let hubcodes_core::policy::Decision::Deny(rule) = hubcodes_core::policy::decide_tool(&policy, tool, &input) {
                    let _ = hubcodes_core::db::Db::open(&sess).and_then(|db| db.add_event(&agent, "policy", &format!("denied {tool} (rule: {rule})")));
                    println!("{}", serde_json::json!({ "decision": "deny", "reason": format!("hubcodes policy: {rule}") }));
                    return Ok(());
                }
            }
            // A person's prompt to a teammate: it checks with its lead first.
            if provider == "claude" && value.get("hook_event_name").and_then(|v| v.as_str()) == Some("UserPromptSubmit") {
                let prompt = value.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(note) = hubcodes_core::checkin::on_prompt(&t, &reg, &agent, prompt) {
                    let _ = hubcodes_core::db::Db::open(&sess).and_then(|db| db.add_event(&agent, "guard", "a person's request: checks with its lead first"));
                    println!("{}", serde_json::json!({ "hookSpecificOutput": { "hookEventName": "UserPromptSubmit", "additionalContext": note } }));
                }
            }
            // Auto-approve policy: answer Claude Code before it asks.
            if provider == "claude" && value.get("hook_event_name").and_then(|v| v.as_str()) == Some("PreToolUse") {
                let cwd = value.get("cwd").and_then(|v| v.as_str()).map(std::path::PathBuf::from).or_else(|| std::env::current_dir().ok());
                let policy = cwd.as_deref().map(|d| reg.for_dir(d).policy).unwrap_or_else(|| reg.policy.clone());
                let tool = value.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let input = value.get("tool_input").cloned().unwrap_or(serde_json::Value::Null);
                // The orchestrator guard comes first: an orchestrator never edits or builds.
                if let Some(reason) = guard_reason(&reg, &t, &agent, tool, &input) {
                    if let Ok(db) = hubcodes_core::db::Db::open(&sess) {
                        let _ = db.add_event(&agent, "guard", &format!("refused {tool} ({})", reason.chars().take(160).collect::<String>()));
                    }
                    println!("{}", serde_json::json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason } }));
                    return Ok(());
                }
                let d = hubcodes_core::policy::decide_tool(&policy, tool, &input);
                if let hubcodes_core::policy::Decision::Allow(rule) | hubcodes_core::policy::Decision::Deny(rule) = &d {
                    let what = input.get("command").or_else(|| input.get("file_path")).and_then(|v| v.as_str()).unwrap_or(tool);
                    if let Ok(db) = hubcodes_core::db::Db::open(&sess) {
                        let _ = db.add_event(&agent, "policy", &format!("{} {tool} `{}` (rule: {rule})", if d.label() == "allow" { "allowed" } else { "denied" }, what.chars().take(120).collect::<String>()));
                    }
                    println!("{}", serde_json::json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": d.label(), "permissionDecisionReason": format!("hubcodes policy: {rule}") } }));
                }
            }
            let report = match provider.as_str() {
                "claude" => hooks::from_claude(&value),
                "codex" => hooks::from_codex(&value),
                "gemini" => hooks::from_gemini(&value),
                // The managed shims (opencode plugin, pi / omp extensions)
                // and anything else send the generic payload on stdin.
                "generic" | "opencode" | "pi" | "omp" => hooks::from_generic(&value),
                _ => None,
            };
            if let Some(r) = report {
                // Through hubcodesd when it runs (it writes the pane, does the
                // turn-end bookkeeping and pushes the event); straight to tmux otherwise.
                let via_daemon = hubcodes_core::ipc::alive()
                    && hubcodes_core::ipc::call(
                        "hook",
                        serde_json::json!({ "session": sess, "agent": agent, "status": r.status, "detail": r.detail, "turnDone": r.turn_done, "transcript": r.transcript.as_ref().map(|p| p.display().to_string()) }),
                    )
                    .is_ok();
                if !via_daemon {
                    if let Err(e) = hubcodes_core::daemon::ingest_report(&t, &agent, &r) {
                        eprintln!("hubcodes hook: {e:#}");
                    }
                }
            }
            Ok(())
        }
        Cmd::Task { cmd } => {
            use hubcodes_core::tasks;
            hubcodes_core::remote::check_allowed(&reg.peers, "task")?;
            let t = session(&cli.session)?;
            let db = hubcodes_core::db::Db::open(&t.session)?;
            let me = std::env::var("HUBCODES_AGENT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("human:{}", std::env::var("USER").unwrap_or_default()));
            match cmd {
                TaskCmd::Add { title, detail, role, owner } => {
                    let task = tasks::add(&db, &title, &detail, &role, &me, &owner)?;
                    if !owner.is_empty() {
                        let text = tasks::assignment_text(&task, &me);
                        relay::deliver(&t, &reg, "hubcodes", &owner, &text)?;
                    }
                    println!("task #{} {} [{}]{}", task.id, task.title, task.status, if owner.is_empty() { " (an idle agent will pick it up)".to_string() } else { format!(" → {owner}") });
                }
                TaskCmd::List { all, json } => {
                    let list = tasks::list(&db, all)?;
                    if json {
                        println!("{}", serde_json::to_string(&list)?);
                    } else if list.is_empty() {
                        println!("no tasks");
                    } else {
                        for x in list {
                            println!("#{:<4} {:<7} {:<12} {:<12} {}{}", x.id, x.status, if x.owner.is_empty() { "-" } else { &x.owner }, if x.role.is_empty() { "-" } else { &x.role }, x.title, if x.result.is_empty() { String::new() } else { format!("  → {}", x.result) });
                        }
                    }
                }
                TaskCmd::Approve { id } => {
                    let x = tasks::get(&db, id)?.ok_or_else(|| anyhow!("no task #{id}"))?;
                    let role = std::env::var("HUBCODES_ROLE").unwrap_or_default();
                    if !tasks::can_approve(&me, &role, &x) {
                        return Err(anyhow!("{me} may not approve #{id}: reviewers, leads and humans can, never the owner"));
                    }
                    let x = tasks::set_status(&db, id, "done", None)?;
                    let _ = t.record_message_kind(&me, "board", &format!("task #{id} approved: {}", x.title), "task");
                    if !x.owner.is_empty() && x.owner != me {
                        let _ = relay::deliver(&t, &reg, &me, &x.owner, &format!("Task #{id} “{}” approved by {me}.", x.title));
                    }
                    println!("task #{id} approved");
                }
                TaskCmd::Done { id, result, r#as } => {
                    // `mac:3`: the board is on another device; run the same command there, attributed to us.
                    if let Some((device, raw)) = id.split_once(':') {
                        let peer = reg.peers.iter().find(|p| p.name == device).ok_or_else(|| anyhow!("unknown device '{device}'; add it in Settings → Devices (or [[peer]] in hubcodes.toml)"))?;
                        let who = format!("{}/{me}", reg.sharing.name);
                        let joined = result.join(" ");
                        let args = ["task", "done", raw, &joined, "--as", &who];
                        match hubcodes_core::remote::run(peer, &args, std::time::Duration::from_secs(20)) {
                            Ok(_) => println!("task #{raw} on {device} done (reported as {who})"),
                            Err(e) => {
                                // The other Mac is away: keep the report, hubcodesd and the app retry it.
                                let t = session(&cli.session)?;
                                let q = hubcodes_core::remote::queue(&t.session, device, &args, &format!("{e:#}"))?;
                                println!("{device} is unreachable ({e:#}); the report is queued (#{}) and goes out when {device} is back. Nothing else to do.", q.id);
                            }
                        }
                        return Ok(());
                    }
                    let id: i64 = id.parse().map_err(|_| anyhow!("task id must be a number or device:number, got '{id}'"))?;
                    if let Some(device) = r#as.as_deref().and_then(|a| a.split_once('/')).map(|(d, _)| d) {
                        hubcodes_core::remote::verify_as(&reg.peers, device)?;
                    }
                    let me = r#as.filter(|a| a.contains('/')).unwrap_or(me);
                    let review_required = reg.for_dir(&std::env::current_dir()?).repo.board.review_required;
                    let x = tasks::finish(&db, id, &result.join(" "), &me, review_required)?;
                    if x.status == "review" {
                        println!("task #{id} is in Review (this repo requires an approval: hubcodes task approve {id})");
                        return Ok(());
                    }
                    if reg.memory.auto_retain {
                        let _ = hubcodes_core::memory::retain(&reg.memory, &t.session, &me, &format!("Task done: {} — {}", x.title, result.join(" ")), "task");
                    }
                    let _ = t.record_message_kind(&me, "board", &format!("task #{id} done: {}", x.title), "task");
                    // Tell whoever created it.
                    if !x.created_by.is_empty() && x.created_by != me && !x.created_by.starts_with("human:") && !x.created_by.starts_with("trigger:") {
                        let _ = relay::deliver(&t, &reg, &me, &x.created_by, &format!("Task #{id} “{}” is done: {}", x.title, result.join(" ")));
                    }
                    println!("task #{id} done");
                }
                TaskCmd::Review { id, note, r#as } => {
                    if let Some((device, raw)) = id.split_once(':') {
                        let peer = reg.peers.iter().find(|p| p.name == device).ok_or_else(|| anyhow!("unknown device '{device}'; add it in Settings → Devices (or [[peer]] in hubcodes.toml)"))?;
                        let who = format!("{}/{me}", reg.sharing.name);
                        let joined = note.join(" ");
                        let args = ["task", "review", raw, &joined, "--as", &who];
                        match hubcodes_core::remote::run(peer, &args, std::time::Duration::from_secs(20)) {
                            Ok(_) => println!("task #{raw} on {device} in review (reported as {who})"),
                            Err(e) => {
                                // The other Mac is away: keep the report, hubcodesd and the app retry it.
                                let t = session(&cli.session)?;
                                let q = hubcodes_core::remote::queue(&t.session, device, &args, &format!("{e:#}"))?;
                                println!("{device} is unreachable ({e:#}); the report is queued (#{}) and goes out when {device} is back. Nothing else to do.", q.id);
                            }
                        }
                        return Ok(());
                    }
                    let id: i64 = id.parse().map_err(|_| anyhow!("task id must be a number or device:number, got '{id}'"))?;
                    if let Some(device) = r#as.as_deref().and_then(|a| a.split_once('/')).map(|(d, _)| d) {
                        hubcodes_core::remote::verify_as(&reg.peers, device)?;
                    }
                    let me = r#as.filter(|a| a.contains('/')).unwrap_or(me);
                    let x = tasks::set_status(&db, id, "review", Some(&note.join(" ")))?;
                    if !x.created_by.is_empty() && !x.created_by.starts_with("human:") && !x.created_by.starts_with("trigger:") && x.created_by != me {
                        let _ = relay::deliver(&t, &reg, &me, &x.created_by, &format!("Task #{id} “{}” needs review: {}", x.title, note.join(" ")));
                    }
                    println!("task #{id} in review");
                }
                TaskCmd::Todo { id } => {
                    tasks::set_status(&db, id, "todo", None)?;
                    println!("task #{id} back to todo");
                }
                TaskCmd::Move { id, column } => {
                    let x = tasks::move_to(&db, id, &column, None, &reg.board)?;
                    println!("task #{id} in {}", reg.board.columns.iter().find(|c| c.id == x.status).map(|c| c.label.as_str()).unwrap_or(&x.status));
                }
                TaskCmd::Columns { json } => {
                    if json {
                        println!("{}", serde_json::to_string(&reg.board.columns)?);
                        return Ok(());
                    }
                    for c in &reg.board.columns {
                        println!("{:<12} {:<16} {}{}", c.id, c.label, if c.wip > 0 { format!("wip {}", c.wip) } else { String::new() }, if hubcodes_core::board::is_builtin(&c.id) { "" } else { "  (yours)" });
                    }
                }
                TaskCmd::Show { id, json } => {
                    let x = tasks::get(&db, id)?.ok_or_else(|| anyhow!("no task #{id}"))?;
                    let history = tasks::history(&db, id)?;
                    if json {
                        println!("{}", serde_json::json!({ "task": x, "history": history }));
                        return Ok(());
                    }
                    println!("#{} {}\nstatus {}  owner {}  role {}  created by {}  rank {}", x.id, x.title, x.status, if x.owner.is_empty() { "-" } else { &x.owner }, if x.role.is_empty() { "-" } else { &x.role }, if x.created_by.is_empty() { "-" } else { &x.created_by }, x.rank);
                    if !x.detail.is_empty() {
                        println!("detail: {}", x.detail);
                    }
                    if !x.result.is_empty() {
                        println!("result: {}", x.result);
                    }
                    if !x.external.is_empty() {
                        println!("tracker: {} {}", x.external, x.external_url);
                    }
                    if x.run > 0 {
                        println!("playbook run #{}", x.run);
                    }
                    println!("history:");
                    for h in history {
                        println!("  {}  {:<14} [{}] {}", chrono_like(h.at), h.who, h.kind, h.text);
                    }
                }
                TaskCmd::Edit { id, title, detail, role } => {
                    let x = tasks::get(&db, id)?.ok_or_else(|| anyhow!("no task #{id}"))?;
                    let x = tasks::update(&db, id, title.as_deref().unwrap_or(&x.title), detail.as_deref().unwrap_or(&x.detail), role.as_deref().unwrap_or(&x.role))?;
                    println!("task #{id}: {} ({})", x.title, if x.role.is_empty() { "any role" } else { &x.role });
                }
                TaskCmd::Place { id, column, before } => {
                    let x = tasks::place(&db, id, &column, before, &reg.board)?;
                    println!("task #{id} in {} at rank {}", x.status, x.rank);
                }
                TaskCmd::Doing { id } => {
                    tasks::set_status(&db, id, "doing", None)?;
                    println!("task #{id} doing");
                }
                TaskCmd::Assign { id, agent } => {
                    let x = tasks::assign(&db, id, &agent)?;
                    relay::deliver(&t, &reg, "hubcodes", &agent, &tasks::assignment_text(&x, &x.created_by))?;
                    println!("task #{id} → {agent}");
                }
                TaskCmd::Rm { id } => {
                    tasks::remove(&db, id)?;
                    println!("task #{id} removed");
                }
            }
            Ok(())
        }
        Cmd::Fanout { prompt, to, collect, from } => {
            let t = session(&cli.session)?;
            let prompt = prompt.join(" ");
            let from = from.or_else(|| std::env::var("HUBCODES_AGENT").ok()).unwrap_or_else(|| format!("human:{}", std::env::var("USER").unwrap_or_default()));
            let f = hubcodes_core::fanout::start(&t, &reg, &from, &to, collect.as_deref(), &prompt)?;
            println!("fan-out #{} to {} agents{}", f.id, f.expected, if f.collector.is_empty() { String::new() } else { format!(", answers go to {}", f.collector) });
            Ok(())
        }
        Cmd::Note { cmd } => {
            use hubcodes_core::notes;
            let t = session(&cli.session)?;
            match cmd {
                NoteCmd::Show { json } => {
                    let text = notes::read(&t.session);
                    if json {
                        println!("{}", serde_json::json!({ "session": t.session, "text": text }));
                    } else {
                        print!("{text}");
                    }
                }
                NoteCmd::Add { text } => {
                    let by = std::env::var("HUBCODES_AGENT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("human:{}", std::env::var("USER").unwrap_or_default()));
                    notes::add(&t.session, &by, &text.join(" "))?;
                    println!("noted");
                }
                NoteCmd::Clear => {
                    notes::clear(&t.session)?;
                    println!("notes cleared");
                }
            }
            Ok(())
        }
        Cmd::Team { cmd } => {
            use hubcodes_core::team;
            let reg = reg.for_dir(&std::env::current_dir()?);
            match cmd {
                TeamCmd::List { json } => {
                    if json {
                        println!("{}", serde_json::to_string(&reg.templates)?);
                        return Ok(());
                    }
                    for tpl in &reg.templates {
                        println!("{:<12} {}  [{}]  links {}", tpl.name, tpl.description, tpl.agents.iter().map(|a| format!("{}:{}{}", a.name, a.provider, a.role.as_ref().map(|r| format!("/{r}")).unwrap_or_default())).collect::<Vec<_>>().join(" "), tpl.links.len());
                    }
                }
                TeamCmd::Start { name, cwd, model, only } => {
                    let t = session(&cli.session)?;
                    let full = reg.templates.iter().find(|x| x.name == name).ok_or_else(|| anyhow!("no template '{name}'; see `hubcodes team list`"))?;
                    let only: Vec<String> = only.into_iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
                    let narrowed;
                    let tpl = if only.is_empty() {
                        full
                    } else {
                        for o in &only {
                            if !full.agents.iter().any(|a| &a.name == o) {
                                return Err(anyhow!("template '{name}' has no agent '{o}' (it has {})", full.agents.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")));
                            }
                        }
                        narrowed = team::Template {
                            agents: full.agents.iter().filter(|a| only.contains(&a.name)).cloned().collect(),
                            links: full.links.iter().filter(|l| l.len() >= 2 && only.contains(&l[0]) && only.contains(&l[1])).cloned().collect(),
                            ..full.clone()
                        };
                        &narrowed
                    };
                    let exe = hubcodes_core::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into());
                    let mut models = std::collections::BTreeMap::new();
                    for m in model {
                        let (agent, id) = m.split_once('=').ok_or_else(|| anyhow!("--model takes agent=model, got '{m}'"))?;
                        models.insert(agent.trim().to_string(), id.trim().to_string());
                    }
                    let st = team::start_with(&t, &reg, tpl, cwd.as_deref(), &exe, None, &models);
                    println!("started: {}", st.spawned.join(", "));
                    if !st.skipped.is_empty() {
                        println!("already running: {}", st.skipped.join(", "));
                    }
                    println!("links: {}", st.links);
                    return finish_with(&st.errors);
                }
                TeamCmd::Init { cwd } => {
                    let t = session(&cli.session)?;
                    let dir = cwd.map(std::path::PathBuf::from).unwrap_or(std::env::current_dir()?);
                    let root = hubcodes_core::git::repo_root(&dir).ok_or_else(|| anyhow!("{} is not inside a git repository", dir.display()))?;
                    let mut rc = hubcodes_core::repo::load_at(&root);
                    let tpl = team::capture(&t, root.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "team".into()).as_str())?;
                    rc.templates.retain(|x| x.name != tpl.name);
                    rc.templates.insert(0, tpl.clone());
                    if rc.test.command.is_empty() {
                        if let Some(c) = hubcodes_core::repo::detect_test_command(&root) {
                            rc.test.command = c;
                        }
                    }
                    let path = hubcodes_core::repo::save_at(&root, &rc)?;
                    println!("wrote {} ({} agents, {} links, tests: {})", path.display(), tpl.agents.len(), tpl.links.len(), if rc.test.command.is_empty() { "not detected" } else { &rc.test.command });
                }
                TeamCmd::Save { name } => {
                    let t = session(&cli.session)?;
                    let tpl = team::capture(&t, &name)?;
                    let mut next = reg.clone();
                    next.templates.retain(|x| x.name != name);
                    next.templates.insert(0, tpl.clone());
                    let path = next.save()?;
                    println!("saved template '{name}' ({} agents, {} links) to {}", tpl.agents.len(), tpl.links.len(), path.display());
                }
            }
            Ok(())
        }
        Cmd::Role { cmd } => {
            use hubcodes_core::prompts;
            let here = |cwd: Option<String>| -> Result<String> { Ok(cwd.unwrap_or(std::env::current_dir()?.display().to_string())) };
            match cmd {
                RoleCmd::Show { role, cwd } => {
                    let cwd = here(cwd)?;
                    // The roster needs a session; without one the brief still shows the job.
                    let session = session(&cli.session).map(|t| t.session).unwrap_or_default();
                    let (text, file) = prompts::effective_brief(&session, "", Some(&role), Some(&cwd));
                    println!("{text}");
                    match file {
                        Some(p) => {
                            let mode = if std::fs::read_to_string(&p).map(|t| prompts::replaces(&t)).unwrap_or(false) { "replaces" } else { "extends" };
                            eprintln!("role file: {} ({mode} the generated brief)", p.display());
                        }
                        None => eprintln!("no role file for \"{role}\" (looked for {}.md in {}); `hubcodes role init \"{role}\"` writes one", prompts::role_slug(&role), prompts::role_dirs(Some(&cwd)).iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ")),
                    }
                }
                RoleCmd::Init { role, repo, cwd, force } => {
                    let slug = prompts::role_slug(&role);
                    if slug.is_empty() {
                        anyhow::bail!("role \"{role}\" has no letters or digits to name a file after");
                    }
                    let dir = if repo {
                        let cwd = here(cwd)?;
                        prompts::repo_roles_dir(std::path::Path::new(&cwd)).ok_or_else(|| anyhow!("{cwd} is not inside a git repository"))?
                    } else {
                        prompts::user_roles_dir()
                    };
                    let path = dir.join(format!("{slug}.md"));
                    if path.exists() && !force {
                        anyhow::bail!("{} exists; edit it, or pass --force to overwrite", path.display());
                    }
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(&path, prompts::role_template(&role))?;
                    println!("wrote {} (replaces the generated brief; see the comment at the top to extend it instead)", path.display());
                }
                RoleCmd::List { cwd, json } => {
                    let cwd = here(cwd)?;
                    if json {
                        let mut out = Vec::new();
                        for dir in prompts::role_dirs(Some(&cwd)) {
                            let mut files: Vec<_> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "md").unwrap_or(false)).collect()).unwrap_or_default();
                            files.sort();
                            for f in files {
                                let mode = if std::fs::read_to_string(&f).map(|t| prompts::replaces(&t)).unwrap_or(false) { "replaces" } else { "extends" };
                                out.push(serde_json::json!({ "role": f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), "file": f.display().to_string(), "mode": mode }));
                            }
                        }
                        println!("{}", serde_json::Value::Array(out));
                        return Ok(());
                    }
                    for dir in prompts::role_dirs(Some(&cwd)) {
                        let mut files: Vec<_> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "md").unwrap_or(false)).collect()).unwrap_or_default();
                        files.sort();
                        println!("{}{}", dir.display(), if files.is_empty() { "  (none)" } else { "" });
                        for f in files {
                            let mode = if std::fs::read_to_string(&f).map(|t| prompts::replaces(&t)).unwrap_or(false) { "replaces" } else { "extends" };
                            println!("  {:<24} {mode}", f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
                        }
                    }
                }
            }
            Ok(())
        }
        Cmd::Skill { cmd } => {
            use hubcodes_core::skills;
            let peer_of = |device: &str| reg.peers.iter().find(|p| p.name == device).cloned().ok_or_else(|| anyhow!("unknown device '{device}'; add it in Settings → Devices"));
            match cmd {
                SkillCmd::List { json, device } => {
                    let list = match device {
                        Some(d) => hubcodes_core::remote::skills(&peer_of(&d)?)?,
                        None => skills::list(),
                    };
                    if json {
                        println!("{}", serde_json::to_string(&list)?);
                        return Ok(());
                    }
                    for sk in &list {
                        println!("{:<28} {}", sk.name, sk.providers.join(", "));
                    }
                }
                SkillCmd::Show { name } => {
                    let dir = skills::find(&name).ok_or_else(|| anyhow!("no skill {name} on this device; `hubcodes skill list` shows what there is"))?;
                    println!("{}", std::fs::read_to_string(dir.join("SKILL.md"))?);
                    println!("\n(Files it refers to are under {})", dir.display());
                }
                SkillCmd::Copy { name, to, from } => {
                    let added = match from {
                        // From another device: into the shared folder, then linked into every tool.
                        Some(d) => {
                            skills::unpack(&name, &hubcodes_core::remote::skill_pack(&peer_of(&d)?, &name)?, &["shared".to_string()])?;
                            skills::sync_only(&reg.skills, Some(&name))
                        }
                        None if to == ["all"] => skills::sync_only(&reg.skills, Some(&name)),
                        None => skills::copy(&skills::find(&name).ok_or_else(|| anyhow!("no skill {name} on this device"))?, &name, &to)?,
                    };
                    if added.is_empty() {
                        println!("{name}: every tool asked for has it already");
                    } else {
                        println!("{}", added.join("\n"));
                    }
                }
                SkillCmd::Pack { name } => println!("{}", skills::pack(&name)?),
                SkillCmd::Sync => {
                    let ch = skills::sync(&reg.skills);
                    println!("{}", if ch.is_empty() { "every tool has every skill".to_string() } else { ch.join("\n") });
                }
                SkillCmd::Add { source } => {
                    let names = skills::add(&source)?;
                    println!("added {}", names.join(", "));
                    for c in skills::sync(&reg.skills) {
                        println!("{c}");
                    }
                }
                SkillCmd::Pull => {
                    for l in skills::pull(&reg.skills) {
                        println!("{l}");
                    }
                    for c in skills::sync(&reg.skills) {
                        println!("{c}");
                    }
                }
            }
            Ok(())
        }
        Cmd::Prompt { cmd } => {
            use hubcodes_core::prompts;
            match cmd {
                PromptCmd::List { json } => {
                    if json {
                        println!("{}", serde_json::to_string(&reg.prompts)?);
                        return Ok(());
                    }
                    for p in &reg.prompts {
                        println!("/{:<10} {}", p.name, p.description);
                    }
                }
                PromptCmd::Show { name, agent } => {
                    let p = reg.prompts.iter().find(|p| p.name == name).ok_or_else(|| anyhow!("no prompt /{name}"))?;
                    let mut ctx = prompts::Context::default();
                    if let Some(a) = agent {
                        let t = session(&cli.session)?;
                        let pane = t.pane_by_name(&a)?;
                        ctx.agent = a;
                        ctx.role = pane.role.unwrap_or_default();
                        ctx.model = pane.model.unwrap_or_default();
                        ctx.cwd = pane.worktree.unwrap_or_default();
                        ctx.notes = hubcodes_core::notes::read(&t.session);
                    }
                    println!("{}", prompts::expand(&p.text, &ctx));
                }
            }
            Ok(())
        }
        Cmd::Memory { cmd } => {
            use hubcodes_core::memory;
            let t = session(&cli.session)?;
            let by = std::env::var("HUBCODES_AGENT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("human:{}", std::env::var("USER").unwrap_or_default()));
            match cmd {
                MemoryCmd::Recall { query, limit, json } => {
                    let hits = memory::recall(&reg.memory, &t.session, &query.join(" "), limit)?;
                    if json {
                        println!("{}", serde_json::to_string(&hits)?);
                    } else if hits.is_empty() {
                        println!("nothing remembered about that ({} provider)", reg.memory.provider);
                    } else {
                        for h in hits {
                            println!("- {}{}", h.text.replace('\n', " "), if h.source.is_empty() { String::new() } else { format!("  ({})", h.source) });
                        }
                    }
                }
                MemoryCmd::Retain { text, tags } => {
                    let out = memory::retain(&reg.memory, &t.session, &by, &text.join(" "), &tags)?;
                    println!("{out}");
                }
                MemoryCmd::Ping => {
                    let p = memory::ping(&reg.memory, &t.session);
                    println!("{} {} ({} ms): {}", if p.ok { "ok  " } else { "FAIL" }, p.provider, p.ms, p.detail);
                    if !p.ok {
                        std::process::exit(1);
                    }
                }
            }
            Ok(())
        }
        Cmd::Kb { cmd } => {
            use hubcodes_core::kb;
            match cmd {
                KbCmd::List { json } => {
                    if json {
                        println!("{}", serde_json::to_string(&reg.kbs)?);
                        return Ok(());
                    }
                    if reg.kbs.is_empty() {
                        println!("no knowledge sources; add [[kb]] entries to hubcodes.toml or Settings → Knowledge");
                    }
                    for s in &reg.kbs {
                        println!("{:<14} {:<11} {}{}", s.name, s.kind, if s.url.is_empty() { if s.repo.is_empty() { s.path.clone() } else { s.repo.clone() } } else { s.url.clone() }, if s.enabled { "" } else { "  (disabled)" });
                    }
                }
                KbCmd::Search { query, source, limit, json } => {
                    let q = query.join(" ");
                    let (hits, errors) = kb::search(&reg.kbs, source.as_deref(), &q, limit);
                    let t = session(&cli.session).ok();
                    if let Some(t) = &t {
                        let by = std::env::var("HUBCODES_AGENT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "human".into());
                        kb::record(&t.session, &by, &q, &hits);
                    }
                    if json {
                        println!("{}", serde_json::to_string(&serde_json::json!({ "hits": hits, "errors": errors }))?);
                    } else {
                        for h in &hits {
                            println!("[{}] {}\n    {}\n    {}", h.source, h.title, h.reference, h.snippet);
                        }
                        if hits.is_empty() {
                            println!("no hits");
                        }
                        for e in errors {
                            eprintln!("error: {e}");
                        }
                    }
                }
                KbCmd::Get { source, reference } => {
                    print!("{}", kb::get(&reg.kbs, &source, &reference)?);
                }
                KbCmd::Test { name } => {
                    for s in reg.kbs.iter().filter(|s| name.as_ref().map(|n| n == &s.name).unwrap_or(true)) {
                        let c = kb::check(s);
                        println!("{} {:<14} ({} ms) {}", if c.ok { "ok  " } else { "FAIL" }, c.name, c.ms, c.detail);
                    }
                }
            }
            Ok(())
        }
        Cmd::Test { agent, cmd } => {
            let t = session(&cli.session)?;
            let (name, dir) = match agent {
                Some(a) => {
                    let p = t.pane_by_name(&a)?;
                    let d = p.worktree.clone().or_else(|| {
                        std::process::Command::new("tmux").args(["display-message", "-p", "-t", &p.id, "#{pane_current_path}"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    }).unwrap_or_default();
                    (a, std::path::PathBuf::from(d))
                }
                None => (std::env::var("HUBCODES_AGENT").unwrap_or_else(|_| "human".into()), std::env::current_dir()?),
            };
            let r = hubcodes_core::testrun::run_for_agent(&t, &name, &dir, cmd.as_deref())?;
            println!("{} · {} · {} s\n{}", if r.ok { "PASS" } else { "FAIL" }, r.summary, r.secs, r.tail.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
            if !r.ok {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Merge { target, into, cwd, list, run, json } => {
            use hubcodes_core::merge;
            let t = session(&cli.session)?;
            let db = hubcodes_core::db::Db::open(&t.session)?;
            if list || (target.is_none() && !run) {
                let rows = merge::list(&db, 20)?;
                if json {
                    println!("{}", serde_json::to_string(&rows)?);
                    return Ok(());
                }
                if rows.is_empty() {
                    println!("merge queue is empty");
                }
                for m in rows {
                    println!("#{:<3} {:<12} {:<24} → {:<10} {}  {}", m.id, m.status, m.branch, m.into, m.agent, m.log.lines().last().unwrap_or("").chars().take(80).collect::<String>());
                }
                return Ok(());
            }
            if run {
                while let Some(m) = merge::tick(&t, &db)? {
                    println!("#{} {} → {}: {}", m.id, m.branch, m.into, m.status);
                }
                return Ok(());
            }
            let target = target.unwrap();
            // An agent name resolves to its worktree branch; otherwise it is a branch in cwd's repo.
            let (dir, branch, agent) = match t.pane_by_name(&target) {
                Ok(p) if p.worktree.is_some() => {
                    let wt = std::path::PathBuf::from(p.worktree.clone().unwrap());
                    let b = hubcodes_core::git::changes(&wt).map(|c| c.branch).unwrap_or_default();
                    (wt, b, p.name)
                }
                _ => (cwd.clone().map(std::path::PathBuf::from).unwrap_or(std::env::current_dir()?), target.clone(), std::env::var("HUBCODES_AGENT").unwrap_or_else(|_| target.clone())),
            };
            let rc = hubcodes_core::repo::load(&dir);
            let into = into.unwrap_or(rc.merge.into);
            let m = merge::enqueue(&db, &dir, &branch, &into, &agent)?;
            println!("queued merge #{}: {} → {} (hubcodes watch or the app processes it; `hubcodes merge --run` now)", m.id, m.branch, m.into);
            Ok(())
        }
        Cmd::Restore { fresh } => {
            let t = session(&cli.session)?;
            let def = hubcodes_core::sessiondef::load(&t.session).ok_or_else(|| anyhow!("nothing saved for session {}", t.session))?;
            let exe = hubcodes_core::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into());
            let r = hubcodes_core::sessiondef::restore(&t, &reg, &def, &exe, None, !fresh);
            println!("started: {}", r.started.join(", "));
            if !r.skipped.is_empty() {
                println!("already running: {}", r.skipped.join(", "));
            }
            finish_with(&r.errors)
        }
        Cmd::Provider { cmd } => {
            use hubcodes_core::providers as pv;
            match cmd {
                ProviderCmd::List => {
                    for p in &reg.providers {
                        println!("{:<10} {:<24} hooks {:<7} resume {:<14} {}", p.name, p.command, p.hook_kind.as_deref().unwrap_or("-"), p.resume_flag.as_deref().unwrap_or("-"), if pv::is_builtin(&p.name) { "built-in" } else { "custom" });
                    }
                    println!("provider files: {}", pv::providers_dir().display());
                }
                ProviderCmd::Export { name, out } => {
                    let p = reg.get(&name).ok_or_else(|| anyhow!("no provider {name}"))?;
                    let text = pv::export_provider(p)?;
                    match out {
                        Some(f) => {
                            std::fs::write(&f, text)?;
                            println!("wrote {f}");
                        }
                        None => print!("{text}"),
                    }
                }
                ProviderCmd::Import { file } => {
                    let text = if file.starts_with("http://") || file.starts_with("https://") {
                        let agent: ureq::Agent = ureq::Agent::new_with_defaults();
                        agent.get(&file).call().map_err(|e| anyhow!("{file}: {e}"))?.body_mut().read_to_string()?
                    } else {
                        std::fs::read_to_string(&file)?
                    };
                    for p in pv::import_provider_text(&text)? {
                        println!("installed {}", p.display());
                    }
                }
            }
            Ok(())
        }
        Cmd::Usage { json } => {
            let t = session(&cli.session)?;
            let db = hubcodes_core::db::Db::open(&t.session)?;
            let rows = db.all_usage()?;
            if json {
                let agents: serde_json::Map<String, serde_json::Value> = rows.iter().map(|(a, u)| (a.clone(), serde_json::to_value(u).unwrap_or_default())).collect();
                println!("{}", serde_json::json!({ "agents": agents, "limits": { "perAgent": reg.budget.max_tokens_per_agent, "perLoop": reg.budget.max_tokens_per_loop } }));
                return Ok(());
            }
            if rows.is_empty() {
                println!("no usage recorded yet (Claude Code agents report it through hooks after each turn)");
            }
            for (agent, u) in rows {
                println!("{:<16} in {:>9}  out {:>8}  cache read {:>9}  turns {:>4}", agent, u.input_tokens, u.output_tokens, u.cache_read_tokens, u.turns);
            }
            let b = &reg.budget;
            println!("limits: per agent {} · per loop {}", if b.max_tokens_per_agent > 0 { b.max_tokens_per_agent.to_string() } else { "none".into() }, if b.max_tokens_per_loop > 0 { b.max_tokens_per_loop.to_string() } else { "none".into() });
            Ok(())
        }
        Cmd::Watch { serve, once } => {
            let t = session(&cli.session)?;
            if once {
                if !hubcodes_core::daemon::try_tend(&t.session, "watch") {
                    return Err(anyhow!("another hubcodes (the app or hubcodesd) is tending session {}", t.session));
                }
                let mut mem = hubcodes_core::tend::Memory::default();
                hubcodes_core::remote::refresh_cache(&t.session, &reg.peers);
                let agents = hubcodes_core::tend::agent_states(&t);
                for ev in hubcodes_core::tend::tend(&t, &reg, &agents, &mut mem) {
                    println!("{}", ev.text);
                }
                hubcodes_core::daemon::release(&t.session);
                return Ok(());
            }
            if serve {
                let (t2, reg2, cfg) = (t.clone(), reg.clone(), reg.serve.clone());
                let (t3, reg3) = (t.clone(), reg.clone());
                let sink: hubcodes_core::serve::WebhookSink = std::sync::Arc::new(move |name: &str, body: &str| {
                    for tr in reg3.triggers.iter().filter(|x| x.enabled) {
                        if hubcodes_core::triggers::kind(tr) == Some(hubcodes_core::triggers::Kind::Webhook(name.to_string())) {
                            let action = hubcodes_core::triggers::fill(&tr.action, &[("body", body.trim())]);
                            let ev = hubcodes_core::tend::run_action(&t3, &reg3, tr, &action);
                            println!("{}", ev.text);
                        }
                    }
                });
                std::thread::spawn(move || {
                    if let Err(e) = hubcodes_core::serve::run(t2, reg2, cfg, Some(sink)) {
                        eprintln!("serve: {e:#}");
                    }
                });
            }
            let mut mem = hubcodes_core::tend::Memory::default();
            println!("watching session {} (Ctrl-C stops)", t.session);
            let mut passive_said = false;
            loop {
                if !hubcodes_core::daemon::try_tend(&t.session, "watch") {
                    if !passive_said {
                        println!("another hubcodes (the app or hubcodesd) is tending this session; standing by");
                        passive_said = true;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }
                passive_said = false;
                hubcodes_core::tend::refresh_peers_if_due(&t.session, &reg);
                let agents = hubcodes_core::tend::agent_states(&t);
                for ev in hubcodes_core::tend::tend(&t, &reg, &agents, &mut mem) {
                    println!("{}", ev.text);
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
        Cmd::Serve { web } => {
            let t = session(&cli.session)?;
            let mut cfg = reg.serve.clone();
            if let Some(w) = web {
                cfg.web_root = w;
            }
            hubcodes_core::serve::run(t, reg.clone(), cfg, None)
        }
        Cmd::Policy { cmd } => {
            let reg = reg.for_dir(&std::env::current_dir()?);
            match cmd {
                PolicyCmd::Show { json } => {
                    let p = &reg.policy;
                    if json {
                        println!("{}", serde_json::to_string(p)?);
                        return Ok(());
                    }
                    println!("mode: {}", p.mode);
                    println!("allow: {}", if p.allow.is_empty() { "-".into() } else { p.allow.join(" | ") });
                    println!("deny: {}", if p.deny.is_empty() { "-".into() } else { p.deny.join(" | ") });
                    println!("allow_tools: {}", if p.allow_tools.is_empty() { "-".into() } else { p.allow_tools.join(", ") });
                    println!("deny_paths: {}", if p.deny_paths.is_empty() { "-".into() } else { p.deny_paths.join(" | ") });
                }
                PolicyCmd::Test { command } => {
                    let cmd = command.join(" ");
                    let d = hubcodes_core::policy::decide_command(&reg.policy, &cmd);
                    match d {
                        hubcodes_core::policy::Decision::Allow(r) => println!("allow  ({r})"),
                        hubcodes_core::policy::Decision::Deny(r) => println!("deny   ({r})"),
                        hubcodes_core::policy::Decision::Ask => println!("ask    (no rule matches{})", if reg.policy.is_on() { "" } else { "; policy is off" }),
                    }
                }
            }
            Ok(())
        }
        Cmd::Playbook { cmd } => {
            use hubcodes_core::playbook;
            let t = session(&cli.session)?;
            let me = std::env::var("HUBCODES_AGENT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("human:{}", std::env::var("USER").unwrap_or_default()));
            match cmd {
                PlaybookCmd::List => {
                    let reg = reg.for_dir(&std::env::current_dir()?);
                    for p in &reg.playbooks {
                        println!("{:<14} {}", p.name, p.description);
                        for (i, st) in p.steps.iter().enumerate() {
                            println!("  {}. {:<10} {:<22} gate {}{}", i + 1, st.name, if !st.action.is_empty() { format!("action: {}", st.action) } else if !st.agent.is_empty() { st.agent.clone() } else if st.role.is_empty() { "any idle agent".into() } else { format!("role {}", st.role) }, if st.gate.is_empty() { "none" } else { &st.gate }, if st.retries > 0 { format!(", {} retries", st.retries) } else { String::new() });
                        }
                    }
                }
                PlaybookCmd::Start { name, goal, cwd } => {
                    let cwd = cwd.or_else(|| std::env::current_dir().ok().map(|p| p.display().to_string()));
                    let r = playbook::start(&t, &reg, &name, &goal.join(" "), &me, cwd.as_deref())?;
                    println!("run #{} {} started: step 1 {}", r.id, r.playbook, r.log.lines().last().unwrap_or(""));
                }
                PlaybookCmd::Runs { all, json } => {
                    let db = hubcodes_core::db::Db::open(&t.session)?;
                    let runs = playbook::list(&db, !all)?;
                    if json {
                        println!("{}", serde_json::to_string(&runs)?);
                    } else if runs.is_empty() {
                        println!("no runs");
                    } else {
                        for r in runs {
                            println!("#{:<4} {:<9} {:<12} step {:<2} task #{:<4} {}", r.id, r.status, r.playbook, r.step + 1, r.task, r.goal.chars().take(60).collect::<String>());
                        }
                    }
                }
                PlaybookCmd::Show { id } => {
                    let db = hubcodes_core::db::Db::open(&t.session)?;
                    let r = playbook::get(&db, id)?.ok_or_else(|| anyhow!("no run #{id}"))?;
                    println!("run #{} {} [{}] step {} task #{}\ngoal: {}\n\n{}", r.id, r.playbook, r.status, r.step + 1, r.task, r.goal, r.log);
                }
                PlaybookCmd::Approve { id } => {
                    let r = playbook::approve(&t, &reg, id, &me)?;
                    println!("run #{} continues: {}", r.id, r.log.lines().last().unwrap_or(""));
                }
                PlaybookCmd::Abort { id } => {
                    let r = playbook::abort(&t, id, &me)?;
                    println!("run #{} aborted", r.id);
                }
            }
            Ok(())
        }
        Cmd::Review { action, key, branch } => match action.as_str() {
            "checkout" if branch => {
                let (dir, note) = hubcodes_core::reviews::checkout_branch(&key)?;
                if let Some(n) = note {
                    eprintln!("{n}");
                }
                println!("{}", dir.display());
                Ok(())
            }
            "checkout" => {
                let dir = hubcodes_core::reviews::checkout(&key)?;
                println!("{}", dir.display());
                Ok(())
            }
            "clean" => {
                let gone = if branch { hubcodes_core::reviews::cleanup_branch(&key)? } else { hubcodes_core::reviews::cleanup(&key)? };
                println!("{}", if gone { "removed" } else { "nothing to remove" });
                Ok(())
            }
            other => Err(anyhow::anyhow!("unknown review action '{other}' (checkout | clean)")),
        },
        Cmd::Secret { action, name } => {
            use hubcodes_core::secrets;
            match action.as_str() {
                "set" => {
                    // From stdin, so the value is not in the shell history or `ps`.
                    let mut value = String::new();
                    std::io::stdin().read_line(&mut value)?;
                    let r = secrets::set(&name, value.trim())?;
                    println!("stored; use {r} in hubcodes.toml");
                    Ok(())
                }
                "rm" => {
                    secrets::delete(&name)?;
                    println!("removed {name}");
                    Ok(())
                }
                "check" => match secrets::get(&name) {
                    Some(v) => {
                        println!("{name}: set ({} characters)", v.chars().count());
                        Ok(())
                    }
                    None => Err(anyhow!("{name} is not in the secret store")),
                },
                other => Err(anyhow!("unknown action '{other}': set, rm or check")),
            }
        }
        Cmd::Tracker { cmd } => {
            use hubcodes_core::tracker;
            match cmd {
                TrackerCmd::List { json } => {
                    if json {
                        let list: Vec<_> = reg.trackers.iter().map(tracker::redacted).collect();
                        println!("{}", serde_json::to_string(&list)?);
                        return Ok(());
                    }
                    if reg.trackers.is_empty() {
                        println!("no trackers ([[tracker]] in hubcodes.toml, or Settings → Trackers)");
                    }
                    for tr in &reg.trackers {
                        println!("{:<12} {:<8} {}{}  query: {}", tr.name, tr.kind, if tr.enabled { "" } else { "(off) " }, tr.url, tr.query);
                    }
                }
                TrackerCmd::Test { name } => {
                    for tr in reg.trackers.iter().filter(|x| name.as_deref().map(|n| n == x.name).unwrap_or(true)) {
                        let c = tracker::check(tr);
                        println!("{:<12} {} {} ({} ms)", tr.name, if c.ok { "ok " } else { "FAIL" }, c.detail, c.ms);
                        for i in c.issues {
                            println!("    {:<12} {}", i.key, i.title);
                        }
                    }
                }
                TrackerCmd::Pull { name } => {
                    let t = session(&cli.session)?;
                    let db = hubcodes_core::db::Db::open(&t.session)?;
                    for tr in reg.trackers.iter().filter(|x| x.enabled && name.as_deref().map(|n| n == x.name).unwrap_or(true)) {
                        for ev in tracker::pull(&db, tr) {
                            println!("{}", ev.text);
                        }
                    }
                }
            }
            Ok(())
        }
        Cmd::Approve { agent } => {
            hubcodes_core::remote::check_allowed(&reg.peers, "approve")?;
            let t = session(&cli.session)?;
            relay::answer(&t, &reg, &agent, true, &format!("human:{}", std::env::var("USER").unwrap_or_default()))?;
            println!("approved {agent}");
            Ok(())
        }
        Cmd::Deny { agent } => {
            hubcodes_core::remote::check_allowed(&reg.peers, "approve")?;
            let t = session(&cli.session)?;
            relay::answer(&t, &reg, &agent, false, &format!("human:{}", std::env::var("USER").unwrap_or_default()))?;
            println!("denied {agent}");
            Ok(())
        }
        Cmd::Log { limit, grep, agent, task, kind, follow, json } => {
            let t = session(&cli.session)?;
            let db = hubcodes_core::db::Db::open(&t.session)?;
            let tag = task.map(|id| format!("#{id}"));
            let names_task = |text: &str| match &tag {
                None => true,
                Some(tag) => text.match_indices(tag.as_str()).any(|(i, _)| text[i + tag.len()..].chars().next().map(|c| !c.is_ascii_digit()).unwrap_or(true)),
            };
            let keep = |m: &hubcodes_core::db::MessageRow| {
                agent.as_deref().map(|a| m.from == a || m.to == a).unwrap_or(true) && kind.as_deref().map(|k| m.kind == k).unwrap_or(true) && names_task(&m.text)
            };
            let line = |m: &hubcodes_core::db::MessageRow| {
                let k = if m.kind == "message" { String::new() } else { format!("[{}] ", m.kind) };
                format!("{}  {:<18} → {:<18} {k}{}", chrono_like(m.at), m.from, m.to, m.text)
            };
            // Filters narrow a wider read so `-n` still means "this many shown".
            let wide = if agent.is_some() || tag.is_some() || kind.is_some() { limit.max(50) * 20 } else { limit };
            let rows: Vec<_> = db.messages(wide, grep.as_deref())?.into_iter().filter(keep).take(limit).collect();
            if json && !follow {
                println!("{}", serde_json::to_string(&rows)?);
                return Ok(());
            }
            for m in rows.iter().rev() {
                if json {
                    println!("{}", serde_json::to_string(m)?);
                } else {
                    println!("{}", line(m));
                }
            }
            if follow {
                let mut last = rows.first().map(|m| m.seq).unwrap_or(0);
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(700));
                    let fresh: Vec<_> = db.messages(200, grep.as_deref())?.into_iter().filter(|m| m.seq > last).filter(keep).collect();
                    for m in fresh.iter().rev() {
                        if json {
                            println!("{}", serde_json::to_string(m)?);
                        } else {
                            println!("{}", line(m));
                        }
                        last = last.max(m.seq);
                    }
                }
            }
            Ok(())
        }
        Cmd::Pending { action, id, json } => {
            if let Some(p) = hubcodes_core::remote::caller(&reg.peers) {
                return Err(anyhow!("{} cannot decide approvals on {}", p.name, reg.sharing.name));
            }
            let t = session(&cli.session)?;
            match action.as_deref() {
                None | Some("list") => {
                    let open = hubcodes_core::db::Db::open(&t.session)?.pending_open()?;
                    if json {
                        println!("{}", serde_json::to_string(&open)?);
                    } else if open.is_empty() {
                        println!("nothing waiting for approval");
                    } else {
                        for p in open {
                            println!("#{:<4} {} → {}: {}", p.id, p.by, p.to, p.text);
                        }
                    }
                }
                Some(a @ ("approve" | "deny")) => {
                    let id = id.ok_or_else(|| anyhow!("pending {a} <id>"))?;
                    let me = format!("human:{}", std::env::var("USER").unwrap_or_else(|_| "me".into()));
                    println!("{}", hubcodes_core::serve::decide_pending(&t, &reg, id, a == "approve", &me)?);
                }
                Some(other) => return Err(anyhow!("pending takes approve <id> or deny <id>, not '{other}'")),
            }
            Ok(())
        }
        Cmd::Room { cmd } => {
            use hubcodes_core::rooms;
            let t = session(&cli.session)?;
            let me = std::env::var("HUBCODES_AGENT").unwrap_or_else(|_| format!("human:{}", std::env::var("USER").unwrap_or_else(|_| "me".into())));
            // Creating and changing rooms is for this device's owner; these
            // are not on the federation verb list, and refused over SSH too.
            let local_only = || -> Result<()> {
                match hubcodes_core::remote::caller(&reg.peers) {
                    Some(p) => Err(anyhow!("{} cannot change rooms on {}; only its owner can", p.name, reg.sharing.name)),
                    None => Ok(()),
                }
            };
            match cmd {
                RoomCmd::Create { name, members } => {
                    local_only()?;
                    rooms::create(&t, &name, &members, &me)?;
                    println!("room {name} on {} with {}", reg.sharing.name, if members.is_empty() { "no members yet".into() } else { members.join(", ") });
                }
                RoomCmd::Add { name, members } => {
                    local_only()?;
                    rooms::add(&t, &name, &members)?;
                    println!("added to {name}: {}", members.join(", "));
                }
                RoomCmd::Rm { name, members } => {
                    local_only()?;
                    rooms::remove(&t, &name, &members)?;
                    println!("removed from {name}: {}", members.join(", "));
                }
                RoomCmd::Delete { name } => {
                    local_only()?;
                    rooms::delete(&t, &name)?;
                    println!("deleted room {name}");
                }
                RoomCmd::List { json } => {
                    let list = rooms::list(&t)?;
                    if json {
                        println!("{}", serde_json::to_string(&list)?);
                    } else if list.is_empty() {
                        println!("no rooms on {} (`hubcodes room create <name> <members…>`)", reg.sharing.name);
                    } else {
                        for r in list {
                            println!("{:<16} {}", r.name, r.members.join(", "));
                        }
                    }
                }
                RoomCmd::Show { name, limit, json } => {
                    let log = rooms::log(&t, &name, limit)?;
                    if json {
                        println!("{}", serde_json::to_string(&log)?);
                    } else {
                        for m in log {
                            println!("{}  {}: {}", chrono_like(m.at), m.from, m.text);
                        }
                    }
                }
                RoomCmd::Post { room, text, from, r#as } => {
                    let text = text.join(" ");
                    let from = from.unwrap_or_else(|| me.clone());
                    let (home, name) = rooms::split(&room, &reg.peers);
                    if let Some(device) = home.filter(|h| *h != reg.sharing.name) {
                        // The room lives on another device: hand it the post.
                        let peer = reg.peers.iter().find(|p| p.name == device).ok_or_else(|| anyhow!("unknown device '{device}'"))?;
                        let who = format!("{}/{from}", reg.sharing.name);
                        let args = ["room", "post", name.as_str(), text.as_str(), "--as", who.as_str()];
                        match hubcodes_core::remote::run(peer, &args, std::time::Duration::from_secs(20)) {
                            Ok(out) => print!("{out}"),
                            Err(e) => {
                                let q = hubcodes_core::remote::queue(&t.session, &device, &args, &format!("{e:#}"))?;
                                println!("{device} is unreachable ({e:#}); the post is queued (#{}) and goes out when {device} is back.", q.id);
                            }
                        }
                        if let Ok(db) = hubcodes_core::db::Db::open(&t.session) {
                            let _ = db.add_event(&from, "room", &format!("posted to {device}:{name}"));
                        }
                        return Ok(());
                    }
                    let gate = hubcodes_core::remote::send_gate(&reg.peers)?;
                    let sender = match r#as.as_deref() {
                        Some(a) => {
                            let device = a.split_once('/').map(|(d, _)| d).ok_or_else(|| anyhow!("--as takes device/agent"))?;
                            hubcodes_core::remote::verify_as(&reg.peers, device)?;
                            a.to_string()
                        }
                        None => from,
                    };
                    if let hubcodes_core::remote::SendGate::Approve(device) = gate {
                        let by = hubcodes_core::remote::attribute(&device, &sender);
                        let id = hubcodes_core::db::Db::open(&t.session)?.pending_add(&by, &format!("room:{name}"), &hubcodes_core::redact::redact(&text))?;
                        println!("waiting for approval on {} (#{id}): posted to {name} once the owner approves", reg.sharing.name);
                        return Ok(());
                    }
                    let n = rooms::post(&t, &reg, &name, &sender, &text)?;
                    println!("posted to {name} on {} ({n} members)", reg.sharing.name);
                }
            }
            Ok(())
        }
        Cmd::Peer { cmd } => {
            match cmd {
            PeerCmd::Add { name, host, user, session: psession, hubcodes, max_agents, allow } => {
                let name = name.trim().to_string();
                if name.is_empty() || name.contains('/') || name.contains(':') {
                    return Err(anyhow!("a device name is one word without / or :"));
                }
                if reg.peers.iter().any(|p| p.name == name) {
                    return Err(anyhow!("there is already a device named {name}; `hubcodes peer rm {name}` first"));
                }
                for a in &allow {
                    if !hubcodes_core::remote::ALLOW_ALL.contains(&a.as_str()) && a != "all" {
                        return Err(anyhow!("--allow takes {} (or all), not '{a}'", hubcodes_core::remote::ALLOW_ALL.join(", ")));
                    }
                }
                let mut peers = reg.peers.clone();
                peers.push(hubcodes_core::remote::Peer { name: name.clone(), host: host.trim().to_string(), user: user.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()), session: psession, hubcodes, max_agents, allow, transport: String::new(), port: None });
                let added = peers.last().cloned();
                let path = save_peers(&reg, peers)?;
                println!("added {name} → {host} ({})", path.display());
                if let Some(peer) = added {
                    match hubcodes_core::remote::check(&peer) {
                        Ok(v) => println!("reached {name}: {v}"),
                        Err(e) => println!("cannot reach {name} yet: {e:#}"),
                    }
                }
                let me = hubcodes_core::remote::this_device();
                println!("\nOn {name}, add this device back so its agents can answer:\n  hubcodes peer add {} {} --user {}", reg.sharing.name, if me.host.is_empty() { "<this device's host>" } else { &me.host }, me.user);
                println!("`hubcodes peer me` shows what else {name} needs from here (Remote Login or sshd, this device's key).");
            }
            PeerCmd::Me { json, create_key } => {
                if create_key {
                    hubcodes_core::remote::create_key()?;
                }
                let me = hubcodes_core::remote::this_device();
                if json {
                    println!("{}", serde_json::to_string(&me)?);
                    return Ok(());
                }
                println!("This device is \"{}\". On another device, add it with:", reg.sharing.name);
                println!("  hubcodes peer add {} {} --user {}", reg.sharing.name, if me.host.is_empty() { "<host>" } else { &me.host }, me.user);
                println!("  (Settings → Devices → Add device: name {}, SSH host {}, user {})", reg.sharing.name, me.host, me.user);
                if !me.tailscale {
                    println!("  Tailscale is not running here; {} works on the same network only.", me.host);
                }
                println!();
                if me.remote_login {
                    println!("[ok  ] Remote Login is on, so other devices can SSH in.");
                } else {
                    println!("[todo] Remote Login is off: System Settings → General → Sharing → Remote Login, so other devices can SSH in.");
                }
                match &me.authorize_command {
                    Some(cmd) => {
                        println!("[todo] So this device can log in to the other device without a password, run this once in Terminal THERE:");
                        println!("  {cmd}");
                        println!("       (or from here: ssh-copy-id <user>@<that device>, which asks for its password once)");
                    }
                    None => println!("[todo] This device has no SSH key yet: `hubcodes peer me --create-key` makes one."),
                }
            }
            PeerCmd::Rm { name } => {
                if !reg.peers.iter().any(|p| p.name == name) {
                    return Err(anyhow!("no device named {name}"));
                }
                let peers = reg.peers.iter().filter(|p| p.name != name).cloned().collect();
                let path = save_peers(&reg, peers)?;
                println!("removed {name} ({})", path.display());
            }
            PeerCmd::Check { name, json } => return peers_report(&cli.session, &reg, name.as_deref(), json),
            PeerCmd::Queue { retry, json } => {
                let t = session(&cli.session)?;
                let delivered: Vec<String> = if retry {
                    hubcodes_core::remote::flush_queue(&t.session, &reg.peers)
                        .into_iter()
                        .map(|f| match f {
                            hubcodes_core::remote::Flushed::Delivered(c) => format!("delivered to {}: hubcodes {}", c.peer, c.args.join(" ")),
                            hubcodes_core::remote::Flushed::Expired(c) => format!("dropped (too old) for {}: hubcodes {}", c.peer, c.args.join(" ")),
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let q = hubcodes_core::remote::queued(&t.session);
                if json {
                    println!("{}", serde_json::json!({ "queued": q, "retried": delivered }));
                    return Ok(());
                }
                for d in &delivered {
                    println!("{d}");
                }
                if q.is_empty() {
                    println!("nothing queued");
                }
                for c in q {
                    println!("#{:<3} {:<10} {:<3} tries  hubcodes {}  ({})", c.id, c.peer, c.attempts, c.args.join(" "), c.error);
                }
            }
            }
            Ok(())
        }
        Cmd::Board { cmd } => {
            let next = match cmd {
                BoardCmd::Columns { json } => {
                    if json {
                        println!("{}", serde_json::to_string(&reg.board.columns)?);
                    } else {
                        for c in &reg.board.columns {
                            println!("{:<12} {:<16} {}{}", c.id, c.label, if c.wip > 0 { format!("wip {}", c.wip) } else { String::new() }, if hubcodes_core::board::is_builtin(&c.id) { "" } else { "  (yours)" });
                        }
                    }
                    return Ok(());
                }
                BoardCmd::Add { label, before } => reg.board.add_column(&label, before.as_deref())?,
                BoardCmd::Rm { id } => {
                    let next = reg.board.remove_column(&id)?;
                    if let Ok(t) = session(&cli.session) {
                        if let Ok(db) = hubcodes_core::db::Db::open(&t.session) {
                            let n = hubcodes_core::tasks::fold_column(&db, &id, "todo").unwrap_or(0);
                            if n > 0 {
                                println!("{n} task{} moved to To do", if n == 1 { "" } else { "s" });
                            }
                        }
                    }
                    next
                }
                BoardCmd::Rename { id, label } => reg.board.edit_column(&id, Some(&label), None)?,
                BoardCmd::Wip { id, limit } => reg.board.edit_column(&id, None, Some(limit))?,
            };
            let mut cfg = reg.clone();
            cfg.board = next;
            let path = cfg.save()?;
            println!("columns: {}  ({})", cfg.board.columns.iter().map(|c| if c.wip > 0 { format!("{} [{}]", c.id, c.wip) } else { c.id.clone() }).collect::<Vec<_>>().join(" → "), path.display());
            Ok(())
        }
        Cmd::Completions { shell } => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "hubcodes", &mut std::io::stdout());
            Ok(())
        }
        Cmd::Peers { json } => peers_report(&cli.session, &reg, None, json),
        Cmd::Guard { tool, input } => {
            let agent = std::env::var("HUBCODES_AGENT").ok().filter(|s| !s.is_empty());
            let sess = std::env::var("HUBCODES_SESSION").ok().filter(|s| !s.is_empty()).or(cli.session.clone()).unwrap_or_else(|| tmux::DEFAULT_SESSION.into());
            let t = Tmux::named(&sess);
            let input: serde_json::Value = serde_json::from_str(&input).unwrap_or(serde_json::Value::Null);
            let reason = agent.and_then(|a| guard_reason(&reg, &t, &a, &tool, &input));
            if let (Some(r), Some(a)) = (&reason, std::env::var("HUBCODES_AGENT").ok()) {
                if let Ok(db) = hubcodes_core::db::Db::open(&sess) {
                    let _ = db.add_event(&a, "guard", &format!("refused {tool} ({})", r.chars().take(160).collect::<String>()));
                }
            }
            println!("{}", hubcodes_core::guard::verdict(reason));
            Ok(())
        }
        Cmd::Restart { names, fresh } => {
            hubcodes_core::remote::check_allowed(&reg.peers, "restart")?;
            let t = session(&cli.session)?;
            if names.is_empty() {
                return Err(anyhow!("which agents? hubcodes restart <name>..."));
            }
            let exe = hubcodes_core::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into());
            let r = hubcodes_core::sessiondef::restart(&t, &reg, &names, &exe, None, !fresh)?;
            println!("restarted: {}", r.started.join(", "));
            finish_with(&r.errors)
        }
        Cmd::Hire { role, reason } => {
            hubcodes_core::remote::check_allowed(&reg.peers, "spawn")?;
            let t = session(&cli.session)?;
            let exe = hubcodes_core::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into());
            let by = std::env::var("HUBCODES_AGENT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "human".into());
            let name = hubcodes_core::autoteam::hire(&t, &reg, &role, &reason, &by, &exe)?;
            println!("started {name} ({role}); it takes {role} tasks from the board and is removed when idle with nothing waiting");
            Ok(())
        }
        Cmd::Release { name } => {
            hubcodes_core::remote::check_allowed(&reg.peers, "kill")?;
            let t = session(&cli.session)?;
            let r = hubcodes_core::autoteam::release(&t, &reg, &name)?;
            match (r.kept_branch, r.handed_to_task) {
                (Some(b), Some(id)) => println!("removed {name}; its unfinished work stays on {b} and went to task #{id}"),
                _ => println!("removed {name}"),
            }
            Ok(())
        }
        Cmd::Switch { name, provider, model } => {
            hubcodes_core::remote::check_allowed(&reg.peers, "switch")?;
            let t = session(&cli.session)?;
            let provider = match provider {
                Some(p) => p,
                None => t.pane_by_name(&name)?.provider.ok_or_else(|| anyhow!("{name} has no provider; pass -p"))?,
            };
            let exe = hubcodes_core::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into());
            let s = hubcodes_core::sessiondef::switch(&t, &reg, &name, &provider, model.as_deref(), &exe, None)?;
            println!("{name} now runs on {provider}{} ({})", model.as_deref().filter(|m| !m.is_empty()).map(|m| format!(" · {m}")).unwrap_or_default(), if s.kept == "conversation" { "conversation continued" } else { "started with a hand-over note" });
            finish_with(&s.restored.errors)
        }
        Cmd::Kill { name } => {
            hubcodes_core::remote::check_allowed(&reg.peers, "kill")?;
            let t = session(&cli.session)?;
            let w = t.pane_by_name(&name)?;
            t.kill(&w.id)?;
            let _ = t.remove_edges_of(&name);
            let _ = hubcodes_core::sessiondef::forget(&t.session, &name);
            println!("killed {name} ({})", w.id);
            Ok(())
        }
        Cmd::Private { agent, state } => {
            let t = session(&cli.session)?;
            let on = state == "on";
            t.set_private(&agent, on)?;
            println!("agent {agent} is now {}", if on { "private" } else { "public" });
            Ok(())
        }
        Cmd::Open { agent } => {
            let t = session(&cli.session)?;
            let w = t.pane_by_name(&agent)?;
            t.open(&w.id)?;
            println!("opened {agent} ({})", w.id);
            Ok(())
        }
        Cmd::Tile => {
            let t = session(&cli.session)?;
            let canvas = t.canvas_pane()?;
            let mut n = 0;
            for w in t.panes()? {
                if canvas.as_deref() == Some(w.id.as_str()) {
                    continue;
                }
                t.open(&w.id)?;
                n += 1;
            }
            println!("tiled {n} agents");
            Ok(())
        }
        Cmd::Back => {
            let t = session(&cli.session)?;
            let n = t.back()?;
            println!("back to canvas ({n} agents closed)");
            Ok(())
        }
    }
}

/// `HH:MM:SS` local time without pulling in chrono.
/// Print a batch's errors to stderr and end with exit code 1 when there were
/// any, so scripts and agents can tell a partial result from a clean one.
fn finish_with(errors: &[String]) -> Result<()> {
    for e in errors {
        eprintln!("error: {e}");
    }
    if errors.is_empty() {
        Ok(())
    } else {
        std::process::exit(1)
    }
}

/// `hubcodes.toml` with `peers` replaced, written and reported.
fn save_peers(reg: &hubcodes_core::providers::Registry, peers: Vec<hubcodes_core::remote::Peer>) -> Result<std::path::PathBuf> {
    let mut next = reg.clone();
    next.peers = peers;
    next.save()
}

/// `hubcodes peers` / `peer check`: reachability, version, pairing, agents.
fn peers_report(session_arg: &Option<String>, reg: &hubcodes_core::providers::Registry, only: Option<&str>, json: bool) -> Result<()> {
    use hubcodes_core::remote;
    let peers: Vec<&remote::Peer> = reg.peers.iter().filter(|p| only.map(|o| p.name == o).unwrap_or(true)).collect();
    if let Some(o) = only {
        if peers.is_empty() {
            return Err(anyhow!("no device named {o}"));
        }
    }
    if peers.is_empty() && !json {
        println!("no devices; `hubcodes peer add <name> <host>` (this device is \"{}\")", reg.sharing.name);
    }
    let me = env!("CARGO_PKG_VERSION");
    let mut rows = Vec::new();
    let mut problems = 0usize;
    for p in peers {
        let (ok, version, agents, issues, error) = match remote::snapshot(p) {
            Ok(snap) => {
                let issues = remote::pairing_issues(&snap, p, &reg.sharing.name, me);
                (true, snap.version.clone(), snap.agents.len(), issues, None)
            }
            Err(e) => (false, None, 0, Vec::new(), Some(format!("{e:#}"))),
        };
        if !ok || !issues.is_empty() {
            problems += 1;
        }
        if !json {
            match &error {
                Some(e) => println!("{:<10} {:<24} FAIL {e}", p.name, p.host),
                None => println!("{:<10} {:<24} ok   hubcodes {}  {agents} agent{}{}", p.name, p.host, version.clone().unwrap_or_else(|| "<0.1.8".into()), if agents == 1 { "" } else { "s" }, if issues.is_empty() { "  paired" } else { "" }),
            }
            for i in &issues {
                println!("           ! {i}");
            }
        }
        rows.push(serde_json::json!({ "name": p.name, "host": p.host, "ok": ok, "version": version, "agents": agents, "issues": issues, "error": error, "allow": if p.allow.is_empty() { remote::ALLOW_DEFAULT.iter().map(|s| s.to_string()).collect::<Vec<_>>() } else { p.allow.clone() }, "maxAgents": p.max_agents }));
    }
    if json {
        println!("{}", serde_json::json!({ "self": reg.sharing.name, "version": me, "devices": rows }));
    }
    // Refresh what briefs, the guard and `task` commands know about the other devices.
    if let Ok(t) = session(session_arg) {
        remote::refresh_cache(&t.session, &reg.peers);
    }
    if problems > 0 && !json {
        std::process::exit(1);
    }
    Ok(())
}

fn chrono_like(unix: i64) -> String {
    let out = std::process::Command::new("date").args(["-r", &unix.to_string(), "+%H:%M:%S"]).output();
    out.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| unix.to_string())
}

fn session(explicit: &Option<String>) -> Result<Tmux> {
    if !tmux::available() {
        return Err(anyhow!("tmux not found on PATH (brew install tmux); or run `hubcodes --demo`"));
    }
    if let Some(env) = std::env::var("HUBCODES_SESSION").ok().filter(|s| !s.is_empty()) {
        if explicit.is_none() {
            return Ok(Tmux::named(&env));
        }
    }
    match explicit {
        Some(s) => Ok(Tmux::named(s)),
        None => Tmux::current(),
    }
}

/// `hubcodes` with no arguments: inside tmux, open the canvas here; outside,
/// create-or-attach the `hubcodes` session with the canvas in window 0.
fn launch(explicit: &Option<String>, reg: Registry) -> Result<()> {
    if !tmux::available() {
        return Err(anyhow!("tmux not found on PATH (brew install tmux); or run `hubcodes --demo`"));
    }
    if tmux::inside_tmux() {
        let t = session(explicit)?;
        return run_tui(App::tmux(reg, t)?);
    }
    let name = explicit.clone().unwrap_or_else(|| tmux::DEFAULT_SESSION.to_string());
    let exe = std::env::current_exe().context("current exe")?;
    let canvas_cmd = format!("{} --session {} canvas", exe.display(), name);
    let status = std::process::Command::new("tmux")
        .args(["new-session", "-A", "-s", &name, "-n", tmux::CANVAS_WINDOW, &canvas_cmd])
        .status()
        .context("tmux new-session")?;
    if !status.success() {
        return Err(anyhow!("tmux exited with {status}"));
    }
    Ok(())
}

fn run_tui(mut app: App) -> Result<()> {
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let result = app.run(&mut terminal);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

/// The orchestrator guard for one tool call of `agent`: the pane's current
/// role decides, so a role changed on the card applies at once.
fn guard_reason(reg: &hubcodes_core::providers::Registry, t: &Tmux, agent: &str, tool: &str, input: &serde_json::Value) -> Option<String> {
    // A teammate that owes its lead a check-in on a person's request.
    if let Some(r) = hubcodes_core::checkin::reason(t, reg, agent, tool, input) {
        return Some(r);
    }
    if !reg.workspace.guard_orchestrator {
        return None;
    }
    let role = t.pane_by_name(agent).ok().and_then(|p| p.role)?;
    let team = hubcodes_core::prompts::team_roster(&t.session);
    hubcodes_core::guard::decide(Some(&role), tool, input, &team, agent)
}
