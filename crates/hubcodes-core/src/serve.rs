//! The hubcodes server: a small HTTP service for phones and for teammates.
//! Serves one page (agents, status, what each needs, approve / deny, send a
//! message, the activity log), a JSON API the page uses, and `/webhook/<name>`
//! for triggers. Binds the Tailscale IP by default so only the tailnet can
//! reach it; identity comes from `tailscale whois` for that connection, and
//! `[[share]]` decides what each person may do.
//!
//! ```toml
//! [serve]
//! bind = "tailscale"        # or "127.0.0.1", "0.0.0.0", an IP
//! port = 7433
//! token = ""                # optional: ?token=… or `Authorization: Bearer …` for non-tailnet clients
//!
//! [[share]]
//! user = "budi@hubstudio.id"   # tailnet login, or "*"
//! role = "operator"            # viewer | operator | owner
//! ```

use crate::db::Db;
use crate::providers::Registry;
use crate::relay;
use crate::tmux::Tmux;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::net::IpAddr;
use std::sync::Arc;
use tiny_http::{Header, Method, Request, Response, Server};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServeConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token: String,
    #[serde(default, rename = "share", alias = "shares", skip_serializing_if = "Vec::is_empty")]
    pub shares: Vec<Share>,
    /// Folder with the built desktop UI (`desktop/dist`); served at `/canvas/`
    /// so teammates get the full canvas in a browser. Set by the app.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub web_root: String,
}

fn default_bind() -> String {
    "tailscale".into()
}
fn default_port() -> u16 {
    7433
}

impl Default for ServeConfig {
    fn default() -> Self {
        ServeConfig { bind: default_bind(), port: default_port(), token: String::new(), shares: Vec::new(), web_root: String::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Share {
    /// Tailnet login (`budi@hubstudio.id`), device name, or `*`.
    pub user: String,
    /// `viewer` sees; `contributor` sends but each message waits for an operator; `operator` sends, approves and denies; `owner` can do everything the desktop can.
    #[serde(default = "default_role")]
    pub role: String,
}

fn default_role() -> String {
    "viewer".into()
}

/// This machine's Tailscale IPv4, if tailscale is up.
pub fn tailscale_ip() -> Option<IpAddr> {
    // The App Store build keeps its CLI inside the app, off PATH.
    let out = std::process::Command::new(crate::remote::tailscale_bin()?).args(["ip", "-4"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().next()?.trim().parse().ok()
}

/// `tailscale whois <ip>`: login name of the peer behind a connection.
pub fn whois(ip: &IpAddr) -> Option<String> {
    let out = std::process::Command::new(crate::remote::tailscale_bin()?).args(["whois", "--json", &ip.to_string()]).output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v.get("UserProfile").and_then(|u| u.get("LoginName")).and_then(|l| l.as_str()).map(String::from)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    None,
    Viewer,
    /// Sees everything and may write, but every message waits for an operator's approval.
    Contributor,
    Operator,
    Owner,
}

fn parse_role(s: &str) -> Role {
    match s {
        "owner" => Role::Owner,
        "operator" => Role::Operator,
        "contributor" => Role::Contributor,
        "viewer" => Role::Viewer,
        _ => Role::None,
    }
}

/// Who is this request, and what may they do.
pub fn authorize(cfg: &ServeConfig, remote: &IpAddr, token: Option<&str>) -> (String, Role) {
    // The owner's own machine (loopback or this host's tailscale IP) is the owner.
    if remote.is_loopback() || Some(*remote) == tailscale_ip() {
        return ("me".into(), Role::Owner);
    }
    if !cfg.token.is_empty() && token == Some(cfg.token.as_str()) {
        return ("token".into(), Role::Operator);
    }
    let login = whois(remote).unwrap_or_else(|| remote.to_string());
    let role = cfg
        .shares
        .iter()
        .filter(|s| s.user == "*" || s.user.eq_ignore_ascii_case(&login))
        .map(|s| parse_role(&s.role))
        .max()
        .unwrap_or(Role::None);
    (login, role)
}

/// Resolve the bind address.
pub fn bind_addr(cfg: &ServeConfig) -> Result<String> {
    let ip = match cfg.bind.as_str() {
        "tailscale" => tailscale_ip().ok_or_else(|| anyhow!("tailscale is not running; set [serve] bind = \"127.0.0.1\" to serve locally"))?.to_string(),
        other => other.to_string(),
    };
    Ok(format!("{ip}:{}", cfg.port))
}

/// Who is looking at the canvas right now, and where their pointer is (flow coordinates).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    pub who: String,
    pub role: String,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub selected: String,
    pub at: i64,
}

static PRESENCE: std::sync::Mutex<Vec<Presence>> = std::sync::Mutex::new(Vec::new());
const PRESENCE_TTL: i64 = 12;

fn presence_set(p: Presence) {
    let mut v = PRESENCE.lock().unwrap_or_else(|e| e.into_inner());
    v.retain(|x| x.who != p.who && crate::db::now() - x.at < PRESENCE_TTL);
    v.push(p);
}

/// Everyone seen in the last few seconds.
pub fn presence() -> Vec<Presence> {
    let now = crate::db::now();
    PRESENCE.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|p| now - p.at < PRESENCE_TTL).cloned().collect()
}

fn audit(t: &Tmux, who: &str, what: String) {
    if let Ok(db) = Db::open(&t.session) {
        let _ = db.add_event(who, "share", &what);
    }
}

/// What the page needs, in one JSON document.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct View {
    session: String,
    me: String,
    role: String,
    agents: Vec<AgentView>,
    messages: Vec<crate::db::MessageRow>,
    loops: std::collections::BTreeMap<String, relay::LoopState>,
    notes: String,
    edges: Vec<EdgeView>,
    tasks: Vec<crate::tasks::Task>,
    awaiting: std::collections::BTreeMap<String, relay::Awaiting>,
    last_message: Option<MessageView>,
    presence: Vec<Presence>,
    /// Contributor messages waiting for an operator (operators and owners only).
    pending: Vec<crate::db::PendingSend>,
}

#[derive(Serialize)]
struct EdgeView {
    from: String,
    to: String,
    label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageView {
    seq: u64,
    from: String,
    to: String,
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentView {
    name: String,
    provider: String,
    role: Option<String>,
    model: Option<String>,
    status: String,
    detail: String,
    preview: Vec<String>,
    dead: bool,
    holder: Option<String>,
    test: Option<String>,
    /// Guests get this agent's status instead of its screen.
    private: bool,
    // Fields the desktop canvas expects (so the same React UI renders in a browser).
    id: String,
    cwd: String,
    command: String,
    #[serde(rename = "quietSecs")]
    quiet_secs: f32,
    #[serde(rename = "lastLine")]
    last_line: String,
    peer: Option<String>,
    worktree: Option<String>,
}

/// Everyone below the owner is a guest: previews, details and messages are
/// redacted for them, and a private agent shows only its status.
fn is_guest(role: Role) -> bool {
    role < Role::Owner
}

/// What a guest may see of an agent's screen: its status alone when the
/// agent is private, otherwise the screen with secret shapes redacted.
pub fn guest_preview(private: bool, status: &str, preview: &[String]) -> Vec<String> {
    if private {
        vec![format!("[private agent: {status}]")]
    } else {
        crate::redact::redact_lines(preview)
    }
}

/// The one-line detail a guest sees: the status word for a private agent,
/// the redacted detail otherwise.
pub fn guest_detail(private: bool, status: &str, detail: &str) -> String {
    if private {
        status.to_string()
    } else {
        crate::redact::redact(detail)
    }
}

fn view(t: &Tmux, me: &str, role: Role) -> Result<View> {
    let waiting = regex_lite_waiting();
    let guest = is_guest(role);
    let mut agents = Vec::new();
    for p in t.panes()? {
        if p.provider.is_none() || p.provider.as_deref() == Some("proxy") || p.current_command == "hubcodes" {
            continue;
        }
        let preview: Vec<String> = t.capture(&p.id, 10).unwrap_or_default().iter().map(|l| strip_ansi(l)).collect();
        let last = preview.iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
        let fresh = p.hook_at.map(|at| crate::db::now() - at < crate::hooks::FRESH_SECS).unwrap_or(false);
        let status = if p.dead {
            "offline".to_string()
        } else if fresh {
            p.hook_status.clone().unwrap_or_else(|| "idle".into())
        } else if waiting(&last) && !matches!(p.current_command.as_str(), "zsh" | "bash" | "fish" | "sh") {
            "waiting".into()
        } else {
            "idle".into()
        };
        let detail = if fresh { p.hook_detail.clone().unwrap_or(last) } else { last };
        let (preview, detail) = if guest {
            (guest_preview(p.private, &status, &preview), guest_detail(p.private, &status, &detail))
        } else {
            (preview, detail)
        };
        let cwd = std::process::Command::new("tmux").args(["display-message", "-p", "-t", &p.id, "#{pane_current_path}"]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        agents.push(AgentView {
            id: p.name.clone(),
            cwd,
            command: p.current_command.clone(),
            quiet_secs: if status == "working" { 0.0 } else { 30.0 },
            last_line: detail.clone(),
            peer: None,
            worktree: p.worktree.clone(),
            name: p.name,
            provider: p.provider.unwrap_or_default(),
            role: p.role,
            model: p.model,
            status,
            detail,
            preview,
            dead: p.dead || p.exited,
            holder: p.holder,
            test: p.test,
            private: p.private,
        });
    }
    let db = Db::open(&t.session).ok();
    let mut messages = db.as_ref().and_then(|d| d.messages(60, None).ok()).unwrap_or_default();
    let tasks = db.as_ref().and_then(|d| crate::tasks::list(d, false).ok()).unwrap_or_default();
    let mut last_message = t.last_message().ok().flatten().map(|(seq, from, to, text)| MessageView { seq, from, to, text });
    if guest {
        for m in &mut messages {
            m.text = crate::redact::redact(&m.text);
        }
        if let Some(m) = last_message.as_mut() {
            m.text = crate::redact::redact(&m.text);
        }
    }
    Ok(View {
        session: t.session.clone(),
        me: me.into(),
        role: format!("{role:?}").to_lowercase(),
        agents,
        messages,
        loops: relay::loops(t),
        notes: crate::notes::read(&t.session),
        edges: t.edges().unwrap_or_default().into_iter().map(|(from, to, label)| EdgeView { from, to, label }).collect(),
        tasks,
        awaiting: relay::awaiting(t),
        last_message,
        presence: presence(),
        pending: if role >= Role::Operator { Db::open(&t.session).and_then(|db| db.pending_open()).unwrap_or_default() } else { Vec::new() },
    })
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn regex_lite_waiting() -> impl Fn(&str) -> bool {
    |l: &str| {
        let l = l.trim().to_lowercase();
        l.ends_with('?') || l.contains("(y/n)") || l.contains("[y/n]") || l.contains("allow") || l.contains("approve") || l.contains("permission")
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn json<T: Serialize>(v: &T, status: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(v).unwrap_or_default();
    Response::from_data(body)
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
}

fn text(body: &str, status: u16, ctype: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.as_bytes().to_vec()).with_status_code(status).with_header(Header::from_bytes("Content-Type", ctype).unwrap())
}

#[derive(Deserialize)]
struct SendBody {
    to: String,
    text: String,
}

#[derive(Deserialize)]
struct AnswerBody {
    agent: String,
    approve: bool,
}

/// Fired by `POST /webhook/<name>`: the trigger runner picks these up.
pub type WebhookSink = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Run until the process ends. `webhook` receives `(name, body)`.
pub fn run(t: Tmux, reg: Registry, cfg: ServeConfig, webhook: Option<WebhookSink>) -> Result<()> {
    let addr = bind_addr(&cfg)?;
    let server = Server::http(&addr).map_err(|e| anyhow!("listen on {addr}: {e}"))?;
    eprintln!("[hubcodes] serving http://{addr}/  (bind {})", cfg.bind);
    for req in server.incoming_requests() {
        let t = t.clone();
        let reg = reg.clone();
        let cfg = cfg.clone();
        let hook = webhook.clone();
        std::thread::spawn(move || {
            if let Err(e) = handle(req, &t, &reg, &cfg, hook) {
                eprintln!("[hubcodes] serve: {e:#}");
            }
        });
    }
    Ok(())
}

fn handle(mut req: Request, t: &Tmux, reg: &Registry, cfg: &ServeConfig, webhook: Option<WebhookSink>) -> Result<()> {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = path.to_string();
    let token_q = query.split('&').find_map(|kv| kv.strip_prefix("token=")).map(String::from);
    let token_h = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .and_then(|h| h.value.as_str().strip_prefix("Bearer ").map(String::from));
    let token = token_q.or(token_h);
    let remote = req.remote_addr().map(|a| a.ip()).unwrap_or(IpAddr::from([127, 0, 0, 1]));
    let (who, role) = authorize(cfg, &remote, token.as_deref());
    // `?session=<name>` looks at another tmux session on this Mac (hubcodesd
    // tends them all; the page offers the switch). Unknown names fall back.
    let other = query.split('&').find_map(|kv| kv.strip_prefix("session=")).map(|v| v.replace("%20", " ")).filter(|v| !v.is_empty() && *v != t.session);
    let switched = other.map(|name| Tmux::named(&name)).filter(|x| x.has_session());
    let t = switched.as_ref().unwrap_or(t);
    let mut body = String::new();
    if *req.method() == Method::Post {
        let _ = req.as_reader().take(1 << 20).read_to_string(&mut body);
    }

    // Webhooks need no identity beyond the token (when set).
    if let Some(name) = path.strip_prefix("/webhook/") {
        if !cfg.token.is_empty() && token.as_deref() != Some(cfg.token.as_str()) && role < Role::Operator {
            return req.respond(text("forbidden", 403, "text/plain")).context("respond");
        }
        if let Some(h) = webhook {
            h(name, &body);
        }
        return req.respond(json(&serde_json::json!({ "ok": true, "webhook": name }), 200)).context("respond");
    }
    if role == Role::None {
        return req.respond(text(&format!("hubcodes: {who} is not shared with. Add [[share]] user = \"{who}\" to hubcodes.toml.\n"), 403, "text/plain")).context("respond");
    }
    // The full canvas (built desktop UI) at /canvas/, when the app told us where it is.
    if path == "/canvas" || path.starts_with("/canvas/") {
        if cfg.web_root.is_empty() {
            return req.respond(text("The canvas is served by the hubcodes desktop app (Settings → Remote access) or `hubcodes serve --web <dist>`.", 404, "text/plain")).context("respond");
        }
        let rel = path.trim_start_matches("/canvas").trim_start_matches('/');
        let rel = if rel.is_empty() { "index.html" } else { rel };
        let root = std::path::Path::new(&cfg.web_root);
        let file = root.join(rel);
        let safe = file.canonicalize().map(|f| f.starts_with(root.canonicalize().unwrap_or_default())).unwrap_or(false);
        let data = if safe { std::fs::read(&file).ok() } else { None };
        let resp = match data {
            Some(bytes) => Response::from_data(bytes).with_status_code(200).with_header(Header::from_bytes("Content-Type", content_type(rel)).unwrap()),
            None => match std::fs::read(root.join("index.html")) {
                Ok(b) => Response::from_data(b).with_status_code(200).with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap()),
                Err(_) => Response::from_data(b"canvas not built".to_vec()).with_status_code(404).with_header(Header::from_bytes("Content-Type", "text/plain").unwrap()),
            },
        };
        return req.respond(resp).context("respond");
    }
    let resp = match (req.method().clone(), path.as_str()) {
        (Method::Get, "/") => text(PAGE, 200, "text/html; charset=utf-8"),
        (Method::Get, "/api/history") => {
            let q = query.split('&').find_map(|kv| kv.strip_prefix("q=")).map(|v| v.replace('+', " ")).unwrap_or_default();
            match Db::open(&t.session).and_then(|db| db.messages(200, if q.is_empty() { None } else { Some(&q) })) {
                Ok(mut rows) => {
                    if is_guest(role) {
                        for r in &mut rows {
                            r.text = crate::redact::redact(&r.text);
                        }
                    }
                    json(&rows, 200)
                }
                Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 500),
            }
        }
        (Method::Get, "/api/tasks") => match Db::open(&t.session).and_then(|db| crate::tasks::list(&db, true)) {
            Ok(rows) => json(&rows, 200),
            Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 500),
        },
        (Method::Post, "/api/task") if role >= Role::Operator => {
            #[derive(Deserialize)]
            struct B { id: Option<i64>, title: Option<String>, status: Option<String>, result: Option<String>, role: Option<String>, owner: Option<String> }
            match serde_json::from_str::<B>(&body) {
                Ok(b) => {
                    let r = Db::open(&t.session).and_then(|db| match (b.id, b.title) {
                        (Some(id), _) => crate::tasks::set_status(&db, id, b.status.as_deref().unwrap_or("todo"), b.result.as_deref()),
                        (None, Some(title)) => crate::tasks::add(&db, &title, "", b.role.as_deref().unwrap_or(""), &format!("human:{who}"), b.owner.as_deref().unwrap_or("")),
                        _ => Err(anyhow!("id or title required")),
                    });
                    match r {
                        Ok(task) => json(&task, 200),
                        Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 400),
                    }
                }
                Err(e) => json(&serde_json::json!({ "error": e.to_string() }), 400),
            }
        }
        // Approving acts as this Mac's GitHub account: owners only. Ignoring only stops hubcodes.
        (Method::Post, "/api/pr") if role >= Role::Operator => {
            #[derive(Deserialize)]
            struct B { id: i64, action: String }
            match serde_json::from_str::<B>(&body) {
                Ok(b) => {
                    let r = Db::open(&t.session).and_then(|db| match b.action.as_str() {
                        "approve" if role >= Role::Owner => crate::tracker::approve(&db, b.id, &who),
                        "approve" => Err(anyhow!("only an owner may approve on GitHub from here")),
                        "ignore" => crate::tracker::ignore(&db, b.id, &who),
                        _ => Err(anyhow!("action is approve or ignore")),
                    });
                    match r {
                        Ok(msg) => json(&serde_json::json!({ "ok": true, "result": msg }), 200),
                        Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 400),
                    }
                }
                Err(e) => json(&serde_json::json!({ "error": e.to_string() }), 400),
            }
        }
        (Method::Post, "/api/loop") if role >= Role::Operator => {
            #[derive(Deserialize)]
            struct B { a: String, b: String, on: Option<bool>, goal: Option<String>, reset: Option<bool> }
            match serde_json::from_str::<B>(&body) {
                Ok(x) => {
                    let r = if x.reset.unwrap_or(false) {
                        relay::reset_loop(t, &x.a, &x.b).map(|_| 0)
                    } else if let Some(goal) = x.goal.filter(|g| !g.trim().is_empty()) {
                        relay::kickoff(t, reg, &format!("human:{who}"), &x.a, &x.b, &goal, reg.loop_cfg.max_rounds).map(|d| d.seq)
                    } else {
                        relay::set_loop(t, &x.a, &x.b, x.on.unwrap_or(true), reg.loop_cfg.max_rounds).map(|_| 0)
                    };
                    match r {
                        Ok(seq) => json(&serde_json::json!({ "ok": true, "seq": seq }), 200),
                        Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 400),
                    }
                }
                Err(e) => json(&serde_json::json!({ "error": e.to_string() }), 400),
            }
        }
        (Method::Post, "/api/link") if role >= Role::Operator => {
            #[derive(Deserialize)]
            struct B { a: String, b: String, remove: Option<bool> }
            match serde_json::from_str::<B>(&body) {
                Ok(x) => {
                    let r = if x.remove.unwrap_or(false) { t.remove_edge(&x.a, &x.b) } else { t.add_edge(&x.a, &x.b, "link") };
                    match r {
                        Ok(()) => json(&serde_json::json!({ "ok": true }), 200),
                        Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 400),
                    }
                }
                Err(e) => json(&serde_json::json!({ "error": e.to_string() }), 400),
            }
        }
        (Method::Get, "/api/sessions") => json(&serde_json::json!({ "current": t.session, "sessions": crate::daemon::sessions_with_agents() }), 200),
        (Method::Get, "/api/view") => match view(t, &who, role) {
            Ok(v) => json(&v, 200),
            Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 500),
        },
        (Method::Post, "/api/presence") if role >= Role::Viewer => {
            #[derive(Deserialize)]
            struct B { x: f64, y: f64, #[serde(default)] selected: String }
            match serde_json::from_str::<B>(&body) {
                Ok(b) => {
                    presence_set(Presence { who: who.clone(), role: format!("{role:?}").to_lowercase(), x: b.x, y: b.y, selected: b.selected, at: crate::db::now() });
                    json(&serde_json::json!({ "ok": true }), 200)
                }
                Err(e) => json(&serde_json::json!({ "error": e.to_string() }), 400),
            }
        }
        (Method::Post, "/api/send") if role == Role::Contributor => match serde_json::from_str::<SendBody>(&body) {
            Ok(b) => match Db::open(&t.session).and_then(|db| db.pending_add(&who, &b.to, &b.text)) {
                Ok(id) => {
                    audit(t, &who, format!("asked to send to {}: {}", b.to, b.text.chars().take(120).collect::<String>()));
                    json(&serde_json::json!({ "ok": true, "pending": id }), 202)
                }
                Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 500),
            },
            Err(e) => json(&serde_json::json!({ "error": e.to_string() }), 400),
        },
        (Method::Get, "/api/pending") if role >= Role::Operator => match Db::open(&t.session).and_then(|db| db.pending_open()) {
            Ok(rows) => json(&rows, 200),
            Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 500),
        },
        (Method::Post, "/api/pending") if role >= Role::Operator => {
            #[derive(Deserialize)]
            struct B { id: i64, approve: bool }
            match serde_json::from_str::<B>(&body) {
                Ok(b) => match decide_pending(t, reg, b.id, b.approve, &who) {
                    Ok(msg) => json(&serde_json::json!({ "ok": true, "result": msg }), 200),
                    Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 400),
                },
                Err(e) => json(&serde_json::json!({ "error": e.to_string() }), 400),
            }
        }
        (Method::Get, "/api/audit") if role >= Role::Operator => match Db::open(&t.session).and_then(|db| db.audit(100)) {
            Ok(rows) => json(&rows, 200),
            Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 500),
        },
        (Method::Post, "/api/send") if role >= Role::Operator => match serde_json::from_str::<SendBody>(&body) {
            Ok(b) => {
                audit(t, &who, format!("sent to {}: {}", b.to, b.text.chars().take(120).collect::<String>()));
                // Two people typing into one agent at once is the one thing sharing must not allow.
                let held = t.pane_by_name(&b.to).ok().map(|p| t.check_hold(&p.id, &who)).unwrap_or(Ok(()));
                match held.and_then(|_| relay::deliver_from_person(t, reg, &format!("human:{who}"), &b.to, &b.text, false, false).map(|(d, _)| d)) {
                    Ok(d) => {
                        if let Ok(p) = t.pane_by_name(&b.to) {
                            let _ = t.hold(&p.id, &who);
                        }
                        json(&serde_json::json!({ "ok": true, "seq": d.seq }), 200)
                    }
                    Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 409),
                }
            }
            Err(e) => json(&serde_json::json!({ "error": e.to_string() }), 400),
        },
        (Method::Get, "/api/events") => match Db::open(&t.session).and_then(|db| db.events(100, None)) {
            Ok(ev) => json(&ev, 200),
            Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 500),
        },
        (Method::Post, "/api/answer") if role >= Role::Operator => match serde_json::from_str::<AnswerBody>(&body) {
            Ok(b) => match relay::answer(t, reg, &b.agent, b.approve, &format!("human:{who}")) {
                Ok(()) => {
                    audit(t, &who, format!("{} {}", if b.approve { "approved" } else { "denied" }, b.agent));
                    json(&serde_json::json!({ "ok": true }), 200)
                }
                Err(e) => json(&serde_json::json!({ "error": format!("{e:#}") }), 400),
            },
            Err(e) => json(&serde_json::json!({ "error": e.to_string() }), 400),
        },
        (Method::Post, "/api/note") if role >= Role::Operator => {
            let _ = crate::notes::add(&t.session, &format!("human:{who}"), &body);
            audit(t, &who, format!("added a note: {}", body.chars().take(120).collect::<String>()));
            json(&serde_json::json!({ "ok": true }), 200)
        }
        (Method::Post, _) => json(&serde_json::json!({ "error": format!("{who} is a {role:?}; operators can send and approve, contributors' messages wait for approval") }), 403),
        _ => text("not found", 404, "text/plain"),
    };
    req.respond(resp).context("respond")
}

/// Approve (deliver as `human:<by>`) or deny a contributor's message. Used by
/// the web page and by the desktop inbox.
pub fn decide_pending(t: &Tmux, reg: &Registry, id: i64, approve: bool, by: &str) -> Result<String> {
    let db = Db::open(&t.session)?;
    let item = db.pending_decide(id, approve, by)?.ok_or_else(|| anyhow!("pending message #{id} is not open"))?;
    if approve {
        // A person on the shared page is `human:<login>`; a message parked
        // from another device keeps its `device/agent` sender. A room post
        // goes to the room.
        let from = if item.by.contains('/') { item.by.clone() } else { format!("human:{}", item.by) };
        if let Some(room) = item.to.strip_prefix("room:") {
            let n = crate::rooms::post(t, reg, room, &from, &item.text)?;
            audit(t, by, format!("approved #{id} from {} to {}", item.by, item.to));
            return Ok(format!("approved and posted to room {room} ({n} members)"));
        }
        let d = relay::deliver(t, reg, &from, &item.to, &item.text)?;
        audit(t, by, format!("approved #{id} from {} to {}", item.by, item.to));
        Ok(if d.queued { format!("approved; queued for {} until its prompt", item.to) } else { format!("approved and sent to {}", item.to) })
    } else {
        audit(t, by, format!("denied #{id} from {} to {}", item.by, item.to));
        Ok("denied".into())
    }
}

/// One file, no build step: works on a phone over the tailnet.
pub const PAGE: &str = include_str!("serve.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_from_shares() {
        let cfg = ServeConfig {
            bind: "127.0.0.1".into(),
            port: 1,
            token: "secret".into(),
            shares: vec![Share { user: "budi@hubstudio.id".into(), role: "operator".into() }, Share { user: "*".into(), role: "viewer".into() }],
            web_root: String::new(),
        };
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(authorize(&cfg, &lo, None).1, Role::Owner);
        let far: IpAddr = "100.64.0.9".parse().unwrap();
        assert_eq!(authorize(&cfg, &far, Some("secret")).1, Role::Operator);
        // Unknown tailnet user without a token: the wildcard viewer share applies (whois fails here, ip stands in).
        assert_eq!(authorize(&cfg, &far, None).1, Role::Viewer);
        assert!(Role::Operator > Role::Viewer);
        assert!(Role::Contributor > Role::Viewer && Role::Contributor < Role::Operator);
        assert_eq!(parse_role("contributor"), Role::Contributor);
    }

    #[test]
    fn only_the_owner_is_not_a_guest() {
        assert!(!is_guest(Role::Owner));
        assert!(is_guest(Role::Operator));
        assert!(is_guest(Role::Contributor));
        assert!(is_guest(Role::Viewer));
    }

    #[test]
    fn private_agents_show_status_and_public_ones_are_redacted() {
        let screen = vec!["cat .env".to_string(), "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz0123".to_string(), "❯".to_string()];
        assert_eq!(guest_preview(true, "working", &screen), vec!["[private agent: working]"]);
        assert_eq!(guest_preview(false, "working", &screen), vec!["cat .env", "OPENAI_API_KEY=[redacted]", "❯"]);
        assert_eq!(guest_detail(true, "waiting", "Allow access to sk-abcdefghijklmnopqrstuvwxyz0123?"), "waiting");
        assert_eq!(guest_detail(false, "waiting", "Allow access to sk-abcdefghijklmnopqrstuvwxyz0123?"), "Allow access to [redacted]?");
    }
}
