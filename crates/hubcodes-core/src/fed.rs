//! Federation: devices reach each other's `hubcodesd` over the tailnet
//! instead of opening an SSH session for every hop.
//!
//! The listener (`[sharing] listen = true`) runs commands for its peers:
//! `POST /fed/v1/run {session, args}`: a `hubcodes` command from a short
//! list of verbs, run here as if typed, with the calling device named in
//! [`CALLER_ENV`] so every check the CLI makes over SSH (`[[peer]] allow`,
//! `--as`) applies unchanged.
//! `GET /fed/v1/hello` answers version and name.
//! `GET /fed/v1/watch` is a long poll: it answers as soon as the agent list
//! there changes (or after [`WATCH_HOLD`]), so a device sees another's
//! cards move without polling.
//!
//! Who is calling: a tailnet address is matched to a configured `[[peer]]`
//! with `tailscale whois`; nobody else gets in. When `HUBCODES_FED_TOKEN` is
//! set, every request must also carry it (`Authorization: Bearer`). A
//! loopback request is accepted only with that token and names its device in
//! [`PEER_HEADER`] (two sessions on one Mac, and the tests).
//!
//! The client side is [`call`], used by [`crate::remote::run`] when a peer's
//! `transport` is `http` or `auto`. With `auto`, a device whose listener is
//! not there (refused, unknown route, old version) is reached over SSH and
//! the fallback is remembered for five minutes. A request the listener
//! accepted is never retried over SSH, so nothing runs twice.

use crate::remote::{Peer, DEFAULT_LISTEN_PORT};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::net::IpAddr;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Set on a command the listener runs: the device that asked for it.
pub const CALLER_ENV: &str = "HUBCODES_FED_CALLER";
/// Shared secret, read from the environment on both devices (never stored in config).
pub const TOKEN_ENV: &str = "HUBCODES_FED_TOKEN";
/// The calling device's name, used only for loopback requests.
pub const PEER_HEADER: &str = "X-Hubcodes-Peer";
/// How long `auto` keeps using SSH after the listener was not there.
const FALLBACK_FOR: Duration = Duration::from_secs(300);
/// Longest a command may run on behalf of another device.
const RUN_LIMIT: Duration = Duration::from_secs(90);
const MAX_BODY: u64 = 1 << 20;
/// Longest a watch request is held open when nothing changes.
pub const WATCH_HOLD: Duration = Duration::from_secs(25);

/// The `allow` action a federated command needs, or `None` when the verb
/// cannot be run over federation at all. `read` needs no permission.
pub fn action_for(args: &[String]) -> Option<&'static str> {
    let first = args.first().map(String::as_str)?;
    match first {
        "send" => Some("send"),
        "task" => match args.get(1).map(String::as_str) {
            Some("done" | "review") => Some("task"),
            _ => None,
        },
        "spawn" => Some("spawn"),
        // A post to a room that lives here; creating and changing rooms is local only.
        "room" => match args.get(1).map(String::as_str) {
            Some("post") => Some("send"),
            _ => None,
        },
        "approve" | "deny" => Some("approve"),
        "kill" => Some("kill"),
        "restart" => Some("restart"),
        "ls" if args.iter().any(|a| a == "--json") => Some("read"),
        "providers" | "--version" => Some("read"),
        "skill" => match args.get(1).map(String::as_str) {
            Some("list" | "pack") => Some("read"),
            _ => None,
        },
        _ => None,
    }
}

/// A session name as tmux and the state files accept it.
fn valid_session(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Tailnet addresses: 100.64.0.0/10 and fd7a:115c:a1e0::/48.
pub fn is_tailnet(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (64..128).contains(&o[1])
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    }
}

fn token() -> Option<String> {
    std::env::var(TOKEN_ENV).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Equal-length comparison that does not stop at the first difference.
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ---------------------------------------------------------------- client

/// Why a federated call did not return output.
#[derive(Debug)]
pub enum CallError {
    /// The listener is not there (refused, unknown route, not authorised):
    /// `auto` may use SSH instead.
    Unavailable(String),
    /// The device refused this command (its `allow` list).
    Refused(String),
    /// The command ran there and failed, or the answer was lost.
    Failed(String),
}

impl CallError {
    pub fn into_error(self, peer: &Peer) -> anyhow::Error {
        match self {
            CallError::Unavailable(why) => anyhow!("{} federation listener: {why}", peer.name),
            CallError::Refused(why) | CallError::Failed(why) => anyhow!("{why}"),
        }
    }
}

fn fallbacks() -> &'static Mutex<HashMap<String, Instant>> {
    static F: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Did `auto` recently find this peer's listener missing?
pub fn fell_back(peer: &Peer) -> bool {
    fallbacks().lock().map(|m| m.get(&peer.name).is_some_and(|at| at.elapsed() < FALLBACK_FOR)).unwrap_or(false)
}

pub fn remember_fallback(peer: &Peer, _why: &str) {
    if let Ok(mut m) = fallbacks().lock() {
        m.insert(peer.name.clone(), Instant::now());
    }
}

fn base_url(peer: &Peer) -> String {
    let host = if peer.host == "local" { "127.0.0.1" } else { peer.host.as_str() };
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
    format!("http://{host}:{}", peer.port.unwrap_or(DEFAULT_LISTEN_PORT))
}

/// This device's name as its peers know it (`[sharing] name`).
fn self_name() -> String {
    static N: OnceLock<String> = OnceLock::new();
    N.get_or_init(|| crate::providers::Registry::load().map(|r| r.sharing.name).unwrap_or_else(|_| crate::remote::default_self_name())).clone()
}

#[derive(Deserialize)]
struct RunReply {
    ok: bool,
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
}

/// Run `hubcodes <args>` on the peer through its federation listener.
pub fn call(peer: &Peer, args: &[&str], limit: Duration) -> std::result::Result<String, CallError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(limit))
        .timeout_connect(Some(Duration::from_secs(2)))
        .http_status_as_error(false)
        .build()
        .into();
    let url = format!("{}/fed/v1/run", base_url(peer));
    let mut req = agent.post(&url).header(PEER_HEADER, &self_name());
    if let Some(t) = token() {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }
    let body = serde_json::json!({ "session": peer.session, "args": args });
    let mut resp = match req.send_json(&body) {
        Ok(r) => r,
        // Nothing was accepted yet: safe to try SSH.
        Err(ureq::Error::Timeout(ureq::Timeout::Connect | ureq::Timeout::Resolve)) | Err(ureq::Error::ConnectionFailed) | Err(ureq::Error::HostNotFound) => {
            return Err(CallError::Unavailable("not reachable".into()))
        }
        Err(ureq::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::AddrNotAvailable) => {
            return Err(CallError::Unavailable("connection refused".into()))
        }
        Err(e) => return Err(CallError::Failed(format!("{} over federation: {e}", args.first().unwrap_or(&"hubcodes")))),
    };
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    match status {
        200 => {
            let r: RunReply = serde_json::from_str(&text).map_err(|e| CallError::Failed(format!("bad answer from {}: {e}", peer.name)))?;
            if r.ok {
                Ok(r.stdout)
            } else {
                Err(CallError::Failed(format!("{} on {}: {}", args.first().unwrap_or(&"hubcodes"), peer.name, r.stderr.trim().lines().last().unwrap_or("failed"))))
            }
        }
        403 => Err(CallError::Refused(text.trim().to_string())),
        401 | 404 | 405 => Err(CallError::Unavailable(format!("HTTP {status}: {}", text.trim()))),
        _ => Err(CallError::Failed(format!("HTTP {status} from {}: {}", peer.name, text.trim()))),
    }
}

/// What changed on the device since `etag` (empty = now): returns the new
/// etag and its agent list. Blocks up to [`WATCH_HOLD`] when nothing moves.
pub fn watch(peer: &Peer, etag: &str) -> std::result::Result<(String, crate::remote::RemoteSnapshot), CallError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(WATCH_HOLD + Duration::from_secs(20)))
        .timeout_connect(Some(Duration::from_secs(2)))
        .http_status_as_error(false)
        .build()
        .into();
    let url = format!("{}/fed/v1/watch", base_url(peer));
    let mut req = agent.get(&url).query("session", &peer.session).query("etag", etag).header(PEER_HEADER, &self_name());
    if let Some(t) = token() {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }
    let mut resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Timeout(ureq::Timeout::Connect | ureq::Timeout::Resolve)) | Err(ureq::Error::ConnectionFailed) | Err(ureq::Error::HostNotFound) => {
            return Err(CallError::Unavailable("not reachable".into()))
        }
        Err(ureq::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::AddrNotAvailable) => {
            return Err(CallError::Unavailable("connection refused".into()))
        }
        Err(e) => return Err(CallError::Failed(format!("watch {}: {e}", peer.name))),
    };
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    match status {
        200 => {
            #[derive(Deserialize)]
            struct W {
                etag: String,
                snapshot: crate::remote::RemoteSnapshot,
            }
            let w: W = serde_json::from_str(&text).map_err(|e| CallError::Failed(format!("bad watch answer from {}: {e}", peer.name)))?;
            Ok((w.etag, w.snapshot))
        }
        403 => Err(CallError::Refused(text.trim().to_string())),
        401 | 404 | 405 => Err(CallError::Unavailable(format!("HTTP {status}: {}", text.trim()))),
        _ => Err(CallError::Failed(format!("HTTP {status} from {}: {}", peer.name, text.trim()))),
    }
}

/// A fingerprint of an agent list that ignores the ever-growing quiet time
/// (only whether an agent has been still for 5 s counts, as for dispatch).
pub fn etag_of(snapshot_json: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut v: serde_json::Value = serde_json::from_str(snapshot_json).unwrap_or(serde_json::Value::Null);
    if let Some(agents) = v.get_mut("agents").and_then(|a| a.as_array_mut()) {
        for a in agents {
            let still = a.get("quiet_secs").and_then(|q| q.as_f64()).unwrap_or(0.0) >= 5.0;
            a["quiet_secs"] = serde_json::json!(still);
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.to_string().hash(&mut h);
    format!("{:016x}", h.finish())
}

// ---------------------------------------------------------------- listener

/// Where the listener binds: `tailscale` = this Mac's tailnet IP. Anything
/// that is neither tailnet nor loopback is refused, so it never sits on a LAN.
pub fn bind_addr(bind: &str, port: u16) -> Result<String> {
    let ip: IpAddr = match bind {
        "tailscale" => crate::serve::tailscale_ip().ok_or_else(|| anyhow!("tailscale is not running; federation listens on the tailnet only"))?,
        other => other.parse().with_context(|| format!("[sharing] listen_bind = {other:?} is not an IP"))?,
    };
    if !(ip.is_loopback() || is_tailnet(&ip)) {
        return Err(anyhow!("refusing to listen on {ip}: federation binds to a tailnet or loopback address only"));
    }
    Ok(if ip.is_ipv6() { format!("[{ip}]:{port}") } else { format!("{ip}:{port}") })
}

/// Serve federation requests until the process ends.
pub fn listen(bind: &str, port: u16) -> Result<()> {
    let addr = bind_addr(bind, port)?;
    let server = tiny_http::Server::http(&addr).map_err(|e| anyhow!("federation listener on {addr}: {e}"))?;
    eprintln!("hubcodesd: federation on http://{addr}/");
    for req in server.incoming_requests() {
        std::thread::spawn(move || {
            if let Err(e) = handle(req) {
                eprintln!("hubcodesd: federation: {e:#}");
            }
        });
    }
    Ok(())
}

fn reply(req: tiny_http::Request, status: u16, body: String) -> Result<()> {
    let json = body.starts_with('{');
    let mut resp = tiny_http::Response::from_string(body).with_status_code(status);
    if json {
        if let Ok(h) = tiny_http::Header::from_bytes("Content-Type", "application/json") {
            resp = resp.with_header(h);
        }
    }
    req.respond(resp).context("send federation reply")
}

fn header(req: &tiny_http::Request, name: &str) -> Option<String> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string())
}

/// The configured device behind a request, or the status and reason to refuse it.
fn identify(req: &tiny_http::Request, peers: &[Peer]) -> std::result::Result<crate::remote::Caller, (u16, String)> {
    let ip = req.remote_addr().map(|a| a.ip()).ok_or((403, "no remote address".to_string()))?;
    let want = token();
    if let Some(t) = &want {
        let got = header(req, "Authorization").and_then(|v| v.strip_prefix("Bearer ").map(str::to_string)).unwrap_or_default();
        if !same_secret(got.trim(), t) {
            return Err((401, format!("{TOKEN_ENV} does not match")));
        }
    }
    if ip.is_loopback() {
        if want.is_none() {
            return Err((401, format!("loopback calls need {TOKEN_ENV} set on both sides")));
        }
        let name = header(req, PEER_HEADER).unwrap_or_default();
        // Same Mac, holding the token: one of the owner's own sessions.
        return peers.iter().find(|p| p.name == name).cloned().map(|peer| crate::remote::Caller { peer, foreign: false }).ok_or((403, format!("'{name}' is not a configured device here")));
    }
    if !is_tailnet(&ip) {
        return Err((403, format!("{ip} is not on the tailnet")));
    }
    let out = Command::new("tailscale").args(["whois", "--json", &ip.to_string()]).output().map_err(|_| (403, "tailscale whois failed".to_string()))?;
    let json = String::from_utf8_lossy(&out.stdout).to_string();
    let peer = peers
        .iter()
        .find(|p| p.host != "local" && crate::remote::whois_matches(&json, &p.host))
        .cloned()
        .ok_or((403, format!("{ip} is not a configured device here (add it under Settings → Devices)")))?;
    Ok(crate::remote::Caller { peer, foreign: crate::remote::is_foreign(&json) })
}

/// `a=1&b=2` into a map, with `%xx` and `+` decoded.
fn query(url: &str) -> HashMap<String, String> {
    fn dec(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'+' => out.push(b' '),
                b'%' if i + 2 < b.len() => match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                },
                c => out.push(c),
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    url.split_once('?')
        .map(|(_, q)| q.split('&').filter_map(|kv| kv.split_once('=').map(|(k, v)| (dec(k), dec(v)))).collect())
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct RunRequest {
    session: String,
    args: Vec<String>,
}

fn handle(mut req: tiny_http::Request) -> Result<()> {
    let reg = crate::providers::Registry::load()?;
    let caller = match identify(&req, &reg.peers) {
        Ok(c) => c,
        Err((code, why)) => return reply(req, code, why),
    };
    let path = req.url().split('?').next().unwrap_or("").to_string();
    match (req.method(), path.as_str()) {
        (tiny_http::Method::Get, "/fed/v1/hello") => {
            let body = serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "self_name": reg.sharing.name, "caller": caller.peer.name, "foreign": caller.foreign }).to_string();
            reply(req, 200, body)
        }
        (tiny_http::Method::Get, "/fed/v1/watch") => {
            let q = query(req.url());
            let session = q.get("session").cloned().unwrap_or_default();
            if !valid_session(&session) {
                return reply(req, 400, format!("bad session name {session:?}"));
            }
            let since = q.get("etag").cloned().unwrap_or_default();
            let run = RunRequest { session, args: vec!["ls".into(), "--json".into(), "--preview".into()] };
            let until = Instant::now() + WATCH_HOLD;
            loop {
                let (ok, out, err) = exec(&caller, &run)?;
                if !ok {
                    return reply(req, 500, err.trim().to_string());
                }
                let etag = etag_of(&out);
                if etag != since || Instant::now() >= until {
                    let snapshot: serde_json::Value = serde_json::from_str(&out).unwrap_or(serde_json::Value::Null);
                    return reply(req, 200, serde_json::json!({ "etag": etag, "snapshot": snapshot }).to_string());
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        (tiny_http::Method::Post, "/fed/v1/run") => {
            let mut raw = String::new();
            req.as_reader().take(MAX_BODY).read_to_string(&mut raw)?;
            let run: RunRequest = match serde_json::from_str(&raw) {
                Ok(r) => r,
                Err(e) => return reply(req, 400, format!("bad request: {e}")),
            };
            if !valid_session(&run.session) {
                return reply(req, 400, format!("bad session name {:?}", run.session));
            }
            let Some(action) = action_for(&run.args) else {
                return reply(req, 403, format!("`{}` cannot be run over federation", run.args.first().map(String::as_str).unwrap_or("")));
            };
            // Someone else's device may still send: the CLI parks the
            // message in the Inbox for approval (remote::send_gate).
            let parked = action == "send" && caller.foreign;
            if action != "read" && !parked && !caller.peer.allows_as(action, caller.foreign) {
                let why = if caller.foreign && caller.peer.allow.is_empty() { " (someone else's device: view only)" } else { "" };
                return reply(req, 403, format!("{} may not `{action}` on this device{why}; add \"{action}\" to its allow list in Settings → Devices", caller.peer.name));
            }
            let (ok, stdout, stderr) = exec(&caller, &run)?;
            reply(req, 200, serde_json::json!({ "ok": ok, "stdout": stdout, "stderr": stderr }).to_string())
        }
        _ => reply(req, 404, "not found".into()),
    }
}

/// Run the command with this binary, attributed to the calling device.
fn exec(caller: &crate::remote::Caller, run: &RunRequest) -> Result<(bool, String, String)> {
    let exe = std::env::current_exe().context("locate hubcodes")?;
    let mut child = Command::new(exe)
        .arg("--session")
        .arg(&run.session)
        .args(&run.args)
        .env(CALLER_ENV, &caller.peer.name)
        .env(crate::remote::FOREIGN_ENV, if caller.foreign { "1" } else { "0" })
        .env_remove("SSH_CONNECTION")
        .env_remove("HUBCODES_AGENT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start hubcodes")?;
    let deadline = Instant::now() + RUN_LIMIT;
    loop {
        if let Some(st) = child.try_wait()? {
            let mut out = String::new();
            let mut err = String::new();
            if let Some(mut o) = child.stdout.take() {
                let _ = o.read_to_string(&mut out);
            }
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            return Ok((st.success(), out, err));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Ok((false, String::new(), format!("did not finish in {} s", RUN_LIMIT.as_secs())));
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_the_peer_verbs_run_over_federation() {
        assert_eq!(action_for(&a(&["send", "dev", "hi"])), Some("send"));
        assert_eq!(action_for(&a(&["task", "done", "a:1", "x"])), Some("task"));
        assert_eq!(action_for(&a(&["task", "add", "x"])), None);
        assert_eq!(action_for(&a(&["approve", "dev"])), Some("approve"));
        assert_eq!(action_for(&a(&["kill", "dev"])), Some("kill"));
        assert_eq!(action_for(&a(&["ls", "--json", "--preview"])), Some("read"));
        assert_eq!(action_for(&a(&["ls"])), None);
        assert_eq!(action_for(&a(&["daemon", "stop"])), None);
        assert_eq!(action_for(&a(&["account", "rm", "claude", "work"])), None);
        assert_eq!(action_for(&a(&["room", "post", "design", "hi"])), Some("send"));
        assert_eq!(action_for(&a(&["room", "create", "design"])), None);
        assert_eq!(action_for(&[]), None);
    }

    #[test]
    fn the_listener_binds_to_the_tailnet_or_loopback_only() {
        assert!(bind_addr("127.0.0.1", 7434).is_ok());
        assert!(bind_addr("100.101.102.103", 7434).is_ok());
        assert!(bind_addr("192.168.1.20", 7434).is_err());
        assert!(bind_addr("0.0.0.0", 7434).is_err());
        assert!(bind_addr("not an ip", 7434).is_err());
        assert!(is_tailnet(&"fd7a:115c:a1e0::1".parse().unwrap()));
        assert!(!is_tailnet(&"100.128.0.1".parse().unwrap()));
    }

    #[test]
    fn session_names_are_checked_before_they_reach_tmux() {
        assert!(valid_session("hubcodes"));
        assert!(valid_session("hubpeerB-123"));
        assert!(!valid_session(""));
        assert!(!valid_session("a b"));
        assert!(!valid_session("x;rm"));
    }

    #[test]
    fn the_watch_etag_ignores_quiet_time_but_not_still_or_busy() {
        let busy = r#"{"agents":[{"name":"dev","quiet_secs":1.0,"preview":["a"]}]}"#;
        let busy2 = r#"{"agents":[{"name":"dev","quiet_secs":3.0,"preview":["a"]}]}"#;
        let still = r#"{"agents":[{"name":"dev","quiet_secs":6.0,"preview":["a"]}]}"#;
        let still2 = r#"{"agents":[{"name":"dev","quiet_secs":60.0,"preview":["a"]}]}"#;
        let moved = r#"{"agents":[{"name":"dev","quiet_secs":1.0,"preview":["b"]}]}"#;
        assert_eq!(etag_of(busy), etag_of(busy2));
        assert_eq!(etag_of(still), etag_of(still2));
        assert_ne!(etag_of(busy), etag_of(still));
        assert_ne!(etag_of(busy), etag_of(moved));
    }

    #[test]
    fn query_strings_decode() {
        let q = query("/fed/v1/watch?session=hub-1&etag=ab%20c+d");
        assert_eq!(q.get("session").map(String::as_str), Some("hub-1"));
        assert_eq!(q.get("etag").map(String::as_str), Some("ab c d"));
        assert!(query("/x").is_empty());
        assert_eq!(query("/x?a=%zz").get("a").map(String::as_str), Some("%zz"));
        assert_eq!(query("/x?a=%2").get("a").map(String::as_str), Some("%2"));
    }

    #[test]
    fn secrets_compare_whole() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
    }
}
