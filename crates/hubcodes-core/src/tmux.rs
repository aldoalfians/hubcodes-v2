//! Thin tmux backend. tmux owns every PTY; hubcodes only creates panes,
//! captures their screen for previews, pastes text into them and moves them
//! between their own window and the canvas window (open / tile / back).
//! State that the CLI and the TUI must share (edges, last message, canvas
//! pane id) lives in tmux session options, so no daemon is needed.
//!
//! The unit is a *pane*, not a window: a pane id (`%7`) survives
//! `join-pane` / `break-pane`, a window id does not.

use anyhow::{anyhow, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

pub const DEFAULT_SESSION: &str = "hubcodes";
pub const CANVAS_WINDOW: &str = "canvas";
const OPT_EDGES: &str = "@hubcodes_edges";
pub const OPT_LAST_MSG: &str = "@hubcodes_last_msg";
const OPT_PROVIDER: &str = "@hubcodes_provider";
const OPT_NAME: &str = "@hubcodes_name";
const OPT_ROLE: &str = "@hubcodes_role";
const OPT_MODEL: &str = "@hubcodes_model";
pub const OPT_WORKTREE: &str = "@hubcodes_worktree";
/// `who <unix>`: the human currently typing into the pane (expires after [`HOLD_SECS`]).
pub const OPT_HOLDER: &str = "@hubcodes_holder";
/// `1` when guest viewers of the shared canvas get the agent's status instead of its screen.
pub const OPT_PRIVATE: &str = "@hubcodes_private";
/// Version of the team brief the agent was started with (see `prompts::BRIEF_VERSION`).
pub const OPT_ACCOUNT: &str = "@hubcodes_account";
/// The model the agent last answered with, read from its own transcript.
pub const OPT_MODEL_SEEN: &str = "@hubcodes_model_seen";
pub const OPT_BRIEF_V: &str = "@hubcodes_brief_v";
/// The window a fresh session starts with, so it exists before any agent.
pub const PLACEHOLDER_WINDOW: &str = "hubcodes-session";
pub const HOLD_SECS: i64 = 45;
const OPT_CANVAS_PANE: &str = "@hubcodes_canvas_pane";
/// Width of the canvas pane when agents are tiled next to it.
pub const SIDEBAR_WIDTH: u16 = 38;

/// `who <unix>` → `who` while fresh.
fn parse_holder(v: &str) -> Option<String> {
    let (who, ts) = v.trim().rsplit_once(' ')?;
    let ts: i64 = ts.parse().ok()?;
    (crate::db::now() - ts <= HOLD_SECS).then(|| who.to_string())
}

#[derive(Clone, Debug)]
pub struct Pane {
    /// Pane id (`%7`), stable for the pane's lifetime.
    pub id: String,
    pub window_id: String,
    /// Agent name: `@hubcodes_name` pane option, else the window name.
    pub name: String,
    pub current_command: String,
    pub provider: Option<String>,
    pub dead: bool,
    /// Job in the team (`orchestrator`, `frontend`…), `@hubcodes_role`.
    pub role: Option<String>,
    /// Model the agent was started with, `@hubcodes_model`.
    pub model: Option<String>,
    /// Account (login) it runs under, `@hubcodes_account`; `None` = the tool's own.
    pub account: Option<String>,
    /// The model it last answered with (Claude transcript), when known.
    pub model_seen: Option<String>,
    /// Status reported by the agent's own hooks (`@hubcodes_status`), with its time and detail.
    pub hook_status: Option<String>,
    pub hook_at: Option<i64>,
    pub hook_detail: Option<String>,
    /// Path of the agent's own git worktree, when it has one.
    pub worktree: Option<String>,
    /// Human holding the keyboard right now (`@hubcodes_holder`, if fresh).
    pub holder: Option<String>,
    /// `@hubcodes_test`: `pass|fail <unix> <summary>`.
    pub test: Option<String>,
    /// Guests of the shared canvas see this agent's status, not its screen (`@hubcodes_private`).
    pub private: bool,
    /// `prompts::BRIEF_VERSION` the agent was started with; `None` for panes from before 0.1.6.
    pub brief_v: Option<String>,
    /// Process id of the pane's shell.
    pub pid: u32,
    /// The agent's process is gone and the pane sits at a shell prompt
    /// (`[hubcodes] agent exited`). Never true for the `shell` provider.
    pub exited: bool,
}

#[derive(Clone, Debug)]
pub struct Tmux {
    pub session: String,
}

fn run(args: &[&str]) -> Result<String> {
    let out = Command::new("tmux")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("spawn tmux")?;
    if !out.status.success() {
        return Err(anyhow!(
            "tmux {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn inside_tmux() -> bool {
    std::env::var_os("TMUX").is_some()
}

/// Give the process a UTF-8 locale when it has none (an app started from
/// Finder or a launchd job has no `LANG`). tmux's client sanitises its
/// output for the locale it runs in: without UTF-8, every control or
/// non-ASCII byte comes out as `_`, which breaks pane listing, previews and
/// the waiting-prompt detection, and agents started from such a session
/// draw their UI in plain ASCII. Returns the locale that was set, if any.
pub fn ensure_utf8_locale() -> Option<&'static str> {
    let set = ["LC_ALL", "LC_CTYPE", "LANG"].iter().any(|k| std::env::var_os(k).map(|v| !v.is_empty()).unwrap_or(false));
    if set {
        return None;
    }
    let locale = if cfg!(target_os = "macos") { "en_US.UTF-8" } else { "C.UTF-8" };
    std::env::set_var("LANG", locale);
    Some(locale)
}

pub fn available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl Tmux {
    /// Use the session we are attached to when running inside tmux, otherwise
    /// the default session name.
    pub fn current() -> Result<Self> {
        let session = if inside_tmux() {
            run(&["display-message", "-p", "#S"])?.trim().to_string()
        } else {
            DEFAULT_SESSION.to_string()
        };
        Ok(Tmux { session })
    }

    pub fn named(session: &str) -> Self {
        Tmux {
            session: session.to_string(),
        }
    }

    /// Create the session when it does not exist yet (detached, with the
    /// placeholder window the desktop also uses), so `hubcodes spawn` works
    /// from a shell before the app has ever opened this session.
    pub fn ensure(&self) -> Result<()> {
        if self.has_session() {
            return Ok(());
        }
        run(&["new-session", "-d", "-s", &self.session, "-n", PLACEHOLDER_WINDOW, "-x", "200", "-y", "50"])?;
        let _ = run(&["set-option", "-t", &window_target(&self.session), "window-size", "manual"]);
        Ok(())
    }

    pub fn has_session(&self) -> bool {
        Command::new("tmux")
            .args(["has-session", "-t", &exact_session(&self.session)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Every pane in the session, canvas pane included (callers skip it via
    /// [`Tmux::canvas_pane`]). Panes without a `@hubcodes_name` are named
    /// after their window; a second unnamed pane in the same window gets
    /// `name.<pane_index>` so names stay unique.
    pub fn panes(&self) -> Result<Vec<Pane>> {
        let out = run(&[
            "list-panes",
            "-s",
            "-t",
            &window_target(&self.session),
            "-F",
            PANE_FORMAT,
        ])?;
        let mut panes = parse_panes(&out);
        // The wrapper is `zsh -c "agent; …; exec $SHELL"`, so tmux reports
        // `zsh` as the pane command whether the agent runs or has quit; the
        // process tree tells: a running agent is a child of the pane's shell.
        // A shell plugin may keep a daemon child (gitstatusd…), so a pane with
        // children is also checked for the exit marker still on its screen.
        let parents = parent_pids();
        for p in &mut panes {
            let agent = p.provider.as_deref().map(|v| v != "shell" && v != "proxy").unwrap_or(false);
            p.exited = agent && p.pid != 0 && (!parents.contains(&p.pid) || tail_has_exit_marker(&p.id));
        }
        Ok(panes)
    }
}

/// `new-window -t` takes a *window* target, and tmux matches a bare name
/// against window names first (exact, then as a prefix) before trying it as
/// a session. The desktop's placeholder window `hubcodes-session` therefore
/// resolved `-t hubcodes` to window 0 and every spawn into an empty session
/// died with `create window failed: index 0 in use`. `=name:` pins the
/// session exactly and leaves the index to tmux (next free one).
fn window_target(session: &str) -> String {
    format!("={session}:")
}

/// Exact session target for commands that take a *session* (`has-session`).
/// Without `=`, tmux also accepts a prefix, so `hubcodes` could resolve to a
/// session named `hubcodes-old`.
fn exact_session(session: &str) -> String {
    format!("={session}")
}

/// Field separator for [`PANE_FORMAT`]. Printable on purpose: when the
/// client runs without a UTF-8 locale (an app started from Finder has no
/// `LANG`), tmux prints control characters such as TAB as `_`, and a
/// tab-separated line would then look like one field.
const PANE_SEP: &str = "~|~";

/// `list-panes -F` format behind [`Tmux::panes`]; fields in the order
/// [`parse_panes`] reads them.
const PANE_FORMAT: &str = "#{pane_id}~|~#{window_id}~|~#{window_name}~|~#{pane_current_command}~|~#{pane_dead}~|~#{@hubcodes_provider}~|~#{@hubcodes_name}~|~#{pane_index}~|~#{@hubcodes_role}~|~#{@hubcodes_model}~|~#{@hubcodes_status}~|~#{@hubcodes_status_at}~|~#{@hubcodes_detail}~|~#{@hubcodes_worktree}~|~#{@hubcodes_holder}~|~#{@hubcodes_test}~|~#{pane_pid}~|~#{@hubcodes_private}~|~#{@hubcodes_brief_v}~|~#{@hubcodes_account}~|~#{@hubcodes_model_seen}";

/// Parse `list-panes -F PANE_FORMAT` output. A line with fewer than eight
/// fields (a continuation of a multi-line option value) is skipped.
fn parse_panes(out: &str) -> Vec<Pane> {
    let mut panes: Vec<Pane> = Vec::new();
    for l in out.lines() {
        let f: Vec<&str> = l.split(PANE_SEP).collect();
        if f.len() < 8 {
            continue;
        }
        let (id, window_id) = (f[0].to_string(), f[1].to_string());
        let mut name = if f[6].is_empty() { f[2].to_string() } else { f[6].to_string() };
        if f[6].is_empty() && panes.iter().any(|p| p.name == name) {
            name = format!("{name}.{}", f[7]);
        }
        panes.push(Pane {
            id,
            window_id,
            name,
            current_command: f[3].to_string(),
            dead: f[4] == "1",
            provider: Some(f[5].to_string()).filter(|s| !s.is_empty()),
            role: f.get(8).map(|s| s.to_string()).filter(|s| !s.is_empty()),
            model: f.get(9).map(|s| s.to_string()).filter(|s| !s.is_empty()),
            hook_status: f.get(10).map(|s| s.to_string()).filter(|s| !s.is_empty()),
            hook_at: f.get(11).and_then(|s| s.parse().ok()),
            hook_detail: f.get(12).map(|s| s.to_string()).filter(|s| !s.is_empty()),
            worktree: f.get(13).map(|s| s.to_string()).filter(|s| !s.is_empty()),
            holder: f.get(14).and_then(|h| parse_holder(h)),
            test: f.get(15).map(|s| s.to_string()).filter(|s| !s.is_empty()),
            pid: f.get(16).and_then(|s| s.trim().parse().ok()).unwrap_or(0),
            private: f.get(17).map(|s| s.trim() == "1").unwrap_or(false),
            brief_v: f.get(18).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            account: f.get(19).map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "default"),
            model_seen: f.get(20).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            exited: false,
        });
    }
    panes
}

/// Printed by the spawn wrapper when the agent's process ends.
pub const EXIT_MARKER: &str = "[hubcodes] agent exited";

/// The marker is among the last screen lines (a running TUI uses the
/// alternate screen, so its main-screen history is not visible here).
fn tail_has_exit_marker(pane: &str) -> bool {
    run(&["capture-pane", "-p", "-t", pane, "-S", "-8"])
        .map(|out| out.lines().rev().filter(|l| !l.trim().is_empty()).take(4).any(|l| l.contains(EXIT_MARKER)))
        .unwrap_or(false)
}

/// Every pid that has at least one child process.
fn parent_pids() -> std::collections::HashSet<u32> {
    std::process::Command::new("ps")
        .args(["-axo", "ppid="])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().filter_map(|x| x.parse().ok()).collect())
        .unwrap_or_default()
}

impl Tmux {
    /// Create a detached window running `command`; returns the pane id.
    /// Refuses a name that is already taken.
    pub fn spawn(&self, name: &str, provider: &str, command: &str, cwd: Option<&str>) -> Result<String> {
        self.spawn_with(name, provider, command, cwd, None, None)
    }

    /// [`Tmux::spawn`] plus the agent's role and model, kept as pane options
    /// so every client shows them.
    pub fn spawn_with(
        &self,
        name: &str,
        provider: &str,
        command: &str,
        cwd: Option<&str>,
        role: Option<&str>,
        model: Option<&str>,
    ) -> Result<String> {
        self.spawn_with_account(name, provider, command, cwd, role, model, None)
    }

    /// [`spawn_with`] recording the account the agent runs under.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with_account(
        &self,
        name: &str,
        provider: &str,
        command: &str,
        cwd: Option<&str>,
        role: Option<&str>,
        model: Option<&str>,
        account: Option<&str>,
    ) -> Result<String> {
        self.ensure()?;
        if self.panes()?.iter().any(|p| p.name == name) {
            return Err(anyhow!("agent '{name}' already exists in session '{}'", self.session));
        }
        let mut args: Vec<String> = vec![
            "new-window".into(),
            "-d".into(),
            "-P".into(),
            "-F".into(),
            "#{pane_id}".into(),
            "-t".into(),
            window_target(&self.session),
            "-n".into(),
            name.into(),
        ];
        // Without a folder tmux would inherit this process's one, which is `/`
        // for an app launched from Finder; home is what people expect.
        let home = std::env::var("HOME").ok();
        if let Some(c) = cwd.filter(|c| !c.trim().is_empty()).or(home.as_deref()) {
            args.push("-c".into());
            args.push(c.into());
        }
        // Keep the window around when the agent exits so the error is visible.
        args.push(format!("{command}; echo; echo '{EXIT_MARKER}'; exec $SHELL"));
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let id = run(&refs)?.trim().to_string();
        run(&["set-option", "-p", "-t", &id, OPT_PROVIDER, provider])?;
        run(&["set-option", "-p", "-t", &id, OPT_NAME, name])?;
        if let Some(r) = role.filter(|r| !r.trim().is_empty()) {
            run(&["set-option", "-p", "-t", &id, OPT_ROLE, r.trim()])?;
        }
        if let Some(m) = model.filter(|m| !m.trim().is_empty()) {
            run(&["set-option", "-p", "-t", &id, OPT_MODEL, m.trim()])?;
        }
        run(&["set-option", "-p", "-t", &id, OPT_BRIEF_V, crate::prompts::BRIEF_VERSION])?;
        if let Some(a) = account.map(str::trim).filter(|a| !a.is_empty() && *a != crate::accounts::DEFAULT) {
            run(&["set-option", "-p", "-t", &id, OPT_ACCOUNT, a])?;
        }
        Ok(id)
    }

    /// Mark `who` as typing into the pane (refreshed on every keystroke batch).
    pub fn hold(&self, target: &str, who: &str) -> Result<()> {
        self.set_pane_option(target, OPT_HOLDER, &format!("{who} {}", crate::db::now()))
    }

    /// Who holds the pane, when the mark is fresh. `Err` = held by someone else.
    pub fn check_hold(&self, target: &str, who: &str) -> Result<()> {
        let v = run(&["show-options", "-pqv", "-t", target, OPT_HOLDER]).unwrap_or_default();
        match parse_holder(v.trim()) {
            Some(h) if h != who => Err(anyhow!("{h} is typing into this agent right now; try again in a moment")),
            _ => Ok(()),
        }
    }

    /// Press keys in a pane (`Enter`, `Escape`, `y`…): answers a permission prompt.
    pub fn send_keys(&self, target: &str, keys: &[String]) -> Result<()> {
        let mut args: Vec<&str> = vec!["send-keys", "-t", target];
        args.extend(keys.iter().map(|k| k.as_str()));
        run(&args).map(|_| ())
    }

    /// Set any pane option (hooks write status here).
    pub fn set_pane_option(&self, target: &str, name: &str, value: &str) -> Result<()> {
        run(&["set-option", "-p", "-t", target, name, value]).map(|_| ())
    }

    /// Remove a pane option.
    pub fn unset_pane_option(&self, target: &str, name: &str) -> Result<()> {
        run(&["set-option", "-p", "-u", "-t", target, name]).map(|_| ())
    }

    /// Read a pane option; empty when unset.
    pub fn pane_option(&self, target: &str, name: &str) -> String {
        run(&["show-options", "-pqv", "-t", target, name]).map(|v| v.trim().to_string()).unwrap_or_default()
    }

    /// Change an agent's role label (`@hubcodes_role`); empty clears it.
    pub fn set_role(&self, target: &str, role: &str) -> Result<()> {
        if role.trim().is_empty() {
            run(&["set-option", "-p", "-u", "-t", target, OPT_ROLE])?;
        } else {
            run(&["set-option", "-p", "-t", target, OPT_ROLE, role.trim()])?;
        }
        Ok(())
    }

    /// Hide (`on`) or show the agent's screen to guest viewers of the shared
    /// canvas. `agent` is a name, or a pane id (`%7`) as the desktop passes.
    pub fn set_private(&self, agent: &str, on: bool) -> Result<()> {
        let target = if agent.starts_with('%') { agent.to_string() } else { self.pane_by_name(agent)?.id };
        self.set_pane_option(&target, OPT_PRIVATE, if on { "1" } else { "0" })
    }

    pub fn kill(&self, target: &str) -> Result<()> {
        run(&["kill-pane", "-t", target]).map(|_| ())
    }

    /// Last `lines` rows of the pane with ANSI styling preserved.
    pub fn capture(&self, target: &str, lines: usize) -> Result<Vec<String>> {
        let start = format!("-{lines}");
        let out = run(&["capture-pane", "-p", "-e", "-J", "-t", target, "-S", &start])?;
        let mut v: Vec<String> = out.lines().map(|l| l.to_string()).collect();
        while v.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            v.pop();
        }
        Ok(v)
    }

    /// Paste `text` into the target pane and press Enter. Uses a buffer so
    /// long prompts with quotes or newlines survive intact.
    pub fn send_text(&self, target: &str, text: &str) -> Result<()> {
        let mut child = Command::new("tmux")
            .args(["load-buffer", "-b", "hubcodes", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .context("tmux load-buffer")?;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("no stdin for tmux load-buffer"))?
            .write_all(text.as_bytes())?;
        let st = child.wait()?;
        if !st.success() {
            return Err(anyhow!("tmux load-buffer failed"));
        }
        run(&["paste-buffer", "-d", "-p", "-b", "hubcodes", "-t", target])?;
        run(&["send-keys", "-t", target, "Enter"])?;
        Ok(())
    }

    /// Put the keyboard in `target`: its window becomes current and the pane
    /// active (works whether the pane lives in its own window or is tiled).
    pub fn focus(&self, target: &str) -> Result<()> {
        run(&["select-window", "-t", target])?;
        run(&["select-pane", "-t", target]).map(|_| ())
    }

    /// Remember which pane runs the canvas so the CLI (`hubcodes back`) and
    /// the return keys can find it.
    pub fn set_canvas_pane(&self, pane: &str) -> Result<()> {
        self.set_opt(OPT_CANVAS_PANE, pane)
    }

    pub fn canvas_pane(&self) -> Result<Option<String>> {
        Ok(Some(self.get_opt(OPT_CANVAS_PANE)?).filter(|s| !s.is_empty()))
    }

    fn canvas_pane_required(&self) -> Result<String> {
        self.canvas_pane()?
            .ok_or_else(|| anyhow!("no canvas running in session '{}'", self.session))
    }

    /// Move an agent pane into the canvas window, next to the canvas
    /// (canvas = narrow main pane on the left, agents stacked on the right),
    /// and give it the keyboard.
    pub fn open(&self, pane: &str) -> Result<()> {
        let canvas = self.canvas_pane_required()?;
        let in_canvas = self
            .panes()?
            .iter()
            .any(|p| p.id == pane && self.window_of(&canvas).ok().as_deref() == Some(p.window_id.as_str()));
        if !in_canvas {
            run(&["join-pane", "-d", "-h", "-s", pane, "-t", &canvas])?;
        }
        self.relayout(&canvas)?;
        run(&["select-pane", "-t", pane]).map(|_| ())
    }

    /// Return a tiled pane to a window of its own (`-d`: keyboard stays).
    pub fn close(&self, pane: &str, name: &str) -> Result<()> {
        let canvas = self.canvas_pane_required()?;
        run(&["break-pane", "-d", "-s", pane, "-n", name])?;
        self.relayout(&canvas)
    }

    /// Break every agent pane out of the canvas window and focus the canvas.
    pub fn back(&self) -> Result<usize> {
        let canvas = self.canvas_pane_required()?;
        let win = self.window_of(&canvas)?;
        let tiled: Vec<Pane> = self
            .panes()?
            .into_iter()
            .filter(|p| p.window_id == win && p.id != canvas)
            .collect();
        for p in &tiled {
            run(&["break-pane", "-d", "-s", &p.id, "-n", &p.name])?;
        }
        run(&["select-window", "-t", &canvas])?;
        run(&["select-pane", "-t", &canvas])?;
        Ok(tiled.len())
    }

    /// Cycle tmux's built-in layouts for the canvas window.
    pub fn next_layout(&self) -> Result<()> {
        let canvas = self.canvas_pane_required()?;
        run(&["next-layout", "-t", &canvas]).map(|_| ())
    }

    fn relayout(&self, canvas: &str) -> Result<()> {
        let w = SIDEBAR_WIDTH.to_string();
        run(&["set-option", "-w", "-t", canvas, "main-pane-width", &w])?;
        run(&["select-layout", "-t", canvas, "main-vertical"]).map(|_| ())
    }

    pub fn window_of(&self, pane: &str) -> Result<String> {
        Ok(run(&["display-message", "-p", "-t", pane, "#{window_id}"])?.trim().to_string())
    }

    /// Bind Alt+Esc (no prefix) and prefix+g to `hubcodes back`: agents go
    /// back to their own windows and the canvas gets the keyboard.
    pub fn bind_return_keys(&self, exe: &str) -> Result<()> {
        let cmd = format!("'{exe}' --session '{}' back", self.session);
        run(&["bind-key", "-n", "M-Escape", "run-shell", &cmd])?;
        run(&["bind-key", "g", "run-shell", &cmd])?;
        // Clicking between the canvas and tiled agents needs tmux's mouse.
        run(&["set-option", "-t", &window_target(&self.session), "mouse", "on"])?;
        Ok(())
    }

    /// Read a session option (shared state for relay/loops lives here too).
    pub fn get_option(&self, name: &str) -> Result<String> {
        self.get_opt(name)
    }

    pub fn set_option(&self, name: &str, value: &str) -> Result<()> {
        self.set_opt(name, value)
    }

    fn get_opt(&self, name: &str) -> Result<String> {
        Ok(run(&["show-options", "-qv", "-t", &window_target(&self.session), name])?
            .trim()
            .to_string())
    }

    fn set_opt(&self, name: &str, value: &str) -> Result<()> {
        run(&["set-option", "-t", &window_target(&self.session), name, value]).map(|_| ())
    }

    /// Edges as `(from_name, to_name, label)` stored in a session option.
    pub fn edges(&self) -> Result<Vec<(String, String, String)>> {
        let raw = self.get_opt(OPT_EDGES)?;
        Ok(raw
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| {
                let mut it = s.split('>');
                let a = it.next()?.trim().to_string();
                let b = it.next()?.trim().to_string();
                let l = it.next().unwrap_or("link").trim().to_string();
                Some((a, b, l))
            })
            .collect())
    }

    pub fn set_edges(&self, edges: &[(String, String, String)]) -> Result<()> {
        let raw = edges
            .iter()
            .map(|(a, b, l)| format!("{a}>{b}>{l}"))
            .collect::<Vec<_>>()
            .join(",");
        self.set_opt(OPT_EDGES, &raw)
    }

    pub fn add_edge(&self, from: &str, to: &str, label: &str) -> Result<()> {
        let mut e = self.edges()?;
        if !e.iter().any(|(a, b, _)| (a == from && b == to) || (a == to && b == from)) {
            e.push((from.into(), to.into(), label.into()));
            self.set_edges(&e)?;
        }
        Ok(())
    }

    /// Drop every link that touches `name` (an agent that is gone).
    pub fn remove_edges_of(&self, name: &str) -> Result<()> {
        let e: Vec<_> = self.edges()?.into_iter().filter(|(a, b, _)| a != name && b != name).collect();
        self.set_edges(&e)
    }

    pub fn remove_edge(&self, a: &str, b: &str) -> Result<()> {
        let e: Vec<_> = self
            .edges()?
            .into_iter()
            .filter(|(x, y, _)| !((x == a && y == b) || (x == b && y == a)))
            .collect();
        self.set_edges(&e)
    }

    /// Record the last message so the TUI can animate it. Format:
    /// `seq\tfrom\tto\ttext`.
    pub fn record_message(&self, from: &str, to: &str, text: &str) -> Result<u64> {
        self.record_message_kind(from, to, text, "message")
    }

    /// Same, with a kind for the transcript (`nudge`, `loop`, `task`, `fanout`).
    pub fn record_message_kind(&self, from: &str, to: &str, text: &str, kind: &str) -> Result<u64> {
        // The durable transcript gives the sequence number; the tmux option is
        // the live signal every client watches.
        let seq = match crate::db::Db::open(&self.session).and_then(|db| db.add_message(from, to, text, kind)) {
            Ok(seq) => seq as u64,
            Err(e) => {
                eprintln!("[hubcodes] transcript write failed: {e:#}");
                self.last_message()?.map(|m| m.0 + 1).unwrap_or(1)
            }
        };
        let one_line = text.replace(['\n', '\t'], " ");
        self.set_opt(OPT_LAST_MSG, &format!("{seq}\t{from}\t{to}\t{one_line}"))?;
        Ok(seq)
    }

    pub fn last_message(&self) -> Result<Option<(u64, String, String, String)>> {
        let raw = self.get_opt(OPT_LAST_MSG)?;
        if raw.is_empty() {
            return Ok(None);
        }
        let mut it = raw.splitn(4, '\t');
        let seq = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let from = it.next().unwrap_or("").to_string();
        let to = it.next().unwrap_or("").to_string();
        let text = it.next().unwrap_or("").to_string();
        Ok(Some((seq, from, to, text)))
    }

    /// Resolve an agent name to its pane.
    pub fn pane_by_name(&self, name: &str) -> Result<Pane> {
        let canvas = self.canvas_pane().ok().flatten();
        self.panes()?
            .into_iter()
            .filter(|p| canvas.as_deref() != Some(p.id.as_str()))
            .find(|p| p.name == name)
            .ok_or_else(|| anyhow!("no agent named '{name}' in session '{}'", self.session))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pane_lines_and_skips_short_ones() {
        let out = format!(
            "%0{s}@0{s}canvas{s}hubcodes{s}0{s}{s}{s}0{s}{s}{s}{s}{s}{s}{s}{s}{s}74096\n\
             %106{s}@106{s}lead{s}zsh{s}0{s}claude{s}lead{s}0{s}orchestrator{s}{s}idle{s}1794452452{s}ready{s}{s}aldohub 1794421674{s}{s}20385{s}1\n\
             tail of a multi-line detail\n",
            s = PANE_SEP
        );
        let panes = parse_panes(&out);
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].name, "canvas");
        assert!(panes[0].provider.is_none());
        let lead = &panes[1];
        assert_eq!((lead.id.as_str(), lead.name.as_str(), lead.provider.as_deref(), lead.role.as_deref(), lead.pid), ("%106", "lead", Some("claude"), Some("orchestrator"), 20385));
        assert_eq!(lead.hook_status.as_deref(), Some("idle"));
        // The hold timestamp is old, so nobody holds the pane any more.
        assert_eq!(lead.holder, None);
        assert!(lead.private, "@hubcodes_private=1 marks the pane private");
        assert!(!panes[0].private, "a pane without the option is public");
    }

    #[test]
    fn new_window_targets_the_session_not_a_window_named_like_it() {
        let t = window_target("hubcodes");
        assert_eq!(t, "=hubcodes:");
        assert!(t.ends_with(':'), "a trailing colon makes tmux read it as a session");
        assert!(t.starts_with('='), "an = prefix disables prefix matching");
    }

    /// `list-panes -s -t NAME` parses NAME as a *pane* target, so a bare
    /// session name matches a window called `NAME-anything` in the most
    /// recently used session first (every session's placeholder window is
    /// `hubcodes-session`): after switching to a second session the app read
    /// the wrong session's panes and showed "no agents". Needs a tmux server.
    #[test]
    fn list_panes_reads_the_named_session_not_a_window_prefix_elsewhere() {
        if Command::new("tmux").arg("-V").output().is_err() {
            return;
        }
        let a = format!("hc-lp-{}", std::process::id());
        let b = format!("{a}-other");
        let _ = run(&["kill-session", "-t", &exact_session(&a)]);
        let _ = run(&["kill-session", "-t", &exact_session(&b)]);
        if run(&["new-session", "-d", "-s", &a, "-n", "lead"]).is_err() {
            return; // no server allowed here (CI sandbox)
        }
        // `b` is newer, so it is the "current" session a bare target falls back
        // to, and its window name starts with `a`'s session name.
        run(&["new-session", "-d", "-s", &b, "-n", &format!("{a}-session")]).unwrap();
        let out = run(&["list-panes", "-s", "-t", &window_target(&a), "-F", "#{session_name}"]).unwrap();
        let _ = run(&["kill-session", "-t", &exact_session(&a)]);
        let _ = run(&["kill-session", "-t", &exact_session(&b)]);
        assert!(out.lines().all(|l| l.trim() == a), "listed panes of the wrong session: {out:?}");
    }

    #[test]
    fn separator_survives_a_c_locale_client() {
        // tmux without a UTF-8 locale prints control characters as `_`;
        // the separator must be plain printable ASCII so that cannot happen.
        assert!(PANE_SEP.bytes().all(|b| (0x20..0x7f).contains(&b)));
        assert!(!PANE_FORMAT.contains('\t'));
    }

    #[test]
    fn second_unnamed_pane_in_a_window_gets_a_suffix() {
        let out = format!("%1{s}@1{s}shell{s}zsh{s}0{s}{s}{s}0\n%2{s}@1{s}shell{s}zsh{s}0{s}{s}{s}1\n", s = PANE_SEP);
        let names: Vec<String> = parse_panes(&out).into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["shell", "shell.1"]);
    }
}
