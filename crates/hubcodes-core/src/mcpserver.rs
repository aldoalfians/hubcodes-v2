//! `hubcodes mcp`: the team as MCP tools, over stdio. An agent started by
//! hubcodes gets this server registered (Claude Code `--mcp-config`, Codex
//! `-c mcp_servers…`), so `send`, `read`, `task_*`, `note_*`, `memory_*`,
//! `kb_search` and `ask_human` are real tool calls instead of shell strings
//! the model has to get right. Every tool is the same code path as the CLI,
//! so the transcript, loops, policy and the board see no difference.
//!
//! Identity comes from the pane's environment (`HUBCODES_AGENT`,
//! `HUBCODES_SESSION`), the same as the CLI.

use crate::{db::Db, kb, memory, notes, providers::Registry, relay, tasks, tmux::Tmux};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const PROTOCOL: &str = "2025-06-18";

pub struct Server {
    pub t: Tmux,
    pub reg: Registry,
    pub agent: String,
    /// hubcodes.toml's modification time when `reg` was read. The server
    /// lives as long as its agent, so a device or prompt added later has to
    /// be picked up here, not only by agents started after it.
    seen: Option<std::time::SystemTime>,
}

/// One tool definition for `tools/list`.
fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required }
    })
}

pub fn tools() -> Vec<Value> {
    vec![
        tool("send", "Send a message to another agent of the team (by name, or device/agent). The message is queued when that agent is mid-turn and delivered at its next prompt.", json!({ "to": { "type": "string" }, "text": { "type": "string" } }), &["to", "text"]),
        tool("read", "What another agent is doing: its status and the last lines of its screen.", json!({ "agent": { "type": "string" }, "lines": { "type": "integer", "default": 30 } }), &["agent"]),
        tool("agents", "Every agent in the session with role, provider and status.", json!({}), &[]),
        tool("messages", "Recent messages in the session transcript, newest first.", json!({ "limit": { "type": "integer", "default": 20 }, "query": { "type": "string" } }), &[]),
        tool("task_add", "Put a task on the goal board. Unowned tasks go to an idle agent with the role.", json!({ "title": { "type": "string" }, "detail": { "type": "string" }, "role": { "type": "string" } }), &["title"]),
        tool("task_list", "Open tasks on the goal board.", json!({ "include_done": { "type": "boolean", "default": false } }), &[]),
        tool("room_post", "Post to a room: every other member hears it. A room on another device is written as device:room (e.g. mini:design).", json!({ "room": { "type": "string" }, "text": { "type": "string" } }), &["room", "text"]),
        tool("task_done", "Finish the task you own with a short result. Goes to review when the repo requires it.", json!({ "id": { "type": "integer" }, "result": { "type": "string" } }), &["id", "result"]),
        tool("note_add", "Append to the team's shared notes (what everyone should know).", json!({ "text": { "type": "string" } }), &["text"]),
        tool("note_show", "Read the shared notes.", json!({}), &[]),
        tool("memory_recall", "Search the team memory (decisions, conventions, past results).", json!({ "query": { "type": "string" }, "limit": { "type": "integer", "default": 5 } }), &["query"]),
        tool("memory_retain", "Remember something for the team across sessions.", json!({ "text": { "type": "string" }, "tags": { "type": "string" } }), &["text"]),
        tool("kb_search", "Search the knowledge sources (docs, wiki, tickets).", json!({ "query": { "type": "string" }, "limit": { "type": "integer", "default": 5 } }), &["query"]),
        tool("ask_human", "Ask the human a question. Marks you as needing them (notification, inbox); their answer arrives as a message.", json!({ "question": { "type": "string" } }), &["question"]),
        tool("task_show", "One task in full: fields, result, and what happened to it (messages and events).", json!({ "id": { "type": "integer" } }), &["id"]),
        tool("task_review", "Say you are blocked on a task or hand it to review with a note; the task's creator is told.", json!({ "id": { "type": "integer" }, "note": { "type": "string" } }), &["id", "note"]),
        tool("task_edit", "Change a task's title, detail or role (fields left out stay).", json!({ "id": { "type": "integer" }, "title": { "type": "string" }, "detail": { "type": "string" }, "role": { "type": "string" } }), &["id"]),
        tool("task_move", "Move a task to a board column (todo, doing, review, done, or one the team added).", json!({ "id": { "type": "integer" }, "column": { "type": "string" } }), &["id", "column"]),
        tool("task_assign", "Hand a task to a named agent now (it gets the assignment as a message).", json!({ "id": { "type": "integer" }, "agent": { "type": "string" } }), &["id", "agent"]),
        tool("task_columns", "The board's columns in order, with WIP limits.", json!({}), &[]),
        tool("peers", "The other devices and their agents (device/agent names you can send to), as last seen.", json!({}), &[]),
        tool("loop_status", "Running loops (pairs that answer each other) and who owes a reply.", json!({}), &[]),
        tool("fanout", "One prompt to several agents at once; a collector agent gathers the answers.", json!({ "prompt": { "type": "string" }, "to": { "type": "array", "items": { "type": "string" } }, "collector": { "type": "string" } }), &["prompt", "to"]),
        tool("role_show", "What an agent with this role is told at start (the brief plus the role file).", json!({ "role": { "type": "string" } }), &["role"]),
        tool("spawn", "Start a teammate: name, provider (claude, codex, opencode, pi, omp, gemini), role, optional model, folder, account (a separate login), or a device to run it on.", json!({ "name": { "type": "string" }, "provider": { "type": "string" }, "role": { "type": "string" }, "model": { "type": "string" }, "cwd": { "type": "string" }, "device": { "type": "string" }, "account": { "type": "string" } }), &["name", "provider"]),
        tool("accounts", "The logins (accounts) known for a provider, so several agents can run on different subscriptions.", json!({ "provider": { "type": "string" } }), &["provider"]),
        tool("policy_test", "What the auto-approve policy would answer for a shell command here (allow, deny, ask).", json!({ "command": { "type": "string" } }), &["command"]),
    ]
}

/// What `resources/list` offers: the live state an agent may want to read whole.
pub fn resources() -> Vec<Value> {
    let res = |uri: &str, name: &str, description: &str| json!({ "uri": uri, "name": name, "description": description, "mimeType": "text/plain" });
    vec![
        res("hubcodes://brief", "Your brief", "What you were told at start: your job, the team, how to reach them."),
        res("hubcodes://notes", "Shared notes", "What everyone on the team should know; grows as people add to it."),
        res("hubcodes://board", "Goal board", "Every open task with its column, owner and role."),
        res("hubcodes://agents", "Agents", "Who is on the team right now, here and on other devices."),
    ]
}

impl Server {
    pub fn new(session: &str, agent: &str) -> Result<Self> {
        let reg = Registry::load()?;
        let seen = reg.config_mtime();
        Ok(Server { t: Tmux::named(session), reg, agent: agent.to_string(), seen })
    }

    /// Read hubcodes.toml again when it changed since the last call.
    fn refresh(&mut self) {
        let now = self.reg.config_mtime();
        if now.is_some() && now != self.seen {
            if let Ok(next) = Registry::load() {
                self.reg = next;
                self.seen = now;
            }
        }
    }

    /// Serve stdin/stdout until EOF. Each line is one JSON-RPC message.
    pub fn run(&mut self) -> Result<()> {
        let stdin = std::io::stdin();
        let mut out = std::io::stdout();
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let msg: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    write_msg(&mut out, &json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } }))?;
                    continue;
                }
            };
            let id = msg.get("id").cloned();
            let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let params = msg.get("params").cloned().unwrap_or(json!({}));
            // Notifications have no id and get no reply.
            if id.is_none() || id == Some(Value::Null) {
                continue;
            }
            let reply = match self.handle(method, &params) {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": format!("{e:#}") } }),
            };
            write_msg(&mut out, &reply)?;
        }
        Ok(())
    }

    pub fn handle(&mut self, method: &str, params: &Value) -> Result<Value> {
        self.refresh();
        // Started by a tool hubcodes did not launch (its config dir carries
        // the entry for every session): offer nothing and do nothing.
        if self.agent.is_empty() {
            return match method {
                "initialize" => Ok(json!({ "protocolVersion": PROTOCOL, "capabilities": { "tools": {}, "resources": {}, "prompts": {} }, "serverInfo": { "name": "hubcodes", "version": env!("CARGO_PKG_VERSION") } })),
                "ping" => Ok(json!({})),
                "tools/list" => Ok(json!({ "tools": [] })),
                "resources/list" => Ok(json!({ "resources": [] })),
                "prompts/list" => Ok(json!({ "prompts": [] })),
                _ => Err(anyhow!("hubcodes tools are for agents hubcodes started (no HUBCODES_AGENT here)")),
            };
        }
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL,
                "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
                "serverInfo": { "name": "hubcodes", "version": env!("CARGO_PKG_VERSION") },
                "instructions": format!("You are agent '{}' in hubcodes session '{}'. Use these tools to talk to the team instead of shell commands.", self.agent, self.t.session)
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).ok_or_else(|| anyhow!("tools/call needs a name"))?;
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match self.call(name, &args) {
                    Ok(text) => Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": false })),
                    Err(e) => Ok(json!({ "content": [{ "type": "text", "text": format!("{e:#}") }], "isError": true })),
                }
            }
            "resources/list" => Ok(json!({ "resources": resources() })),
            "resources/read" => {
                let uri = params.get("uri").and_then(|u| u.as_str()).ok_or_else(|| anyhow!("resources/read needs a uri"))?;
                let text = self.resource(uri)?;
                Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/plain", "text": text }] }))
            }
            "prompts/list" => Ok(json!({ "prompts": self.reg.prompts.iter().map(|p| json!({ "name": p.name, "description": p.description, "arguments": [] })).collect::<Vec<_>>() })),
            "prompts/get" => {
                let name = params.get("name").and_then(|n| n.as_str()).ok_or_else(|| anyhow!("prompts/get needs a name"))?;
                let p = self.reg.prompts.iter().find(|p| p.name == name).ok_or_else(|| anyhow!("no prompt /{name}"))?;
                let text = crate::prompts::expand(&p.text, &self.prompt_context());
                Ok(json!({ "description": p.description, "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }))
            }
            _ => Err(anyhow!("method not found: {method}")),
        }
    }

    /// The pane this agent runs in (role, model, folder), for prompts and the brief.
    fn prompt_context(&self) -> crate::prompts::Context {
        let mut ctx = crate::prompts::Context { agent: self.agent.clone(), ..Default::default() };
        if let Ok(pane) = self.t.pane_by_name(&self.agent) {
            ctx.role = pane.role.unwrap_or_default();
            ctx.model = pane.model.unwrap_or_default();
            ctx.cwd = crate::sessiondef::pane_cwd(&pane.id).unwrap_or_default();
        }
        if ctx.cwd.is_empty() {
            ctx.cwd = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default();
        }
        ctx.notes = notes::read(&self.t.session);
        ctx
    }

    fn resource(&self, uri: &str) -> Result<String> {
        match uri {
            "hubcodes://brief" => {
                let ctx = self.prompt_context();
                let role = if ctx.role.is_empty() { None } else { Some(ctx.role.as_str()) };
                Ok(crate::prompts::effective_brief(&self.t.session, &self.agent, role, Some(&ctx.cwd)).0)
            }
            "hubcodes://notes" => Ok(notes::read(&self.t.session)),
            "hubcodes://board" => {
                let db = Db::open(&self.t.session)?;
                let list = tasks::list(&db, false)?;
                let cols = &self.reg.board.columns;
                let mut out = Vec::new();
                for c in cols {
                    let mine: Vec<String> = list.iter().filter(|t| t.status == c.id).map(|t| format!("  #{} {}{}{}", t.id, t.title, if t.owner.is_empty() { String::new() } else { format!(" · {}", t.owner) }, if t.role.is_empty() { String::new() } else { format!(" · role {}", t.role) })).collect();
                    out.push(format!("{} ({}){}", c.label, mine.len(), if c.wip > 0 { format!(" wip {}", c.wip) } else { String::new() }));
                    out.extend(mine);
                }
                Ok(out.join("\n"))
            }
            "hubcodes://agents" => self.agents_text(),
            _ => Err(anyhow!("no resource {uri}")),
        }
    }

    /// Everyone on the team: this session's panes plus other devices' agents from the peer cache.
    fn agents_text(&self) -> Result<String> {
        let panes = self.t.panes()?;
        let mut out = Vec::new();
        for p in panes.iter().filter(|p| p.provider.is_some() && p.provider.as_deref() != Some("proxy")) {
            out.push(format!("{}\t{}\t{}\t{}\there", p.name, p.role.clone().unwrap_or_default(), p.provider.clone().unwrap_or_default(), p.hook_status.clone().unwrap_or_else(|| if p.exited { "exited".into() } else { "unknown".into() })));
        }
        for a in crate::remote::cached_agents(&self.t.session) {
            out.push(format!("{}\t{}\t{}\t{}\t{}", a.name, a.role.clone().unwrap_or_default(), a.provider, a.status, a.peer));
        }
        Ok(if out.is_empty() { "no agents".into() } else { format!("name\trole\tprovider\tstatus\tdevice\n{}", out.join("\n")) })
    }

    pub fn call(&mut self, name: &str, a: &Value) -> Result<String> {
        let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let n = |k: &str, d: i64| a.get(k).and_then(|v| v.as_i64()).unwrap_or(d);
        match name {
            "send" => {
                let d = relay::deliver(&self.t, &self.reg, &self.agent, &s("to"), &s("text"))?;
                Ok(if d.queued { format!("queued for {}: it is mid-turn, delivered at its next prompt", s("to")) } else { format!("sent to {}", s("to")) })
            }
            "read" => {
                let agent = s("agent");
                let pane = self.t.pane_by_name(&agent)?;
                let lines = self.t.capture(&pane.id, n("lines", 30).clamp(1, 200) as usize)?;
                let status = pane.hook_status.clone().unwrap_or_else(|| if pane.exited { "exited".into() } else { "unknown".into() });
                Ok(format!("{agent} ({}, {status}):\n{}", pane.provider.clone().unwrap_or_default(), lines.join("\n")))
            }
            "agents" => self.agents_text(),
            "messages" => {
                let db = Db::open(&self.t.session)?;
                let q = s("query");
                let rows = db.messages(n("limit", 20).clamp(1, 200) as usize, if q.is_empty() { None } else { Some(q.as_str()) })?;
                Ok(rows.iter().map(|m| format!("[{}] {} → {}: {}", m.kind, m.from, m.to, m.text.replace('\n', " "))).collect::<Vec<_>>().join("\n"))
            }
            "task_add" => {
                let db = Db::open(&self.t.session)?;
                let t = tasks::add(&db, &s("title"), &s("detail"), &s("role"), &self.agent, "")?;
                Ok(format!("task #{} added", t.id))
            }
            "task_list" => {
                let db = Db::open(&self.t.session)?;
                let list = tasks::list(&db, a.get("include_done").and_then(|v| v.as_bool()).unwrap_or(false))?;
                Ok(list.iter().map(|t| format!("#{} [{}] {}{}{}", t.id, t.status, t.title, if t.owner.is_empty() { String::new() } else { format!(" · {}", t.owner) }, if t.role.is_empty() { String::new() } else { format!(" · role {}", t.role) })).collect::<Vec<_>>().join("\n"))
            }
            "room_post" => {
                let (home, room) = crate::rooms::split(&s("room"), &self.reg.peers);
                match home.filter(|h| *h != self.reg.sharing.name) {
                    Some(device) => {
                        let peer = self.reg.peers.iter().find(|p| p.name == device).ok_or_else(|| anyhow::anyhow!("unknown device '{device}'"))?;
                        let who = format!("{}/{}", self.reg.sharing.name, self.agent);
                        let text = s("text");
                        let args = ["room", "post", room.as_str(), text.as_str(), "--as", who.as_str()];
                        match crate::remote::run(peer, &args, std::time::Duration::from_secs(20)) {
                            Ok(out) => Ok(out.trim().to_string()),
                            Err(e) => {
                                let q = crate::remote::queue(&self.t.session, &device, &args, &format!("{e:#}"))?;
                                Ok(format!("{device} is unreachable; the post is queued (#{}) and goes out when it is back", q.id))
                            }
                        }
                    }
                    None => {
                        let n = crate::rooms::post(&self.t, &self.reg, &room, &self.agent, &s("text"))?;
                        Ok(format!("posted to {room} ({n} members)"))
                    }
                }
            }
            "task_done" => {
                let db = Db::open(&self.t.session)?;
                let review = crate::repo::load(&std::env::current_dir().unwrap_or_default()).board.review_required;
                let t = tasks::finish(&db, n("id", 0), &s("result"), &self.agent, review)?;
                Ok(format!("task #{} is now {}", t.id, t.status))
            }
            "note_add" => {
                notes::add(&self.t.session, &self.agent, &s("text"))?;
                Ok("noted".into())
            }
            "note_show" => Ok(notes::read(&self.t.session)),
            "memory_recall" => {
                let hits = memory::recall(&self.reg.memory, &self.t.session, &s("query"), n("limit", 5).clamp(1, 50) as usize)?;
                Ok(if hits.is_empty() { "nothing remembered about that".into() } else { hits.iter().map(|m| format!("- {}{}", m.text, if m.source.is_empty() { String::new() } else { format!(" ({})", m.source) })).collect::<Vec<_>>().join("\n") })
            }
            "memory_retain" => memory::retain(&self.reg.memory, &self.t.session, &self.agent, &s("text"), &s("tags")),
            "kb_search" => {
                let (hits, errors) = kb::search(&self.reg.kbs, None, &s("query"), n("limit", 5).clamp(1, 50) as usize);
                let mut out: Vec<String> = hits.iter().map(|h| format!("- [{}] {}\n  {}\n  ref: {}", h.source, h.title, h.snippet.replace('\n', " "), h.reference)).collect();
                for e in errors {
                    out.push(format!("! {e}"));
                }
                Ok(if out.is_empty() { "no hits".into() } else { out.join("\n") })
            }
            "ask_human" => {
                let q = s("question");
                let pane = self.t.pane_by_name(&self.agent)?;
                self.t.set_pane_option(&pane.id, crate::hooks::OPT_STATUS, "waiting")?;
                self.t.set_pane_option(&pane.id, crate::hooks::OPT_STATUS_AT, &crate::db::now().to_string())?;
                self.t.set_pane_option(&pane.id, crate::hooks::OPT_DETAIL, &q.replace('\n', " "))?;
                let _ = self.t.record_message_kind(&self.agent, "human", &q, "ask");
                if let Ok(db) = Db::open(&self.t.session) {
                    let _ = db.add_event(&self.agent, "status", &format!("waiting {q}"));
                }
                Ok("asked; the answer arrives as a message".into())
            }
            "task_show" => {
                let db = Db::open(&self.t.session)?;
                let id = n("id", 0);
                let t = tasks::get(&db, id)?.ok_or_else(|| anyhow!("no task #{id}"))?;
                let mut out = format!("#{} {}\nstatus {} · owner {} · role {} · created by {}", t.id, t.title, t.status, if t.owner.is_empty() { "-" } else { &t.owner }, if t.role.is_empty() { "-" } else { &t.role }, if t.created_by.is_empty() { "-" } else { &t.created_by });
                if !t.detail.is_empty() {
                    out.push_str(&format!("\ndetail: {}", t.detail));
                }
                if !t.result.is_empty() {
                    out.push_str(&format!("\nresult: {}", t.result));
                }
                let history = tasks::history(&db, id)?;
                if !history.is_empty() {
                    out.push_str("\nhistory:");
                    for h in history {
                        out.push_str(&format!("\n  {} [{}] {}", h.who, h.kind, h.text.replace('\n', " ")));
                    }
                }
                Ok(out)
            }
            "task_review" => {
                let db = Db::open(&self.t.session)?;
                let id = n("id", 0);
                let note = s("note");
                let x = tasks::set_status(&db, id, "review", Some(&note))?;
                if !x.created_by.is_empty() && !x.created_by.starts_with("human:") && !x.created_by.starts_with("trigger:") && x.created_by != self.agent {
                    let _ = relay::deliver(&self.t, &self.reg, &self.agent, &x.created_by, &format!("Task #{id} “{}” needs review: {note}", x.title));
                }
                Ok(format!("task #{id} is in review"))
            }
            "task_edit" => {
                let db = Db::open(&self.t.session)?;
                let id = n("id", 0);
                let cur = tasks::get(&db, id)?.ok_or_else(|| anyhow!("no task #{id}"))?;
                let pick = |k: &str, old: &str| a.get(k).and_then(|v| v.as_str()).map(|v| v.to_string()).unwrap_or_else(|| old.to_string());
                let x = tasks::update(&db, id, &pick("title", &cur.title), &pick("detail", &cur.detail), &pick("role", &cur.role))?;
                Ok(format!("task #{id}: {} ({})", x.title, if x.role.is_empty() { "any role" } else { &x.role }))
            }
            "task_move" => {
                let db = Db::open(&self.t.session)?;
                let x = tasks::move_to(&db, n("id", 0), &s("column"), None, &self.reg.board)?;
                Ok(format!("task #{} in {}", x.id, x.status))
            }
            "task_assign" => {
                let db = Db::open(&self.t.session)?;
                let agent = s("agent");
                let x = tasks::assign(&db, n("id", 0), &agent)?;
                relay::deliver(&self.t, &self.reg, "hubcodes", &agent, &tasks::assignment_text(&x, &x.created_by))?;
                Ok(format!("task #{} → {agent}", x.id))
            }
            "task_columns" => Ok(self.reg.board.columns.iter().map(|c| format!("{}\t{}{}{}", c.id, c.label, if c.wip > 0 { format!("\twip {}", c.wip) } else { String::new() }, if crate::board::is_builtin(&c.id) { "" } else { "\t(yours)" })).collect::<Vec<_>>().join("\n")),
            "peers" => {
                if self.reg.peers.is_empty() {
                    return Ok("no other devices".into());
                }
                let cached = crate::remote::cached_agents(&self.t.session);
                Ok(self.reg.peers.iter().map(|p| {
                    let theirs: Vec<String> = cached.iter().filter(|a| a.peer == p.name).map(|a| format!("{} ({}, {})", a.name, a.role.clone().unwrap_or_else(|| "no role".into()), a.status)).collect();
                    format!("{} @ {}: {}", p.name, p.host, if theirs.is_empty() { "no agents seen (or unreachable)".to_string() } else { theirs.join(", ") })
                }).collect::<Vec<_>>().join("\n"))
            }
            "loop_status" => {
                let loops = relay::loops(&self.t);
                let mut out: Vec<String> = loops.iter().map(|(k, st)| format!("{}: {} round {}{}", k.replace('|', " ↔ "), if st.done { "done" } else if st.paused { "paused" } else { "running" }, crate::relay::round_text(st.rounds, st.max), if st.goal.is_empty() { String::new() } else { format!(" · {}", st.goal) })).collect();
                for (who, aw) in relay::awaiting(&self.t) {
                    out.push(format!("{who} owes a reply to {}{}", aw.to, if aw.nudged { " (nudged)" } else { "" }));
                }
                Ok(if out.is_empty() { "no loops".into() } else { out.join("\n") })
            }
            "fanout" => {
                let targets: Vec<String> = a.get("to").and_then(|v| v.as_array()).map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
                if targets.is_empty() {
                    return Err(anyhow!("fanout needs at least one target"));
                }
                let collector = s("collector");
                let f = crate::fanout::start(&self.t, &self.reg, &self.agent, &targets, if collector.is_empty() { None } else { Some(collector.as_str()) }, &s("prompt"))?;
                Ok(format!("fanout #{} sent to {}{}", f.id, targets.join(", "), if f.collector.is_empty() { String::new() } else { format!("; {} gathers the answers", f.collector) }))
            }
            "role_show" => {
                let ctx = self.prompt_context();
                Ok(crate::prompts::effective_brief(&self.t.session, "", Some(&s("role")), Some(&ctx.cwd)).0)
            }
            "spawn" => {
                let name = s("name");
                let provider = s("provider");
                if name.trim().is_empty() || provider.trim().is_empty() {
                    return Err(anyhow!("spawn needs a name and a provider"));
                }
                let role = s("role");
                let model = s("model");
                let cwd = s("cwd");
                let device = s("device");
                let account = s("account");
                let opt = |v: &str| if v.trim().is_empty() { None } else { Some(v.trim().to_string()) };
                if let Some(device) = opt(&device) {
                    let peer = self.reg.peers.iter().find(|p| p.name == device).ok_or_else(|| anyhow!("unknown device '{device}'"))?;
                    let full = crate::remote::spawn_account(peer, &name, &provider, opt(&cwd).as_deref(), opt(&role).as_deref(), opt(&model).as_deref(), opt(&account).as_deref())?;
                    return Ok(format!("started {full}"));
                }
                if self.t.pane_by_name(&name).is_ok() {
                    return Err(anyhow!("an agent named {name} is already running"));
                }
                self.reg.get(&provider).ok_or_else(|| anyhow!("unknown provider '{provider}'"))?;
                let exe = crate::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into());
                let tpl = crate::team::TemplateAgent { name: name.clone(), provider: provider.clone(), role: opt(&role), model: opt(&model), worktree: None, sandbox: false, device: None, account: opt(&account) };
                let cmd = crate::team::launch_command_in(&self.reg, &self.t.session, &tpl, &exe, None, opt(&cwd).as_deref())?;
                let dir = opt(&cwd).filter(|c| std::path::Path::new(c).is_dir());
                let id = self.t.spawn_with_account(&name, &provider, &cmd, dir.as_deref(), tpl.role.as_deref(), tpl.model.as_deref(), tpl.account.as_deref())?;
                if let Ok(db) = Db::open(&self.t.session) {
                    let _ = db.add_event(&name, "spawn", &format!("{provider}{} by {}", tpl.role.as_ref().map(|r| format!(" {r}")).unwrap_or_default(), self.agent));
                }
                Ok(format!("started {name} ({id}); send it work with the send tool once it is at its prompt"))
            }
            "accounts" => {
                let provider = s("provider");
                let list = crate::accounts::list(&provider);
                let p = self.reg.get(&provider).ok_or_else(|| anyhow!("unknown provider '{provider}'"))?;
                Ok(list.iter().map(|a| { let st = crate::models::status_for(p, a); format!("{a}\t{}", match st.logged_in { Some(true) => "signed in", Some(false) => "not signed in", None => "unknown" }) }).collect::<Vec<_>>().join("\n"))
            }
            "policy_test" => {
                let cwd = self.prompt_context().cwd;
                let policy = self.reg.for_dir(std::path::Path::new(&cwd)).policy;
                Ok(match crate::policy::decide_command(&policy, &s("command")) {
                    crate::policy::Decision::Allow(r) => format!("allow ({r})"),
                    crate::policy::Decision::Deny(r) => format!("deny ({r})"),
                    crate::policy::Decision::Ask => format!("ask (no rule matches{})", if policy.is_on() { "" } else { "; policy is off" }),
                })
            }
            _ => Err(anyhow!("unknown tool {name}")),
        }
    }
}

// ----- registering the server with the tools that read a config file for
// MCP servers (Claude Code and Codex take it on the command line). Each
// write is idempotent and keeps everything else in the file.

/// opencode: `mcp.hubcodes` in `<config dir>/opencode.json` (`type: local`).
pub fn ensure_opencode_mcp(exe: &str) -> Result<PathBuf> {
    let dir = std::env::var_os("OPENCODE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config/opencode"));
    let path = dir.join("opencode.json");
    merge_json_file(&path, |root| {
        let mcp = root.entry("mcp").or_insert_with(|| json!({}));
        if !mcp.is_object() {
            *mcp = json!({});
        }
        mcp.as_object_mut().unwrap().insert("hubcodes".into(), json!({ "type": "local", "enabled": true, "command": [exe, "mcp"] }));
    })?;
    Ok(path)
}

/// pi and omp: `mcpServers.hubcodes` in `<agent dir>/mcp.json` (omp under its profile).
pub fn ensure_agent_dir_mcp(kind: &str, exe: &str, profile: Option<&str>) -> Result<PathBuf> {
    ensure_agent_dir_mcp_at(kind, exe, profile, None)
}

/// [`ensure_agent_dir_mcp`] with an explicit agent dir (an account's) for pi.
pub fn ensure_agent_dir_mcp_at(kind: &str, exe: &str, profile: Option<&str>, agent_dir: Option<&Path>) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("no home directory"))?;
    let shim = crate::hooks::shim_path_at(kind, &home, profile, agent_dir).ok_or_else(|| anyhow!("{kind} has no agent dir"))?;
    // `<agent dir>/extensions/hubcodes.ts` → `<agent dir>/mcp.json`
    let agent_dir = shim.parent().and_then(|p| p.parent()).ok_or_else(|| anyhow!("odd shim path {}", shim.display()))?.to_path_buf();
    let path = agent_dir.join("mcp.json");
    merge_json_file(&path, |root| insert_mcp_server(root, exe))?;
    Ok(path)
}

/// gemini: `mcpServers.hubcodes` in `~/.gemini/settings.json`.
pub fn ensure_gemini_mcp(exe: &str) -> Result<PathBuf> {
    ensure_gemini_mcp_for(exe, &crate::accounts::Paths::default())
}

/// [`ensure_gemini_mcp`] in an account's `.gemini` dir.
pub fn ensure_gemini_mcp_for(exe: &str, paths: &crate::accounts::Paths) -> Result<PathBuf> {
    let path = crate::hooks::gemini_settings_path_for(paths).ok_or_else(|| anyhow!("no home directory"))?;
    merge_json_file(&path, |root| insert_mcp_server(root, exe))?;
    Ok(path)
}

fn insert_mcp_server(root: &mut serde_json::Map<String, Value>, exe: &str) {
    let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
    if !servers.is_object() {
        *servers = json!({});
    }
    servers.as_object_mut().unwrap().insert("hubcodes".into(), json!({ "command": exe, "args": ["mcp"] }));
}

/// Read a JSON object file (missing = empty), let `edit` change it, write it
/// back only when something changed. A file that is not a JSON object is
/// left alone and reported.
fn merge_json_file(path: &Path, edit: impl FnOnce(&mut serde_json::Map<String, Value>)) -> Result<bool> {
    let current: Value = match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => serde_json::from_str(&text).with_context(|| format!("{} is not JSON; add the hubcodes MCP server by hand or fix the file", path.display()))?,
        _ => json!({}),
    };
    let mut root = current.as_object().cloned().ok_or_else(|| anyhow!("{} is not a JSON object", path.display()))?;
    edit(&mut root);
    let next = Value::Object(root);
    if next == current {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&next)?).with_context(|| format!("write {}", path.display()))?;
    Ok(true)
}

fn write_msg(out: &mut impl Write, v: &Value) -> Result<()> {
    out.write_all(v.to_string().as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// The `--mcp-config` JSON for Claude Code (one server: this binary, `mcp`).
pub fn claude_mcp_config(exe: &str) -> String {
    json!({ "mcpServers": { "hubcodes": { "command": exe, "args": ["mcp"] } } }).to_string()
}

/// The `-c` overrides for Codex. Codex starts MCP servers with a short,
/// fixed environment, so without `env_vars` the server would not know which
/// agent it serves and every `send` would be signed as a person.
pub fn codex_mcp_args(exe: &str) -> Vec<String> {
    vec![
        "-c".into(),
        format!("mcp_servers.hubcodes.command={}", serde_json::to_string(exe).unwrap_or_default()),
        "-c".into(),
        "mcp_servers.hubcodes.args=[\"mcp\"]".into(),
        "-c".into(),
        format!("mcp_servers.hubcodes.env_vars={}", serde_json::to_string(IDENTITY_VARS).unwrap_or_default()),
    ]
}

/// What an MCP server needs from the agent's pane to act as that agent.
const IDENTITY_VARS: &[&str] = &["HUBCODES_AGENT", "HUBCODES_SESSION", "HUBCODES_ROLE", "HUBCODES_MODEL", "HUBCODES_ACCOUNT", "TMUX", "TMUX_PANE"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inert_outside_hubcodes() {
        let mut s = Server::new("inert-test", "").unwrap();
        assert_eq!(s.handle("tools/list", &json!({})).unwrap()["tools"], json!([]));
        assert_eq!(s.handle("prompts/list", &json!({})).unwrap()["prompts"], json!([]));
        assert!(s.handle("initialize", &json!({})).unwrap().get("instructions").is_none());
        assert!(s.handle("tools/call", &json!({ "name": "send", "arguments": { "to": "dev", "text": "hi" } })).is_err());
    }

    #[test]
    fn tool_list_has_schemas() {
        let t = tools();
        assert!(t.iter().any(|x| x["name"] == "send"));
        for x in &t {
            assert_eq!(x["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn every_tool_has_a_dispatch_arm_and_resources_are_listed() {
        let names: Vec<String> = tools().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for want in ["task_show", "task_review", "task_edit", "task_move", "task_assign", "task_columns", "peers", "loop_status", "fanout", "role_show", "spawn", "policy_test"] {
            assert!(names.contains(&want.to_string()), "{want} missing");
        }
        assert!(resources().iter().any(|r| r["uri"] == "hubcodes://board"));
    }

    #[test]
    fn mcp_registration_files_merge_without_clobbering() {
        let dir = std::env::temp_dir().join(format!("hubmcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        std::fs::write(&p, r#"{ "theme": "dark", "mcpServers": { "fs": { "command": "npx" } } }"#).unwrap();
        assert!(merge_json_file(&p, |r| insert_mcp_server(r, "/x/hubcodes")).unwrap());
        assert!(!merge_json_file(&p, |r| insert_mcp_server(r, "/x/hubcodes")).unwrap(), "second time: nothing to write");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["fs"]["command"], "npx");
        assert_eq!(v["mcpServers"]["hubcodes"]["args"][0], "mcp");
        std::fs::write(&p, "not json").unwrap();
        assert!(merge_json_file(&p, |r| insert_mcp_server(r, "/x/hubcodes")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn launch_configs() {
        assert!(claude_mcp_config("/usr/local/bin/hubcodes").contains("\"args\":[\"mcp\"]"));
        let c = codex_mcp_args("/x/hubcodes");
        assert_eq!(c[1], "mcp_servers.hubcodes.command=\"/x/hubcodes\"");
        assert!(c[5].starts_with("mcp_servers.hubcodes.env_vars=[\"HUBCODES_AGENT\""), "{}", c[5]);
    }
}
