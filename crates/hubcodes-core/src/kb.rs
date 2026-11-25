//! Knowledge sources behind one interface: `search(query)` → hits with a
//! title, a reference and a snippet; `get(reference)` → the text. As many
//! sources as the team has, each a `[[kb]]`:
//!
//! ```toml
//! [[kb]]
//! name = "product"
//! kind = "mcp"                          # mcp | folder | git | confluence | jira | command
//! url = "https://kb.example/mcp/"
//! headers = { Authorization = "Bearer ${KB_TOKEN}" }
//! search_tool = "product_kb_search"
//! search_args = { query = "{query}" }
//! get_tool = "product_kb_get"
//! get_args = { id = "{id}" }
//!
//! [[kb]]
//! name = "hermina-docs"
//! kind = "git"                          # cloned under ~/.local/state/hubcodes/kb/, pulled every hour
//! repo = "git@github.com:hubstudio/hermina-docs.git"
//!
//! [[kb]]
//! name = "wiki"
//! kind = "confluence"
//! url = "https://hubstudio.atlassian.net/wiki"
//! email = "aldo@hubstudio.id"
//! token = "${ATLASSIAN_TOKEN}"         # or "twg": sign in through the twg CLI, no email
//! space = "ENG"                         # optional
//! ```

use crate::db::Db;
use crate::mcp::{self, McpTarget};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KbSource {
    pub name: String,
    pub kind: String,
    // mcp
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub search_tool: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub search_args: Value,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub get_tool: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub get_args: Value,
    // folder / git
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    // confluence / jira
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub space: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub project: String,
    // command: JSON on stdin ({"op":"search","query":…} / {"op":"get","id":…}), JSON or text out
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub source: String,
    pub title: String,
    /// What `get` takes back: a URL, an id, a path.
    pub reference: String,
    pub snippet: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub ms: u64,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(25))).build().into()
}

fn target(s: &KbSource) -> McpTarget {
    McpTarget { url: s.url.clone(), command: s.command.clone(), args: s.args.clone(), headers: s.headers.clone(), env: BTreeMap::new() }
}

fn kb_dir() -> PathBuf {
    crate::db::state_dir().join("kb")
}

/// Folder for a git source, cloned on first use and pulled at most hourly.
fn git_folder(s: &KbSource) -> Result<PathBuf> {
    let dir = kb_dir().join(&s.name);
    if !dir.join(".git").exists() {
        std::fs::create_dir_all(kb_dir())?;
        let st = std::process::Command::new("git").args(["clone", "--depth", "1", "-q", &mcp::expand_env(&s.repo), &dir.display().to_string()]).status().context("git clone")?;
        if !st.success() {
            return Err(anyhow!("git clone {} failed", s.repo));
        }
        return Ok(dir);
    }
    let stamp = dir.join(".git").join("FETCH_HEAD");
    let stale = std::fs::metadata(&stamp).and_then(|m| m.modified()).map(|t| t.elapsed().map(|e| e > Duration::from_secs(3600)).unwrap_or(true)).unwrap_or(true);
    if stale {
        let _ = std::process::Command::new("git").args(["-C", &dir.display().to_string(), "pull", "-q", "--ff-only"]).status();
    }
    Ok(dir)
}

/// Score markdown/text files by query terms; title = first heading or file name.
fn search_folder(root: &Path, source: &str, query: &str, limit: usize) -> Result<Vec<Hit>> {
    let terms: Vec<String> = query.to_lowercase().split_whitespace().filter(|w| w.len() > 2).map(String::from).collect();
    if terms.is_empty() {
        return Ok(vec![]);
    }
    let mut scored: Vec<(usize, Hit)> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if !name.starts_with('.') && name != "node_modules" && name != "target" {
                    stack.push(p);
                }
                continue;
            }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if !matches!(ext.as_str(), "md" | "mdx" | "txt" | "rst" | "adoc" | "markdown") {
                continue;
            }
            seen += 1;
            if seen > 5000 {
                break;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let lower = text.to_lowercase();
            let score: usize = terms.iter().map(|t| lower.matches(t.as_str()).count().min(20) * (if name.to_lowercase().contains(t.as_str()) { 3 } else { 1 })).sum();
            if score == 0 || !terms.iter().any(|t| lower.contains(t.as_str())) {
                continue;
            }
            let title = text.lines().find(|l| l.starts_with('#')).map(|l| l.trim_start_matches('#').trim().to_string()).unwrap_or_else(|| name.clone());
            let pos = terms.iter().filter_map(|t| lower.find(t.as_str())).min().unwrap_or(0);
            let start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let snippet: String = text[start..].chars().take(240).collect::<String>().replace('\n', " ");
            scored.push((score, Hit { source: source.into(), title, reference: p.display().to_string(), snippet }));
        }
    }
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    Ok(scored.into_iter().take(limit).map(|(_, h)| h).collect())
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"")
}

fn atlassian_get(s: &KbSource, url: &str) -> Result<Value> {
    let auth = base64_encode(&format!("{}:{}", mcp::expand_env(&s.email), mcp::expand_env(&s.token)));
    let mut resp = match agent().get(url).header("Authorization", &format!("Basic {auth}")).header("Accept", "application/json").call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(code)) => return Err(anyhow!("HTTP {code} from {url}{}", if code == 401 || code == 403 { " (check email + API token)" } else { "" })),
        Err(e) => return Err(anyhow!("{url}: {e}")),
    };
    let text = resp.body_mut().read_to_string()?;
    serde_json::from_str(&text).with_context(|| format!("parse {url}"))
}

fn base64_encode(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = s.as_bytes();
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn search_one(s: &KbSource, query: &str, limit: usize) -> Result<Vec<Hit>> {
    match s.kind.as_str() {
        "folder" => search_folder(Path::new(&mcp::expand_env(&s.path)), &s.name, query, limit),
        "git" => search_folder(&git_folder(s)?, &s.name, query, limit),
        "mcp" => {
            let mut c = mcp::Client::connect(&target(s))?;
            let tool = if s.search_tool.is_empty() { "search" } else { &s.search_tool };
            let args_t = if s.search_args.is_null() { json!({ "query": "{query}" }) } else { s.search_args.clone() };
            let (text, raw) = c.call_tool(tool, mcp::fill_args(&args_t, &[("query", query), ("limit", &limit.to_string())]))?;
            Ok(hits_from(&s.name, &text, &raw, limit))
        }
        "confluence" => {
            let base = mcp::expand_env(&s.url).trim_end_matches('/').to_string();
            let cql = format!("type=page AND text~\"{}\"{}", query.replace('"', ""), if s.space.is_empty() { String::new() } else { format!(" AND space=\"{}\"", s.space) });
            if crate::twg::is_twg(&s.token) {
                let snip = |x: &str| strip_html(x).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(240).collect();
                return Ok(crate::twg::search_pages(&base, &cql, limit)?.into_iter().map(|(id, title, ex)| Hit { source: s.name.clone(), title, reference: format!("{base}/pages/{id}"), snippet: snip(&ex) }).collect());
            }
            let v = atlassian_get(s, &format!("{base}/rest/api/content/search?cql={}&limit={limit}&expand=body.view", url_encode(&cql)))?;
            let hits = v
                .get("results")
                .and_then(|r| r.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|p| {
                            let id = p.get("id")?.as_str()?;
                            let title = p.get("title")?.as_str()?.to_string();
                            let body = p.get("body").and_then(|b| b.get("view")).and_then(|v| v.get("value")).and_then(|v| v.as_str()).map(strip_html).unwrap_or_default();
                            Some(Hit { source: s.name.clone(), title, reference: format!("{base}/pages/{id}"), snippet: body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(240).collect() })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(hits)
        }
        "jira" => {
            let base = mcp::expand_env(&s.url).trim_end_matches('/').to_string();
            let jql = format!("text ~ \"{}\"{} ORDER BY updated DESC", query.replace('"', ""), if s.project.is_empty() { String::new() } else { format!(" AND project = {}", s.project) });
            let v = if crate::twg::is_twg(&s.token) {
                json!({ "issues": crate::twg::issues(&base, &jql, limit, "summary,status,description")? })
            } else {
                atlassian_get(s, &format!("{base}/rest/api/3/search/jql?jql={}&maxResults={limit}&fields=summary,status,description", url_encode(&jql)))?
            };
            let hits = v
                .get("issues")
                .and_then(|r| r.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|i| {
                            let key = i.get("key")?.as_str()?;
                            let f = i.get("fields")?;
                            let summary = f.get("summary").and_then(|x| x.as_str()).unwrap_or("");
                            let status = f.get("status").and_then(|x| x.get("name")).and_then(|x| x.as_str()).unwrap_or("");
                            Some(Hit { source: s.name.clone(), title: format!("{key} {summary}"), reference: format!("{base}/browse/{key}"), snippet: status.to_string() })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(hits)
        }
        "command" => {
            let out = run_json(&s.run, &json!({ "op": "search", "query": query, "limit": limit }))?;
            let raw: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
            Ok(hits_from(&s.name, &out, &raw, limit))
        }
        other => Err(anyhow!("unknown kb kind '{other}'")),
    }
}

fn hits_from(source: &str, text: &str, raw: &Value, limit: usize) -> Vec<Hit> {
    // JSON handed back as text (common for MCP tools) counts as structured.
    let parsed: Value = serde_json::from_str(text.trim()).unwrap_or(Value::Null);
    let raw = if parsed.is_object() || parsed.is_array() { &parsed } else { raw };
    if parsed.is_object() && parsed.get("results").and_then(|r| r.as_array()).map(|a| a.is_empty()).unwrap_or(false) {
        return vec![];
    }
    let arr = raw.as_array().or_else(|| raw.get("results").and_then(|r| r.as_array())).or_else(|| raw.get("hits").and_then(|r| r.as_array())).or_else(|| raw.get("items").and_then(|r| r.as_array()));
    if let Some(a) = arr {
        let list: Vec<Hit> = a
            .iter()
            .filter_map(|h| {
                let title = h.get("title").or_else(|| h.get("name")).and_then(|t| t.as_str()).unwrap_or("").to_string();
                // An id the get tool takes beats a citation URL (Product KB: document_id).
                let reference = h.get("document_id").or_else(|| h.get("doc_id")).or_else(|| h.get("url")).or_else(|| h.get("id")).or_else(|| h.get("path")).or_else(|| h.get("reference")).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
                let snippet = h.get("snippet").or_else(|| h.get("text")).or_else(|| h.get("content")).or_else(|| h.get("summary")).and_then(|t| t.as_str()).unwrap_or("").chars().take(240).collect();
                if title.is_empty() && reference.is_empty() {
                    return None;
                }
                Some(Hit { source: source.into(), title: if title.is_empty() { reference.clone() } else { title }, reference, snippet })
            })
            .take(limit)
            .collect();
        if !list.is_empty() {
            return list;
        }
    }
    // Plain text: paragraphs become hits without a reference (get returns the same text).
    text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).take(limit).map(|p| Hit { source: source.into(), title: p.lines().next().unwrap_or("").chars().take(80).collect(), reference: String::new(), snippet: p.chars().take(240).collect() }).collect()
}

/// Full text behind a hit.
pub fn get_one(s: &KbSource, reference: &str) -> Result<String> {
    match s.kind.as_str() {
        "folder" | "git" => {
            let root = if s.kind == "git" { git_folder(s)? } else { PathBuf::from(mcp::expand_env(&s.path)) };
            let p = Path::new(reference);
            let p = if p.is_absolute() { p.to_path_buf() } else { root.join(p) };
            if !p.canonicalize()?.starts_with(root.canonicalize()?) {
                return Err(anyhow!("{reference} is outside {}", root.display()));
            }
            Ok(std::fs::read_to_string(&p)?)
        }
        "mcp" => {
            let mut c = mcp::Client::connect(&target(s))?;
            let tool = if s.get_tool.is_empty() { return Err(anyhow!("no get_tool configured for {}", s.name)) } else { &s.get_tool };
            let args_t = if s.get_args.is_null() { json!({ "id": "{id}" }) } else { s.get_args.clone() };
            let (text, _) = c.call_tool(tool, mcp::fill_args(&args_t, &[("id", reference), ("url", reference)]))?;
            Ok(text)
        }
        "confluence" => {
            let base = mcp::expand_env(&s.url).trim_end_matches('/').to_string();
            let id = reference.rsplit('/').next().unwrap_or(reference);
            if crate::twg::is_twg(&s.token) {
                let (title, body) = crate::twg::page(&base, id)?;
                return Ok(format!("# {title}\n\n{}", body.trim()));
            }
            let v = atlassian_get(s, &format!("{base}/rest/api/content/{id}?expand=body.view"))?;
            let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("");
            let body = v.get("body").and_then(|b| b.get("view")).and_then(|v| v.get("value")).and_then(|v| v.as_str()).map(strip_html).unwrap_or_default();
            Ok(format!("# {title}\n\n{}", body.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")))
        }
        "jira" => {
            let base = mcp::expand_env(&s.url).trim_end_matches('/').to_string();
            let key = reference.rsplit('/').next().unwrap_or(reference);
            let v = if crate::twg::is_twg(&s.token) { crate::twg::issue(&base, key)? } else { atlassian_get(s, &format!("{base}/rest/api/3/issue/{key}?fields=summary,status,description,assignee"))? };
            let f = v.get("fields").cloned().unwrap_or(Value::Null);
            let desc = adf_text(f.get("description").unwrap_or(&Value::Null));
            Ok(format!(
                "# {key} {}\nStatus: {} · Assignee: {}\n\n{desc}",
                f.get("summary").and_then(|x| x.as_str()).unwrap_or(""),
                f.get("status").and_then(|x| x.get("name")).and_then(|x| x.as_str()).unwrap_or(""),
                f.get("assignee").and_then(|x| x.get("displayName")).and_then(|x| x.as_str()).unwrap_or("-")
            ))
        }
        "command" => run_json(&s.run, &json!({ "op": "get", "id": reference })),
        other => Err(anyhow!("unknown kb kind '{other}'")),
    }
}

/// Atlassian Document Format → plain text (paragraphs and list items).
fn adf_text(v: &Value) -> String {
    fn walk(v: &Value, out: &mut String) {
        if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
            out.push_str(t);
        }
        if let Some(c) = v.get("content").and_then(|c| c.as_array()) {
            for x in c {
                walk(x, out);
                if matches!(x.get("type").and_then(|t| t.as_str()), Some("paragraph") | Some("heading") | Some("listItem")) {
                    out.push('\n');
                }
            }
        }
    }
    let mut s = String::new();
    walk(v, &mut s);
    s
}

fn run_json(cmd: &str, input: &Value) -> Result<String> {
    if cmd.trim().is_empty() {
        return Err(anyhow!("run command is empty"));
    }
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-lc", &mcp::expand_env(cmd)])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
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

/// Search every enabled source (or one by name); failures become a hit-less
/// source noted in `errors`.
pub fn search(sources: &[KbSource], only: Option<&str>, query: &str, limit: usize) -> (Vec<Hit>, Vec<String>) {
    let mut hits = Vec::new();
    let mut errors = Vec::new();
    for s in sources.iter().filter(|s| s.enabled && only.map(|o| o == s.name).unwrap_or(true)) {
        match search_one(s, query, limit) {
            Ok(h) => hits.extend(h),
            Err(e) => errors.push(format!("{}: {e:#}", s.name)),
        }
    }
    (hits, errors)
}

pub fn get(sources: &[KbSource], source: &str, reference: &str) -> Result<String> {
    let s = sources.iter().find(|s| s.name == source).ok_or_else(|| anyhow!("no kb source '{source}'"))?;
    get_one(s, reference)
}

/// Settings → Knowledge → Test: can we reach it and does a search answer?
pub fn check(s: &KbSource) -> Check {
    let t0 = std::time::Instant::now();
    let r: Result<String> = (|| match s.kind.as_str() {
        "mcp" => {
            let mut c = mcp::Client::connect(&target(s))?;
            let tools = c.tools()?;
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            let want = if s.search_tool.is_empty() { "search" } else { &s.search_tool };
            if !names.contains(&want) {
                return Err(anyhow!("no tool '{want}'; the server offers: {}", names.join(", ")));
            }
            Ok(format!("MCP reachable, {} tools; search via '{want}'{}", tools.len(), if s.get_tool.is_empty() { ", no get_tool (hits show snippets only)" } else { "" }))
        }
        "folder" => {
            let p = PathBuf::from(mcp::expand_env(&s.path));
            if !p.is_dir() {
                return Err(anyhow!("{} is not a folder", p.display()));
            }
            let n = search_folder(&p, &s.name, "the", 1).map(|h| h.len()).unwrap_or(0);
            Ok(format!("{} readable{}", p.display(), if n > 0 { ", text files found" } else { " (no .md/.txt files matched a test search)" }))
        }
        "git" => {
            let d = git_folder(s)?;
            Ok(format!("cloned at {}", d.display()))
        }
        "confluence" => {
            let hits = search_one(s, "the", 1)?;
            Ok(format!("Confluence answers ({} hit for a test search)", hits.len()))
        }
        "jira" => {
            let hits = search_one(s, "the", 1)?;
            Ok(format!("Jira answers ({} hit for a test search)", hits.len()))
        }
        "command" => {
            let out = run_json(&s.run, &json!({ "op": "search", "query": "test", "limit": 1 }))?;
            Ok(format!("command answered ({} bytes)", out.len()))
        }
        other => Err(anyhow!("unknown kind '{other}'")),
    })();
    let ms = t0.elapsed().as_millis() as u64;
    match r {
        Ok(detail) => Check { name: s.name.clone(), ok: true, detail, ms },
        Err(e) => Check { name: s.name.clone(), ok: false, detail: format!("{e:#}"), ms },
    }
}

/// Log a search to the transcript so it is clear what the agent looked at.
pub fn record(session: &str, by: &str, query: &str, hits: &[Hit]) {
    if let Ok(db) = Db::open(session) {
        let _ = db.add_event(by, "kb", &format!("search “{}”: {} hits{}", query, hits.len(), hits.first().map(|h| format!(" · {} ({})", h.title, h.source)).unwrap_or_default()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_search_and_get() {
        let dir = std::env::temp_dir().join(format!("hubcodes-kb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ops")).unwrap();
        std::fs::write(dir.join("ops").join("deploy.md"), "# Deploy runbook\n\nDeploys happen on Friday after the e2e suite.\n").unwrap();
        std::fs::write(dir.join("auth.md"), "# Auth\n\nThe token store refreshes tokens.\n").unwrap();
        let s = KbSource { name: "docs".into(), kind: "folder".into(), path: dir.display().to_string(), url: String::new(), command: String::new(), args: vec![], headers: BTreeMap::new(), search_tool: String::new(), search_args: Value::Null, get_tool: String::new(), get_args: Value::Null, repo: String::new(), email: String::new(), token: String::new(), space: String::new(), project: String::new(), run: String::new(), enabled: true };
        let hits = search_one(&s, "deploy friday", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Deploy runbook");
        let text = get_one(&s, &hits[0].reference).unwrap();
        assert!(text.contains("Friday"));
        assert!(get_one(&s, "/etc/passwd").is_err());
        assert!(check(&s).ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn helpers() {
        assert_eq!(base64_encode("a@b:c"), "YUBiOmM=");
        assert_eq!(url_encode("a b\"c"), "a%20b%22c");
        assert_eq!(strip_html("<p>Hi <b>there</b>&amp;</p>"), "Hi there&");
        let adf = json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "one" }] }, { "type": "paragraph", "content": [{ "type": "text", "text": "two" }] }] });
        assert_eq!(adf_text(&adf), "one\ntwo\n");
    }
}
