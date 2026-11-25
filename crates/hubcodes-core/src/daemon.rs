//! hubcodesd: one long-lived process per machine, kept alive by launchd
//! (macOS) or a systemd user service (Linux, WSL), so
//! triggers, trackers, loops, queued deliveries and the phone page keep
//! working while the app is closed. It tends every tmux session that has
//! hubcodes agents, takes hook reports and sends over its unix socket
//! ([`crate::ipc`]) and pushes events to whoever subscribed (the desktop app
//! refreshes on them instead of waiting for its next poll).
//!
//! tmux and SQLite stay the state store; the daemon owns *tending* (one lock
//! file per session, held by a live pid, so an app that is open at the same
//! time stays passive) and the event stream. Without a daemon every client
//! does the same work itself, as before.

use crate::db;
use crate::ipc::{self, Event, Hub};
use crate::providers::Registry;
use crate::tend;
use crate::tmux::Tmux;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// The LaunchAgent label of the one daemon.
pub const LABEL: &str = "id.hubstudio.hubcodes.daemon";
/// Seconds between tend passes.
pub const TICK_SECS: u64 = 5;

/// Label of the pre-0.1.5 per-session watchers, kept so `install` can retire them.
pub fn label(session: &str) -> String {
    format!("id.hubstudio.hubcodes.{session}")
}

fn launch_agents_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("Library/LaunchAgents")
}

/// The service file: the LaunchAgent plist on macOS, the systemd user unit elsewhere.
pub fn plist_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        launch_agents_dir().join(format!("{LABEL}.plist"))
    } else {
        unit_path()
    }
}

/// systemd user unit name (Linux, WSL with systemd).
pub const UNIT: &str = "hubcodesd.service";

fn unit_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    base.join("systemd/user").join(UNIT)
}

/// The systemd user unit that runs the daemon.
fn unit(exe: &str, serve: bool, primary: &str) -> String {
    let mut args = vec![exe.to_string(), "daemon".into(), "run".into()];
    if serve {
        args.push("--serve".into());
    }
    let quote = |a: &str| if a.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\') { format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\"")) } else { a.to_string() };
    let exec: Vec<String> = args.iter().map(|a| quote(a)).collect();
    let home = dirs::home_dir().unwrap_or_default().display().to_string();
    format!(
        "[Unit]\nDescription=hubcodes daemon (triggers, trackers, loops, phone page)\n\n[Service]\nExecStart={exec}\nEnvironment=HUBCODES_SESSION={session}\nEnvironment=LANG=C.UTF-8\nEnvironment=PATH={home}/.local/bin:/usr/local/bin:/usr/bin:/bin\nRestart=always\nRestartSec=10\nStandardOutput=append:{log}\nStandardError=append:{log}\n\n[Install]\nWantedBy=default.target\n",
        exec = exec.join(" "),
        session = quote(primary),
        log = log_path().display(),
    )
}

fn systemctl(args: &[&str]) -> Result<String> {
    let out = Command::new("systemctl").arg("--user").args(args).output().context("run systemctl --user (is systemd running? on WSL set systemd=true in /etc/wsl.conf)")?;
    if !out.status.success() {
        return Err(anyhow!("systemctl --user {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn log_path() -> PathBuf {
    db::state_dir().join("hubcodesd.log")
}

pub fn pid_path() -> PathBuf {
    db::state_dir().join("hubcodesd.pid")
}

fn plist(exe: &str, serve: bool, primary: &str) -> String {
    let mut args = vec![exe.to_string(), "daemon".into(), "run".into()];
    if serve {
        args.push("--serve".into());
    }
    let args_xml: String = args.iter().map(|a| format!("      <string>{}</string>\n", xml(a))).collect();
    let home = dirs::home_dir().unwrap_or_default().display().to_string();
    let path = format!("{home}/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{args_xml}  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HUBCODES_SESSION</key><string>{session}</string>
    <key>LANG</key><string>en_US.UTF-8</string>
    <key>PATH</key><string>{path}</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        session = xml(primary),
        log = xml(&log_path().display().to_string()),
    )
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn launchctl(args: &[&str]) -> Result<String> {
    let out = Command::new("launchctl").args(args).output().context("run launchctl")?;
    if !out.status.success() {
        return Err(anyhow!("launchctl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn uid() -> String {
    let out = Command::new("id").arg("-u").output().ok();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(|| "501".into())
}

/// The per-session `hubcodes watch` LaunchAgents from before the daemon.
fn legacy_plists() -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(launch_agents_dir()) else { return vec![] };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|f| f.to_str()).map(|f| f.starts_with("id.hubstudio.hubcodes.") && f.ends_with(".plist") && f != format!("{LABEL}.plist")).unwrap_or(false))
        .collect()
}

fn retire_legacy() {
    for p in legacy_plists() {
        let _ = launchctl(&["bootout", &format!("gui/{}", uid()), &p.display().to_string()]);
        let _ = std::fs::remove_file(&p);
    }
}

/// Write the LaunchAgent and start it. `exe` is the hubcodes binary agents
/// also use (the app's sidecar or ~/.local/bin/hubcodes); `primary` is the
/// session the phone page serves. Retires the old per-session watchers.
pub fn install(primary: &str, exe: &str, serve: bool) -> Result<PathBuf> {
    let p = plist_path();
    if !cfg!(target_os = "macos") {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::create_dir_all(db::state_dir())?;
        std::fs::write(&p, unit(exe, serve, primary)).with_context(|| format!("write {}", p.display()))?;
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", UNIT])?;
        systemctl(&["restart", UNIT])?;
        return Ok(p);
    }
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::create_dir_all(db::state_dir())?;
    retire_legacy();
    // Reload cleanly when it already runs.
    let _ = launchctl(&["bootout", &format!("gui/{}", uid()), &p.display().to_string()]);
    std::fs::write(&p, plist(exe, serve, primary)).with_context(|| format!("write {}", p.display()))?;
    launchctl(&["bootstrap", &format!("gui/{}", uid()), &p.display().to_string()])?;
    Ok(p)
}

pub fn uninstall(_primary: &str) -> Result<()> {
    let p = plist_path();
    if !cfg!(target_os = "macos") {
        if p.exists() {
            let _ = systemctl(&["disable", "--now", UNIT]);
            std::fs::remove_file(&p).with_context(|| format!("remove {}", p.display()))?;
            let _ = systemctl(&["daemon-reload"]);
        }
        return Ok(());
    }
    retire_legacy();
    if p.exists() {
        let _ = launchctl(&["bootout", &format!("gui/{}", uid()), &p.display().to_string()]);
        std::fs::remove_file(&p).with_context(|| format!("remove {}", p.display()))?;
    }
    Ok(())
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub running: bool,
    pub pid: Option<u32>,
    pub plist: String,
    pub log: String,
    /// Who holds the tend lock for this session right now (`desktop`, `watch`, `hubcodesd`), if anyone.
    pub tender: Option<String>,
    /// A daemon answers on the socket (launchd or started by hand).
    pub socket: bool,
    /// Sessions the daemon tends right now.
    pub sessions: Vec<String>,
    pub subscribers: usize,
    pub version: Option<String>,
}

pub fn status(session: &str) -> Status {
    let p = plist_path();
    let mut st = Status { installed: p.exists(), plist: p.display().to_string(), log: log_path().display().to_string(), ..Default::default() };
    if !cfg!(target_os = "macos") {
        st.running = systemctl(&["is-active", UNIT]).map(|o| o.trim() == "active").unwrap_or(false);
        if st.running {
            st.pid = systemctl(&["show", "-p", "MainPID", "--value", UNIT]).ok().and_then(|o| o.trim().parse().ok()).filter(|p| *p > 0);
        }
    } else if let Ok(out) = launchctl(&["print", &format!("gui/{}/{LABEL}", uid())]) {
        st.running = out.contains("state = running");
        st.pid = out.lines().find_map(|l| l.trim().strip_prefix("pid = ")).and_then(|v| v.trim().parse().ok());
    }
    st.tender = lock_holder(session).map(|(who, _)| who);
    if let Ok(v) = ipc::call("ping", Value::Null) {
        st.socket = true;
        st.sessions = v.get("sessions").and_then(|s| s.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        st.subscribers = v.get("subscribers").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
        st.version = v.get("version").and_then(|s| s.as_str()).map(String::from);
        if st.pid.is_none() {
            st.pid = v.get("pid").and_then(|s| s.as_u64()).map(|p| p as u32);
        }
    }
    st
}

// ----- the tend lock -----

fn lock_path(dir: &std::path::Path, session: &str) -> PathBuf {
    dir.join(format!("{session}.tend.lock"))
}

fn pid_alive(pid: u32) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// `(owner, pid)` of a live lock holder.
pub fn lock_holder(session: &str) -> Option<(String, u32)> {
    holder_in(&db::state_dir(), session)
}

fn holder_in(dir: &std::path::Path, session: &str) -> Option<(String, u32)> {
    let text = std::fs::read_to_string(lock_path(dir, session)).ok()?;
    let (who, pid) = text.trim().split_once(' ')?;
    let pid: u32 = pid.parse().ok()?;
    if pid_alive(pid) { Some((who.to_string(), pid)) } else { None }
}

/// Take the lock for this process if nobody alive holds it. Returns whether
/// this process should tend. Cheap enough to call every tick.
pub fn try_tend(session: &str, who: &str) -> bool {
    try_tend_in(&db::state_dir(), session, who)
}

// The state folder is read once per call: tests that move XDG_STATE_HOME
// meanwhile cannot split a write and its read across two folders.
fn try_tend_in(dir: &std::path::Path, session: &str, who: &str) -> bool {
    let me = std::process::id();
    match holder_in(dir, session) {
        Some((_, pid)) if pid == me => true,
        Some(_) => false,
        None => {
            let _ = std::fs::create_dir_all(dir);
            std::fs::write(lock_path(dir, session), format!("{who} {me}")).is_ok()
        }
    }
}

pub fn release(session: &str) {
    release_in(&db::state_dir(), session)
}

fn release_in(dir: &std::path::Path, session: &str) {
    if let Some((_, pid)) = holder_in(dir, session) {
        if pid == std::process::id() {
            let _ = std::fs::remove_file(lock_path(dir, session));
        }
    }
}

// ----- the daemon process -----

/// tmux sessions that hold at least one hubcodes agent pane.
pub fn sessions_with_agents() -> Vec<String> {
    let names: Vec<String> = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    names
        .into_iter()
        .filter(|n| Tmux::named(n).panes().map(|ps| ps.iter().any(|p| p.provider.as_deref().map(|v| v != "proxy").unwrap_or(false))).unwrap_or(false))
        .collect()
}

/// A hook report arrived (over the socket or from `hubcodes hook` directly):
/// write it to the pane, account usage, and run the turn-end bookkeeping.
/// Returns what the daemon broadcasts.
pub fn ingest_report(t: &Tmux, agent: &str, r: &crate::hooks::Report) -> Result<Event> {
    crate::hooks::apply(t, agent, r)?;
    if let Some(tp) = r.transcript.as_ref().filter(|_| r.turn_done) {
        if let Err(e) = crate::hooks::account_claude_turn(&t.session, agent, tp) {
            eprintln!("hubcodes hook: usage: {e:#}");
        }
    }
    let mut delivered = false;
    if r.turn_done {
        delivered = crate::relay::turn_finished(t, agent).unwrap_or(false);
    }
    Ok(Event::new("status", &t.session, json!({ "agent": agent, "status": r.status, "detail": r.detail, "turnDone": r.turn_done, "delivered": delivered })))
}

struct State {
    /// hubcodes.toml as last read; [`State::reload_if_changed`] keeps it
    /// current, so trackers, triggers and peers added in Settings apply
    /// without restarting the daemon.
    reg: RwLock<Registry>,
    seen: Mutex<Option<std::time::SystemTime>>,
    hub: Arc<Hub>,
    started: i64,
    mem: Mutex<BTreeMap<String, tend::Memory>>,
    tended: Mutex<Vec<String>>,
}

impl State {
    fn new(reg: Registry, hub: Arc<Hub>, started: i64) -> Self {
        let seen = reg.config_mtime();
        State { reg: RwLock::new(reg), seen: Mutex::new(seen), hub, started, mem: Mutex::new(BTreeMap::new()), tended: Mutex::new(Vec::new()) }
    }

    fn reg(&self) -> Registry {
        self.reg.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Load hubcodes.toml again when it changed. A file that does not parse
    /// keeps the last good registry and says so in the log.
    fn reload_if_changed(&self) -> Option<String> {
        let now = self.reg.read().unwrap_or_else(|e| e.into_inner()).config_mtime();
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if now.is_none() || now == *seen {
            return None;
        }
        *seen = now;
        let source = self.reg.read().unwrap_or_else(|e| e.into_inner()).source.clone();
        let next = match source {
            Some(p) => Registry::from_file(&p),
            None => Registry::load(),
        };
        match next {
            Ok(next) => {
                let msg = format!("hubcodes.toml changed; {} tracker(s), {} trigger(s), {} device(s)", next.trackers.len(), next.triggers.len(), next.peers.len());
                *self.reg.write().unwrap_or_else(|e| e.into_inner()) = next;
                Some(msg)
            }
            Err(e) => Some(format!("hubcodes.toml changed but does not read, keeping the last one: {e:#}")),
        }
    }
}

fn session_of(params: &Value, fallback: &str) -> String {
    params.get("session").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or(fallback).to_string()
}

fn str_param<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("missing {key}"))
}

fn handle(st: &State, primary: &str, method: &str, params: &Value) -> Result<Value> {
    match method {
        "ping" => Ok(json!({
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
            "startedAt": st.started,
            "sessions": *st.tended.lock().unwrap_or_else(|e| e.into_inner()),
            "subscribers": st.hub.subscribers(),
        })),
        "sessions" => Ok(json!(sessions_with_agents())),
        "agents" => {
            let t = Tmux::named(&session_of(params, primary));
            let agents: Vec<Value> = tend::agent_states(&t)
                .into_iter()
                .map(|a| json!({ "name": a.name, "role": a.role, "status": a.status, "dead": a.dead, "lastLine": a.last_line, "dir": a.dir }))
                .collect();
            Ok(json!(agents))
        }
        "send" => {
            let t = Tmux::named(&session_of(params, primary));
            let from = params.get("from").and_then(|v| v.as_str()).unwrap_or("human:daemon");
            let to = str_param(params, "to")?;
            let text = str_param(params, "text")?;
            let now = params.get("now").and_then(|v| v.as_bool()).unwrap_or(false);
            let d = crate::relay::deliver_with(&t, &st.reg(), from, to, text, now)?;
            st.hub.broadcast(&Event::new("message", &t.session, json!({ "from": from, "to": to, "seq": d.seq, "queued": d.queued })));
            Ok(json!({ "seq": d.seq, "queued": d.queued, "target": d.target }))
        }
        "hook" => {
            let t = Tmux::named(&session_of(params, primary));
            let agent = str_param(params, "agent")?;
            let r = crate::hooks::Report {
                status: match str_param(params, "status")? {
                    "working" => "working",
                    "waiting" => "waiting",
                    "offline" => "offline",
                    _ => "idle",
                },
                detail: params.get("detail").and_then(|v| v.as_str()).unwrap_or("").chars().take(200).collect(),
                turn_done: params.get("turnDone").and_then(|v| v.as_bool()).unwrap_or(false),
                transcript: params.get("transcript").and_then(|v| v.as_str()).map(PathBuf::from),
            };
            let ev = ingest_report(&t, agent, &r)?;
            st.hub.broadcast(&ev);
            Ok(ev.data)
        }
        "events" => {
            let t = Tmux::named(&session_of(params, primary));
            let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
            let db = db::Db::open(&t.session)?;
            Ok(serde_json::to_value(db.events(limit, None)?)?)
        }
        "tend" => {
            // Force a pass now (the app asks after a change it made).
            let t = Tmux::named(&session_of(params, primary));
            Ok(json!(tend_once(st, &t)))
        }
        _ => Err(anyhow!("unknown method {method}")),
    }
}

/// After a reboot the LaunchAgent starts the daemon before anyone opens the
/// app: every session with a saved definition and no agent running gets its
/// agents back (conversations continued where the provider can), so a Mac
/// mini in a cupboard comes back on its own. Sessions that already run
/// agents are left alone.
pub fn restore_saved(reg: &Registry) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(db::state_dir()) else { return out };
    let exe = crate::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into());
    let path_prefix = std::path::Path::new(&exe).parent().map(|d| d.display().to_string());
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(session) = name.strip_suffix("-def.json") else { continue };
        let Some(def) = crate::sessiondef::load(session) else { continue };
        if def.agents.is_empty() {
            continue;
        }
        let t = Tmux::named(session);
        let running = t.panes().map(|ps| ps.iter().any(|p| p.provider.is_some() && p.provider.as_deref() != Some("proxy") && !p.exited && !p.dead)).unwrap_or(false);
        if running {
            continue;
        }
        if let Err(e) = t.ensure() {
            out.push(format!("[{session}] restore: {e:#}"));
            continue;
        }
        let r = crate::sessiondef::restore(&t, reg, &def, &exe, path_prefix.as_deref(), true);
        if let Ok(d) = db::Db::open(session) {
            let _ = d.add_event("hubcodes", "restore", &format!("hubcodesd restored {} after start", r.started.join(", ")));
        }
        out.push(format!("[{session}] restored after start: {}{}", r.started.join(", "), if r.errors.is_empty() { String::new() } else { format!(" (errors: {})", r.errors.join("; ")) }));
    }
    out
}

fn tend_once(st: &State, t: &Tmux) -> Vec<tend::Event> {
    if !try_tend(&t.session, "hubcodesd") {
        return vec![];
    }
    // Other devices' agents: refresh the peer cache when it is about to go stale.
    let reg = st.reg();
    tend::refresh_peers_if_due(&t.session, &reg);
    let agents = tend::agent_states(t);
    let mut mems = st.mem.lock().unwrap_or_else(|e| e.into_inner());
    let mem = mems.entry(t.session.clone()).or_default();
    let events = tend::tend(t, &reg, &agents, mem);
    for ev in &events {
        st.hub.broadcast(&Event::new("tend", &t.session, json!({ "kind": ev.kind, "text": ev.text })));
    }
    events
}

/// Run the daemon in this process: socket, phone page for `primary` when
/// `serve`, and a tend pass over every session every [`TICK_SECS`].
pub fn run(reg: Registry, primary: &str, serve: bool, webhook: Option<crate::serve::WebhookSink>) -> Result<()> {
    if ipc::alive() {
        return Err(anyhow!("hubcodesd already runs on {}", ipc::socket_path().display()));
    }
    std::fs::create_dir_all(db::state_dir())?;
    let _ = std::fs::write(pid_path(), std::process::id().to_string());
    let hub = Arc::new(Hub::default());
    let st = Arc::new(State::new(reg.clone(), hub.clone(), db::now()));
    if serve {
        let (t2, reg2, cfg) = (Tmux::named(primary), reg.clone(), reg.serve.clone());
        std::thread::spawn(move || {
            if let Err(e) = crate::serve::run(t2, reg2, cfg, webhook) {
                eprintln!("hubcodesd: serve: {e:#}");
            }
        });
    }
    if reg.sharing.listen {
        let (bind, port) = (reg.sharing.listen_bind.clone(), reg.sharing.listen_port);
        std::thread::spawn(move || {
            if let Err(e) = crate::fed::listen(&bind, port) {
                eprintln!("hubcodesd: {e:#}");
            }
        });
    }
    let (st2, primary2) = (st.clone(), primary.to_string());
    let handler: ipc::Handler = Arc::new(move |m, p| handle(&st2, &primary2, m, p));
    let hub2 = hub.clone();
    std::thread::spawn(move || {
        if let Err(e) = ipc::serve(hub2, handler) {
            eprintln!("hubcodesd: socket: {e:#}");
            std::process::exit(1);
        }
    });
    println!("hubcodesd {} on {} (pid {}); tending every {TICK_SECS}s", env!("CARGO_PKG_VERSION"), ipc::socket_path().display(), std::process::id());
    for line in restore_saved(&reg) {
        println!("{line}");
    }
    loop {
        if let Some(line) = st.reload_if_changed() {
            println!("{line}");
        }
        let sessions = sessions_with_agents();
        let mut tended = Vec::new();
        for s in &sessions {
            let t = Tmux::named(s);
            if !try_tend(s, "hubcodesd") {
                continue;
            }
            tended.push(s.clone());
            for ev in tend_once(&st, &t) {
                println!("[{s}] {}", ev.text);
            }
        }
        *st.tended.lock().unwrap_or_else(|e| e.into_inner()) = tended;
        std::thread::sleep(Duration::from_secs(TICK_SECS));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_runs_the_daemon_once_for_all_sessions() {
        let p = plist("/x/hubcodes", true, "demo");
        assert!(p.contains("<string>/x/hubcodes</string>"));
        assert!(p.contains("<string>daemon</string>"));
        assert!(p.contains("<string>run</string>"));
        assert!(p.contains("<string>--serve</string>"));
        assert!(p.contains("<key>HUBCODES_SESSION</key><string>demo</string>"));
        assert!(p.contains(LABEL));
        assert!(!plist("/x/hubcodes", false, "demo").contains("--serve"));
    }

    #[test]
    fn a_tracker_added_while_running_is_picked_up() {
        let dir = std::env::temp_dir().join(format!("hubcodes-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hubcodes.toml");
        std::fs::write(&path, "").unwrap();
        let st = State::new(Registry::from_file(&path).unwrap(), Arc::new(Hub::default()), 1);
        assert!(st.reg().trackers.is_empty());
        assert!(st.reload_if_changed().is_none(), "unchanged file reloads nothing");
        // mtime has one-second resolution on some filesystems.
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&path, "[[tracker]]\nname = \"gh\"\nkind = \"github\"\nrepo = \"o/r\"\n").unwrap();
        let line = st.reload_if_changed().expect("a changed file is read again");
        assert!(line.contains("1 tracker"), "{line}");
        assert_eq!(st.reg().trackers[0].name, "gh");
        // A broken edit keeps the last good config.
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&path, "[[tracker]\n").unwrap();
        assert!(st.reload_if_changed().unwrap().contains("does not read"));
        assert_eq!(st.reg().trackers.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_is_per_process() {
        // XDG_STATE_HOME is process-global and other tests move it, so this
        // test names its own folder instead of reading the variable twice.
        let session = format!("locktest-{}", std::process::id());
        let dir = std::env::temp_dir().join(format!("hubcodes-lock-{}", std::process::id()));
        assert!(try_tend_in(&dir, &session, "a"));
        assert!(try_tend_in(&dir, &session, "a"));
        assert_eq!(holder_in(&dir, &session).map(|h| h.0), Some("a".into()));
        release_in(&dir, &session);
        assert!(holder_in(&dir, &session).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hook_method_rejects_unknown_agent_and_ping_answers() {
        let st = State::new(Registry::load().expect("a registry (defaults when no config file)"), Arc::new(Hub::default()), 1);
        *st.tended.lock().unwrap() = vec!["s".into()];
        let v = handle(&st, "s", "ping", &Value::Null).unwrap();
        assert_eq!(v["sessions"], json!(["s"]));
        assert!(handle(&st, "s", "nope", &Value::Null).is_err());
        assert!(handle(&st, "s", "hook", &json!({"status": "idle"})).unwrap_err().to_string().contains("missing agent"));
    }

    #[test]
    fn systemd_unit_runs_the_daemon_and_quotes_paths() {
        let u = unit("/opt/my apps/hubcodes", true, "hubcodes");
        assert!(u.contains("ExecStart=\"/opt/my apps/hubcodes\" daemon run --serve\n"), "{u}");
        assert!(u.contains("Environment=HUBCODES_SESSION=hubcodes\n") && u.contains("Restart=always") && u.contains("WantedBy=default.target"));
    }

}
