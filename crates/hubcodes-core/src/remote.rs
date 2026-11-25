//! Other devices. A peer is another machine (or another tmux session on this
//! one) running hubcodes; we reach its agents by running *its* `hubcodes`
//! over SSH. Tailscale SSH or plain key-based SSH both work; the only
//! requirement is that `ssh <host> true` succeeds without a prompt.
//!
//! `host = "local"` is the same machine with a different session, handy for
//! trying the feature without a second device.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Peer {
    /// Short name used in addresses: `mini/reviewer`.
    pub name: String,
    /// SSH host (MagicDNS name, IP, or an entry in ~/.ssh/config); `local` = this machine.
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// tmux session on the peer.
    #[serde(default = "default_session")]
    pub session: String,
    /// `hubcodes` binary on the peer, when it is not on the login shell's PATH.
    #[serde(default = "default_bin", skip_serializing_if = "is_default_bin")]
    pub hubcodes: String,
    /// Refuse to start more agents than this on the device (0 = no limit),
    /// so a small Mac is not loaded past what it can carry.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_agents: u32,
    /// What that device may do here over SSH (checked when the caller can
    /// be told apart with `tailscale whois`): `send`, `task`, `spawn`,
    /// `approve` (answer agents' permission prompts), `kill`, `restart`.
    /// Empty = the default set, which leaves out approve, kill and restart;
    /// for a device signed in to another Tailscale login, empty = view only
    /// (its messages wait for approval in the Inbox).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    /// How to reach the device: `ssh` (default, empty), `http` (its
    /// hubcodesd federation listener only) or `auto` (the listener first,
    /// SSH when it is not there). See [`crate::fed`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transport: String,
    /// Port of the device's federation listener (default 7434).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

/// What a device may do here unless its `allow` says otherwise.
pub const ALLOW_DEFAULT: [&str; 3] = ["send", "task", "spawn"];
/// Everything `allow` can name.
pub const ALLOW_ALL: [&str; 6] = ["send", "task", "spawn", "approve", "kill", "restart"];

impl Peer {
    pub fn allows(&self, action: &str) -> bool {
        self.allows_as(action, false)
    }

    /// `foreign`: the device is signed in to another Tailscale login than
    /// this Mac. Such a device only reads until its `allow` names actions.
    pub fn allows_as(&self, action: &str, foreign: bool) -> bool {
        if !self.allow.is_empty() {
            self.allow.iter().any(|a| a == action || a == "all")
        } else if foreign {
            false
        } else {
            ALLOW_DEFAULT.contains(&action)
        }
    }
}

/// A request from another device, as far as it can be attributed.
#[derive(Clone, Debug)]
pub struct Caller {
    pub peer: Peer,
    /// Signed in to another Tailscale login than this Mac (someone else's device).
    pub foreign: bool,
}

/// Set with [`crate::fed::CALLER_ENV`] when the caller is someone else's device.
pub const FOREIGN_ENV: &str = "HUBCODES_FED_FOREIGN";

/// The Tailscale login (`UserProfile.LoginName`) in a whois answer.
pub fn whois_login(whois_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(whois_json).ok()?;
    v["UserProfile"]["LoginName"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_lowercase())
}

/// This Mac's Tailscale login, asked once per process.
pub fn own_login() -> Option<String> {
    static L: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    L.get_or_init(|| {
        let ip = crate::serve::tailscale_ip()?;
        let out = Command::new(tailscale_bin()?).args(["whois", "--json", &ip.to_string()]).output().ok().filter(|o| o.status.success())?;
        whois_login(&String::from_utf8_lossy(&out.stdout))
    })
    .clone()
}

/// Is the device behind this whois answer someone else's? When either
/// login cannot be told, it is treated as someone else's (fails closed).
pub fn is_foreign(whois_json: &str) -> bool {
    match (whois_login(whois_json), own_login()) {
        (Some(theirs), Some(mine)) => theirs != mine,
        _ => true,
    }
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn default_session() -> String {
    "hubcodes".into()
}
fn default_bin() -> String {
    "hubcodes".into()
}
fn is_default_bin(s: &str) -> bool {
    s == "hubcodes"
}

/// What this device calls itself in `from` labels seen by peers.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sharing {
    #[serde(default = "default_self_name")]
    pub name: String,
    /// Let configured devices reach this one through hubcodesd over the
    /// tailnet instead of SSH (`[sharing] listen = true`). Off by default.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub listen: bool,
    /// Address for that listener: `tailscale` (this Mac's tailnet IP) or an IP.
    #[serde(default = "default_listen_bind", skip_serializing_if = "is_default_listen_bind")]
    pub listen_bind: String,
    #[serde(default = "default_listen_port", skip_serializing_if = "is_default_listen_port")]
    pub listen_port: u16,
}

fn default_listen_bind() -> String {
    "tailscale".into()
}
fn is_default_listen_bind(s: &str) -> bool {
    s == "tailscale"
}
pub const DEFAULT_LISTEN_PORT: u16 = 7434;
fn default_listen_port() -> u16 {
    DEFAULT_LISTEN_PORT
}
fn is_default_listen_port(p: &u16) -> bool {
    *p == DEFAULT_LISTEN_PORT
}

impl Default for Sharing {
    fn default() -> Self {
        Sharing { name: default_self_name(), listen: false, listen_bind: default_listen_bind(), listen_port: DEFAULT_LISTEN_PORT }
    }
}

pub fn default_self_name() -> String {
    std::process::Command::new("hostname")
        .arg("-s")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "this-device".into())
}

/// One agent as the peer's `hubcodes ls --json` reports it.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgent {
    pub name: String,
    pub id: String,
    pub provider: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub dead: bool,
    /// Last screen lines, ANSI stripped.
    #[serde(default)]
    pub preview: Vec<String>,
    /// Guests of the shared canvas see its status, not its screen.
    #[serde(default)]
    pub private: bool,
    /// The agent's own hook status on its device (`idle` | `working` |
    /// `waiting`), when its tool reports one; the canvas falls back to
    /// screen changes.
    #[serde(default)]
    pub status: Option<String>,
    /// Seconds since its screen last changed, measured on its own device
    /// (`hubcodes ls --json` keeps the screen hash on the pane), so a
    /// device-less `idle` is not mistaken for a quiet agent.
    #[serde(default)]
    pub quiet_secs: f32,
    /// The account (login) it runs under on its device.
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSnapshot {
    pub session: String,
    pub agents: Vec<RemoteAgent>,
    #[serde(default)]
    pub edges: Vec<(String, String, String)>,
    /// The device's hubcodes version (missing before 0.1.8).
    #[serde(default)]
    pub version: Option<String>,
    /// What the device calls itself, and the peers it knows (name, host):
    /// enough to check the pairing without another round trip.
    #[serde(default)]
    pub self_name: Option<String>,
    #[serde(default)]
    pub peers: Vec<(String, String)>,
}

/// What is off about a pairing, one line each; empty when it is sound.
pub fn pairing_issues(snap: &RemoteSnapshot, peer: &Peer, my_name: &str, my_version: &str) -> Vec<String> {
    let mut out = Vec::new();
    match snap.version.as_deref() {
        None => out.push(format!("{} runs hubcodes older than 0.1.8 (no version in its answer); update it", peer.name)),
        Some(v) if v != my_version => out.push(format!("{} runs hubcodes {v}, this device {my_version}; keep both on the same release", peer.name)),
        _ => {}
    }
    if let Some(theirs) = snap.self_name.as_deref() {
        if theirs != peer.name {
            out.push(format!("{} calls itself \"{theirs}\" but is listed here as \"{}\"; name it \"{theirs}\" in Settings → Devices so its replies route", peer.name, peer.name));
        }
    }
    if (!snap.peers.is_empty() || snap.version.is_some()) && !snap.peers.iter().any(|(n, _)| n == my_name) {
        let listed: Vec<&str> = snap.peers.iter().map(|(n, _)| n.as_str()).collect();
        out.push(format!("{} does not list this device under the name \"{my_name}\" ({}); add it there so its agents can answer and report back", peer.name, if listed.is_empty() { "it has no devices".to_string() } else { format!("it knows {}", listed.join(", ")) }));
    }
    out
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Prepended to every command run on a device. A command over SSH gets a
/// bare PATH (`/usr/bin:/bin:…`) and `bash -l` does not read `~/.zprofile`,
/// where Homebrew puts itself on a Mac, so neither tmux nor the agents' CLIs
/// (nor hubcodes in `~/.local/bin`) would be found there.
pub const FAR_PATH: &str = "export PATH=\"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH\"; ";

/// The hubcodes path for the far side's shell: quoted, but a leading `~/`
/// still means that device's home.
fn exe_word(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => format!("\"$HOME\"/{}", sh_quote(rest)),
        None => sh_quote(path),
    }
}

/// What to do about a failed SSH hop, in words a person can act on.
pub fn ssh_hint(peer: &Peer, err: &str) -> Option<String> {
    let e = err.to_lowercase();
    let target = match &peer.user {
        Some(u) if !u.is_empty() => format!("{u}@{}", peer.host),
        _ => peer.host.clone(),
    };
    Some(if e.contains("permission denied") {
        format!("this device's SSH key is not on {host} yet: run `ssh-copy-id {target}` once in Terminal (it asks for {host}'s login password), or Settings → Devices → Pair with another device, step 3", host = peer.host)
    } else if e.contains("connection refused") {
        format!("Remote Login is off on {}: there, System Settings → General → Sharing → Remote Login", peer.host)
    } else if e.contains("could not resolve") || e.contains("timed out") || e.contains("no route") || e.contains("host is down") || e.contains("network is unreachable") {
        format!("{} cannot be reached: check that it is awake and that Tailscale is on, on both devices (`tailscale status`)", peer.host)
    } else if e.contains("host key verification failed") || e.contains("remote host identification has changed") {
        format!("{}'s SSH host key changed since the last connection; if that is expected (reinstall, new computer) run `ssh-keygen -R {}` and try again", peer.host, peer.host)
    } else if e.contains("tmux not found") {
        format!("tmux is not installed on {}: there, `brew install tmux`", peer.host)
    } else if e.contains("command not found") || e.contains("no such file") {
        format!("hubcodes is not at `{}` on {}: open hubcodes.app there once (it installs ~/.local/bin/hubcodes), or set the path in Settings → Devices", peer.hubcodes, peer.host)
    } else {
        return None;
    })
}

/// Run `hubcodes <args>` on the peer and return stdout: through its
/// federation listener when `transport` asks for it, over SSH otherwise.
pub fn run(peer: &Peer, args: &[&str], limit: Duration) -> Result<String> {
    match peer.transport.as_str() {
        "http" => crate::fed::call(peer, args, limit).map_err(|e| e.into_error(peer)),
        "auto" if !crate::fed::fell_back(peer) => match crate::fed::call(peer, args, limit) {
            Ok(out) => Ok(out),
            Err(crate::fed::CallError::Unavailable(why)) => {
                crate::fed::remember_fallback(peer, &why);
                run_ssh(peer, args, limit)
            }
            Err(e) => Err(e.into_error(peer)),
        },
        _ => run_ssh(peer, args, limit),
    }
}

/// The transport `run` would use for this peer right now (doctor, peer check).
pub fn transport_in_use(peer: &Peer) -> &'static str {
    match peer.transport.as_str() {
        "http" => "http",
        "auto" if !crate::fed::fell_back(peer) => "http",
        _ => "ssh",
    }
}

/// `hubcodes <args>` on the peer over SSH (or a local shell for `host = "local"`).
fn run_ssh(peer: &Peer, args: &[&str], limit: Duration) -> Result<String> {
    let remote_cmd = format!(
        "{FAR_PATH}{} --session {} {}",
        exe_word(&peer.hubcodes),
        sh_quote(&peer.session),
        args.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" ")
    );
    let mut cmd = if peer.host == "local" {
        let mut c = Command::new("/bin/sh");
        c.args(["-lc", &remote_cmd]);
        c
    } else {
        let mut c = Command::new("ssh");
        // Login shell on the far side so nvm/cargo/brew paths are there.
        c.args(ssh_options());
        c.args(["-T", &ssh_target(peer), "--", "bash", "-lc", &sh_quote(&remote_cmd)]);
        c
    };
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().with_context(|| format!("start ssh to {}", peer.host))?;
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait()? {
            Some(st) => {
                use std::io::Read;
                let mut out = String::new();
                let mut err = String::new();
                if let Some(mut o) = child.stdout.take() {
                    let _ = o.read_to_string(&mut out);
                }
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                if !st.success() {
                    let last = err.trim().lines().last().unwrap_or("failed").to_string();
                    let hint = ssh_hint(peer, &err).map(|h| format!(" ({h})")).unwrap_or_default();
                    return Err(anyhow!("{} on {}: {last}{hint}", args.first().unwrap_or(&"hubcodes"), peer.host));
                }
                return Ok(out);
            }
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(40)),
            None => {
                let _ = child.kill();
                return Err(anyhow!("{} did not answer in {} s", peer.host, limit.as_secs()));
            }
        }
    }
}

/// `user@host`, or the host alone when no user is set.
fn ssh_target(peer: &Peer) -> String {
    match &peer.user {
        Some(u) if !u.is_empty() => format!("{u}@{}", peer.host),
        _ => peer.host.clone(),
    }
}

/// Options for every ssh and scp to a device. One connection per host,
/// kept for a minute and shared by every poll, send and file copy
/// (ControlMaster), so a 3 s poll does not pay a handshake each time.
/// accept-new: the first connection to a device records its host key
/// instead of failing with "Host key verification failed"; a key that
/// changes later is still refused.
fn ssh_options() -> Vec<String> {
    let mut v: Vec<String> = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=accept-new", "-o", "ControlMaster=auto", "-o", "ControlPersist=60"].iter().map(|s| s.to_string()).collect();
    v.push("-o".into());
    v.push(format!("ControlPath={}", control_path().display()));
    v
}

/// Run a command (sh syntax) on the device and return stdout.
fn ssh_sh(peer: &Peer, script: &str, limit: Duration) -> Result<String> {
    let mut c = Command::new("ssh");
    c.args(ssh_options());
    c.args(["-T", &ssh_target(peer), "--", "sh", "-c", &sh_quote(script)]);
    wait_out(c, limit, &format!("ssh to {}", peer.name), peer)
}

/// Wait for a command with a deadline; stdout on success, the last stderr
/// line (plus the setup hint) on failure.
fn wait_out(mut cmd: Command, limit: Duration, what: &str, peer: &Peer) -> Result<String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().with_context(|| format!("start {what}"))?;
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait()? {
            Some(st) => {
                use std::io::Read;
                let (mut out, mut err) = (String::new(), String::new());
                if let Some(mut o) = child.stdout.take() {
                    let _ = o.read_to_string(&mut out);
                }
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                if !st.success() {
                    let last = err.trim().lines().last().unwrap_or("failed").to_string();
                    let hint = ssh_hint(peer, &err).map(|h| format!(" ({h})")).unwrap_or_default();
                    return Err(anyhow!("{what}: {last}{hint}"));
                }
                return Ok(out);
            }
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(40)),
            None => {
                let _ = child.kill();
                return Err(anyhow!("{what} did not finish in {} s", limit.as_secs()));
            }
        }
    }
}

/// Copy a local file to the device's attachments folder and return its
/// path there, for a message to an agent on that device. Same SSH
/// connection as everything else (scp shares the ControlMaster). A device
/// on `host = "local"` (tests) is this machine: the path stays.
pub fn push_file(peer: &Peer, local: &std::path::Path) -> Result<String> {
    crate::attach::check(local)?;
    if peer.host == "local" {
        return Ok(local.display().to_string());
    }
    let name = crate::attach::safe_name(&local.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    // Their attachments folder, made if missing; `pwd -P` gives the absolute path the agent there can open.
    let dir = ssh_sh(peer, "d=\"${XDG_STATE_HOME:-$HOME/.local/state}/hubcodes/attachments\"; mkdir -p \"$d\" && cd \"$d\" && pwd -P", Duration::from_secs(15))?;
    let dir = dir.trim();
    if dir.is_empty() || !dir.starts_with('/') || dir.contains(char::is_whitespace) {
        return Err(anyhow!("{} did not say where its attachments folder is", peer.name));
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_millis()).unwrap_or(0);
    let far = format!("{dir}/{stamp}-{name}");
    let mut c = Command::new("scp");
    c.args(ssh_options());
    // -q: no progress meter. The path is not quoted: scp speaks SFTP by
    // default now, where quotes would become part of the name; safe_name
    // keeps the file part to one plain word, and a folder with a space is refused above.
    c.args(["-q", "-p"]).arg(local).arg(format!("{}:{far}", ssh_target(peer)));
    let size_mb = std::fs::metadata(local).map(|m| m.len() / (1024 * 1024)).unwrap_or(0);
    wait_out(c, Duration::from_secs(30 + size_mb * 2), &format!("copy {} to {}", local.display(), peer.name), peer)?;
    Ok(far)
}

/// Where the shared SSH connections live (`%C` = hash of host, port, user);
/// kept short because unix socket paths are limited.
fn control_path() -> std::path::PathBuf {
    let dir = crate::db::state_dir().join("ssh");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("%C")
}

pub fn snapshot(peer: &Peer) -> Result<RemoteSnapshot> {
    let out = run(peer, &["ls", "--json", "--preview"], Duration::from_secs(12))?;
    serde_json::from_str(&out).with_context(|| format!("parse `hubcodes ls --json` from {}", peer.host))
}

/// `direct`: a person's message sent past the lead on purpose (`--direct`),
/// which the device must honour too.
pub fn send(peer: &Peer, agent: &str, text: &str, from: &str, direct: bool) -> Result<()> {
    run(peer, &send_args(agent, text, from, direct), Duration::from_secs(15)).map(|_| ())
}

/// The `hubcodes send …` arguments for a device (also what the queue keeps).
pub fn send_args<'a>(agent: &'a str, text: &'a str, from: &'a str, direct: bool) -> Vec<&'a str> {
    let mut v = vec!["send", agent, text, "--from", from];
    if direct {
        v.push("--direct");
    }
    v
}

/// The peer's providers with the model catalogs its tools carry there
/// (`hubcodes providers --json` on that device).
pub fn provider_statuses(peer: &Peer) -> Result<Vec<crate::models::ProviderStatus>> {
    let out = run(peer, &["providers", "--json"], Duration::from_secs(45))?;
    serde_json::from_str(&out).with_context(|| format!("parse `hubcodes providers --json` from {}", peer.host))
}

/// The skills on the peer and its providers that have each (`hubcodes skill list --json` there).
pub fn skills(peer: &Peer) -> Result<Vec<crate::skills::Skill>> {
    let out = run(peer, &["skill", "list", "--json"], Duration::from_secs(20))?;
    serde_json::from_str(&out).with_context(|| format!("parse `hubcodes skill list --json` from {}", peer.host))
}

/// One skill from the peer, packed (`hubcodes skill pack <name>` there).
pub fn skill_pack(peer: &Peer, name: &str) -> Result<String> {
    if !crate::skills::valid_name(name) {
        return Err(anyhow!("not a skill name: {name}"));
    }
    run(peer, &["skill", "pack", name], Duration::from_secs(60))
}

/// The trackers the peer polls, without secrets on peers that redact them
/// (`hubcodes tracker list --json` there; secrets from an older peer are
/// dropped here before anything keeps them).
pub fn trackers(peer: &Peer) -> Result<Vec<crate::tracker::Tracker>> {
    let out = run(peer, &["tracker", "list", "--json"], Duration::from_secs(20))?;
    let list: Vec<crate::tracker::Tracker> = serde_json::from_str(&out).with_context(|| format!("parse `hubcodes tracker list --json` from {}", peer.host))?;
    Ok(list.iter().map(crate::tracker::redacted).collect())
}

/// Start an agent on the peer by running its `hubcodes spawn`. Returns the
/// canvas name `peer/agent`.
pub fn spawn(peer: &Peer, name: &str, provider: &str, cwd: Option<&str>, role: Option<&str>, model: Option<&str>) -> Result<String> {
    spawn_account(peer, name, provider, cwd, role, model, None)
}

/// [`spawn`] under one of the device's accounts (a separate login there).
#[allow(clippy::too_many_arguments)]
pub fn spawn_account(peer: &Peer, name: &str, provider: &str, cwd: Option<&str>, role: Option<&str>, model: Option<&str>, account: Option<&str>) -> Result<String> {
    if peer.max_agents > 0 {
        let running = snapshot(peer)?.agents.into_iter().filter(|a| !a.dead && a.provider != "shell" && a.provider != "proxy").count();
        if running >= peer.max_agents as usize {
            return Err(anyhow!("{} already runs {running} agents, its limit (max_agents = {}); stop one there or raise the limit in Settings → Devices", peer.name, peer.max_agents));
        }
    }
    let mut args: Vec<&str> = vec!["spawn", name, "-p", provider];
    if let Some(c) = cwd.filter(|c| !c.trim().is_empty()) {
        args.extend(["--cwd", c]);
    }
    if let Some(r) = role.filter(|r| !r.trim().is_empty()) {
        args.extend(["-r", r]);
    }
    if let Some(m) = model.filter(|m| !m.trim().is_empty()) {
        args.extend(["-m", m]);
    }
    if let Some(a) = account.filter(|a| !a.trim().is_empty() && *a != crate::accounts::DEFAULT) {
        args.extend(["--account", a]);
    }
    run(peer, &args, Duration::from_secs(30))?;
    Ok(format!("{}/{name}", peer.name))
}

// ----- the peer cache: what every process on this device knows about the
// other devices' agents without an SSH round trip of its own. The app's peer
// poll (and the daemon's) write it; briefs, the guard and `hubcodes` commands
// read it while it is fresh.

/// Cache entries older than this are ignored.
pub const CACHE_FRESH_SECS: i64 = 90;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PeerCache {
    pub at: i64,
    /// Peer name → its last good snapshot.
    pub peers: std::collections::BTreeMap<String, RemoteSnapshot>,
}

pub fn cache_path(session: &str) -> std::path::PathBuf {
    crate::db::state_dir().join(format!("peers-{session}.json"))
}

pub fn cache_write(session: &str, peers: &std::collections::BTreeMap<String, RemoteSnapshot>) -> Result<()> {
    let p = cache_path(session);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let body = serde_json::to_string(&PeerCache { at: crate::db::now(), peers: peers.clone() })?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

/// The cache when it is fresh.
pub fn cached(session: &str) -> Option<PeerCache> {
    let c: PeerCache = serde_json::from_str(&std::fs::read_to_string(cache_path(session)).ok()?).ok()?;
    (crate::db::now() - c.at <= CACHE_FRESH_SECS).then_some(c)
}

/// One of another device's agents, as the cache knows it.
#[derive(Clone, Debug)]
pub struct CachedAgent {
    /// `peer/agent`, the name used everywhere on this device.
    pub name: String,
    pub peer: String,
    pub provider: String,
    pub role: Option<String>,
    /// `idle` | `working` | `waiting` | `offline`; `idle` when the device does not say.
    pub status: String,
    pub dead: bool,
    /// Seconds since its screen last changed, as its device measured it.
    pub quiet_secs: f32,
}

/// Live agents on the other devices (no shells), from the fresh cache.
pub fn cached_agents(session: &str) -> Vec<CachedAgent> {
    cached(session).map(agents_of).unwrap_or_default()
}

/// [`cached_agents`] for a cache already read.
pub fn agents_of(c: PeerCache) -> Vec<CachedAgent> {
    let mut out = Vec::new();
    for (peer, snap) in c.peers {
        for a in snap.agents {
            // The provider is what counts: an agent's pane reads as `zsh` for a
            // moment while its tool starts, and that is still an agent.
            if a.provider == "shell" || a.provider == "proxy" {
                continue;
            }
            out.push(CachedAgent {
                name: format!("{peer}/{}", a.name),
                peer: peer.clone(),
                provider: a.provider,
                role: a.role,
                status: if a.dead { "offline".into() } else { a.status.clone().unwrap_or_else(|| "idle".into()) },
                dead: a.dead,
                quiet_secs: a.quiet_secs,
            });
        }
    }
    out
}

/// Ask every peer and refresh the cache; errors are per peer and do not
/// stop the others. Returns the names that failed with why.
pub fn refresh_cache(session: &str, peers: &[Peer]) -> Vec<(String, String)> {
    let mut good = cached(session).map(|c| c.peers).unwrap_or_default();
    let mut failed = Vec::new();
    for p in peers {
        match snapshot(p) {
            Ok(s) => {
                good.insert(p.name.clone(), s);
            }
            Err(e) => {
                good.remove(&p.name);
                failed.push((p.name.clone(), format!("{e:#}")));
            }
        }
    }
    good.retain(|k, _| peers.iter().any(|p| &p.name == k));
    let _ = cache_write(session, &good);
    failed
}

// ----- the remote queue: `hubcodes` commands for another device that could
// not be delivered because it was unreachable (its laptop closed, a wifi
// change). They wait in a file and every tend pass retries them, so a
// `task done mac:3` reported while the MacBook is off lands when it is back.

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedCommand {
    pub id: i64,
    pub peer: String,
    pub args: Vec<String>,
    pub at: i64,
    pub attempts: u32,
    #[serde(default)]
    pub error: String,
}

/// Give up after this long (two days): the other board has moved on.
pub const QUEUE_MAX_AGE_SECS: i64 = 48 * 3600;
/// Retry no more often than this, per command.
pub const QUEUE_RETRY_SECS: i64 = 30;

pub fn queue_path(session: &str) -> std::path::PathBuf {
    crate::db::state_dir().join(format!("remote-queue-{session}.json"))
}

pub fn queued(session: &str) -> Vec<QueuedCommand> {
    std::fs::read_to_string(queue_path(session)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn queue_write(session: &str, q: &[QueuedCommand]) -> Result<()> {
    let p = queue_path(session);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(q)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

/// Keep a command for the peer until it answers again.
pub fn queue(session: &str, peer: &str, args: &[&str], error: &str) -> Result<QueuedCommand> {
    let mut q = queued(session);
    let id = q.iter().map(|c| c.id).max().unwrap_or(0) + 1;
    let c = QueuedCommand { id, peer: peer.into(), args: args.iter().map(|a| a.to_string()).collect(), at: crate::db::now(), attempts: 1, error: error.into() };
    q.push(c.clone());
    queue_write(session, &q)?;
    Ok(c)
}

/// What happened to a queued command on a flush pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Flushed {
    Delivered(QueuedCommand),
    /// Too old; dropped with the last error.
    Expired(QueuedCommand),
}

/// Retry every queued command whose peer is configured; delivered and
/// expired ones leave the queue.
pub fn flush_queue(session: &str, peers: &[Peer]) -> Vec<Flushed> {
    let mut q = queued(session);
    if q.is_empty() {
        return Vec::new();
    }
    let now = crate::db::now();
    let mut out = Vec::new();
    let mut keep = Vec::new();
    // One SSH attempt per peer per pass: if the first command fails, the rest wait.
    let mut down: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_try: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for mut c in q.drain(..) {
        if now - c.at > QUEUE_MAX_AGE_SECS {
            out.push(Flushed::Expired(c));
            continue;
        }
        let Some(peer) = peers.iter().find(|p| p.name == c.peer) else {
            keep.push(c);
            continue;
        };
        let recent = last_try.get(&c.peer).map(|t| now - t < QUEUE_RETRY_SECS).unwrap_or(false);
        if down.contains(&c.peer) || recent {
            keep.push(c);
            continue;
        }
        let args: Vec<&str> = c.args.iter().map(|a| a.as_str()).collect();
        match run(peer, &args, Duration::from_secs(20)) {
            Ok(_) => out.push(Flushed::Delivered(c)),
            Err(e) => {
                c.attempts += 1;
                c.error = format!("{e:#}");
                down.insert(c.peer.clone());
                last_try.insert(c.peer.clone(), now);
                keep.push(c);
            }
        }
    }
    let _ = queue_write(session, &keep);
    out
}

/// How soon to ask the devices again: often while something moves there,
/// seldom when every agent sits quiet, rarely when a device is away.
pub fn next_poll_secs(cache: Option<&PeerCache>, any_unreachable: bool) -> u64 {
    if any_unreachable {
        return 60;
    }
    let Some(c) = cache else { return 15 };
    let active = c.peers.values().flat_map(|s| s.agents.iter()).any(|a| !a.dead && (a.status.as_deref() == Some("working") || a.quiet_secs < 15.0));
    if active {
        3
    } else {
        15
    }
}

// ----- `--as device/agent` on a forwarded report: who is really calling?
// The report arrives over SSH; when Tailscale is there, `tailscale whois`
// says which node the connection comes from, and it has to be the device
// the caller claims. Without Tailscale (LAN SSH, local) nothing can be
// checked and the claim is taken, as before.

/// The Tailscale CLI, when installed.
pub fn tailscale_bin() -> Option<std::path::PathBuf> {
    for p in ["/usr/local/bin/tailscale", "/opt/homebrew/bin/tailscale", "/Applications/Tailscale.app/Contents/MacOS/Tailscale"] {
        if std::path::Path::new(p).is_file() {
            return Some(p.into());
        }
    }
    which_tailscale()
}

/// Is Tailscale up here, and is each of `hosts` online in the tailnet?
/// `None` for a host the tailnet does not know (LAN SSH, `local`). Cheap
/// (local CLI, no network), so a device that is away can be watched every
/// few seconds and polled the moment it comes back.
pub fn tailnet_state(hosts: &[String]) -> Option<(bool, Vec<Option<bool>>)> {
    let out = Command::new(tailscale_bin()?).args(["status", "--json"]).output().ok()?;
    Some(parse_tailnet_state(&String::from_utf8_lossy(&out.stdout), hosts))
}

pub fn parse_tailnet_state(json: &str, hosts: &[String]) -> (bool, Vec<Option<bool>>) {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let running = v.get("BackendState").and_then(|s| s.as_str()) == Some("Running");
    let peers: Vec<&serde_json::Value> = v.get("Peer").and_then(|p| p.as_object()).map(|m| m.values().collect()).unwrap_or_default();
    let names = |p: &serde_json::Value| -> Vec<String> {
        let mut n = Vec::new();
        if let Some(d) = p.get("DNSName").and_then(|d| d.as_str()) {
            let d = d.trim_end_matches('.').to_lowercase();
            n.push(d.split('.').next().unwrap_or_default().to_string());
            n.push(d);
        }
        if let Some(h) = p.get("HostName").and_then(|h| h.as_str()) {
            n.push(h.to_lowercase());
        }
        for ip in p.get("TailscaleIPs").and_then(|i| i.as_array()).into_iter().flatten() {
            if let Some(ip) = ip.as_str() {
                n.push(ip.to_string());
            }
        }
        n
    };
    let state = hosts
        .iter()
        .map(|h| {
            let h = h.trim_end_matches('.').to_lowercase();
            peers.iter().find(|p| names(p).contains(&h)).map(|p| running && p.get("Online").and_then(|o| o.as_bool()).unwrap_or(false))
        })
        .collect();
    (running, state)
}

fn which_tailscale() -> Option<std::path::PathBuf> {
    let out = Command::new("which").arg("tailscale").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!p.is_empty()).then(|| p.into())
}

/// Does the whois answer name this peer's host? Matches the MagicDNS name,
/// its first label, the computed name, the hostname, or any tailnet address.
pub fn whois_matches(whois_json: &str, host: &str) -> bool {
    let v: serde_json::Value = match serde_json::from_str(whois_json) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let node = &v["Node"];
    let want = host.trim().trim_end_matches('.').to_lowercase();
    let want_label = want.split('.').next().unwrap_or("").to_string();
    let mut names: Vec<String> = Vec::new();
    for k in ["Name", "ComputedName"] {
        if let Some(n) = node[k].as_str() {
            let n = n.trim_end_matches('.').to_lowercase();
            names.push(n.split('.').next().unwrap_or("").to_string());
            names.push(n);
        }
    }
    if let Some(h) = node["Hostinfo"]["Hostname"].as_str() {
        names.push(h.to_lowercase());
    }
    if let Some(addrs) = node["Addresses"].as_array() {
        for a in addrs.iter().filter_map(|a| a.as_str()) {
            names.push(a.split('/').next().unwrap_or(a).to_lowercase());
        }
    }
    names.iter().any(|n| !n.is_empty() && (n == &want || n == &want_label))
}

/// Which configured device this process was started by over SSH, when that
/// can be told (`SSH_CONNECTION` plus `tailscale whois`). `None` = a local
/// call, or a remote one that cannot be attributed.
pub fn caller(peers: &[Peer]) -> Option<Peer> {
    caller_info(peers).map(|c| c.peer)
}

/// [`caller`] plus whether that device belongs to someone else.
pub fn caller_info(peers: &[Peer]) -> Option<Caller> {
    // Started by hubcodesd for a federation request: the listener already
    // matched the device and names it here.
    if let Some(name) = std::env::var(crate::fed::CALLER_ENV).ok().filter(|n| !n.is_empty()) {
        let foreign = std::env::var(FOREIGN_ENV).is_ok_and(|v| v == "1");
        return peers.iter().find(|p| p.name == name).cloned().map(|peer| Caller { peer, foreign });
    }
    let conn = std::env::var("SSH_CONNECTION").ok().filter(|c| !c.trim().is_empty())?;
    let ip = conn.split_whitespace().next().unwrap_or("").to_string();
    let ts = tailscale_bin()?;
    let out = Command::new(ts).args(["whois", "--json", &ip]).output().ok().filter(|o| o.status.success())?;
    let json = String::from_utf8_lossy(&out.stdout).to_string();
    let peer = peers.iter().find(|p| p.host != "local" && whois_matches(&json, &p.host)).cloned()?;
    Some(Caller { peer, foreign: is_foreign(&json) })
}

/// What happens to a message from the calling device.
#[derive(Debug, PartialEq, Eq)]
pub enum SendGate {
    /// A local call, or a device allowed to send.
    Deliver,
    /// Someone else's device without `send` in its allow list: the message
    /// waits in the Inbox, attributed to this device name.
    Approve(String),
}

/// Decide what to do with a `send` (or a room post) from the caller.
pub fn send_gate(peers: &[Peer]) -> Result<SendGate> {
    match caller_info(peers) {
        None => Ok(SendGate::Deliver),
        Some(c) if c.peer.allows_as("send", c.foreign) => Ok(SendGate::Deliver),
        Some(c) if c.foreign => Ok(SendGate::Approve(c.peer.name)),
        Some(c) => Err(refusal(&c.peer, "send", false)),
    }
}

/// Name a sender from another device as `device/agent` (`b/dev`,
/// `b/budi` for a person there), whatever form it arrived in.
pub fn attribute(device: &str, from: &str) -> String {
    match from.split_once('/') {
        Some((d, _)) if d == device => from.to_string(),
        _ => format!("{device}/{}", from.trim_start_matches("human:").rsplit('/').next().unwrap_or(from)),
    }
}

fn refusal(p: &Peer, action: &str, foreign: bool) -> anyhow::Error {
    let list = if !p.allow.is_empty() {
        format!("{:?}", p.allow)
    } else if foreign {
        "empty (someone else's device: view only)".to_string()
    } else {
        format!("{:?}", ALLOW_DEFAULT)
    };
    anyhow!("{} may not `{action}` on this device; its [[peer]] allow list is {list} (add \"{action}\" in Settings → Devices)", p.name)
}

/// Refuse `action` when it comes from a device whose `allow` leaves it out.
pub fn check_allowed(peers: &[Peer], action: &str) -> Result<()> {
    if let Some(c) = caller_info(peers) {
        if !c.peer.allows_as(action, c.foreign) {
            return Err(refusal(&c.peer, action, c.foreign));
        }
    }
    Ok(())
}

/// Refuse a `--as <device>/…` claim when the SSH connection demonstrably
/// comes from another Tailscale node than the device's host.
pub fn verify_as(peers: &[Peer], device: &str) -> Result<()> {
    let Some(peer) = peers.iter().find(|p| p.name == device) else {
        return Err(anyhow!("--as names an unknown device '{device}'"));
    };
    if let Some(name) = std::env::var(crate::fed::CALLER_ENV).ok().filter(|n| !n.is_empty()) {
        return if name == peer.name { Ok(()) } else { Err(anyhow!("--as {device}/… refused: this request comes from {name}")) };
    }
    let Some(conn) = std::env::var("SSH_CONNECTION").ok().filter(|c| !c.trim().is_empty()) else {
        return Ok(()); // local call, nothing to check
    };
    let ip = conn.split_whitespace().next().unwrap_or("").to_string();
    let Some(ts) = tailscale_bin() else { return Ok(()) };
    let out = match Command::new(ts).args(["whois", "--json", &ip]).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Ok(()), // not a tailnet connection: cannot tell
    };
    if whois_matches(&out, &peer.host) || peer.host == "local" {
        Ok(())
    } else {
        let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
        let who = v["Node"]["ComputedName"].as_str().or(v["Node"]["Name"].as_str()).unwrap_or(&ip).to_string();
        Err(anyhow!("--as {device}/… refused: this connection comes from {who}, not from {} ({})", peer.name, peer.host))
    }
}

/// `ssh host true` plus the peer's hubcodes version: what Settings → Devices shows.
pub fn check(peer: &Peer) -> Result<String> {
    let out = run(peer, &["--version"], Duration::from_secs(10))?;
    Ok(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mini() -> Peer {
        Peer { name: "mini".into(), host: "mac-mini".into(), user: Some("aldo".into()), session: "hubcodes".into(), hubcodes: "hubcodes".into(), max_agents: 0, allow: vec![], transport: String::new(), port: None }
    }

    #[test]
    fn tailnet_state_follows_tailscale_and_each_device() {
        let j = r#"{"BackendState":"Running","Peer":{"a":{"HostName":"Mini","DNSName":"aldo-mini.tail1.ts.net.","TailscaleIPs":["100.1.2.3"],"Online":true},"b":{"HostName":"air","DNSName":"air.tail1.ts.net.","Online":false}}}"#;
        let hosts = vec!["aldo-mini".to_string(), "air.tail1.ts.net".into(), "100.1.2.3".into(), "nas.local".into()];
        assert_eq!(parse_tailnet_state(j, &hosts), (true, vec![Some(true), Some(false), Some(true), None]));
        let stopped = j.replace("Running", "Stopped");
        assert_eq!(parse_tailnet_state(&stopped, &hosts).1[0], Some(false), "Tailscale off here: nobody is online");
    }

    #[test]
    fn a_failed_ssh_hop_says_what_to_do() {
        let p = mini();
        assert!(ssh_hint(&p, "aldo@mac-mini: Permission denied (publickey,password,keyboard-interactive).").unwrap().contains("ssh-copy-id aldo@mac-mini"));
        assert!(ssh_hint(&p, "ssh: connect to host mac-mini port 22: Connection refused").unwrap().contains("Remote Login"));
        assert!(ssh_hint(&p, "ssh: Could not resolve hostname mac-mini").unwrap().contains("Tailscale"));
        assert!(ssh_hint(&p, "Error: tmux not found on PATH (brew install tmux)").unwrap().contains("brew install tmux"));
        assert!(ssh_hint(&p, "bash: hubcodes: command not found").unwrap().contains("hubcodes.app"));
        assert!(ssh_hint(&p, "something else").is_none());
    }

    #[test]
    fn the_far_command_finds_homebrew_and_a_home_relative_hubcodes() {
        assert!(FAR_PATH.contains("/opt/homebrew/bin") && FAR_PATH.contains("$HOME/.local/bin"));
        assert_eq!(exe_word("~/.local/bin/hubcodes"), "\"$HOME\"/'.local/bin/hubcodes'");
        assert_eq!(exe_word("/usr/bin/hub codes"), "'/usr/bin/hub codes'");
        let out = Command::new("/bin/sh").args(["-c", &format!("HOME=/h; echo {}", exe_word("~/x y"))]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "/h/x y");
    }

    #[test]
    fn direct_travels_with_a_message_to_another_device() {
        assert_eq!(send_args("dev", "hi", "human:aldo", true), vec!["send", "dev", "hi", "--from", "human:aldo", "--direct"]);
        assert!(!send_args("dev", "hi", "mac/lead", false).contains(&"--direct"));
    }

    #[test]
    fn authorizing_a_key_twice_adds_it_once() {
        let home = std::env::temp_dir().join(format!("hubcodes-authkey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let cmd = authorize_command("ssh-ed25519 AAAAtest it's me");
        for _ in 0..2 {
            let st = Command::new("/bin/sh").args(["-c", &cmd]).env("HOME", &home).status().unwrap();
            assert!(st.success());
        }
        let keys = std::fs::read_to_string(home.join(".ssh/authorized_keys")).unwrap();
        assert_eq!(keys, "ssh-ed25519 AAAAtest it's me\n");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_queued_command_waits_for_its_peer_and_expires_after_two_days() {
        let session = format!("rq-{}", std::process::id());
        let _ = std::fs::remove_file(queue_path(&session));
        let c = queue(&session, "mac", &["task", "done", "3", "ok", "--as", "mini/dev"], "mac did not answer").unwrap();
        assert_eq!(c.id, 1);
        assert_eq!(queued(&session).len(), 1);
        // An unknown peer keeps the command; nothing is attempted.
        assert!(flush_queue(&session, &[]).is_empty());
        assert_eq!(queued(&session).len(), 1);
        // Old commands are dropped as expired.
        let mut q = queued(&session);
        q[0].at -= QUEUE_MAX_AGE_SECS + 1;
        queue_write(&session, &q).unwrap();
        let out = flush_queue(&session, &[]);
        assert!(matches!(out.as_slice(), [Flushed::Expired(_)]));
        assert!(queued(&session).is_empty());
        let _ = std::fs::remove_file(queue_path(&session));
    }

    #[test]
    fn pairing_issues_name_version_and_listing_problems() {
        let peer = Peer { name: "mini".into(), host: "mac-mini".into(), user: None, session: "hubcodes".into(), hubcodes: "hubcodes".into(), max_agents: 0, allow: vec![], transport: String::new(), port: None };
        let good = RemoteSnapshot { session: "hubcodes".into(), version: Some("0.1.8".into()), self_name: Some("mini".into()), peers: vec![("mac".into(), "macbook".into())], ..Default::default() };
        assert!(pairing_issues(&good, &peer, "mac", "0.1.8").is_empty());
        let old = RemoteSnapshot { version: None, ..good.clone() };
        assert!(pairing_issues(&old, &peer, "mac", "0.1.8")[0].contains("older than 0.1.8"));
        let misnamed = RemoteSnapshot { self_name: Some("studio".into()), peers: vec![], ..good.clone() };
        let issues = pairing_issues(&misnamed, &peer, "mac", "0.1.7");
        assert_eq!(issues.len(), 3, "{issues:?}");
        assert!(issues[1].contains("calls itself \"studio\"") && issues[2].contains("does not list this device"));
        assert!(peer.allows("send") && !peer.allows("approve"));
        let strict = Peer { allow: vec!["approve".into()], ..peer.clone() };
        assert!(strict.allows("approve") && !strict.allows("send"));
    }

    #[test]
    fn someone_elses_device_only_reads_until_its_allow_says_more() {
        let peer = Peer { name: "budi".into(), host: "budi-mac".into(), user: None, session: "hubcodes".into(), hubcodes: "hubcodes".into(), max_agents: 0, allow: vec![], transport: String::new(), port: None };
        assert!(peer.allows_as("send", false) && peer.allows_as("spawn", false));
        assert!(!peer.allows_as("send", true) && !peer.allows_as("task", true) && !peer.allows_as("spawn", true));
        let trusted = Peer { allow: vec!["send".into()], ..peer.clone() };
        assert!(trusted.allows_as("send", true) && !trusted.allows_as("spawn", true));
    }

    #[test]
    fn whois_login_and_attribution() {
        let j = r#"{"Node":{"Name":"budi-mac.tail.ts.net."},"UserProfile":{"LoginName":"Budi@HubStudio.id"}}"#;
        assert_eq!(whois_login(j).as_deref(), Some("budi@hubstudio.id"));
        assert_eq!(whois_login(r#"{"Node":{}}"#), None);
        assert_eq!(attribute("b", "b/dev"), "b/dev");
        assert_eq!(attribute("b", "dev"), "b/dev");
        assert_eq!(attribute("b", "human:budi"), "b/budi");
        assert_eq!(attribute("b", "x/dev"), "b/dev");
    }

    #[test]
    fn whois_names_match_the_peer_host_in_any_spelling() {
        let j = r#"{"Node":{"Name":"mac-mini.tail1234.ts.net.","ComputedName":"mac-mini","Hostinfo":{"Hostname":"Aldos-Mac-mini"},"Addresses":["100.64.0.7/32","fd7a::7/128"]}}"#;
        assert!(whois_matches(j, "mac-mini"));
        assert!(whois_matches(j, "mac-mini.tail1234.ts.net"));
        assert!(whois_matches(j, "100.64.0.7"));
        assert!(whois_matches(j, "aldos-mac-mini"));
        assert!(!whois_matches(j, "macbook"));
        assert!(!whois_matches("not json", "mac-mini"));
    }

    #[test]
    fn poll_slows_down_when_quiet_and_when_a_device_is_away() {
        let mut peers = std::collections::BTreeMap::new();
        peers.insert("mini".to_string(), RemoteSnapshot { session: "s".into(), agents: vec![RemoteAgent { name: "dev".into(), provider: "claude".into(), quiet_secs: 120.0, ..Default::default() }], ..Default::default() });
        let quiet = PeerCache { at: 0, peers: peers.clone() };
        assert_eq!(next_poll_secs(Some(&quiet), false), 15);
        peers.get_mut("mini").unwrap().agents[0].quiet_secs = 2.0;
        assert_eq!(next_poll_secs(Some(&PeerCache { at: 0, peers }), false), 3);
        assert_eq!(next_poll_secs(None, true), 60);
    }

    #[test]
    fn cached_agents_are_named_by_device_and_skip_shells() {
        let mut peers = std::collections::BTreeMap::new();
        peers.insert(
            "mini".to_string(),
            RemoteSnapshot {
                session: "hubcodes".into(),
                agents: vec![
                    RemoteAgent { name: "reviewer".into(), provider: "codex".into(), role: Some("reviewer".into()), status: Some("working".into()), ..Default::default() },
                    RemoteAgent { name: "dev".into(), provider: "claude".into(), role: Some("software engineer".into()), ..Default::default() },
                    RemoteAgent { name: "sh".into(), provider: "shell".into(), command: "zsh".into(), ..Default::default() },
                    RemoteAgent { name: "gone".into(), provider: "claude".into(), dead: true, ..Default::default() },
                ],
                ..Default::default()
            },
        );
        let got = agents_of(PeerCache { at: crate::db::now(), peers });
        let names: Vec<&str> = got.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["mini/reviewer", "mini/dev", "mini/gone"]);
        assert_eq!(got[0].status, "working");
        assert_eq!(got[1].status, "idle", "no hook status means idle");
        assert_eq!(got[2].status, "offline");
        assert_eq!(got[0].peer, "mini");
    }
}

// ----- Pairing help: what another device needs to reach this one.

/// What a person types on another device to add this one, and whether this
/// Mac is ready to be reached (Remote Login on, an SSH key to hand out).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThisDevice {
    /// Login name here: the "User" on the other device.
    pub user: String,
    /// Short Tailscale name (MagicDNS), else the local host name: the "SSH host" there.
    pub host: String,
    pub tailscale: bool,
    /// Tailscale is installed, whether or not it runs (offer Open rather than Install).
    pub tailscale_installed: bool,
    /// Something listens on port 22 here (System Settings → Sharing → Remote Login).
    pub remote_login: bool,
    /// `~/.ssh/id_ed25519.pub` (or the first other key there), to add to the other device's authorized_keys.
    pub public_key: Option<String>,
    /// One line to paste in Terminal on the other device so this Mac may log in there.
    pub authorize_command: Option<String>,
}

/// Tailscale is on this Mac, running or not.
pub fn tailscale_installed() -> bool {
    tailscale_bin().is_some() || std::path::Path::new("/Applications/Tailscale.app").exists()
}

pub fn this_device() -> ThisDevice {
    let user = std::env::var("USER").unwrap_or_default();
    let ts_name = tailscale_bin()
        .and_then(|b| Command::new(b).args(["status", "--json"]).output().ok())
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v.pointer("/Self/DNSName").and_then(|d| d.as_str()).map(|d| d.split('.').next().unwrap_or("").to_string()))
        .filter(|n| !n.is_empty());
    let tailscale = ts_name.is_some();
    let host = ts_name.unwrap_or_else(|| {
        Command::new("scutil").args(["--get", "LocalHostName"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).map(|s| format!("{s}.local")).unwrap_or_default()
    });
    let remote_login = std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], 22).into(), Duration::from_millis(400)).is_ok();
    let public_key = public_key();
    let authorize_command = public_key.as_deref().map(authorize_command);
    ThisDevice { user, host, tailscale, tailscale_installed: tailscale_installed(), remote_login, public_key, authorize_command }
}

fn ssh_dir() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_default().join(".ssh")
}

fn public_key() -> Option<String> {
    let dir = ssh_dir();
    let mut names = vec!["id_ed25519.pub".to_string(), "id_ecdsa.pub".into(), "id_rsa.pub".into()];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut more: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".pub") && !names.contains(n)).collect();
        more.sort();
        names.extend(more);
    }
    names.iter().find_map(|n| std::fs::read_to_string(dir.join(n)).ok()).map(|s| s.trim().to_string()).filter(|s| s.starts_with("ssh-") || s.starts_with("ecdsa-"))
}

/// The line that adds `key` to `~/.ssh/authorized_keys` on another device,
/// once (running it twice adds nothing).
pub fn authorize_command(key: &str) -> String {
    let k = sh_quote(key);
    format!("mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && (grep -qxF {k} ~/.ssh/authorized_keys || echo {k} >> ~/.ssh/authorized_keys) && chmod 600 ~/.ssh/authorized_keys")
}

/// Create `~/.ssh/id_ed25519` (no passphrase, so agents can use it unattended)
/// when this Mac has no SSH key yet. Never overwrites one.
pub fn create_key() -> Result<String> {
    if let Some(k) = public_key() {
        return Ok(k);
    }
    let dir = ssh_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("id_ed25519");
    let comment = format!("hubcodes {}@{}", std::env::var("USER").unwrap_or_default(), this_device_name());
    let st = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-C", &comment, "-f"]).arg(&path).stdin(Stdio::null()).status().context("run ssh-keygen")?;
    if !st.success() {
        return Err(anyhow!("ssh-keygen failed"));
    }
    public_key().ok_or_else(|| anyhow!("ssh-keygen wrote no key"))
}

fn this_device_name() -> String {
    Command::new("scutil").args(["--get", "LocalHostName"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}
