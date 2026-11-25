//! Durable record of what happened in a session: every message, every status
//! event, tasks, fan-outs and token counts. SQLite at
//! `$XDG_STATE_HOME/hubcodes/<session>.db` (default `~/.local/state/…`).
//! tmux options stay the live bus; this is the memory.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("hubcodes")
}

pub fn path_for(session: &str) -> PathBuf {
    let safe: String = session.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    state_dir().join(format!("{safe}.db"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRow {
    pub seq: i64,
    pub at: i64,
    pub from: String,
    pub to: String,
    pub text: String,
    /// `message` (human or agent), `nudge`, `loop`, `task`, `fanout`
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRow {
    pub id: i64,
    pub at: i64,
    pub agent: String,
    /// `status`, `approval`, `spawn`, `kill`, `hook`, `budget`
    pub kind: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub turns: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    pub at: i64,
    pub agent: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub run: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingSend {
    pub id: i64,
    pub at: i64,
    /// Tailnet login of the contributor.
    pub by: String,
    pub to: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutboxRow {
    pub id: i64,
    pub at: i64,
    pub seq: i64,
    pub from: String,
    pub to: String,
    pub payload: String,
    pub attempts: i64,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(session: &str) -> Result<Self> {
        let path = path_for(session);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(3))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS messages (
               seq INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL,
               sender TEXT NOT NULL, receiver TEXT NOT NULL, text TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'message');
             CREATE TABLE IF NOT EXISTS events (
               id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL,
               agent TEXT NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '');
             CREATE TABLE IF NOT EXISTS usage (
               agent TEXT PRIMARY KEY, input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
               cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_write_tokens INTEGER NOT NULL DEFAULT 0, turns INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS tasks (
               id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, updated INTEGER NOT NULL,
               title TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'todo',
               role TEXT NOT NULL DEFAULT '', owner TEXT NOT NULL DEFAULT '', created_by TEXT NOT NULL DEFAULT '',
               result TEXT NOT NULL DEFAULT '', goal INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS fanouts (
               id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, prompt TEXT NOT NULL,
               collector TEXT NOT NULL, expected INTEGER NOT NULL, received INTEGER NOT NULL DEFAULT 0, done INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS fanout_replies (
               fanout INTEGER NOT NULL, agent TEXT NOT NULL, at INTEGER NOT NULL, text TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS outbox (
               id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, seq INTEGER NOT NULL DEFAULT 0,
               sender TEXT NOT NULL, receiver TEXT NOT NULL, payload TEXT NOT NULL,
               state TEXT NOT NULL DEFAULT 'queued', attempts INTEGER NOT NULL DEFAULT 0,
               delivered_at INTEGER NOT NULL DEFAULT 0, error TEXT NOT NULL DEFAULT '');
             CREATE TABLE IF NOT EXISTS pending (
               id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, by TEXT NOT NULL, receiver TEXT NOT NULL, text TEXT NOT NULL,
               state TEXT NOT NULL DEFAULT 'open', decided_by TEXT NOT NULL DEFAULT '', decided_at INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS usage_log (
               id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, agent TEXT NOT NULL, model TEXT NOT NULL DEFAULT '',
               input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
               cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_write_tokens INTEGER NOT NULL DEFAULT 0, run INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS rooms (
               name TEXT PRIMARY KEY, at INTEGER NOT NULL, created_by TEXT NOT NULL DEFAULT '');
             CREATE TABLE IF NOT EXISTS room_members (
               room TEXT NOT NULL, member TEXT NOT NULL, PRIMARY KEY (room, member));
             CREATE TABLE IF NOT EXISTS room_messages (
               id INTEGER PRIMARY KEY AUTOINCREMENT, room TEXT NOT NULL, at INTEGER NOT NULL,
               sender TEXT NOT NULL, text TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS room_messages_room ON room_messages(room, id);
             CREATE INDEX IF NOT EXISTS usage_log_at ON usage_log(at);
             CREATE INDEX IF NOT EXISTS messages_at ON messages(at);
             CREATE INDEX IF NOT EXISTS events_at ON events(at);",
        )?;
        // Columns added later (SQLite has no ADD COLUMN IF NOT EXISTS).
        for col in ["external TEXT NOT NULL DEFAULT ''", "external_url TEXT NOT NULL DEFAULT ''", "synced TEXT NOT NULL DEFAULT ''", "gate TEXT NOT NULL DEFAULT ''", "run INTEGER NOT NULL DEFAULT 0", "rank INTEGER NOT NULL DEFAULT 0"] {
            let _ = conn.execute(&format!("ALTER TABLE tasks ADD COLUMN {col}"), []);
        }
        Ok(Db { conn })
    }

    pub fn add_message(&self, from: &str, to: &str, text: &str, kind: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO messages (at, sender, receiver, text, kind) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), from, to, text, kind],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Newest first. `query` matches sender, receiver or text (case-insensitive).
    pub fn messages(&self, limit: usize, query: Option<&str>) -> Result<Vec<MessageRow>> {
        let like = format!("%{}%", query.unwrap_or("").trim());
        let mut st = self.conn.prepare(
            "SELECT seq, at, sender, receiver, text, kind FROM messages
             WHERE ?1 = '%%' OR sender LIKE ?1 OR receiver LIKE ?1 OR text LIKE ?1
             ORDER BY seq DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![like, limit as i64], |r| {
            Ok(MessageRow { seq: r.get(0)?, at: r.get(1)?, from: r.get(2)?, to: r.get(3)?, text: r.get(4)?, kind: r.get(5)? })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn add_event(&self, agent: &str, kind: &str, detail: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO events (at, agent, kind, detail) VALUES (?1, ?2, ?3, ?4)", params![now(), agent, kind, detail])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn events(&self, limit: usize, agent: Option<&str>) -> Result<Vec<EventRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, at, agent, kind, detail FROM events WHERE ?1 IS NULL OR agent = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![agent, limit as i64], |r| Ok(EventRow { id: r.get(0)?, at: r.get(1)?, agent: r.get(2)?, kind: r.get(3)?, detail: r.get(4)? }))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn add_usage(&self, agent: &str, input: i64, output: i64, cache_read: i64, cache_write: i64) -> Result<Usage> {
        self.add_usage_with(agent, "", 0, input, output, cache_read, cache_write)
    }

    /// Same, and one `usage_log` row per turn so cost can be read per day,
    /// per model and per playbook run.
    #[allow(clippy::too_many_arguments)]
    pub fn add_usage_with(&self, agent: &str, model: &str, run: i64, input: i64, output: i64, cache_read: i64, cache_write: i64) -> Result<Usage> {
        self.conn.execute(
            "INSERT INTO usage_log (at, agent, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, run) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![now(), agent, model, input, output, cache_read, cache_write, run],
        )?;
        self.conn.execute(
            "INSERT INTO usage (agent, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, turns)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)
             ON CONFLICT(agent) DO UPDATE SET input_tokens = input_tokens + ?2, output_tokens = output_tokens + ?3,
               cache_read_tokens = cache_read_tokens + ?4, cache_write_tokens = cache_write_tokens + ?5, turns = turns + 1",
            params![agent, input, output, cache_read, cache_write],
        )?;
        Ok(self.usage(agent)?.unwrap_or_default())
    }

    /// Usage rows of the last `days` days, newest first (for the cost page).
    pub fn usage_log(&self, days: i64) -> Result<Vec<UsageRow>> {
        let since = now() - days.max(1) * 86_400;
        let mut st = self.conn.prepare(
            "SELECT at, agent, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, run FROM usage_log WHERE at >= ?1 ORDER BY at DESC",
        )?;
        let rows = st.query_map(params![since], |r| {
            Ok(UsageRow { at: r.get(0)?, agent: r.get(1)?, model: r.get(2)?, input_tokens: r.get(3)?, output_tokens: r.get(4)?, cache_read_tokens: r.get(5)?, cache_write_tokens: r.get(6)?, run: r.get(7)? })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ----- outbox: messages waiting for a busy agent -----

    pub fn outbox_add(&self, seq: i64, from: &str, to: &str, payload: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO outbox (at, seq, sender, receiver, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), seq, from, to, payload],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn outbox_pending(&self) -> Result<Vec<OutboxRow>> {
        let mut st = self.conn.prepare("SELECT id, at, seq, sender, receiver, payload, attempts FROM outbox WHERE state = 'queued' ORDER BY id")?;
        let rows = st.query_map([], |r| Ok(OutboxRow { id: r.get(0)?, at: r.get(1)?, seq: r.get(2)?, from: r.get(3)?, to: r.get(4)?, payload: r.get(5)?, attempts: r.get(6)? }))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn outbox_mark(&self, id: i64, state: &str, error: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE outbox SET state = ?2, error = ?3, attempts = attempts + 1, delivered_at = CASE WHEN ?2 = 'delivered' THEN ?4 ELSE delivered_at END WHERE id = ?1",
            params![id, state, error, now()],
        )?;
        Ok(())
    }

    /// Queued messages per receiver (for the card badge).
    pub fn outbox_counts(&self) -> Result<Vec<(String, i64)>> {
        let mut st = self.conn.prepare("SELECT receiver, COUNT(*) FROM outbox WHERE state = 'queued' GROUP BY receiver")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ----- rooms: one transcript here, members anywhere -----

    pub fn room_create(&self, name: &str, by: &str) -> Result<bool> {
        Ok(self.conn.execute("INSERT OR IGNORE INTO rooms (name, at, created_by) VALUES (?1, ?2, ?3)", params![name, now(), by])? > 0)
    }

    pub fn room_delete(&self, name: &str) -> Result<bool> {
        self.conn.execute("DELETE FROM room_members WHERE room = ?1", params![name])?;
        self.conn.execute("DELETE FROM room_messages WHERE room = ?1", params![name])?;
        Ok(self.conn.execute("DELETE FROM rooms WHERE name = ?1", params![name])? > 0)
    }

    pub fn room_exists(&self, name: &str) -> Result<bool> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM rooms WHERE name = ?1", params![name], |r| r.get::<_, i64>(0))? > 0)
    }

    pub fn room_add_member(&self, room: &str, member: &str) -> Result<()> {
        self.conn.execute("INSERT OR IGNORE INTO room_members (room, member) VALUES (?1, ?2)", params![room, member])?;
        Ok(())
    }

    pub fn room_remove_member(&self, room: &str, member: &str) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM room_members WHERE room = ?1 AND member = ?2", params![room, member])? > 0)
    }

    pub fn room_members(&self, room: &str) -> Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT member FROM room_members WHERE room = ?1 ORDER BY member")?;
        let rows = st.query_map(params![room], |r| r.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Every room: (name, created at, created by).
    pub fn rooms(&self) -> Result<Vec<(String, i64, String)>> {
        let mut st = self.conn.prepare("SELECT name, at, created_by FROM rooms ORDER BY name")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn room_post(&self, room: &str, sender: &str, text: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO room_messages (room, at, sender, text) VALUES (?1, ?2, ?3, ?4)", params![room, now(), sender, text])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The last `limit` messages of a room, oldest first: (id, at, sender, text).
    pub fn room_log(&self, room: &str, limit: usize) -> Result<Vec<(i64, i64, String, String)>> {
        let mut st = self.conn.prepare("SELECT id, at, sender, text FROM (SELECT * FROM room_messages WHERE room = ?1 ORDER BY id DESC LIMIT ?2) ORDER BY id")?;
        let rows = st.query_map(params![room, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ----- pending: what a contributor sent, waiting for an operator -----

    pub fn pending_add(&self, by: &str, to: &str, text: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO pending (at, by, receiver, text) VALUES (?1, ?2, ?3, ?4)", params![now(), by, to, text])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn pending_open(&self) -> Result<Vec<PendingSend>> {
        let mut st = self.conn.prepare("SELECT id, at, by, receiver, text FROM pending WHERE state = 'open' ORDER BY id")?;
        let rows = st.query_map([], |r| Ok(PendingSend { id: r.get(0)?, at: r.get(1)?, by: r.get(2)?, to: r.get(3)?, text: r.get(4)? }))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn pending_decide(&self, id: i64, approved: bool, by: &str) -> Result<Option<PendingSend>> {
        let item = {
            let mut st = self.conn.prepare("SELECT id, at, by, receiver, text FROM pending WHERE id = ?1 AND state = 'open'")?;
            st.query_row(params![id], |r| Ok(PendingSend { id: r.get(0)?, at: r.get(1)?, by: r.get(2)?, to: r.get(3)?, text: r.get(4)? })).ok()
        };
        if item.is_some() {
            self.conn.execute("UPDATE pending SET state = ?2, decided_by = ?3, decided_at = ?4 WHERE id = ?1", params![id, if approved { "approved" } else { "denied" }, by, now()])?;
        }
        Ok(item)
    }

    /// Share audit: events of kind `share` (who did what through the web page).
    pub fn audit(&self, limit: usize) -> Result<Vec<EventRow>> {
        let mut st = self.conn.prepare("SELECT id, at, agent, kind, detail FROM events WHERE kind = 'share' ORDER BY id DESC LIMIT ?1")?;
        let rows = st.query_map(params![limit as i64], |r| Ok(EventRow { id: r.get(0)?, at: r.get(1)?, agent: r.get(2)?, kind: r.get(3)?, detail: r.get(4)? }))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn set_message_kind(&self, seq: i64, kind: &str) -> Result<()> {
        self.conn.execute("UPDATE messages SET kind = ?2 WHERE seq = ?1", params![seq, kind])?;
        Ok(())
    }

    pub fn usage(&self, agent: &str) -> Result<Option<Usage>> {
        self.conn
            .query_row(
                "SELECT input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, turns FROM usage WHERE agent = ?1",
                params![agent],
                |r| Ok(Usage { input_tokens: r.get(0)?, output_tokens: r.get(1)?, cache_read_tokens: r.get(2)?, cache_write_tokens: r.get(3)?, turns: r.get(4)? }),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn all_usage(&self) -> Result<Vec<(String, Usage)>> {
        let mut st = self.conn.prepare("SELECT agent, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, turns FROM usage")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, Usage { input_tokens: r.get(1)?, output_tokens: r.get(2)?, cache_read_tokens: r.get(3)?, cache_write_tokens: r.get(4)?, turns: r.get(5)? }))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn reset_usage(&self, agent: &str) -> Result<()> {
        self.conn.execute("DELETE FROM usage WHERE agent = ?1", params![agent])?;
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn outbox_round_trip() {
        let session = format!("outbox-test-{}", std::process::id());
        let db = super::Db::open(&session).unwrap();
        let seq = db.add_message("lead", "dev", "do it", "queued").unwrap();
        let id = db.outbox_add(seq, "lead", "dev", "[from lead] do it").unwrap();
        assert_eq!(db.outbox_pending().unwrap().len(), 1);
        assert_eq!(db.outbox_counts().unwrap(), vec![("dev".to_string(), 1)]);
        db.outbox_mark(id, "delivered", "").unwrap();
        db.set_message_kind(seq, "message").unwrap();
        assert!(db.outbox_pending().unwrap().is_empty());
        assert_eq!(db.messages(5, None).unwrap()[0].kind, "message");
        drop(db);
        let _ = std::fs::remove_file(super::path_for(&session));
    }

    use super::*;

    fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("hubcodes-core-tests-{}", std::process::id()));
        std::env::set_var("XDG_STATE_HOME", &dir);
        let _ = std::fs::remove_file(path_for(name));
        Db::open(name).unwrap()
    }

    #[test]
    fn messages_round_trip_and_search() {
        let db = temp_db("messages");
        db.add_message("human:me", "planner", "build the thing", "message").unwrap();
        db.add_message("planner", "builder", "step one", "message").unwrap();
        assert_eq!(db.messages(10, None).unwrap().len(), 2);
        let hits = db.messages(10, Some("step")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].to, "builder");
    }

    #[test]
    fn usage_accumulates() {
        let db = temp_db("usage");
        db.add_usage("a", 10, 5, 0, 0).unwrap();
        let u = db.add_usage("a", 1, 1, 7, 0).unwrap();
        assert_eq!((u.input_tokens, u.output_tokens, u.cache_read_tokens, u.turns), (11, 6, 7, 2));
    }
}
