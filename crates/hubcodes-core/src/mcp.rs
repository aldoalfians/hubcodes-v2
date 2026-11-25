//! A small MCP client (JSON-RPC 2.0) so hubcodes can use any MCP server as a
//! memory or knowledge source: stdio (`command` + args, newline-delimited
//! JSON) and streamable HTTP (`url`, optional headers; JSON or SSE replies,
//! `Mcp-Session-Id` kept between calls). Only `initialize`, `tools/list` and
//! `tools/call` are used.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct McpTarget {
    /// HTTP endpoint (`https://host/mcp/`), or empty for stdio.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// Stdio server: the command to run.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Extra HTTP headers (`Authorization = "Bearer ${HINDSIGHT_KEY}"`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// One tool as the server lists it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The tool's JSON Schema for its arguments (`inputSchema`).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub input_schema: Value,
}

pub struct Client {
    target: McpTarget,
    next_id: u64,
    session: Option<String>,
    child: Option<Child>,
    agent: ureq::Agent,
}

const PROTOCOL: &str = "2025-06-18";

/// `${NAME}` → the environment variable and `${keychain:NAME}` → the Keychain
/// item ([`crate::secrets`]), so secrets stay out of the file.
pub fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        if let Some(j) = rest[i..].find('}') {
            let name = &rest[i + 2..i + j];
            match name.strip_prefix("keychain:") {
                Some(k) => out.push_str(&crate::secrets::get(k).unwrap_or_default()),
                None => out.push_str(&std::env::var(name).unwrap_or_default()),
            }
            rest = &rest[i + j + 1..];
        } else {
            out.push_str(&rest[i..]);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

impl Client {
    pub fn connect(target: &McpTarget) -> Result<Self> {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(30))).build().into();
        let mut c = Client { target: target.clone(), next_id: 1, session: None, child: None, agent };
        if c.target.url.is_empty() {
            if c.target.command.is_empty() {
                return Err(anyhow!("MCP target needs a url or a command"));
            }
            let mut cmd = Command::new(expand_env(&c.target.command));
            cmd.args(c.target.args.iter().map(|a| expand_env(a)));
            for (k, v) in &c.target.env {
                cmd.env(k, expand_env(v));
            }
            let child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    anyhow!("{} is not installed{}", c.target.command, if c.target.command.ends_with("npx") { ": the browser sign-in runs through Node.js (install it: brew install node, sudo dnf install nodejs, or sudo apt install nodejs npm)" } else { "" })
                } else {
                    anyhow!("start MCP server {}: {e}", c.target.command)
                }
            })?;
            c.child = Some(child);
        }
        let init = c.call("initialize", json!({ "protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": { "name": "hubcodes", "version": env!("CARGO_PKG_VERSION") } }))?;
        if init.get("protocolVersion").is_none() && init.get("serverInfo").is_none() {
            return Err(anyhow!("not an MCP server: initialize returned {}", init));
        }
        let _ = c.notify("notifications/initialized", json!({}));
        Ok(c)
    }

    pub fn tools(&mut self) -> Result<Vec<Tool>> {
        let r = self.call("tools/list", json!({}))?;
        let list = r.get("tools").and_then(|t| t.as_array()).cloned().unwrap_or_default();
        Ok(list
            .into_iter()
            .filter_map(|t| {
                Some(Tool {
                    name: t.get("name")?.as_str()?.to_string(),
                    description: t.get("description").and_then(|d| d.as_str()).unwrap_or("").chars().take(200).collect(),
                    input_schema: t.get("inputSchema").cloned().unwrap_or(Value::Null),
                })
            })
            .collect())
    }

    /// Call a tool; returns the concatenated text content and the raw result.
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<(String, Value)> {
        let r = self.call("tools/call", json!({ "name": name, "arguments": arguments }))?;
        if r.get("isError").and_then(|e| e.as_bool()).unwrap_or(false) {
            return Err(anyhow!("tool {name} failed: {}", text_of(&r)));
        }
        Ok((text_of(&r), r))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        if self.child.is_some() {
            self.write_stdio(&msg)?;
            return Ok(());
        }
        let _ = self.post(&msg);
        Ok(())
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let reply = if self.child.is_some() {
            self.write_stdio(&msg)?;
            self.read_stdio(id)?
        } else {
            self.post(&msg)?.ok_or_else(|| anyhow!("empty reply to {method}"))?
        };
        if let Some(e) = reply.get("error") {
            return Err(anyhow!("{method}: {}", e.get("message").and_then(|m| m.as_str()).unwrap_or("error")));
        }
        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
    }

    fn write_stdio(&mut self, msg: &Value) -> Result<()> {
        let child = self.child.as_mut().ok_or_else(|| anyhow!("no stdio child"))?;
        let stdin = child.stdin.as_mut().ok_or_else(|| anyhow!("no stdin"))?;
        stdin.write_all(msg.to_string().as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    fn read_stdio(&mut self, id: u64) -> Result<Value> {
        let child = self.child.as_mut().ok_or_else(|| anyhow!("no stdio child"))?;
        let stdout = child.stdout.as_mut().ok_or_else(|| anyhow!("no stdout"))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        for _ in 0..200 {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                return Err(anyhow!("MCP server closed its output"));
            }
            let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { continue };
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                return Ok(v);
            }
        }
        Err(anyhow!("no reply for request {id}"))
    }

    fn post(&mut self, msg: &Value) -> Result<Option<Value>> {
        let mut req = self.agent.post(expand_env(&self.target.url)).header("Content-Type", "application/json").header("Accept", "application/json, text/event-stream");
        for (k, v) in &self.target.headers {
            req = req.header(k, &expand_env(v));
        }
        if let Some(s) = &self.session {
            req = req.header("Mcp-Session-Id", s);
        }
        let mut resp = match req.send(msg.to_string().as_bytes()) {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(code)) if code == 401 || code == 403 => {
                return Err(anyhow!(
                    "HTTP {code} from {}: the server wants a sign-in. A browser login (OAuth, e.g. Google): pick Sign in with the browser for this server. A token: paste it as the token",
                    self.target.url
                ))
            }
            Err(ureq::Error::StatusCode(code)) => return Err(anyhow!("HTTP {code} from {}", self.target.url)),
            Err(e) => return Err(anyhow!("{}: {e}", self.target.url)),
        };
        if let Some(sid) = resp.headers().get("mcp-session-id").and_then(|h| h.to_str().ok()) {
            self.session = Some(sid.to_string());
        }
        let ctype = resp.headers().get("content-type").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        if body.trim().is_empty() {
            return Ok(None);
        }
        if ctype.contains("text/event-stream") {
            // Take the last JSON-RPC message with our id, or the last data line.
            let mut last = None;
            for line in body.lines() {
                if let Some(data) = line.strip_prefix("data:") {
                    if let Ok(v) = serde_json::from_str::<Value>(data.trim()) {
                        if v.get("id") == msg.get("id") || last.is_none() {
                            last = Some(v);
                        }
                    }
                }
            }
            return Ok(last);
        }
        Ok(Some(serde_json::from_str(&body).with_context(|| format!("parse MCP reply: {}", body.chars().take(200).collect::<String>()))?))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
        }
    }
}

/// The text parts of a tools/call result.
pub fn text_of(result: &Value) -> String {
    result
        .get("content")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n"))
        .unwrap_or_else(|| result.get("structuredContent").map(|s| s.to_string()).unwrap_or_default())
}

/// Fill `{query}`, `{text}`, `{id}`… in an argument template (a JSON object
/// whose string values may contain placeholders).
pub fn fill_args(template: &Value, vars: &[(&str, &str)]) -> Value {
    match template {
        Value::String(s) => {
            let mut out = s.clone();
            for (k, v) in vars {
                out = out.replace(&format!("{{{k}}}"), v);
            }
            // A value that is exactly a placeholder keeps the raw string; nothing else to type-convert.
            Value::String(out)
        }
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), fill_args(v, vars))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(|v| fill_args(v, vars)).collect()),
        other => other.clone(),
    }
}

/// Pick the tool for a purpose from a server's list, with the arguments
/// filled from its schema, so a person pastes a URL instead of reading the
/// server's docs. Purposes: `search`, `get`, `list`, `transition`,
/// `comment`, `retain`, `recall`. `None` when nothing fits.
pub fn pick_tool(tools: &[Tool], purpose: &str) -> Option<(String, Value)> {
    let (names, slots): (&[&str], &[(&[&str], &str)]) = match purpose {
        "search" => (&["search", "query", "find", "lookup", "ask"], &[(&["query", "q", "search", "text", "keywords", "keyword", "question", "term"], "{query}")]),
        "recall" => (&["recall", "search", "query", "find", "retrieve"], &[(&["query", "q", "text", "question"], "{query}")]),
        "get" => (&["get", "read", "fetch", "page", "document", "open", "retrieve"], &[(&["id", "page_id", "doc_id", "document_id", "key", "ref", "reference", "uri", "url", "path"], "{id}")]),
        "list" => (&["list", "search", "query", "find"], &[(&["query", "jql", "q", "filter", "search"], "{query}")]),
        "transition" => (&["transition", "status", "move", "update"], &[(&["key", "issue", "issue_key", "issueidorkey", "id"], "{key}"), (&["to", "status", "transition", "state", "transition_name"], "{to}")]),
        "comment" => (&["comment"], &[(&["key", "issue", "issue_key", "issueidorkey", "id"], "{key}"), (&["text", "body", "comment", "content", "message"], "{text}")]),
        "retain" => (&["retain", "remember", "store", "save", "add", "memorize"], &[(&["content", "text", "memory", "note", "body"], "{text}")]),
        _ => return None,
    };
    let props = |t: &Tool| -> Vec<String> { t.input_schema.get("properties").and_then(|p| p.as_object()).map(|o| o.keys().cloned().collect()).unwrap_or_default() };
    let slot_for = |t: &Tool, aliases: &[&str]| -> Option<String> {
        let ps = props(t);
        aliases.iter().find_map(|a| ps.iter().find(|p| p.to_lowercase().replace(['-', '_'], "") == a.replace('_', "")).cloned())
    };
    let mut best: Option<(i32, String, Value)> = None;
    for t in tools {
        let lower = t.name.to_lowercase();
        let Some(rank) = names.iter().position(|n| lower.contains(n)) else { continue };
        let mut args = serde_json::Map::new();
        let mut filled = 0;
        for (aliases, value) in slots {
            if let Some(p) = slot_for(t, aliases) {
                args.insert(p, Value::String(value.to_string()));
                filled += 1;
            }
        }
        // Every slot named, or a tool without a schema (guess the first alias).
        if filled < slots.len() {
            if !props(t).is_empty() {
                continue;
            }
            for (aliases, value) in slots {
                args.entry(aliases[0].to_string()).or_insert(Value::String(value.to_string()));
            }
        }
        let score = 100 - rank as i32 * 10 + filled as i32 * 5 - (lower.len() as i32 / 8);
        if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
            best = Some((score, t.name.clone(), Value::Object(args)));
        }
    }
    best.map(|(_, n, a)| (n, a))
}

/// (purpose, tool, arguments) for each purpose a tool was found for.
pub type Picks = Vec<(String, String, Value)>;

/// Connect, list the tools and pick one per purpose.
pub fn discover(target: &McpTarget, purposes: &[&str]) -> Result<(Vec<Tool>, Picks)> {
    let tools = Client::connect(target)?.tools()?;
    let picks = purposes.iter().filter_map(|p| pick_tool(&tools, p).map(|(n, a)| (p.to_string(), n, a))).collect();
    Ok((tools, picks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_and_args() {
        std::env::set_var("HUBCODES_TEST_KEY", "abc");
        assert_eq!(expand_env("Bearer ${HUBCODES_TEST_KEY}"), "Bearer abc");
        assert_eq!(expand_env("no vars"), "no vars");
        let t = json!({ "query": "{query}", "limit": 5, "nested": { "q": "x {query} y" } });
        let f = fill_args(&t, &[("query", "token store")]);
        assert_eq!(f["query"], "token store");
        assert_eq!(f["nested"]["q"], "x token store y");
        assert_eq!(f["limit"], 5);
        assert_eq!(text_of(&json!({ "content": [{ "type": "text", "text": "a" }, { "type": "text", "text": "b" }] })), "a\nb");
    }

    #[test]
    fn tools_are_picked_by_name_and_filled_from_their_schema() {
        let tool = |name: &str, props: &[&str]| Tool { name: name.into(), description: String::new(), input_schema: json!({ "type": "object", "properties": props.iter().map(|p| (p.to_string(), json!({"type": "string"}))).collect::<serde_json::Map<_, _>>() }) };
        let tools = vec![tool("product_kb_search", &["query", "limit"]), tool("product_kb_get", &["doc_id"]), tool("jira_get", &["issue_key"]), tool("jira_comment", &["issue_key", "body"]), tool("unrelated", &[])];
        assert_eq!(pick_tool(&tools, "search"), Some(("product_kb_search".into(), json!({"query": "{query}"}))));
        assert_eq!(pick_tool(&tools, "get"), Some(("product_kb_get".into(), json!({"doc_id": "{id}"}))));
        assert_eq!(pick_tool(&tools, "comment"), Some(("jira_comment".into(), json!({"issue_key": "{key}", "body": "{text}"}))));
        assert_eq!(pick_tool(&tools, "retain"), None);
        // No schema: the usual argument name is guessed.
        assert_eq!(pick_tool(&[Tool { name: "recall".into(), description: String::new(), input_schema: Value::Null }], "recall"), Some(("recall".into(), json!({"query": "{query}"}))));
    }
}

/// The pinned `mcp-remote` that signs in to an OAuth MCP server through the
/// browser (Google and the like) and keeps the login under `~/.mcp-auth`.
pub const BRIDGE: &str = "mcp-remote@0.16.7";

/// The command that reaches `url` through the browser sign-in bridge.
/// The callback is fixed (localhost:31419/callback) so servers that only
/// accept a registered redirect work too.
pub fn oauth_bridge(url: &str) -> (String, Vec<String>) {
    let args = ["-y", BRIDGE, url.trim(), "31419", "--host", "localhost", "--callback-path", "/callback", "--auth-timeout", "180"];
    ("npx".to_string(), args.iter().map(|s| s.to_string()).collect())
}

/// The server a bridge command signs in to, if `command`/`args` is one.
pub fn bridged_url(command: &str, args: &[String]) -> Option<String> {
    if !command.trim().ends_with("npx") {
        return None;
    }
    let i = args.iter().position(|a| a.starts_with("mcp-remote"))?;
    args.get(i + 1).filter(|u| u.starts_with("http")).cloned()
}

#[cfg(test)]
mod bridge_tests {
    use super::*;

    #[test]
    fn the_bridge_round_trips_its_url() {
        let (cmd, args) = oauth_bridge(" https://mcp.example/kb ");
        assert_eq!(cmd, "npx");
        assert_eq!(bridged_url(&cmd, &args).as_deref(), Some("https://mcp.example/kb"));
        assert!(args.contains(&"31419".to_string()));
        assert_eq!(bridged_url("node", &args), None);
    }
}
