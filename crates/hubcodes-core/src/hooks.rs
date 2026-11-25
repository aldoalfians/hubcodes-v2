//! Status reported by the agents themselves. Claude Code runs hook commands
//! on its lifecycle events, Codex has a `notify` command on turn end, and
//! opencode, pi and omp load a managed plugin / extension file (see
//! [`ensure_shim`]) that calls back with the generic payload. All inherit
//! `HUBCODES_AGENT` / `HUBCODES_SESSION` from the pane hubcodes started them
//! in, so `hubcodes hook <provider>` can find its own pane and write what
//! happened into pane options that every client reads:
//!
//!   `@hubcodes_status`      working | waiting | idle
//!   `@hubcodes_status_at`   unix seconds, so a stale report loses to the heuristic
//!   `@hubcodes_detail`      what it is doing or asking (tool name, permission text)
//!
//! Token usage from Claude's transcript goes to the SQLite `usage` table.

use crate::db::Db;
use crate::tmux::Tmux;
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const OPT_STATUS: &str = "@hubcodes_status";
pub const OPT_STATUS_AT: &str = "@hubcodes_status_at";
pub const OPT_DETAIL: &str = "@hubcodes_detail";
/// A hook report older than this is ignored in favour of the screen heuristic.
pub const FRESH_SECS: i64 = 600;

/// What a hook told us, normalised across providers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub status: &'static str,
    pub detail: String,
    /// `true` when the agent's turn ended (loop and task bookkeeping key off it).
    pub turn_done: bool,
    pub transcript: Option<PathBuf>,
}

/// Map a Claude Code hook payload (stdin JSON) to a report.
pub fn from_claude(payload: &Value) -> Option<Report> {
    let event = payload.get("hook_event_name").and_then(|v| v.as_str()).unwrap_or("");
    let transcript = payload.get("transcript_path").and_then(|v| v.as_str()).map(PathBuf::from);
    let r = match event {
        "UserPromptSubmit" => Report { status: "working", detail: "thinking".into(), turn_done: false, transcript },
        "PreToolUse" | "PostToolUse" => {
            let tool = payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("tool");
            let hint = payload
                .get("tool_input")
                .and_then(|i| i.get("command").or_else(|| i.get("file_path")).or_else(|| i.get("pattern")))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Report { status: "working", detail: format!("{tool} {hint}").trim().chars().take(120).collect(), turn_done: false, transcript }
        }
        "Notification" => {
            let kind = payload.get("notification_type").and_then(|v| v.as_str()).unwrap_or("");
            let msg = payload.get("message").and_then(|v| v.as_str()).unwrap_or("");
            // "Claude is waiting for your input" is the idle nudge (sent a
            // minute after the turn ended): the agent is free, not stuck on
            // a question, so it is `idle` (deliveries flow) rather than
            // `waiting` (needs you). Newer Claude Code versions name it
            // `idle` instead of `idle_prompt`, hence the message check.
            let idle_nudge = matches!(kind, "idle_prompt" | "idle") || msg.to_lowercase().contains("waiting for your input");
            match kind {
                "permission_prompt" | "elicitation_dialog" => Report { status: "waiting", detail: msg.chars().take(200).collect(), turn_done: false, transcript },
                _ if idle_nudge => Report { status: "idle", detail: "ready".into(), turn_done: false, transcript },
                _ => Report { status: "working", detail: msg.chars().take(200).collect(), turn_done: false, transcript },
            }
        }
        "Stop" | "SubagentStop" => Report { status: "idle", detail: "turn finished".into(), turn_done: event == "Stop", transcript },
        "SessionStart" => Report { status: "idle", detail: "ready".into(), turn_done: false, transcript },
        "SessionEnd" => Report { status: "offline", detail: "session ended".into(), turn_done: true, transcript },
        _ => return None,
    };
    Some(r)
}

/// Codex `notify` payload (single JSON argument).
pub fn from_codex(payload: &Value) -> Option<Report> {
    match payload.get("type").and_then(|v| v.as_str()) {
        Some("agent-turn-complete") => Some(Report {
            status: "idle",
            detail: payload.get("last-assistant-message").and_then(|v| v.as_str()).unwrap_or("turn finished").chars().take(200).collect(),
            turn_done: true,
            transcript: None,
        }),
        _ => None,
    }
}

/// The provider-neutral payload the managed shims send (opencode plugin,
/// pi / omp extensions, or anything else that can run `hubcodes hook generic`):
/// `{"status": "working|waiting|idle|offline", "detail": "…", "turn_done": bool}`.
/// `status` is required, the rest optional; `detail` is cut to 200 chars.
pub fn from_generic(payload: &Value) -> Option<Report> {
    let status = match payload.get("status").and_then(|v| v.as_str())? {
        "working" => "working",
        "waiting" => "waiting",
        "idle" => "idle",
        "offline" => "offline",
        _ => return None,
    };
    Some(Report {
        status,
        detail: payload.get("detail").and_then(|v| v.as_str()).unwrap_or("").chars().take(200).collect(),
        turn_done: payload.get("turn_done").and_then(|v| v.as_bool()).unwrap_or(false),
        transcript: None,
    })
}

/// Gemini CLI hook payload (settings.json `hooks`, stdin JSON with
/// `hook_event_name`): SessionStart → idle, BeforeTool/AfterTool → working,
/// Notification (a permission ask) → waiting, AfterAgent → idle + turn done,
/// SessionEnd → offline.
pub fn from_gemini(payload: &Value) -> Option<Report> {
    let event = payload.get("hook_event_name").and_then(|v| v.as_str())?;
    let tool = payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
    let detail = |s: String| s.chars().take(200).collect::<String>();
    Some(match event {
        "SessionStart" => Report { status: "idle", detail: String::new(), turn_done: false, transcript: None },
        "BeforeAgent" => Report { status: "working", detail: detail(payload.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string()), turn_done: false, transcript: None },
        "BeforeTool" | "AfterTool" => Report { status: "working", detail: detail(tool.to_string()), turn_done: false, transcript: None },
        "Notification" => Report { status: "waiting", detail: detail(payload.get("message").and_then(|v| v.as_str()).unwrap_or("needs an answer").to_string()), turn_done: false, transcript: None },
        "AfterAgent" => Report { status: "idle", detail: String::new(), turn_done: true, transcript: None },
        "SessionEnd" => Report { status: "offline", detail: String::new(), turn_done: true, transcript: None },
        _ => return None,
    })
}

/// Write the report to the agent's pane; returns the pane id.
pub fn apply(t: &Tmux, agent: &str, r: &Report) -> Result<String> {
    let pane = t.pane_by_name(agent).with_context(|| format!("agent {agent} not found in session {}", t.session))?;
    let now = crate::db::now();
    t.set_pane_option(&pane.id, OPT_STATUS, r.status)?;
    t.set_pane_option(&pane.id, OPT_STATUS_AT, &now.to_string())?;
    t.set_pane_option(&pane.id, OPT_DETAIL, &r.detail.replace('\n', " "))?;
    if let Ok(db) = Db::open(&t.session) {
        let _ = db.add_event(agent, "status", &format!("{} {}", r.status, r.detail));
    }
    Ok(pane.id)
}

/// Sum the usage of the newest assistant turn in a Claude transcript
/// (`.jsonl`, one message per line) into the agent's usage row.
pub fn account_claude_turn(session: &str, agent: &str, transcript: &Path) -> Result<Option<crate::db::Usage>> {
    let text = std::fs::read_to_string(transcript).with_context(|| format!("read {}", transcript.display()))?;
    // Walk back to the last user message; everything after it is this turn.
    let lines: Vec<&str> = text.lines().collect();
    let mut turn_start = 0;
    for (i, l) in lines.iter().enumerate().rev() {
        if let Ok(v) = serde_json::from_str::<Value>(l) {
            if v.get("type").and_then(|t| t.as_str()) == Some("user") && v.get("message").and_then(|m| m.get("role")).and_then(|r| r.as_str()) == Some("user") {
                let is_tool_result = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()).map(|a| a.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))).unwrap_or(false);
                if !is_tool_result {
                    turn_start = i;
                    break;
                }
            }
        }
    }
    let (mut inp, mut out, mut cr, mut cw) = (0i64, 0i64, 0i64, 0i64);
    let mut seen = std::collections::HashSet::new();
    let mut answered_with: Option<String> = None;
    for l in &lines[turn_start..] {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        // Streaming writes the same message id in several lines; count it once.
        if let Some(id) = v.get("message").and_then(|m| m.get("id")).and_then(|i| i.as_str()) {
            if !seen.insert(id.to_string()) {
                continue;
            }
        }
        if let Some(m) = v.get("message").and_then(|m| m.get("model")).and_then(|m| m.as_str()).filter(|m| !m.is_empty() && *m != "<synthetic>") {
            answered_with = Some(m.to_string());
        }
        let Some(u) = v.get("message").and_then(|m| m.get("usage")) else { continue };
        let g = |k: &str| u.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        inp += g("input_tokens");
        out += g("output_tokens");
        cr += g("cache_read_input_tokens");
        cw += g("cache_creation_input_tokens");
    }
    if inp + out + cr + cw == 0 {
        return Ok(None);
    }
    let db = Db::open(session)?;
    let t = crate::tmux::Tmux::named(session);
    let pane = t.pane_by_name(agent).ok();
    // What it really answered with: shown on its card next to what was picked,
    // and what its tokens are priced as.
    if let (Some(p), Some(m)) = (&pane, &answered_with) {
        let _ = t.set_pane_option(&p.id, crate::tmux::OPT_MODEL_SEEN, m);
    }
    let model = answered_with.or_else(|| pane.and_then(|p| p.model)).unwrap_or_default();
    let run = crate::tasks::list(&db, false).ok().and_then(|ts| ts.into_iter().find(|t| t.owner == agent && t.status == "doing").map(|t| t.run)).unwrap_or(0);
    db.add_usage_with(agent, &model, run, inp, out, cr, cw).map(Some)
}

/// The settings JSON Claude Code is started with (`--settings <file>`): one
/// hook per event, all calling back into this binary. `exe` is the absolute
/// path of `hubcodes`.
pub fn claude_settings_json(exe: &str) -> String {
    let cmd = format!("{} hook claude", shell_word(exe));
    let hook = |event: &str| format!(r#""{event}": [{{ "hooks": [{{ "type": "command", "command": {} , "timeout": 10 }}] }}]"#, serde_json::to_string(&cmd).unwrap_or_default());
    format!(
        "{{\n  \"hooks\": {{\n    {},\n    {},\n    {},\n    {},\n    {},\n    {},\n    {}\n  }}\n}}\n",
        hook("SessionStart"),
        hook("UserPromptSubmit"),
        hook("PreToolUse"),
        hook("PostToolUse"),
        hook("Notification"),
        hook("Stop"),
        hook("SessionEnd")
    )
}

fn shell_word(s: &str) -> String {
    if s.chars().all(|c| c.is_alphanumeric() || "/._-".contains(c)) { s.to_string() } else { format!("'{}'", s.replace('\'', "'\\''")) }
}

/// Where the generated settings file lives for a session.
pub fn claude_settings_path(session: &str) -> PathBuf {
    crate::db::state_dir().join(format!("claude-hooks-{session}.json"))
}

/// Write (or refresh) the settings file; returns its path.
pub fn ensure_claude_settings(session: &str, exe: &str) -> Result<PathBuf> {
    let path = claude_settings_path(session);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = claude_settings_json(exe);
    if std::fs::read_to_string(&path).map(|s| s != body).unwrap_or(true) {
        std::fs::write(&path, body)?;
    }
    Ok(path)
}

// ----- Gemini CLI: hooks live in its settings.json (no flag takes a file),
// so the user's `~/.gemini/settings.json` gets one managed group per event,
// named "hubcodes", kept idempotent. The hook command is inert outside
// hubcodes (`hubcodes hook` returns at once without HUBCODES_AGENT).

const GEMINI_EVENTS: [&str; 6] = ["SessionStart", "BeforeAgent", "BeforeTool", "AfterTool", "Notification", "AfterAgent"];

/// `~/.gemini/settings.json` (`GEMINI_CLI_HOME` moves the home).
pub fn gemini_settings_path() -> Option<PathBuf> {
    let home = std::env::var_os("GEMINI_CLI_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(dirs::home_dir)?;
    Some(home.join(".gemini").join("settings.json"))
}

/// The settings file under an account's own `.gemini` dir, else the user's.
pub fn gemini_settings_path_for(paths: &crate::accounts::Paths) -> Option<PathBuf> {
    match &paths.gemini_dir {
        Some(d) => Some(d.join("settings.json")),
        None => gemini_settings_path(),
    }
}

/// The settings with hubcodes's hook groups in place (older ones replaced,
/// everything else untouched). Pure, for tests.
pub fn merge_gemini_hooks(mut settings: Value, exe: &str) -> Value {
    if !settings.is_object() {
        settings = serde_json::json!({});
    }
    let cmd = format!("{} hook gemini", shell_word(exe));
    let hooks = settings.as_object_mut().unwrap().entry("hooks").or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }
    let map = hooks.as_object_mut().unwrap();
    for ev in GEMINI_EVENTS {
        let groups = map.entry(ev).or_insert_with(|| serde_json::json!([]));
        if !groups.is_array() {
            *groups = serde_json::json!([]);
        }
        let arr = groups.as_array_mut().unwrap();
        arr.retain(|g| !g["hooks"].as_array().map(|hs| hs.iter().any(|h| h["name"] == "hubcodes")).unwrap_or(false));
        arr.push(serde_json::json!({ "matcher": "*", "hooks": [{ "name": "hubcodes", "type": "command", "command": cmd, "timeout": 10000 }] }));
    }
    settings
}

/// Put the hook groups into the user's Gemini settings; returns the path.
/// A settings file that is not JSON is left alone (and reported).
pub fn ensure_gemini_hooks(exe: &str) -> Result<PathBuf> {
    ensure_gemini_hooks_for(exe, &crate::accounts::Paths::default())
}

/// [`ensure_gemini_hooks`] in an account's `.gemini` dir.
pub fn ensure_gemini_hooks_for(exe: &str, paths: &crate::accounts::Paths) -> Result<PathBuf> {
    let path = gemini_settings_path_for(paths).ok_or_else(|| anyhow!("no home directory"))?;
    let current = match std::fs::read_to_string(&path) {
        Ok(text) if !text.trim().is_empty() => serde_json::from_str::<Value>(&text).with_context(|| format!("{} is not JSON; add the hubcodes hooks by hand or fix the file", path.display()))?,
        _ => serde_json::json!({}),
    };
    let merged = merge_gemini_hooks(current.clone(), exe);
    if merged != current {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(&merged)?).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(path)
}

/// Extra arguments that make a provider report through hooks. Empty when the
/// provider has no hook mechanism we know.
pub fn launch_args(provider: &str, session: &str, exe: &str) -> Result<Vec<String>> {
    launch_args_kind(provider, session, exe)
}

/// Same, by hook kind (a custom provider can declare `hook_kind = "claude"`).
pub fn launch_args_kind(kind: &str, session: &str, exe: &str) -> Result<Vec<String>> {
    launch_args_for(kind, session, exe, "")
}

/// `--profile <name>` / `--profile=<name>` in a launch command (omp).
pub fn profile_in(command: &str) -> Option<String> {
    let mut it = command.split_whitespace();
    while let Some(w) = it.next() {
        if w == "--profile" {
            return it.next().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string()).filter(|s| !s.is_empty());
        }
        if let Some(v) = w.strip_prefix("--profile=") {
            let v = v.trim_matches(|c| c == '"' || c == '\'');
            return (!v.is_empty()).then(|| v.to_string());
        }
    }
    None
}

/// [`launch_args_kind`] knowing the provider's command, so an omp started
/// with `--profile work` gets its shim under that profile's agent dir.
pub fn launch_args_for(kind: &str, session: &str, exe: &str, command: &str) -> Result<Vec<String>> {
    launch_args_account(kind, session, exe, command, &crate::accounts::Paths::default())
}

/// [`launch_args_for`] under an account: the shims, hook groups and MCP
/// registrations land in the account's own folders.
pub fn launch_args_account(kind: &str, session: &str, exe: &str, command: &str, paths: &crate::accounts::Paths) -> Result<Vec<String>> {
    Ok(match kind {
        "claude" => {
            let mut v = vec!["--settings".into(), ensure_claude_settings(session, exe)?.display().to_string()];
            // The team as MCP tools (send, read, task_*, note_*, memory_*, kb_search, ask_human).
            v.push("--mcp-config".into());
            v.push(crate::mcpserver::claude_mcp_config(exe));
            v
        }
        "codex" => {
            let mut v = vec!["-c".into(), format!("notify=[{},\"hook\",\"codex\"]", serde_json::to_string(exe)?)];
            v.extend(crate::mcpserver::codex_mcp_args(exe));
            v
        }
        // File-based: the tool auto-loads a plugin/extension from its config
        // dir, so the shim is (re)written here and no flag is needed.
        // opencode: the plugin and MCP entry live in its config dir, which an
        // account does not move (only its auth data does).
        "opencode" => {
            ensure_shim(kind, exe)?;
            let _ = crate::mcpserver::ensure_opencode_mcp(exe);
            vec![]
        }
        "pi" => {
            ensure_shim_at(kind, exe, None, paths.pi_agent_dir.as_deref())?;
            let _ = crate::mcpserver::ensure_agent_dir_mcp_at(kind, exe, None, paths.pi_agent_dir.as_deref());
            vec![]
        }
        "omp" => {
            let profile = paths.omp_profile.clone().or_else(|| profile_in(command));
            ensure_shim_at(kind, exe, profile.as_deref(), None)?;
            let _ = crate::mcpserver::ensure_agent_dir_mcp_at(kind, exe, profile.as_deref(), None);
            vec![]
        }
        // Gemini: hook groups and the MCP server in its settings.json, nothing on the command line.
        "gemini" => {
            ensure_gemini_hooks_for(exe, paths)?;
            let _ = crate::mcpserver::ensure_gemini_mcp_for(exe, paths);
            vec![]
        }
        _ => vec![],
    })
}

/// Where the managed status shim of a hook kind lives under `home`:
/// opencode scans `{plugin,plugins}/*.{ts,js}` in its config dir
/// (`OPENCODE_CONFIG_DIR` or `~/.config/opencode`), pi and omp scan
/// `<agent dir>/extensions/*.ts` (`~/.pi/agent`, `~/.omp/agent`; pi also
/// honours `PI_CODING_AGENT_DIR`).
pub fn shim_path(kind: &str, home: &Path) -> Option<PathBuf> {
    shim_path_profile(kind, home, None)
}

/// [`shim_path`] for omp with a named profile: `~/.omp/profiles/<name>/agent/extensions`
/// (`OMP_PROFILE` / `PI_PROFILE` when no explicit name; `PI_CONFIG_DIR`
/// renames the `.omp` root; `PI_CODING_AGENT_DIR` moves the default
/// profile's agent dir only).
pub fn shim_path_profile(kind: &str, home: &Path, profile: Option<&str>) -> Option<PathBuf> {
    shim_path_at(kind, home, profile, None)
}

/// [`shim_path_profile`] with an explicit agent dir (an account's) for pi.
pub fn shim_path_at(kind: &str, home: &Path, profile: Option<&str>, agent_dir: Option<&Path>) -> Option<PathBuf> {
    let env_dir = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let env_str = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    Some(match kind {
        "opencode" => env_dir("OPENCODE_CONFIG_DIR").unwrap_or_else(|| home.join(".config/opencode")).join("plugins/hubcodes.js"),
        "pi" => agent_dir.map(Path::to_path_buf).or_else(|| env_dir("PI_CODING_AGENT_DIR")).unwrap_or_else(|| home.join(".pi/agent")).join("extensions/hubcodes.ts"),
        "omp" => {
            let profile = profile.map(str::to_string).or_else(|| env_str("OMP_PROFILE")).or_else(|| env_str("PI_PROFILE")).filter(|p| p != "default");
            let root = home.join(env_str("PI_CONFIG_DIR").unwrap_or_else(|| ".omp".into()));
            let agent = match profile {
                Some(p) => root.join("profiles").join(p).join("agent"),
                None => env_dir("PI_CODING_AGENT_DIR").unwrap_or_else(|| root.join("agent")),
            };
            agent.join("extensions/hubcodes.ts")
        }
        _ => return None,
    })
}

/// The shim's source for a hook kind, calling back into `exe`.
pub fn shim_source(kind: &str, exe: &str) -> Option<String> {
    let exe_js = serde_json::to_string(exe).unwrap_or_else(|_| "\"hubcodes\"".into());
    let body = match kind {
        "opencode" => OPENCODE_PLUGIN,
        "pi" | "omp" => PI_EXTENSION,
        _ => return None,
    };
    Some(body.replace("__REPORT__", SHIM_REPORT_JS).replace("__HUBCODES_EXE__", &exe_js).replace("__HUBCODES_KIND__", kind))
}

/// Write (or refresh) the shim for a hook kind; returns its path, `None`
/// for kinds without one.
pub fn ensure_shim(kind: &str, exe: &str) -> Result<Option<PathBuf>> {
    ensure_shim_profile(kind, exe, None)
}

/// [`ensure_shim`] under an omp profile's agent dir.
pub fn ensure_shim_profile(kind: &str, exe: &str, profile: Option<&str>) -> Result<Option<PathBuf>> {
    ensure_shim_at(kind, exe, profile, None)
}

/// [`ensure_shim_profile`] with an explicit agent dir (an account's) for pi.
pub fn ensure_shim_at(kind: &str, exe: &str, profile: Option<&str>, agent_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("no home directory"))?;
    let (Some(path), Some(body)) = (shim_path_at(kind, &home, profile, agent_dir), shim_source(kind, exe)) else { return Ok(None) };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    if std::fs::read_to_string(&path).map(|s| s != body).unwrap_or(true) {
        std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(Some(path))
}

/// Shared by every shim: a fire-and-forget `hubcodes hook <kind>` with the
/// generic payload on stdin. Never throws into the host, never waits.
const SHIM_REPORT_JS: &str = r#"const EXE = __HUBCODES_EXE__;
const KIND = "__HUBCODES_KIND__";
const MANAGED = !!process.env.HUBCODES_AGENT;
let last = "";
let lastStatus = "";
function report(status, detail, turnDone) {
  if (!MANAGED) return;
  const body = JSON.stringify({ status, detail: String(detail ?? "").replace(/\s+/g, " ").trim().slice(0, 200), turn_done: !!turnDone });
  if (body === last) return;
  last = body;
  lastStatus = status;
  try {
    const child = spawn(EXE, ["hook", KIND], { stdio: ["pipe", "ignore", "ignore"], detached: true, env: process.env });
    child.on("error", () => {});
    child.stdin.on("error", () => {});
    child.stdin.end(body);
    child.unref();
  } catch {}
}
function hint(args) {
  if (!args || typeof args !== "object") return "";
  const v = args.command ?? args.path ?? args.file_path ?? args.filePath ?? args.pattern ?? "";
  return typeof v === "string" ? v.slice(0, 120) : "";
}
// The orchestrator guard: ask hubcodes whether this agent's role may run
// the tool. Synchronous and short; any failure means "allow".
function guard(tool, args) {
  if (!MANAGED) return null;
  try {
    const out = execFileSync(EXE, ["guard", "--tool", String(tool ?? ""), "--input", JSON.stringify(args ?? {})], { encoding: "utf8", timeout: 4000, stdio: ["ignore", "pipe", "ignore"], env: process.env });
    const v = JSON.parse(out);
    return v && v.allow === false && typeof v.reason === "string" ? v.reason : null;
  } catch {
    return null;
  }
}
"#;

/// opencode (V1 plugin API, verified against 1.19.5): a module exporting an
/// async function that returns hooks. Events seen in the binary:
/// `session.status` (`status.type` idle|busy|retry), `session.idle`,
/// `session.error`, `permission.asked`, `question.asked`, `*.replied`;
/// hooks `chat.message` and `tool.execute.before` (`{tool, sessionID}`).
/// Child sessions (sub-agents, `info.parentID`) never flip the pane to
/// idle or waiting; only the root session does.
const OPENCODE_PLUGIN: &str = r#"// managed by hubcodes; rewritten at every start
// Reports what this opencode is doing to the hubcodes pane it runs in.
// Inert outside hubcodes (no HUBCODES_AGENT in the environment).
import { spawn, execFileSync } from "node:child_process";
__REPORT__
export const HubcodesStatus = async () => {
  if (!MANAGED) return {};
  const children = new Set();
  const isChild = (id) => typeof id === "string" && children.has(id);
  const text = (p) => {
    if (!p || typeof p !== "object") return "";
    const q = Array.isArray(p.questions) ? p.questions.find((x) => x && typeof x.question === "string") : undefined;
    if (q?.question) return q.question;
    // 1.19.5 sends `{permission, patterns, ...}` (no title): "external_directory /tmp/x/*".
    const patterns = Array.isArray(p.patterns) ? p.patterns.join(" ") : typeof p.pattern === "string" ? p.pattern : "";
    return [p.title ?? p.message ?? p.permission ?? p.type ?? "", patterns].join(" ").trim();
  };
  return {
    "chat.message": async (input) => {
      if (isChild(input?.sessionID)) return;
      report("working", "thinking", false);
    },
    "tool.execute.before": async (input, output) => {
      report("working", input?.tool ?? "tool", false);
      // Throwing here stops the tool; the model sees the reason as the tool's error.
      const refused = guard(input?.tool, output?.args);
      if (refused) throw new Error(refused);
    },
    event: async ({ event }) => {
      const type = event?.type;
      const p = event?.properties ?? {};
      if (p.info && typeof p.info === "object" && p.info.parentID && p.info.id) children.add(p.info.id);
      if (isChild(p.sessionID)) return;
      switch (type) {
        case "session.status": {
          const s = typeof p.status === "string" ? p.status : p.status?.type;
          // `busy` fires again after every tool; keep the tool name on the card.
          if (s === "busy") { if (lastStatus !== "working") report("working", "thinking", false); }
          else if (s === "retry") report("working", "retry " + (p.status?.message ?? ""), false);
          else if (s === "idle") report("idle", "turn finished", true);
          break;
        }
        case "session.idle":
          report("idle", "turn finished", true);
          break;
        case "session.error":
          report("idle", "error " + (p.error?.data?.message ?? p.error?.name ?? ""), true);
          break;
        case "permission.asked":
        case "permission.updated":
        case "question.asked":
          report("waiting", text(p) || "permission", false);
          break;
        case "permission.replied":
        case "question.replied":
        case "question.rejected":
        case "session.compacted":
          report("working", "resumed", false);
          break;
        default:
          break;
      }
    },
  };
};
"#;

/// pi (0.85) and omp (18.1) extensions: `export default function (pi)`,
/// `pi.on(event, handler)`. Both have `agent_start`, `tool_execution_start`
/// / `_end` (`toolName`, `args`), `agent_end`, `session_start`. pi adds
/// `agent_settled` (the run will not auto-continue) and `ui_prompt_start`
/// / `_end` around blocking prompts; omp instead has
/// `tool_approval_requested` / `_resolved` and an `ask` tool. The file is
/// plain JavaScript (valid TypeScript) so `node --check` can parse it.
const PI_EXTENSION: &str = r#"// managed by hubcodes; rewritten at every start
// Reports what this __HUBCODES_KIND__ is doing to the hubcodes pane it runs in.
// Inert outside hubcodes (no HUBCODES_AGENT in the environment).
// @ts-nocheck
import { spawn, execFileSync } from "node:child_process";
__REPORT__
export default function (pi) {
  if (!MANAGED) return;
  // The orchestrator guard: `tool_call` may block with a reason the model sees.
  pi.on("tool_call", (event) => {
    const refused = guard(event?.toolName, event?.input);
    if (refused) return { block: true, reason: refused };
  });
  const askText = (args) => {
    const q = Array.isArray(args?.questions) ? args.questions.find((x) => x && typeof x.question === "string") : undefined;
    return q?.question ?? args?.question ?? "question";
  };
  pi.on("session_start", (_event, ctx) => {
    report(ctx?.isIdle?.() === false ? "working" : "idle", "ready", false);
  });
  pi.on("agent_start", () => report("working", "thinking", false));
  pi.on("tool_execution_start", (event) => {
    if (event?.toolName === "ask") report("waiting", askText(event.args), false);
    else report("working", `${event?.toolName ?? "tool"} ${hint(event?.args)}`, false);
  });
  pi.on("tool_execution_end", (event) => {
    if (event?.toolName === "ask") report("working", "resumed", false);
  });
  if (KIND === "pi") {
    pi.on("ui_prompt_start", (event) => report("waiting", event?.title ?? event?.kind ?? "prompt", false));
    pi.on("ui_prompt_end", () => report("working", "resumed", false));
    pi.on("agent_settled", (_event, ctx) => {
      if (ctx?.isIdle?.() !== false) report("idle", "turn finished", true);
    });
  } else {
    pi.on("tool_approval_requested", (event) => report("waiting", event?.reason ?? `${event?.toolName ?? "tool"} approval`, false));
    pi.on("tool_approval_resolved", () => report("working", "resumed", false));
    pi.on("agent_end", () => report("idle", "turn finished", true));
  }
}
"#;

/// Absolute path of the running `hubcodes` binary, for hook commands.
pub fn self_exe() -> Result<String> {
    std::env::current_exe().map(|p| p.display().to_string()).map_err(|e| anyhow!("current_exe: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_events_map_to_statuses() {
        let p = serde_json::json!({"hook_event_name": "Notification", "notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash"});
        let r = from_claude(&p).unwrap();
        assert_eq!(r.status, "waiting");
        assert!(r.detail.contains("permission"));
        let idle = serde_json::json!({"hook_event_name": "Notification", "notification_type": "idle", "message": "Claude is waiting for your input"});
        let r = from_claude(&idle).unwrap();
        assert_eq!((r.status, r.detail.as_str()), ("idle", "ready"), "the idle nudge must not block deliveries or count as needs-you");
        let s = from_claude(&serde_json::json!({"hook_event_name": "Stop", "transcript_path": "/tmp/x.jsonl"})).unwrap();
        assert!(s.turn_done);
        assert_eq!(s.transcript.unwrap(), PathBuf::from("/tmp/x.jsonl"));
        let t = from_claude(&serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "cargo test"}})).unwrap();
        assert_eq!((t.status, t.detail.as_str()), ("working", "Bash cargo test"));
        assert!(from_claude(&serde_json::json!({"hook_event_name": "Unknown"})).is_none());
    }

    #[test]
    fn codex_turn_complete() {
        let r = from_codex(&serde_json::json!({"type": "agent-turn-complete", "last-assistant-message": "done"})).unwrap();
        assert!(r.turn_done);
        assert_eq!(r.status, "idle");
    }

    #[test]
    fn gemini_events_map_to_statuses_and_hooks_merge_once() {
        let r = from_gemini(&serde_json::json!({ "hook_event_name": "BeforeTool", "tool_name": "write_file", "tool_input": {} })).unwrap();
        assert_eq!((r.status, r.detail.as_str(), r.turn_done), ("working", "write_file", false));
        let r = from_gemini(&serde_json::json!({ "hook_event_name": "AfterAgent", "prompt": "x" })).unwrap();
        assert_eq!((r.status, r.turn_done), ("idle", true));
        let r = from_gemini(&serde_json::json!({ "hook_event_name": "Notification", "notification_type": "ToolPermission", "message": "Allow shell?" })).unwrap();
        assert_eq!((r.status, r.detail.as_str()), ("waiting", "Allow shell?"));
        assert!(from_gemini(&serde_json::json!({ "hook_event_name": "PreCompress" })).is_none());
        // Merging keeps the user's own groups and never doubles ours.
        let mine = serde_json::json!({ "theme": "dark", "hooks": { "BeforeTool": [{ "matcher": "write_file", "hooks": [{ "name": "security", "type": "command", "command": "./sec.sh" }] }] } });
        let once = merge_gemini_hooks(mine.clone(), "/x/hubcodes");
        let twice = merge_gemini_hooks(once.clone(), "/x/hubcodes");
        assert_eq!(once, twice);
        assert_eq!(once["theme"], "dark");
        let bt = once["hooks"]["BeforeTool"].as_array().unwrap();
        assert_eq!(bt.len(), 2, "the user's group stays, ours is added once");
        assert_eq!(bt[1]["hooks"][0]["command"], "/x/hubcodes hook gemini");
        assert!(once["hooks"]["AfterAgent"].is_array() && once["hooks"]["Notification"].is_array());
        assert_eq!(profile_in("omp --profile work --model=x").as_deref(), Some("work"));
        assert_eq!(profile_in("omp --profile=\"home\"").as_deref(), Some("home"));
        assert_eq!(profile_in("omp"), None);
        let home = std::path::Path::new("/h");
        assert_eq!(shim_path_profile("omp", home, Some("work")).unwrap(), std::path::PathBuf::from("/h/.omp/profiles/work/agent/extensions/hubcodes.ts"));
        assert_eq!(shim_path_profile("omp", home, Some("default")).unwrap(), std::path::PathBuf::from("/h/.omp/agent/extensions/hubcodes.ts"));
    }

    #[test]
    fn generic_payload_maps_to_report() {
        let r = from_generic(&serde_json::json!({"status": "waiting", "detail": "Allow rm -rf?", "turn_done": false})).unwrap();
        assert_eq!((r.status, r.detail.as_str(), r.turn_done), ("waiting", "Allow rm -rf?", false));
        let r = from_generic(&serde_json::json!({"status": "idle", "turn_done": true})).unwrap();
        assert_eq!((r.status, r.detail.as_str(), r.turn_done), ("idle", "", true));
        assert!(r.transcript.is_none());
        let long = "x".repeat(500);
        let r = from_generic(&serde_json::json!({"status": "working", "detail": long})).unwrap();
        assert_eq!(r.detail.chars().count(), 200);
        assert_eq!(from_generic(&serde_json::json!({"status": "offline"})).unwrap().status, "offline");
        assert!(from_generic(&serde_json::json!({"status": "busy"})).is_none(), "unknown statuses are ignored, not guessed");
        assert!(from_generic(&serde_json::json!({"detail": "no status"})).is_none());
        assert!(from_generic(&Value::Null).is_none());
    }

    #[test]
    fn shims_are_managed_and_parse_as_javascript() {
        let exe = "/Applications/hub codes.app/Contents/MacOS/hubcodes";
        let home = Path::new("/home/x");
        assert_eq!(shim_path("opencode", home).unwrap(), home.join(".config/opencode/plugins/hubcodes.js"));
        assert_eq!(shim_path("pi", home).unwrap(), home.join(".pi/agent/extensions/hubcodes.ts"));
        assert_eq!(shim_path("omp", home).unwrap(), home.join(".omp/agent/extensions/hubcodes.ts"));
        assert!(shim_path("claude", home).is_none() && shim_source("claude", exe).is_none());
        let dir = std::env::temp_dir().join(format!("hubcodes-shims-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for kind in ["opencode", "pi", "omp"] {
            let src = shim_source(kind, exe).unwrap();
            assert!(src.starts_with("// managed by hubcodes; rewritten at every start"), "{kind}: header");
            assert!(src.contains(&format!("const EXE = \"{exe}\";")), "{kind}: exe path is a JSON string");
            assert!(src.contains("[\"hook\", KIND]") && src.contains(&format!("const KIND = \"{kind}\";")), "{kind}: calls hubcodes hook {kind}");
            assert!(src.contains("process.env.HUBCODES_AGENT"), "{kind}: inert outside hubcodes");
            assert!(!src.contains("__REPORT__") && !src.contains("__HUBCODES_"), "{kind}: every placeholder filled");
            // The pi/omp file is .ts for the loader but written as plain JS,
            // so node can check its syntax too (when node is on PATH).
            let f = dir.join(format!("{kind}.mjs"));
            std::fs::write(&f, &src).unwrap();
            if let Ok(out) = std::process::Command::new("node").arg("--check").arg(&f).output() {
                assert!(out.status.success(), "{kind}: node --check: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
        assert!(shim_source("opencode", exe).unwrap().contains("export const HubcodesStatus = async () =>"));
        assert!(shim_source("pi", exe).unwrap().contains("export default function (pi)"));
        for ev in ["session.idle", "session.status", "session.error", "permission.asked", "question.asked", "tool.execute.before", "chat.message"] {
            assert!(shim_source("opencode", exe).unwrap().contains(&format!("\"{ev}\"")), "opencode shim handles {ev}");
        }
        for ev in ["session_start", "agent_start", "tool_execution_start", "agent_settled", "ui_prompt_start", "tool_approval_requested", "agent_end"] {
            assert!(shim_source("pi", exe).unwrap().contains(&format!("\"{ev}\"")), "pi/omp shim handles {ev}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_json_is_valid_and_points_at_exe() {
        let s = claude_settings_json("/Applications/hubcodes.app/Contents/MacOS/hubcodes");
        let v: Value = serde_json::from_str(&s).unwrap();
        let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "/Applications/hubcodes.app/Contents/MacOS/hubcodes hook claude");
        let args = launch_args("codex", "s", "/usr/local/bin/hubcodes").unwrap();
        assert_eq!(args[1], "notify=[\"/usr/local/bin/hubcodes\",\"hook\",\"codex\"]");
    }

    #[test]
    fn claude_turn_usage_counts_last_turn_once() {
        let dir = std::env::temp_dir().join(format!("hubcodes-core-tests-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_STATE_HOME", &dir);
        let f = dir.join("t.jsonl");
        std::fs::write(&f, concat!(
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"old\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"id\":\"m0\",\"usage\":{\"input_tokens\":999,\"output_tokens\":999}}}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"new\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"usage\":{\"input_tokens\":10,\"output_tokens\":20,\"cache_read_input_tokens\":5}}}\n",
            "{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"usage\":{\"input_tokens\":10,\"output_tokens\":20,\"cache_read_input_tokens\":5}}}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\"}]}}\n",
            "{\"type\":\"assistant\",\"message\":{\"id\":\"m2\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n"
        )).unwrap();
        let _ = std::fs::remove_file(crate::db::path_for("hooktest"));
        let u = account_claude_turn("hooktest", "a", &f).unwrap().unwrap();
        assert_eq!((u.input_tokens, u.output_tokens, u.cache_read_tokens, u.turns), (11, 22, 5, 1));
    }
}

/// Is `seen` (the model id a tool reports, `claude-opus-5-5`) the one that
/// was `picked` (an alias like `opus`, or a full id)? Unknown either way
/// counts as a match: only a clear difference is flagged.
pub fn model_matches(picked: &str, seen: &str) -> bool {
    let norm = |s: &str| s.trim().to_ascii_lowercase().split('[').next().unwrap_or("").to_string();
    let (p, s) = (norm(picked), norm(seen));
    if p.is_empty() || s.is_empty() || p == "default" || p == "inherit" {
        return true;
    }
    s == p || s.split(['-', '/', '.', ':']).any(|w| w == p) || s.ends_with(&p) || p.ends_with(&s)
}

#[cfg(test)]
mod model_match_tests {
    use super::model_matches;

    #[test]
    fn picked_and_seen_models_compare_by_alias_or_id() {
        assert!(model_matches("opus", "claude-opus-5-5"));
        assert!(model_matches("claude-opus-5-5", "claude-opus-5-5"));
        assert!(model_matches("claude-fable-5-1[1m]", "claude-fable-5-1"));
        assert!(!model_matches("opus", "claude-fable-5-2"));
        assert!(!model_matches("sonnet", "claude-opus-5-5"));
        assert!(model_matches("", "claude-opus-5-5"), "nothing picked: nothing to flag");
        assert!(model_matches("opus", ""), "nothing seen yet");
        assert!(model_matches("openai/gpt-5.6", "gpt-5.6"));
    }
}
