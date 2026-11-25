//! Goal board: a goal is split into tasks, each with a status and an owner.
//! Agents create and finish tasks with `hubcodes task …`; hubcodes hands
//! unassigned tasks to idle agents whose role matches (or to any idle agent
//! when the task names no role). Stored in the session's SQLite.

use crate::db::{now, Db};
use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const STATUSES: [&str; 4] = ["todo", "doing", "review", "done"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: i64,
    pub at: i64,
    pub updated: i64,
    pub title: String,
    pub detail: String,
    pub status: String,
    /// Role the task wants (`frontend`), empty = anyone.
    pub role: String,
    pub owner: String,
    pub created_by: String,
    pub result: String,
    /// `<tracker>:<key>` when it came from an issue tracker.
    #[serde(default)]
    pub external: String,
    #[serde(default)]
    pub external_url: String,
    /// Tracker sync state: `` | `started` | `done`.
    #[serde(default)]
    pub synced: String,
    /// `review` forces `task done` into Review (playbook review gate).
    #[serde(default)]
    pub gate: String,
    /// Playbook run this task belongs to (0 = none).
    #[serde(default)]
    pub run: i64,
    /// Position inside its column (lower first); idle agents take the
    /// lowest-ranked To do task.
    #[serde(default)]
    pub rank: i64,
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        at: r.get(1)?,
        updated: r.get(2)?,
        title: r.get(3)?,
        detail: r.get(4)?,
        status: r.get(5)?,
        role: r.get(6)?,
        owner: r.get(7)?,
        created_by: r.get(8)?,
        result: r.get(9)?,
        external: r.get(10)?,
        external_url: r.get(11)?,
        synced: r.get(12)?,
        gate: r.get(13)?,
        run: r.get(14)?,
        rank: r.get(15)?,
    })
}

const COLS: &str = "id, at, updated, title, detail, status, role, owner, created_by, result, external, external_url, synced, gate, run, rank";

pub fn add(db: &Db, title: &str, detail: &str, role: &str, by: &str, owner: &str) -> Result<Task> {
    add_full(db, title, detail, role, by, owner, "", 0)
}

/// `add` plus the gate and playbook run.
#[allow(clippy::too_many_arguments)]
pub fn add_full(db: &Db, title: &str, detail: &str, role: &str, by: &str, owner: &str, gate: &str, run: i64) -> Result<Task> {
    if title.trim().is_empty() {
        return Err(anyhow!("task needs a title"));
    }
    let status = if owner.is_empty() { "todo" } else { "doing" };
    db.conn().execute(
        "INSERT INTO tasks (at, updated, title, detail, status, role, owner, created_by, gate, run, rank) VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, (SELECT COALESCE(MAX(rank), 0) + 10 FROM tasks WHERE status = ?4))",
        params![now(), title.trim(), detail.trim(), status, role.trim(), owner.trim(), by, gate, run],
    )?;
    get(db, db.conn().last_insert_rowid())?.ok_or_else(|| anyhow!("task vanished"))
}

/// A task imported from an issue tracker (`external` = `<tracker>:<key>`).
pub fn add_external(db: &Db, title: &str, detail: &str, role: &str, by: &str, external: &str, url: &str) -> Result<Task> {
    if title.trim().is_empty() {
        return Err(anyhow!("task needs a title"));
    }
    db.conn().execute(
        "INSERT INTO tasks (at, updated, title, detail, status, role, owner, created_by, external, external_url, rank) VALUES (?1, ?1, ?2, ?3, 'todo', ?4, '', ?5, ?6, ?7, (SELECT COALESCE(MAX(rank), 0) + 10 FROM tasks WHERE status = 'todo'))",
        params![now(), title.trim(), detail.trim(), role.trim(), by, external, url],
    )?;
    get(db, db.conn().last_insert_rowid())?.ok_or_else(|| anyhow!("task vanished"))
}

pub fn set_synced(db: &Db, id: i64, synced: &str) -> Result<()> {
    db.conn().execute("UPDATE tasks SET synced = ?2 WHERE id = ?1", params![id, synced])?;
    Ok(())
}

pub fn get(db: &Db, id: i64) -> Result<Option<Task>> {
    db.conn()
        .query_row(&format!("SELECT {COLS} FROM tasks WHERE id = ?1"), params![id], row)
        .optional()
        .map_err(Into::into)
}

pub fn list(db: &Db, include_done: bool) -> Result<Vec<Task>> {
    let mut st = db.conn().prepare(&format!("SELECT {COLS} FROM tasks WHERE ?1 OR status != 'done' ORDER BY CASE status WHEN 'doing' THEN 0 WHEN 'review' THEN 1 WHEN 'todo' THEN 2 ELSE 3 END, rank, id"))?;
    let rows = st.query_map(params![include_done], row)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn set_status(db: &Db, id: i64, status: &str, result: Option<&str>) -> Result<Task> {
    if !STATUSES.contains(&status) {
        return Err(anyhow!("status must be one of {}", STATUSES.join(", ")));
    }
    db.conn().execute(
        "UPDATE tasks SET status = ?2, updated = ?3, result = COALESCE(?4, result) WHERE id = ?1",
        params![id, status, now(), result],
    )?;
    get(db, id)?.ok_or_else(|| anyhow!("no task #{id}"))
}

/// Move a task to any column the board has, built in or the user's own.
pub fn move_to(db: &Db, id: i64, column: &str, result: Option<&str>, board: &crate::board::BoardConfig) -> Result<Task> {
    if !board.allows(column) {
        return Err(anyhow!("no column “{column}”; the board has {}", board.ids().join(", ")));
    }
    db.conn().execute(
        "UPDATE tasks SET status = ?2, updated = ?3, result = COALESCE(?4, result) WHERE id = ?1",
        params![id, column, now(), result],
    )?;
    get(db, id)?.ok_or_else(|| anyhow!("no task #{id}"))
}

/// Edit what a task says; empty title is refused, role may be cleared.
pub fn update(db: &Db, id: i64, title: &str, detail: &str, role: &str) -> Result<Task> {
    if title.trim().is_empty() {
        return Err(anyhow!("task needs a title"));
    }
    db.conn().execute("UPDATE tasks SET title = ?2, detail = ?3, role = ?4, updated = ?5 WHERE id = ?1", params![id, title.trim(), detail.trim(), role.trim(), now()])?;
    get(db, id)?.ok_or_else(|| anyhow!("no task #{id}"))
}

/// Put a task in a column at a position: before `before` (a task in that
/// column) or at the end. The column's tasks are renumbered 10, 20, 30…
pub fn place(db: &Db, id: i64, column: &str, before: Option<i64>, board: &crate::board::BoardConfig) -> Result<Task> {
    if !board.allows(column) {
        return Err(anyhow!("no column “{column}”; the board has {}", board.ids().join(", ")));
    }
    let mut ids: Vec<i64> = {
        let mut st = db.conn().prepare("SELECT id FROM tasks WHERE status = ?1 AND id != ?2 ORDER BY rank, id")?;
        let rows = st.query_map(params![column, id], |r| r.get::<_, i64>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    let at = before.and_then(|b| ids.iter().position(|x| *x == b)).unwrap_or(ids.len());
    ids.insert(at, id);
    let tx = db.conn().unchecked_transaction()?;
    for (i, tid) in ids.iter().enumerate() {
        tx.execute("UPDATE tasks SET rank = ?2 WHERE id = ?1", params![tid, (i as i64 + 1) * 10])?;
    }
    tx.execute("UPDATE tasks SET status = ?2, updated = ?3 WHERE id = ?1", params![id, column, now()])?;
    tx.commit()?;
    get(db, id)?.ok_or_else(|| anyhow!("no task #{id}"))
}

/// Something that happened to a task, from the message and event logs.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub at: i64,
    pub who: String,
    pub kind: String,
    pub text: String,
}

/// Messages and events that name this task (`#3`, `task #3`), oldest first.
pub fn history(db: &Db, id: i64) -> Result<Vec<HistoryItem>> {
    let tag = format!("#{id}");
    let names_it = |text: &str| text.match_indices(&tag).any(|(i, _)| text[i + tag.len()..].chars().next().map(|c| !c.is_ascii_digit()).unwrap_or(true));
    let mut out: Vec<HistoryItem> = db
        .messages(400, Some(&tag))?
        .into_iter()
        .filter(|m| names_it(&m.text))
        .map(|m| HistoryItem { at: m.at, who: m.from.clone(), kind: m.kind.clone(), text: format!("to {}: {}", m.to, m.text.chars().take(400).collect::<String>()) })
        .collect();
    out.extend(db.events(400, None)?.into_iter().filter(|e| names_it(&e.detail)).map(|e| HistoryItem { at: e.at, who: e.agent.clone(), kind: e.kind.clone(), text: e.detail.chars().take(400).collect() }));
    out.sort_by_key(|h| h.at);
    Ok(out)
}

/// Every task in `from` goes to `to` (a removed column folds into To do).
pub fn fold_column(db: &Db, from: &str, to: &str) -> Result<usize> {
    Ok(db.conn().execute("UPDATE tasks SET status = ?2, updated = ?3 WHERE status = ?1", params![from, to, now()])?)
}

/// An agent finished: `done`, or `review` when the repo requires a reviewer
/// (`[board] review_required`). Humans always close tasks.
pub fn finish(db: &Db, id: i64, result: &str, by: &str, review_required: bool) -> Result<Task> {
    let human = by.starts_with("human:");
    let task = get(db, id)?;
    // A pull request review must say whether anything blocks merging: the
    // Approve button and `auto_approve` read that last line.
    if let Some(t) = task.as_ref().filter(|t| !human && crate::tracker::is_review_task(t)) {
        let asks = result.trim_start().to_ascii_uppercase().starts_with("NEEDS HUMAN");
        if !asks && !crate::tracker::has_verdict(result) {
            return Err(anyhow!(
                "task #{} reviews {}: end the result with a last line `VERDICT: approve` (no 🔴, 🟡 or needs_validation finding and the tests you ran passed) or `VERDICT: changes`, then run task done again",
                t.id,
                t.external.split_once(':').map(|x| x.1).unwrap_or(&t.external)
            ));
        }
    }
    let gated = task.map(|t| t.gate == "review").unwrap_or(false);
    let status = if (review_required || gated) && !human { "review" } else { "done" };
    set_status(db, id, status, Some(result))
}

/// Who may approve a task in Review: humans, and agents whose role is a
/// reviewer (`reviewer`, `qa`, `review`…), but never the task's own owner.
pub fn can_approve(by: &str, by_role: &str, task: &Task) -> bool {
    if by.starts_with("human:") {
        return true;
    }
    let r = by_role.to_lowercase();
    (r.contains("review") || r == "qa" || r == "lead" || r == "orchestrator") && task.owner != by
}

pub fn assign(db: &Db, id: i64, owner: &str) -> Result<Task> {
    db.conn().execute(
        "UPDATE tasks SET owner = ?2, status = CASE WHEN status = 'todo' THEN 'doing' ELSE status END, updated = ?3 WHERE id = ?1",
        params![id, owner, now()],
    )?;
    get(db, id)?.ok_or_else(|| anyhow!("no task #{id}"))
}

pub fn remove(db: &Db, id: i64) -> Result<()> {
    db.conn().execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    Ok(())
}

/// What an agent is told when a task is handed to it.
pub fn assignment_text(t: &Task, from: &str) -> String {
    assignment_text_on(t, from, "")
}

/// [`assignment_text`] for an agent on another device: the task id carries
/// this device's name (`mac:3`), so its `hubcodes task done mac:3 …` is
/// forwarded here instead of landing on its own board.
pub fn assignment_text_on(t: &Task, from: &str, home: &str) -> String {
    let id = if home.is_empty() { t.id.to_string() } else { format!("{home}:{}", t.id) };
    let mut s = format!("[hubcodes task #{}] {}", t.id, t.title);
    if !t.detail.is_empty() {
        s.push('\n');
        s.push_str(&skill_detail(&t.detail, |n| crate::skills::find(n).is_some()));
    }
    s.push_str(&format!(
        "\n\nWhen it is done, run:\n  hubcodes task done {id} \"<what you did and how to verify>\"\nIf you are blocked, run:\n  hubcodes task review {id} \"<what you need>\"{}",
        if from.is_empty() { String::new() } else { format!("\nThis task was created by {from}.") }
    ));
    if !home.is_empty() {
        s.push_str(&format!("\nThe board for this task is on {home}; keep the {id} form in both commands."));
    }
    s
}

/// A detail whose first line is `/skill args` (a tracker instruction such as
/// `/pr-review {url}`): that line becomes words any provider follows, since
/// the assignment never starts with it and a slash command is only read as
/// one at the very start of the input.
pub fn skill_detail(detail: &str, known: impl Fn(&str) -> bool) -> String {
    let (first, rest) = detail.split_once('\n').unwrap_or((detail, ""));
    match crate::skills::command_of(first, known) {
        Some((name, args)) => {
            let line = crate::skills::invocation(&name, &args);
            if rest.is_empty() { line } else { format!("{line}\n{rest}") }
        }
        None => detail.to_string(),
    }
}

/// Pick the next `todo` task for an idle agent: same role first, then any
/// task without a role. Tasks whose `external` starts with one of `held`
/// (the `name:` of a tracker that is off) stay where they are.
pub fn next_for(db: &Db, agent_role: &str, held: &[String]) -> Result<Option<Task>> {
    let todo: Vec<Task> = list(db, false)?.into_iter().filter(|t| t.status == "todo" && t.owner.is_empty() && !held.iter().any(|h| t.external.starts_with(h.as_str()))).collect();
    if let Some(t) = todo.iter().find(|t| !t.role.is_empty() && t.role == agent_role) {
        return Ok(Some(t.clone()));
    }
    Ok(todo.into_iter().find(|t| t.role.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_instruction_is_spelled_out() {
        let known = |n: &str| n == "pr-review";
        let d = skill_detail("/pr-review https://github.com/o/a/pull/1\nmore", known);
        assert!(d.starts_with("Use the `pr-review` skill on https://github.com/o/a/pull/1") && d.ends_with("\nmore"), "{d}");
        assert_eq!(skill_detail("/standup", known), "/standup", "not a skill: as written");
        assert_eq!(skill_detail("Review it", known), "Review it");
    }

    #[test]
    fn tasks_of_a_tracker_that_is_off_are_held() {
        let dir = std::env::temp_dir().join(format!("hubcodes-core-tests-{}", std::process::id()));
        std::env::set_var("XDG_STATE_HOME", &dir);
        let db = Db::open("tasks-held").unwrap();
        let t = add_external(&db, "Review o/a#1", "", "reviewer", "reviews", "reviews:o/a#1", "").unwrap();
        assert!(next_for(&db, "reviewer", &["reviews:".into()]).unwrap().is_none());
        assert_eq!(next_for(&db, "reviewer", &["jira:".into()]).unwrap().unwrap().id, t.id);
    }

    #[test]
    fn a_review_cannot_finish_without_a_verdict() {
        let dir = std::env::temp_dir().join(format!("hubcodes-core-tests-{}", std::process::id()));
        std::env::set_var("XDG_STATE_HOME", &dir);
        let _ = std::fs::remove_file(crate::db::path_for("tasks-verdict"));
        let db = Db::open("tasks-verdict").unwrap();
        let t = add_external(&db, "Review o/a#2", "", "reviewer", "reviews", "reviews:o/a#2", "").unwrap();
        let err = finish(&db, t.id, "No defects found.", "Reviewer-1", false).unwrap_err().to_string();
        assert!(err.contains("VERDICT: approve") && err.contains("o/a#2"), "{err}");
        assert!(finish(&db, t.id, "NEEDS HUMAN: which base?", "Reviewer-1", false).is_ok(), "asking a person is fine");
        assert_eq!(finish(&db, t.id, "0 🔴, 0 🟡\nVERDICT: approve", "Reviewer-1", false).unwrap().status, "done");
        assert!(finish(&db, t.id, "closed by hand", "human:aldo", false).is_ok(), "a person needs no verdict");
        let other = add(&db, "write docs", "", "", "lead", "").unwrap();
        assert!(finish(&db, other.id, "done", "eng", false).is_ok(), "only reviews need one");
    }

    #[test]
    fn lifecycle_and_dispatch_order() {
        let dir = std::env::temp_dir().join(format!("hubcodes-core-tests-{}", std::process::id()));
        std::env::set_var("XDG_STATE_HOME", &dir);
        let _ = std::fs::remove_file(crate::db::path_for("tasks"));
        let db = Db::open("tasks").unwrap();
        let a = add(&db, "write store", "in src/auth", "backend", "lead", "").unwrap();
        let b = add(&db, "write docs", "", "", "lead", "").unwrap();
        assert_eq!(next_for(&db, "backend", &[]).unwrap().unwrap().id, a.id);
        assert_eq!(next_for(&db, "frontend", &[]).unwrap().unwrap().id, b.id, "no frontend task: takes the roleless one");
        assign(&db, a.id, "api").unwrap();
        assert_eq!(get(&db, a.id).unwrap().unwrap().status, "doing");
        let done = set_status(&db, a.id, "done", Some("tests green")).unwrap();
        assert_eq!(done.result, "tests green");
        assert_eq!(list(&db, false).unwrap().len(), 1);
        assert_eq!(list(&db, true).unwrap().len(), 2);
        assert!(set_status(&db, b.id, "bogus", None).is_err());
        let c = add(&db, "needs review", "", "", "lead", "api").unwrap();
        assert_eq!(finish(&db, c.id, "ok", "api", true).unwrap().status, "review");
        assert_eq!(finish(&db, c.id, "ok", "human:me", true).unwrap().status, "done");
        let d = add(&db, "no gate", "", "", "lead", "api").unwrap();
        assert_eq!(finish(&db, d.id, "ok", "api", false).unwrap().status, "done");
        let board = crate::board::BoardConfig::default();
        let x = add(&db, "x", "", "", "me", "").unwrap();
        let y = add(&db, "y", "", "", "me", "").unwrap();
        let z = add(&db, "z", "", "", "me", "").unwrap();
        place(&db, z.id, "todo", Some(x.id), &board).unwrap();
        let order: Vec<i64> = list(&db, false).unwrap().into_iter().filter(|t| t.status == "todo" && [x.id, y.id, z.id].contains(&t.id)).map(|t| t.id).collect();
        assert_eq!(order, vec![z.id, x.id, y.id], "z placed before x");
        let first_todo = list(&db, false).unwrap().into_iter().find(|t| t.status == "todo" && t.owner.is_empty() && t.role.is_empty()).unwrap().id;
        assert_eq!(next_for(&db, "", &[]).unwrap().unwrap().id, first_todo, "the lowest rank goes first");
        assert!(place(&db, z.id, "nowhere", None, &board).is_err());
        let e = update(&db, z.id, "z2", "more", "tests").unwrap();
        assert_eq!((e.title.as_str(), e.detail.as_str(), e.role.as_str()), ("z2", "more", "tests"));
        assert!(update(&db, z.id, " ", "", "").is_err());
        let text = assignment_text_on(&d, "human:me", "mac");
        assert!(text.contains("hubcodes task done mac:") && text.contains("hubcodes task review mac:"), "{text}");
        assert!(!assignment_text(&d, "").contains("mac:"));
        assert!(can_approve("reviewer", "reviewer", &c));
        assert!(!can_approve("api", "backend", &c));
        assert!(!can_approve("api", "reviewer", &c), "owner cannot approve its own");
        assert!(assignment_text(&b, "lead").contains("hubcodes task done 2"));
    }
}
