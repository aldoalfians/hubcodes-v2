//! Team memory behind one interface: `retain` (remember), `recall` (find),
//! `ping` (is it reachable). Providers are chosen in `[memory]`:
//!
//! ```toml
//! [memory]
//! provider = "hindsight"          # builtin | hindsight | mcp | command
//! auto_recall_on_start = true     # recall for the agent's role + goal and put it in its system prompt
//! auto_retain = true              # task done, loop DONE, PR opened → retain a summary
//!
//! [memory.hindsight]              # REST API
//! url = "https://memories.hubstudio.id"
//! api_key = "${HINDSIGHT_API_KEY}"
//! bank = "hubstudio"
//!
//! [memory.mcp]                    # any MCP server with a remember/search pair
//! url = "https://memories.hubstudio.id/mcp/me/"
//! headers = { Authorization = "Bearer ${HINDSIGHT_API_KEY}" }
//! retain_tool = "retain"
//! retain_args = { content = "{text}" }
//! recall_tool = "recall"
//! recall_args = { query = "{query}" }
//!
//! [memory.command]                # shell: JSON on stdin, text on stdout
//! retain = "mytool remember"
//! recall = "mytool search"
//! ```

use crate::db::{now, Db};
use crate::mcp::{self, McpTarget};
use anyhow::{anyhow, Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "yes")]
    pub auto_recall_on_start: bool,
    #[serde(default = "yes")]
    pub auto_retain: bool,
    #[serde(default)]
    pub hindsight: HindsightConfig,
    #[serde(default)]
    pub mcp: McpMemoryConfig,
    #[serde(default)]
    pub command: CommandMemoryConfig,
}

fn default_provider() -> String {
    "builtin".into()
}
fn yes() -> bool {
    true
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig { provider: "builtin".into(), auto_recall_on_start: true, auto_retain: true, hindsight: HindsightConfig::default(), mcp: McpMemoryConfig::default(), command: CommandMemoryConfig::default() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HindsightConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub bank: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpMemoryConfig {
    #[serde(flatten)]
    pub target: McpTarget,
    #[serde(default = "d_retain")]
    pub retain_tool: String,
    #[serde(default = "d_retain_args")]
    pub retain_args: Value,
    #[serde(default = "d_recall")]
    pub recall_tool: String,
    #[serde(default = "d_recall_args")]
    pub recall_args: Value,
}

fn d_retain() -> String {
    "retain".into()
}
fn d_recall() -> String {
    "recall".into()
}
fn d_retain_args() -> Value {
    json!({ "content": "{text}" })
}
fn d_recall_args() -> Value {
    json!({ "query": "{query}" })
}

impl Default for McpMemoryConfig {
    fn default() -> Self {
        McpMemoryConfig { target: McpTarget::default(), retain_tool: d_retain(), retain_args: d_retain_args(), recall_tool: d_recall(), recall_args: d_recall_args() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CommandMemoryConfig {
    #[serde(default)]
    pub retain: String,
    #[serde(default)]
    pub recall: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub text: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Ping {
    pub ok: bool,
    pub provider: String,
    pub detail: String,
    pub ms: u64,
}

fn builtin_table(db: &Db) -> Result<()> {
    db.conn().execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memories USING fts5(text, by, tags, at UNINDEXED);",
    )?;
    Ok(())
}

/// Remember something. `by` is the agent or `human:<user>`.
pub fn retain(cfg: &MemoryConfig, session: &str, by: &str, text: &str, tags: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(anyhow!("nothing to remember"));
    }
    let out = match cfg.provider.as_str() {
        "builtin" => {
            let db = Db::open(session)?;
            builtin_table(&db)?;
            db.conn().execute("INSERT INTO memories (text, by, tags, at) VALUES (?1, ?2, ?3, ?4)", params![text, by, tags, now()])?;
            "stored in the session memory".to_string()
        }
        "hindsight" => {
            let h = &cfg.hindsight;
            let url = format!("{}/v1/default/banks/{}/memories", mcp::expand_env(&h.url).trim_end_matches('/'), mcp::expand_env(&h.bank));
            let body = json!({ "items": [{ "content": text, "metadata": { "by": by, "tags": tags, "session": session, "source": "hubcodes" } }] });
            let v = http_json(&url, &h.api_key, &body)?;
            v.get("message").and_then(|m| m.as_str()).map(String::from).unwrap_or_else(|| "stored in Hindsight".into())
        }
        "mcp" => {
            let mut c = mcp::Client::connect(&cfg.mcp.target)?;
            let args = mcp::fill_args(&cfg.mcp.retain_args, &[("text", text), ("by", by), ("tags", tags), ("session", session)]);
            let (t, _) = c.call_tool(&cfg.mcp.retain_tool, args)?;
            if t.is_empty() { "stored".into() } else { t.chars().take(300).collect() }
        }
        "command" => run_cmd(&cfg.command.retain, &json!({ "text": text, "by": by, "tags": tags, "session": session }))?,
        other => return Err(anyhow!("unknown memory provider '{other}'")),
    };
    if let Ok(db) = Db::open(session) {
        let _ = db.add_event(by, "memory", &format!("retain ({}): {}", cfg.provider, text.chars().take(120).collect::<String>()));
    }
    Ok(out)
}

/// Find memories for a query. Every provider returns plain text snippets.
pub fn recall(cfg: &MemoryConfig, session: &str, query: &str, limit: usize) -> Result<Vec<Memory>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(vec![]);
    }
    match cfg.provider.as_str() {
        "builtin" => {
            let db = Db::open(session)?;
            builtin_table(&db)?;
            // FTS5 query: each content word as a prefix term, OR'd, so natural questions still match.
            const STOP: &[&str] = &["the", "and", "for", "with", "that", "this", "where", "what", "how", "does", "are", "was", "were", "from", "into", "about", "which", "when", "who", "why", "our", "your", "their"];
            let terms: Vec<String> = query
                .split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
                .filter(|w| w.len() > 2 && !STOP.contains(&w.as_str()))
                .map(|w| format!("\"{w}\"*"))
                .collect();
            if terms.is_empty() {
                return Ok(vec![]);
            }
            let q = terms.join(" OR ");
            let mut st = db.conn().prepare("SELECT text, by, at FROM memories WHERE memories MATCH ?1 ORDER BY bm25(memories) LIMIT ?2")?;
            let rows = st.query_map(params![q, limit as i64], |r| Ok(Memory { text: r.get(0)?, source: r.get(1)?, at: r.get(2)? }))?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        }
        "hindsight" => {
            let h = &cfg.hindsight;
            let url = format!("{}/v1/default/banks/{}/memories/recall", mcp::expand_env(&h.url).trim_end_matches('/'), mcp::expand_env(&h.bank));
            let v = http_json(&url, &h.api_key, &json!({ "query": query, "max_tokens": 2048 }))?;
            let list = v.get("results").or_else(|| v.get("memories")).and_then(|r| r.as_array()).cloned().unwrap_or_default();
            Ok(list
                .iter()
                .take(limit)
                .filter_map(|m| {
                    let text = m.get("text").or_else(|| m.get("content")).and_then(|t| t.as_str())?;
                    Some(Memory { text: text.to_string(), source: "hindsight".into(), at: 0 })
                })
                .collect())
        }
        "mcp" => {
            let mut c = mcp::Client::connect(&cfg.mcp.target)?;
            let args = mcp::fill_args(&cfg.mcp.recall_args, &[("query", query), ("session", session)]);
            let (text, raw) = c.call_tool(&cfg.mcp.recall_tool, args)?;
            // Many servers return JSON as the text content; use it when it parses.
            let parsed: Value = serde_json::from_str(text.trim()).unwrap_or(raw);
            Ok(split_results(&text, &parsed, limit, "mcp"))
        }
        "command" => {
            let out = run_cmd(&cfg.command.recall, &json!({ "query": query, "session": session, "limit": limit }))?;
            let raw: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
            Ok(split_results(&out, &raw, limit, "command"))
        }
        other => Err(anyhow!("unknown memory provider '{other}'")),
    }
}

/// Reachability and a one-line description, for Settings → Memory → Test.
pub fn ping(cfg: &MemoryConfig, session: &str) -> Ping {
    let t0 = std::time::Instant::now();
    let r: Result<String> = (|| match cfg.provider.as_str() {
        "builtin" => {
            let db = Db::open(session)?;
            builtin_table(&db)?;
            let n: i64 = db.conn().query_row("SELECT count(*) FROM memories", [], |r| r.get(0))?;
            Ok(format!("session memory in {}: {n} memories", crate::db::path_for(session).display()))
        }
        "hindsight" => {
            let h = &cfg.hindsight;
            if h.url.trim().is_empty() || h.bank.trim().is_empty() {
                return Err(anyhow!("url and bank are required"));
            }
            let url = format!("{}/v1/default/banks/{}/memories/recall", mcp::expand_env(&h.url).trim_end_matches('/'), mcp::expand_env(&h.bank));
            let v = http_json(&url, &h.api_key, &json!({ "query": "hubcodes connection test", "max_tokens": 64 }))?;
            let n = v.get("results").or_else(|| v.get("memories")).and_then(|r| r.as_array()).map(|a| a.len()).unwrap_or(0);
            Ok(format!("Hindsight bank '{}' answers ({n} results for a test query)", h.bank))
        }
        "mcp" => {
            let mut c = mcp::Client::connect(&cfg.mcp.target)?;
            let tools = c.tools()?;
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            for want in [&cfg.mcp.retain_tool, &cfg.mcp.recall_tool] {
                if !names.contains(&want.as_str()) {
                    return Err(anyhow!("server has no tool '{want}'; it offers: {}", names.join(", ")));
                }
            }
            Ok(format!("MCP server reachable, {} tools, '{}' and '{}' present", tools.len(), cfg.mcp.retain_tool, cfg.mcp.recall_tool))
        }
        "command" => {
            if cfg.command.recall.trim().is_empty() {
                return Err(anyhow!("recall command is empty"));
            }
            let out = run_cmd(&cfg.command.recall, &json!({ "query": "hubcodes connection test", "limit": 1 }))?;
            Ok(format!("command answered ({} bytes)", out.len()))
        }
        other => Err(anyhow!("unknown memory provider '{other}'")),
    })();
    let ms = t0.elapsed().as_millis() as u64;
    match r {
        Ok(detail) => Ping { ok: true, provider: cfg.provider.clone(), detail, ms },
        Err(e) => Ping { ok: false, provider: cfg.provider.clone(), detail: format!("{e:#}"), ms },
    }
}

/// What to ask memory about when an agent starts.
pub fn start_query(name: &str, role: Option<&str>, cwd: Option<&str>) -> String {
    let repo = cwd.and_then(|c| std::path::Path::new(c).file_name().map(|f| f.to_string_lossy().to_string())).unwrap_or_default();
    format!("{} {} {}", role.unwrap_or(""), repo, name).trim().to_string()
}

/// Lines for an agent's system prompt at start, or empty.
pub fn recall_block(cfg: &MemoryConfig, session: &str, query: &str) -> String {
    if !cfg.auto_recall_on_start {
        return String::new();
    }
    match recall(cfg, session, query, 8) {
        Ok(list) if !list.is_empty() => {
            let mut s = String::from("What the team remembers (hubcodes memory recall <query> for more):\n");
            for m in list {
                s.push_str("- ");
                s.push_str(&m.text.replace('\n', " ").chars().take(300).collect::<String>());
                s.push('\n');
            }
            s
        }
        _ => String::new(),
    }
}

fn split_results(text: &str, raw: &Value, limit: usize, source: &str) -> Vec<Memory> {
    // Structured: an array of objects with text/content; else one memory per non-empty line/paragraph.
    if let Some(arr) = raw.as_array().or_else(|| raw.get("results").and_then(|r| r.as_array())).or_else(|| raw.get("memories").and_then(|r| r.as_array())) {
        let list: Vec<Memory> = arr
            .iter()
            .filter_map(|m| m.get("text").or_else(|| m.get("content")).and_then(|t| t.as_str()).map(|t| Memory { text: t.to_string(), source: source.into(), at: 0 }))
            .take(limit)
            .collect();
        if !list.is_empty() {
            return list;
        }
    }
    text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).take(limit).map(|p| Memory { text: p.to_string(), source: source.into(), at: 0 }).collect()
}

fn http_json(url: &str, api_key: &str, body: &Value) -> Result<Value> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
    let mut req = agent.post(url).header("Content-Type", "application/json");
    let key = mcp::expand_env(api_key);
    if !key.trim().is_empty() {
        req = req.header("Authorization", &format!("Bearer {}", key.trim()));
    }
    let mut resp = match req.send(body.to_string().as_bytes()) {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(code)) => return Err(anyhow!("HTTP {code} from {url}{}", if code == 401 || code == 403 { " (check the API key)" } else { "" })),
        Err(e) => return Err(anyhow!("{url}: {e}")),
    };
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    serde_json::from_str(&text).with_context(|| format!("parse reply from {url}: {}", text.chars().take(160).collect::<String>()))
}

fn run_cmd(cmd: &str, input: &Value) -> Result<String> {
    if cmd.trim().is_empty() {
        return Err(anyhow!("command is empty"));
    }
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-lc", &mcp::expand_env(cmd)])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("run {cmd}"))?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        stdin.write_all(input.to_string().as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(anyhow!("{cmd}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_round_trip() {
        let dir = std::env::temp_dir().join(format!("hubcodes-core-tests-{}", std::process::id()));
        std::env::set_var("XDG_STATE_HOME", &dir);
        let _ = std::fs::remove_file(crate::db::path_for("mem"));
        let cfg = MemoryConfig::default();
        retain(&cfg, "mem", "planner", "The token store lives in src/auth/store.rs and keeps its public API", "auth").unwrap();
        retain(&cfg, "mem", "human:me", "Deploys happen Friday after the e2e suite is green", "ops").unwrap();
        let hits = recall(&cfg, "mem", "where is the token store?", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("src/auth"));
        let p = ping(&cfg, "mem");
        assert!(p.ok, "{}", p.detail);
        assert!(recall_block(&cfg, "mem", "deploy friday").contains("Friday"));
    }
}
