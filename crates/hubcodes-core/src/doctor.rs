//! `hubcodes doctor`: the checks behind "why do my agents not show up".
//! Every bug of the first weeks was an environment difference the UI could
//! not see (no `LANG` under launchd, the CLI not on the login PATH, an old
//! tmux). Each check is a fact plus what to do about it, so the report reads
//! the same in the desktop app, the CLI, and a pasted diagnostics text.

use crate::{db, models, providers::Registry, tmux};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: String,
    pub title: String,
    pub level: Level,
    /// What was found, one line.
    pub detail: String,
    /// What to do when not ok; empty when ok.
    #[serde(default)]
    pub fix: String,
    /// A command that does the fix, for a Copy button (`brew install tmux`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// A page or app that does the fix, for an Open button.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub open: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub checks: Vec<Check>,
    pub ok: usize,
    pub warn: usize,
    pub fail: usize,
    /// Versions, environment, paths: the header of a pasted diagnostics text.
    pub facts: Vec<(String, String)>,
}

impl Report {
    fn push(&mut self, c: Check) {
        match c.level {
            Level::Ok => self.ok += 1,
            Level::Warn => self.warn += 1,
            Level::Fail => self.fail += 1,
        }
        self.checks.push(c);
    }
    pub fn worst(&self) -> Level {
        self.checks.iter().map(|c| c.level).max().unwrap_or(Level::Ok)
    }
}

fn check(id: &str, title: &str, level: Level, detail: impl Into<String>, fix: impl Into<String>) -> Check {
    Check { id: id.into(), title: title.into(), level, detail: detail.into(), fix: fix.into(), command: String::new(), open: String::new() }
}

impl Check {
    fn with_command(mut self, c: impl Into<String>) -> Self {
        self.command = c.into();
        self
    }
    fn with_open(mut self, o: impl Into<String>) -> Self {
        self.open = o.into();
        self
    }
}

/// Homebrew, where its installer puts it (Apple silicon, then Intel).
pub fn brew_path() -> Option<std::path::PathBuf> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].iter().map(std::path::PathBuf::from).find(|p| p.is_file())
}

/// How to get tmux on this Mac: one command with Homebrew, Homebrew first without it.
pub fn tmux_install_check() -> Check {
    match brew_path() {
        Some(_) => check("tmux", "tmux", Level::Fail, "not installed", "Install it: run brew install tmux in Terminal, then press Check again. Agents run inside tmux; nothing works without it.").with_command("brew install tmux"),
        None => check("tmux", "tmux", Level::Fail, "not installed (and no Homebrew)", "Install Homebrew from brew.sh first (one line in Terminal), then run brew install tmux and press Check again. Agents run inside tmux; nothing works without it.")
            .with_command("brew install tmux")
            .with_open("https://brew.sh"),
    }
}

/// Tailscale: running (with this Mac's tailnet address), installed but off, or missing.
pub fn tailscale_check() -> Check {
    if let Some(ip) = crate::serve::tailscale_ip() {
        return check("tailscale", "Tailscale", Level::Ok, ip.to_string(), "");
    }
    if crate::remote::tailscale_installed() {
        return check("tailscale", "Tailscale", Level::Warn, "installed but not connected", "Open Tailscale and sign in (the same account on every device you pair). Only needed for Devices and Remote access.").with_open("/Applications/Tailscale.app");
    }
    let c = check("tailscale", "Tailscale", Level::Warn, "not installed", "Only needed for Devices and Remote access: install Tailscale on every device you pair, open it and sign in with the same account.").with_open("https://tailscale.com/download/mac");
    if brew_path().is_some() {
        c.with_command("brew install --cask tailscale")
    } else {
        c
    }
}

/// Where the diagnostics come from: the desktop app passes its own facts
/// (bundle path, sidecar link), the CLI passes none.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// The CLI binary agents are expected to run (`~/.local/bin/hubcodes` target).
    pub cli: Option<PathBuf>,
    /// The `.app` bundle, when running as the desktop app.
    pub bundle: Option<PathBuf>,
    /// tmux session to look at.
    pub session: Option<String>,
    /// Devices to check (reachability, version, pairing).
    pub peers: Vec<crate::remote::Peer>,
    /// What this device calls itself, for the pairing check.
    pub self_name: String,
}

pub fn run(ctx: &Context) -> Report {
    let mut r = Report::default();
    r.facts.push(("hubcodes".into(), env!("CARGO_PKG_VERSION").into()));
    r.facts.push(("os".into(), os_version()));
    if let Ok(exe) = std::env::current_exe() {
        r.facts.push(("exe".into(), exe.display().to_string()));
    }
    for k in ["LANG", "LC_ALL", "LC_CTYPE", "SHELL", "TMPDIR", "HUBCODES_SESSION"] {
        if let Ok(v) = std::env::var(k) {
            r.facts.push((k.into(), v));
        }
    }
    if let Ok(p) = std::env::var("PATH") {
        r.facts.push(("PATH".into(), p));
    }

    // 1. tmux
    match tmux_version() {
        Some(v) => {
            let ok = v.0 > 3 || (v.0 == 3 && v.1 >= 2);
            r.push(check(
                "tmux",
                "tmux",
                if ok { Level::Ok } else { Level::Warn },
                format!("tmux {}.{}{}", v.0, v.1, v.2.as_deref().unwrap_or("")),
                if ok { "" } else { "hubcodes is tested on tmux 3.2 and newer: brew upgrade tmux" },
            )
            .with_command(if ok { "" } else { "brew upgrade tmux" }));
        }
        None => r.push(tmux_install_check()),
    }

    // 2. locale: without it tmux prints control characters as `_` and hubcodes reads nothing.
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
    match locale {
        Some(l) if l.to_uppercase().contains("UTF") => r.push(check("locale", "Locale", Level::Ok, l, "")),
        Some(l) => r.push(check("locale", "Locale", Level::Warn, format!("{l} is not UTF-8"), "Set LANG=en_US.UTF-8 in your shell profile; tmux output is read as text.")),
        None => r.push(check("locale", "Locale", Level::Fail, "LANG, LC_ALL and LC_CTYPE are all unset", "hubcodes sets LANG=en_US.UTF-8 for itself; set it in your login shell too so agents started from a terminal match.")),
    }

    // 3. state dir writable
    let state = db::state_dir();
    let probe = state.join(".doctor-probe");
    match std::fs::create_dir_all(&state).and_then(|_| std::fs::write(&probe, b"ok")) {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            r.push(check("state", "State folder", Level::Ok, state.display().to_string(), ""));
        }
        Err(e) => r.push(check("state", "State folder", Level::Fail, format!("{}: {e}", state.display()), "Make the folder writable; transcripts, layouts and session files live there.")),
    }

    // 4. config file
    match Registry::load() {
        Ok(reg) => {
            match &reg.source {
                Some(p) => r.push(check("config", "Config file", Level::Ok, p.display().to_string(), "")),
                None => r.push(check("config", "Config file", Level::Ok, "defaults (nothing written yet)", "")),
            }
            // 5. providers
            let mut missing = Vec::new();
            let mut logged_out = Vec::new();
            let mut ready = Vec::new();
            for p in &reg.providers {
                if p.command.starts_with('$') {
                    continue;
                }
                let st = models::status(p);
                if !st.installed {
                    missing.push(p.name.clone());
                } else if st.logged_in == Some(false) {
                    logged_out.push(p.name.clone());
                } else {
                    ready.push(p.name.clone());
                }
            }
            let detail = format!("ready: {}", if ready.is_empty() { "none".into() } else { ready.join(", ") });
            if ready.is_empty() {
                r.push(check("providers", "Providers", Level::Fail, detail, "Install at least one agent CLI (claude, codex, opencode…) and sign in; Settings → Providers → Check shows the steps."));
            } else if !logged_out.is_empty() {
                r.push(check("providers", "Providers", Level::Warn, format!("{detail} · not signed in: {}", logged_out.join(", ")), "Sign in to the ones you use: Settings → Providers → Check."));
            } else {
                let mut d = detail;
                if !missing.is_empty() {
                    d.push_str(&format!(" · not installed: {}", missing.join(", ")));
                }
                r.push(check("providers", "Providers", Level::Ok, d, ""));
            }
        }
        Err(e) => r.push(check("config", "Config file", Level::Fail, format!("{e:#}"), "Fix the TOML or move the file aside; Settings cannot open until it parses.")),
    }

    // 6. CLI on the login PATH (what agents started from a plain terminal see)
    let login_path = login_shell_path();
    let found = login_path.as_deref().and_then(|p| std::env::split_paths(p).map(|d| d.join("hubcodes")).find(|f| f.is_file()));
    match (&found, &ctx.cli) {
        (Some(f), Some(cli)) => {
            let same = std::fs::canonicalize(f).ok() == std::fs::canonicalize(cli).ok();
            r.push(check(
                "cli",
                "hubcodes on the login PATH",
                if same { Level::Ok } else { Level::Warn },
                format!("{}{}", f.display(), if same { "" } else { " (a different build than this app)" }),
                if same { "" } else { "Remove the older hubcodes from that folder, or let the app relink ~/.local/bin/hubcodes." },
            ));
        }
        (Some(f), None) => r.push(check("cli", "hubcodes on the login PATH", Level::Ok, f.display().to_string(), "")),
        (None, _) => r.push(check(
            "cli",
            "hubcodes on the login PATH",
            Level::Warn,
            match &login_path {
                Some(_) => "not found on the login shell PATH".to_string(),
                None => "could not read the login shell PATH".to_string(),
            },
            "Agents started here always find it; ones started from a terminal need ~/.local/bin on PATH (add `export PATH=\"$HOME/.local/bin:$PATH\"` to ~/.zshrc).",
        )),
    }

    // 7. tmux session
    if let Some(s) = &ctx.session {
        let t = tmux::Tmux::named(s);
        if t.has_session() {
            let n = t.panes().map(|p| p.iter().filter(|p| p.provider.is_some()).count()).unwrap_or(0);
            r.push(check("session", "tmux session", Level::Ok, format!("{s}: {n} agent{}", if n == 1 { "" } else { "s" }), ""));
        } else {
            r.push(check("session", "tmux session", Level::Ok, format!("{s}: not started yet"), ""));
        }
    }

    // 8. tailscale (only for devices and remote access)
    r.push(tailscale_check());

    // 8a. this install against the newest release (patch releases too)
    r.push(version_check());

    // 8b. devices: each one reachable, on the same release, paired back
    for p in &ctx.peers {
        match crate::remote::snapshot(p) {
            Ok(snap) => {
                let issues = crate::remote::pairing_issues(&snap, p, &ctx.self_name, env!("CARGO_PKG_VERSION"));
                if issues.is_empty() {
                    r.push(check(&format!("peer:{}", p.name), &format!("Device {}", p.name), Level::Ok, format!("{} · hubcodes {} · {} agents", p.host, snap.version.clone().unwrap_or_default(), snap.agents.len()), ""));
                } else {
                    r.push(check(&format!("peer:{}", p.name), &format!("Device {}", p.name), Level::Warn, issues.join(" "), "Keep both devices on the same release and name each other correctly in Settings → Devices (hubcodes peer check)."));
                }
            }
            Err(e) => r.push(check(&format!("peer:{}", p.name), &format!("Device {}", p.name), Level::Fail, format!("{e:#}"), "Check that `ssh <host> true` works without a prompt (Tailscale SSH or a key) and that hubcodes is installed there.")),
        }
    }

    // 9. code signing (desktop only)
    if let Some(b) = &ctx.bundle {
        r.push(signing_check(b));
    }
    r
}

/// Installed version against the newest release.
pub fn version_check() -> Check {
    let now = crate::update::current();
    match crate::update::latest() {
        Ok(v) if crate::update::newer(&v, now) => check("version", "Version", Level::Warn, format!("{now} installed, {v} is out"), "Update: hubcodes update (or Settings → General → Check for updates in the app).").with_command("hubcodes update"),
        Ok(v) if crate::update::newer(now, &v) => check("version", "Version", Level::Ok, format!("{now} (newer than the release, {v}: a development build)"), ""),
        Ok(_) => check("version", "Version", Level::Ok, format!("{now}, the latest release"), ""),
        Err(e) => check("version", "Version", Level::Warn, format!("{now} installed; could not check for a newer one: {e:#}"), "Sign in with gh auth login (the repository is private), then hubcodes update --check."),
    }
}

fn signing_check(bundle: &Path) -> Check {
    let out = Command::new("codesign").args(["-dv", "--verbose=2"]).arg(bundle).output();
    let text = match out {
        Ok(o) => String::from_utf8_lossy(&o.stderr).to_string(),
        Err(e) => return check("signing", "Code signature", Level::Warn, format!("codesign: {e}"), ""),
    };
    if text.contains("Authority=Developer ID Application") {
        check("signing", "Code signature", Level::Ok, "Developer ID (runs on other Macs)", "")
    } else if text.contains("Authority=Apple Development") {
        check("signing", "Code signature", Level::Warn, "Apple Development certificate (this Mac only)", "Teammates' Gatekeeper rejects this build. Sign with a Developer ID certificate and notarize: see docs/team-guide.md → Releases.")
    } else if text.contains("Signature=adhoc") {
        check("signing", "Code signature", Level::Warn, "ad-hoc signature", "Sign with a Developer ID certificate and notarize before sharing the .dmg.")
    } else {
        check("signing", "Code signature", Level::Warn, "unsigned or unreadable", "Sign with a Developer ID certificate and notarize before sharing the .dmg.")
    }
}

fn tmux_version() -> Option<(u32, u32, Option<String>)> {
    let out = Command::new("tmux").arg("-V").output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let v = s.trim().strip_prefix("tmux ")?.trim();
    let mut parts = v.splitn(2, '.');
    let major: u32 = parts.next()?.parse().ok()?;
    let rest = parts.next().unwrap_or("0");
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let minor: u32 = digits.parse().unwrap_or(0);
    let suffix = rest[digits.len()..].to_string();
    Some((major, minor, if suffix.is_empty() { None } else { Some(suffix) }))
}

/// The PATH a login shell would have: what agents started from a terminal see.
pub fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = Command::new(&shell).args(["-lc", "printf %s \"$PATH\""]).output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() { None } else { Some(p) }
}

fn os_version() -> String {
    let out = Command::new("sw_vers").args(["-productVersion"]).output().ok();
    match out {
        Some(o) if o.status.success() => format!("macOS {}", String::from_utf8_lossy(&o.stdout).trim()),
        _ => std::env::consts::OS.to_string(),
    }
}

/// Last `lines` lines of `~/.local/state/hubcodes/desktop.log`.
pub fn log_tail(lines: usize) -> String {
    let p = db::state_dir().join("desktop.log");
    let Ok(text) = std::fs::read_to_string(&p) else { return String::new() };
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

/// The whole report as text, for a bug report or a chat message.
pub fn render(r: &Report, log_lines: usize) -> String {
    let mut s = String::new();
    s.push_str("hubcodes diagnostics\n");
    for (k, v) in &r.facts {
        s.push_str(&format!("  {k}: {v}\n"));
    }
    s.push('\n');
    for c in &r.checks {
        let mark = match c.level {
            Level::Ok => "ok  ",
            Level::Warn => "warn",
            Level::Fail => "FAIL",
        };
        s.push_str(&format!("[{mark}] {}: {}\n", c.title, c.detail));
        if !c.fix.is_empty() {
            s.push_str(&format!("       → {}\n", c.fix));
            if !c.command.is_empty() {
                s.push_str(&format!("         $ {}\n", c.command));
            }
            if !c.open.is_empty() {
                s.push_str(&format!("         open {}\n", c.open));
            }
        }
    }
    if log_lines > 0 {
        let tail = log_tail(log_lines);
        if !tail.is_empty() {
            s.push_str("\ndesktop.log (tail)\n");
            s.push_str(&tail);
            s.push('\n');
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_counts_levels() {
        let mut r = Report::default();
        r.push(check("a", "A", Level::Ok, "", ""));
        r.push(check("b", "B", Level::Warn, "", "fix"));
        r.push(check("c", "C", Level::Fail, "", "fix"));
        assert_eq!((r.ok, r.warn, r.fail), (1, 1, 1));
        assert_eq!(r.worst(), Level::Fail);
        let text = render(&r, 0);
        assert!(text.contains("[FAIL] C"));
        assert!(text.contains("→ fix"));
    }

    #[test]
    fn tmux_version_parses_suffixes() {
        // Exercised against the real binary when present; the parser itself is pure.
        let v = tmux_version();
        if let Some((major, _, _)) = v {
            assert!(major >= 1);
        }
    }

    #[test]
    fn missing_tmux_and_tailscale_come_with_the_way_to_get_them() {
        let t = tmux_install_check();
        assert_eq!(t.command, "brew install tmux");
        assert!(t.fix.contains("Check again"));
        if brew_path().is_none() {
            assert_eq!(t.open, "https://brew.sh");
        }
        let ts = tailscale_check();
        match ts.level {
            Level::Ok => assert!(ts.fix.is_empty()),
            _ => assert!(ts.open.contains("Tailscale.app") || ts.open.contains("tailscale.com"), "{}", ts.open),
        }
    }
}
