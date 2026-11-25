//! Fan-out / fan-in: one prompt to several agents, their answers gathered by
//! a collector agent (or just logged when there is none). Each receiver is
//! told to reply with `hubcodes send <collector> "…"`; replies that carry the
//! fan-out tag are counted, and when all are in the collector gets a wrap-up
//! note. State in SQLite (`fanouts`, `fanout_replies`).

use crate::db::{now, Db};
use crate::providers::Registry;
use crate::relay;
use crate::tmux::Tmux;
use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Fanout {
    pub id: i64,
    pub at: i64,
    pub prompt: String,
    pub collector: String,
    pub expected: i64,
    pub received: i64,
    pub done: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub agent: String,
    pub at: i64,
    pub text: String,
}

fn tag(id: i64) -> String {
    format!("[fanout #{id}]")
}

/// Send `prompt` to every target. Returns the fan-out record.
pub fn start(t: &Tmux, reg: &Registry, from: &str, targets: &[String], collector: Option<&str>, prompt: &str) -> Result<Fanout> {
    if targets.is_empty() {
        return Err(anyhow!("fan-out needs at least one target"));
    }
    let db = Db::open(&t.session)?;
    let collector = collector.unwrap_or("").to_string();
    db.conn().execute(
        "INSERT INTO fanouts (at, prompt, collector, expected) VALUES (?1, ?2, ?3, ?4)",
        params![now(), prompt, collector, targets.len() as i64],
    )?;
    let id = db.conn().last_insert_rowid();
    let reply_to = if collector.is_empty() { from.to_string() } else { collector.clone() };
    for target in targets {
        let body = format!(
            "{prompt}\n\n{} You are one of {} agents answering this. Reply with:\n  hubcodes send {reply_to} \"{} <your answer>\"",
            tag(id),
            targets.len(),
            tag(id)
        );
        relay::deliver(t, reg, from, target, &body)?;
    }
    let _ = t.record_message_kind(from, &targets.join(","), &format!("{} {prompt}", tag(id)), "fanout");
    get(&db, id)?.ok_or_else(|| anyhow!("fan-out vanished"))
}

pub fn get(db: &Db, id: i64) -> Result<Option<Fanout>> {
    db.conn()
        .query_row("SELECT id, at, prompt, collector, expected, received, done FROM fanouts WHERE id = ?1", params![id], |r| {
            Ok(Fanout { id: r.get(0)?, at: r.get(1)?, prompt: r.get(2)?, collector: r.get(3)?, expected: r.get(4)?, received: r.get(5)?, done: r.get::<_, i64>(6)? != 0 })
        })
        .optional()
        .map_err(Into::into)
}

pub fn list(db: &Db, limit: usize) -> Result<Vec<Fanout>> {
    let mut st = db.conn().prepare("SELECT id, at, prompt, collector, expected, received, done FROM fanouts ORDER BY id DESC LIMIT ?1")?;
    let rows = st.query_map(params![limit as i64], |r| {
        Ok(Fanout { id: r.get(0)?, at: r.get(1)?, prompt: r.get(2)?, collector: r.get(3)?, expected: r.get(4)?, received: r.get(5)?, done: r.get::<_, i64>(6)? != 0 })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn replies(db: &Db, id: i64) -> Result<Vec<Reply>> {
    let mut st = db.conn().prepare("SELECT agent, at, text FROM fanout_replies WHERE fanout = ?1 ORDER BY at")?;
    let rows = st.query_map(params![id], |r| Ok(Reply { agent: r.get(0)?, at: r.get(1)?, text: r.get(2)? }))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Called for every delivered message: if it carries a fan-out tag, record
/// the reply; when the last one lands, tell the collector. Returns the
/// fan-out when the message belonged to one.
pub fn observe(t: &Tmux, reg: &Registry, from: &str, text: &str) -> Result<Option<Fanout>> {
    let Some(rest) = text.trim().strip_prefix("[fanout #") else { return Ok(None) };
    let Some((id_s, answer)) = rest.split_once(']') else { return Ok(None) };
    let Ok(id) = id_s.trim().parse::<i64>() else { return Ok(None) };
    let db = Db::open(&t.session)?;
    let Some(f) = get(&db, id)? else { return Ok(None) };
    if f.done {
        return Ok(Some(f));
    }
    db.conn().execute("INSERT INTO fanout_replies (fanout, agent, at, text) VALUES (?1, ?2, ?3, ?4)", params![id, from, now(), answer.trim()])?;
    db.conn().execute("UPDATE fanouts SET received = received + 1 WHERE id = ?1", params![id])?;
    let f = get(&db, id)?.unwrap_or(f);
    if f.received >= f.expected {
        db.conn().execute("UPDATE fanouts SET done = 1 WHERE id = ?1", params![id])?;
        if !f.collector.is_empty() {
            let all = replies(&db, id)?;
            let summary = all.iter().map(|r| format!("- {}: {}", r.agent, r.text.chars().take(300).collect::<String>())).collect::<Vec<_>>().join("\n");
            let note = format!(
                "{} All {} answers are in for: {}\n{}\n\nCompare them and act on the best one (or combine them).",
                tag(id),
                f.expected,
                f.prompt.chars().take(200).collect::<String>(),
                summary
            );
            // Plain delivery, no trailer: this is a wrap-up, not a loop turn.
            let pane = t.pane_by_name(&f.collector);
            match pane {
                Ok(p) => {
                    let payload = if p.provider.as_deref().unwrap_or("shell") == "shell" { note.clone() } else { format!("[from hubcodes] {note}") };
                    t.send_text(&p.id, &payload)?;
                }
                Err(_) => {
                    relay::deliver(t, reg, "hubcodes", &f.collector, &note)?;
                }
            }
            let _ = t.record_message_kind("hubcodes", &f.collector, &note, "fanout");
        }
        return get(&db, id);
    }
    Ok(Some(f))
}
