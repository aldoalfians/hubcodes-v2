//! Playbooks: a goal walked through named steps, each a prompt to an agent
//! (by name or by role) with a gate before the next one. Steps become board
//! tasks, so everything that already works for tasks (dispatch, `task done`,
//! review, alerts) works for a playbook run.
//!
//! ```toml
//! [[playbook]]
//! name = "feature"
//! description = "Plan, build, test, review, merge"
//!
//! [[playbook.step]]
//! name = "plan"
//! agent = "lead"                       # a named agent…
//! prompt = "Write a plan for: {goal}. Put it in the shared notes (hubcodes note add), then run the task done command."
//! gate = "human"                       # none | human | test | review
//!
//! [[playbook.step]]
//! name = "build"
//! role = "software engineer"           # …or any idle agent with this role
//! prompt = "Implement the plan in the notes for: {goal}. Previous step by {prev_agent} said: {prev_result}"
//! gate = "test"                        # the repo's tests must pass; failures go back to the agent
//! retries = 2
//!
//! [[playbook.step]]
//! name = "review"
//! role = "reviewer"
//! prompt = "Review what {prev_agent} built for: {goal}. Approve the task when it is good, else send the changes back."
//! gate = "review"                      # the task sits in Review until a reviewer or you approve it
//!
//! [[playbook.step]]
//! name = "merge"
//! action = "merge {prev_agent}"        # an action instead of a prompt: merge <agent> | send <agent> "text" | task add "title"
//! ```
//!
//! Gates: `none` goes on as soon as the task is done; `human` pauses the run
//! until `hubcodes playbook approve <run>` (or the button); `test` runs the
//! repository's tests in the agent's folder and, on failure, hands the output
//! back to the same agent up to `retries` times; `review` requires the task
//! to be approved. Runs live in SQLite (`playbook_runs`) and are driven from
//! the poll loop, so the app and `hubcodes watch` behave the same.

use crate::db::{now, Db};
use crate::providers::Registry;
use crate::tasks;
use crate::tend::Event;
use crate::tmux::Tmux;
use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Step {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action: String,
    /// none | human | test | review
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gate: String,
    #[serde(default)]
    pub retries: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Playbook {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
}

/// One run of a playbook.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: i64,
    pub at: i64,
    pub updated: i64,
    pub playbook: String,
    pub goal: String,
    /// running | paused | testing | done | failed | aborted
    pub status: String,
    /// Index of the current step.
    pub step: i64,
    /// Board task of the current step (0 = none yet).
    pub task: i64,
    /// Merge queue row the current step waits for (0 = none).
    pub merge: i64,
    pub attempt: i64,
    pub log: String,
    pub created_by: String,
    pub cwd: String,
    /// What the previous step reported.
    pub prev_result: String,
    pub prev_agent: String,
}

fn ensure_table(db: &Db) -> Result<()> {
    db.conn().execute_batch(
        "CREATE TABLE IF NOT EXISTS playbook_runs (
           id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, updated INTEGER NOT NULL,
           playbook TEXT NOT NULL, goal TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'running',
           step INTEGER NOT NULL DEFAULT 0, task INTEGER NOT NULL DEFAULT 0, merge INTEGER NOT NULL DEFAULT 0,
           attempt INTEGER NOT NULL DEFAULT 0, log TEXT NOT NULL DEFAULT '', created_by TEXT NOT NULL DEFAULT '',
           cwd TEXT NOT NULL DEFAULT '', prev_result TEXT NOT NULL DEFAULT '', prev_agent TEXT NOT NULL DEFAULT '');",
    )?;
    Ok(())
}

const COLS: &str = "id, at, updated, playbook, goal, status, step, task, merge, attempt, log, created_by, cwd, prev_result, prev_agent";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        at: r.get(1)?,
        updated: r.get(2)?,
        playbook: r.get(3)?,
        goal: r.get(4)?,
        status: r.get(5)?,
        step: r.get(6)?,
        task: r.get(7)?,
        merge: r.get(8)?,
        attempt: r.get(9)?,
        log: r.get(10)?,
        created_by: r.get(11)?,
        cwd: r.get(12)?,
        prev_result: r.get(13)?,
        prev_agent: r.get(14)?,
    })
}

pub fn get(db: &Db, id: i64) -> Result<Option<Run>> {
    ensure_table(db)?;
    db.conn().query_row(&format!("SELECT {COLS} FROM playbook_runs WHERE id = ?1"), params![id], row).optional().map_err(Into::into)
}

/// Newest first; `open` keeps only runs that are not finished.
pub fn list(db: &Db, open: bool) -> Result<Vec<Run>> {
    ensure_table(db)?;
    let mut st = db.conn().prepare(&format!("SELECT {COLS} FROM playbook_runs WHERE ?1 = 0 OR status IN ('running', 'paused', 'testing') ORDER BY id DESC LIMIT 50"))?;
    let rows = st.query_map(params![open as i64], row)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn update(db: &Db, r: &Run) -> Result<()> {
    db.conn().execute(
        "UPDATE playbook_runs SET updated = ?2, status = ?3, step = ?4, task = ?5, merge = ?6, attempt = ?7, log = ?8, prev_result = ?9, prev_agent = ?10 WHERE id = ?1",
        params![r.id, now(), r.status, r.step, r.task, r.merge, r.attempt, r.log.chars().rev().take(8000).collect::<String>().chars().rev().collect::<String>(), r.prev_result, r.prev_agent],
    )?;
    Ok(())
}

fn log(r: &mut Run, line: impl AsRef<str>) {
    r.log.push_str(&format!("[{}] {}\n", now(), line.as_ref()));
}

/// Start a run: the first step is set up right away.
pub fn start(t: &Tmux, reg: &Registry, name: &str, goal: &str, by: &str, cwd: Option<&str>) -> Result<Run> {
    let reg = cwd.map(|c| reg.for_dir(std::path::Path::new(c))).unwrap_or_else(|| reg.clone());
    let pb = reg.playbooks.iter().find(|p| p.name == name).ok_or_else(|| anyhow!("no playbook '{name}'; known: {}", reg.playbooks.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")))?.clone();
    if pb.steps.is_empty() {
        return Err(anyhow!("playbook '{name}' has no steps"));
    }
    if goal.trim().is_empty() {
        return Err(anyhow!("a run needs a goal"));
    }
    let db = Db::open(&t.session)?;
    ensure_table(&db)?;
    db.conn().execute(
        "INSERT INTO playbook_runs (at, updated, playbook, goal, created_by, cwd) VALUES (?1, ?1, ?2, ?3, ?4, ?5)",
        params![now(), name, goal.trim(), by, cwd.unwrap_or("")],
    )?;
    let mut run = get(&db, db.conn().last_insert_rowid())?.ok_or_else(|| anyhow!("run vanished"))?;
    log(&mut run, format!("started by {by}: {}", goal.trim()));
    begin_step(t, &reg, &db, &mut run, &pb)?;
    update(&db, &run)?;
    let _ = db.add_event("hubcodes", "playbook", &format!("run #{} {name} started", run.id));
    Ok(run)
}

fn fill(text: &str, run: &Run, step: &Step) -> String {
    text.replace("{goal}", &run.goal)
        .replace("{prev_result}", &run.prev_result)
        .replace("{prev_agent}", &run.prev_agent)
        .replace("{run}", &run.id.to_string())
        .replace("{playbook}", &run.playbook)
        .replace("{step}", &step.name)
}

/// Set the current step in motion: a task for a prompt step, the action for an action step.
fn begin_step(t: &Tmux, reg: &Registry, db: &Db, run: &mut Run, pb: &Playbook) -> Result<()> {
    let Some(step) = pb.steps.get(run.step as usize) else {
        run.status = "done".into();
        log(run, "all steps done");
        return Ok(());
    };
    run.task = 0;
    run.merge = 0;
    if !step.action.is_empty() {
        let action = fill(&step.action, run, step);
        let w = crate::triggers::words(&action);
        match w.as_slice() {
            [m, agent] if m == "merge" => {
                let pane = t.pane_by_name(agent).map_err(|e| anyhow!("step {}: {e}", step.name))?;
                let dir = pane.worktree.clone().or_else(|| crate::sessiondef::pane_cwd(&pane.id)).unwrap_or_default();
                let path = std::path::Path::new(&dir);
                let branch = crate::git::changes(path).map(|c| c.branch).map_err(|e| anyhow!("step {}: {e}", step.name))?;
                let rc = crate::repo::load(path);
                let m = crate::merge::enqueue(db, path, &branch, &rc.merge.into, agent)?;
                run.merge = m.id;
                log(run, format!("step {}: merge #{} queued ({} → {})", step.name, m.id, m.branch, m.into));
            }
            _ => {
                let tr = crate::triggers::Trigger { name: format!("playbook #{}", run.id), on: String::new(), action: action.clone(), enabled: true };
                let ev = crate::tend::run_action(t, reg, &tr, &action);
                if ev.kind == "error" {
                    run.status = "failed".into();
                    log(run, format!("step {}: {}", step.name, ev.text));
                    return Ok(());
                }
                log(run, format!("step {}: {}", step.name, ev.text));
                return advance(t, reg, db, run, pb, String::new(), String::new());
            }
        }
        return Ok(());
    }
    let prompt = fill(&step.prompt, run, step);
    let title = format!("{} · {}: {}", run.playbook, step.name, run.goal.chars().take(60).collect::<String>());
    let gate = if step.gate == "review" { "review" } else { "" };
    let task = tasks::add_full(db, &title, &prompt, &step.role, &format!("playbook:{}", run.id), &step.agent, gate, run.id)?;
    run.task = task.id;
    if !step.agent.is_empty() {
        let text = tasks::assignment_text(&task, "the playbook");
        if let Err(e) = crate::relay::deliver(t, reg, "hubcodes", &step.agent, &text) {
            run.status = "paused".into();
            log(run, format!("step {}: could not reach {}: {e:#}", step.name, step.agent));
            return Ok(());
        }
    }
    log(run, format!("step {}: task #{} {}", step.name, task.id, if step.agent.is_empty() { format!("for an idle {}", if step.role.is_empty() { "agent".into() } else { step.role.clone() }) } else { format!("→ {}", step.agent) }));
    Ok(())
}

fn advance(t: &Tmux, reg: &Registry, db: &Db, run: &mut Run, pb: &Playbook, result: String, agent: String) -> Result<()> {
    run.prev_result = result;
    if !agent.is_empty() {
        run.prev_agent = agent;
    }
    run.step += 1;
    run.attempt = 0;
    run.status = "running".into();
    begin_step(t, reg, db, run, pb)
}

/// `hubcodes playbook approve <run>`: continue a run paused at a human gate.
pub fn approve(t: &Tmux, reg: &Registry, id: i64, by: &str) -> Result<Run> {
    let db = Db::open(&t.session)?;
    let mut run = get(&db, id)?.ok_or_else(|| anyhow!("no run #{id}"))?;
    if run.status != "paused" {
        return Err(anyhow!("run #{id} is {}, not paused", run.status));
    }
    let reg = if run.cwd.is_empty() { reg.clone() } else { reg.for_dir(std::path::Path::new(&run.cwd)) };
    let pb = reg.playbooks.iter().find(|p| p.name == run.playbook).cloned().ok_or_else(|| anyhow!("playbook {} is gone", run.playbook))?;
    log(&mut run, format!("approved by {by}"));
    let (res, agent) = (run.prev_result.clone(), run.prev_agent.clone());
    // A pause after a task: the task's result is already in prev_*; a pause from a failed delivery retries the step.
    if run.task == 0 && run.merge == 0 {
        advance_with(t, &reg, &db, &mut run, &pb, res, agent)?;
    } else {
        run.status = "running".into();
    }
    update(&db, &run)?;
    Ok(run)
}

fn advance_with(t: &Tmux, reg: &Registry, db: &Db, run: &mut Run, pb: &Playbook, res: String, agent: String) -> Result<()> {
    advance(t, reg, db, run, pb, res, agent)
}

pub fn abort(t: &Tmux, id: i64, by: &str) -> Result<Run> {
    let db = Db::open(&t.session)?;
    let mut run = get(&db, id)?.ok_or_else(|| anyhow!("no run #{id}"))?;
    if matches!(run.status.as_str(), "done" | "failed" | "aborted") {
        return Err(anyhow!("run #{id} already {}", run.status));
    }
    run.status = "aborted".into();
    log(&mut run, format!("aborted by {by}"));
    if run.task > 0 {
        let _ = tasks::set_status(&db, run.task, "done", Some("playbook aborted"));
    }
    update(&db, &run)?;
    Ok(run)
}

/// Drive every open run one step further where its gate allows.
pub fn tick(t: &Tmux, reg: &Registry) -> Vec<Event> {
    let mut events = Vec::new();
    let Ok(db) = Db::open(&t.session) else { return events };
    let Ok(runs) = list(&db, true) else { return events };
    for mut run in runs {
        if run.status != "running" {
            continue;
        }
        let reg_r = if run.cwd.is_empty() { reg.clone() } else { reg.for_dir(std::path::Path::new(&run.cwd)) };
        let Some(pb) = reg_r.playbooks.iter().find(|p| p.name == run.playbook).cloned() else {
            run.status = "failed".into();
            let msg = format!("playbook {} no longer exists", run.playbook);
            log(&mut run, msg);
            let _ = update(&db, &run);
            events.push(Event { kind: "error".into(), text: format!("Playbook run #{}: {} is gone", run.id, run.playbook) });
            continue;
        };
        let before = run.clone();
        if let Err(e) = step_tick(t, &reg_r, &db, &mut run, &pb, &mut events) {
            log(&mut run, format!("error: {e:#}"));
            events.push(Event { kind: "error".into(), text: format!("Playbook run #{}: {e:#}", run.id) });
        }
        if run != before {
            let _ = update(&db, &run);
        }
    }
    events
}

fn step_tick(t: &Tmux, reg: &Registry, db: &Db, run: &mut Run, pb: &Playbook, events: &mut Vec<Event>) -> Result<()> {
    let Some(step) = pb.steps.get(run.step as usize).cloned() else {
        run.status = "done".into();
        return Ok(());
    };
    // Action step waiting for the merge queue.
    if run.merge > 0 {
        let Some(m) = crate::merge::get(db, run.merge)? else {
            return Err(anyhow!("merge #{} vanished", run.merge));
        };
        match m.status.as_str() {
            "merged" => {
                log(run, format!("merge #{} done", m.id));
                advance(t, reg, db, run, pb, format!("merged {} into {}", m.branch, m.into), String::new())?;
                finish_events(run, events);
            }
            "queued" | "merging" => {}
            other => {
                run.status = "failed".into();
                log(run, format!("merge #{} {other}: {}", m.id, m.log.lines().last().unwrap_or("")));
                events.push(Event { kind: "error".into(), text: format!("Playbook run #{} failed at {}: merge {other}", run.id, step.name) });
            }
        }
        return Ok(());
    }
    if run.task == 0 {
        return Ok(());
    }
    let Some(task) = tasks::get(db, run.task)? else {
        return Err(anyhow!("task #{} of step {} was removed", run.task, step.name));
    };
    match task.status.as_str() {
        "done" => {
            let gate = step.gate.as_str();
            if gate == "test" {
                // Run the tests off the poll thread; the result lands back in the run row.
                let dir = agent_dir(t, &task.owner).unwrap_or_else(|| run.cwd.clone());
                if dir.is_empty() {
                    return Err(anyhow!("step {}: no folder to run tests in", step.name));
                }
                run.status = "testing".into();
                log(run, format!("step {}: running tests in {dir}", step.name));
                update(db, run)?;
                let (t2, run_id, owner, result, session) = (t.clone(), run.id, task.owner.clone(), task.result.clone(), t.session.clone());
                let retries = step.retries;
                std::thread::spawn(move || {
                    let r = crate::testrun::run_for_agent(&t2, &owner, std::path::Path::new(&dir), None);
                    let Ok(db) = Db::open(&session) else { return };
                    let Ok(Some(mut run)) = get(&db, run_id) else { return };
                    match r {
                        Ok(res) if res.ok => {
                            log(&mut run, format!("tests passed: {}", res.summary));
                            run.status = "running".into();
                            run.prev_result = format!("{result} (tests: {})", res.summary);
                            run.prev_agent = owner;
                            run.step += 1;
                            run.attempt = 0;
                            run.task = 0;
                            // The next step is set up by the next tick (needs the registry).
                            run.merge = -1;
                        }
                        Ok(res) => {
                            if (run.attempt as u32) < retries {
                                run.attempt += 1;
                                let msg = format!("tests failed ({}), retry {}/{}", res.summary, run.attempt, retries);
                                log(&mut run, msg);
                                let title = format!("{}: fix the tests (attempt {})", run.playbook, run.attempt);
                                let detail = format!("The tests failed after your last change:\n{}\n\n{}\n\nFix them, then run the task done command again.", res.summary, res.tail.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
                                if let Ok(task) = tasks::add_full(&db, &title, &detail, "", &format!("playbook:{}", run.id), &owner, "", run.id) {
                                    run.task = task.id;
                                    let _ = crate::relay::deliver_text(&t2, &owner, &tasks::assignment_text(&task, "the playbook"));
                                }
                                run.status = "running".into();
                            } else {
                                run.status = "failed".into();
                                log(&mut run, format!("tests failed ({}) after {} retries", res.summary, retries));
                            }
                        }
                        Err(e) => {
                            run.status = "failed".into();
                            log(&mut run, format!("could not run tests: {e:#}"));
                        }
                    }
                    let _ = update(&db, &run);
                });
                return Ok(());
            }
            if gate == "human" {
                run.status = "paused".into();
                run.prev_result = task.result.clone();
                run.prev_agent = task.owner.clone();
                run.task = 0;
                log(run, format!("step {} done by {}; waiting for your approval", step.name, task.owner));
                events.push(Event { kind: "alert".into(), text: format!("Playbook run #{} finished step {} ({}). Approve it to continue.", run.id, step.name, task.result.chars().take(80).collect::<String>()) });
                return Ok(());
            }
            log(run, format!("step {} done by {}: {}", step.name, task.owner, task.result.chars().take(120).collect::<String>()));
            advance(t, reg, db, run, pb, task.result.clone(), task.owner.clone())?;
            finish_events(run, events);
        }
        "review" if step.gate != "review" => {
            // The agent is blocked: the run waits for a human.
            run.status = "paused".into();
            log(run, format!("step {}: {} needs help: {}", step.name, task.owner, task.result.chars().take(120).collect::<String>()));
            events.push(Event { kind: "alert".into(), text: format!("Playbook run #{} is blocked at {}: {}", run.id, step.name, task.result.chars().take(100).collect::<String>()) });
        }
        _ => {}
    }
    Ok(())
}

/// After the test thread advanced the step index, set the next step up.
pub fn continue_after_tests(t: &Tmux, reg: &Registry) {
    let Ok(db) = Db::open(&t.session) else { return };
    let Ok(runs) = list(&db, true) else { return };
    for mut run in runs {
        if run.status != "running" || run.merge != -1 {
            continue;
        }
        run.merge = 0;
        let reg_r = if run.cwd.is_empty() { reg.clone() } else { reg.for_dir(std::path::Path::new(&run.cwd)) };
        if let Some(pb) = reg_r.playbooks.iter().find(|p| p.name == run.playbook).cloned() {
            let _ = begin_step(t, &reg_r, &db, &mut run, &pb);
        }
        let _ = update(&db, &run);
    }
}

fn finish_events(run: &Run, events: &mut Vec<Event>) {
    if run.status == "done" {
        events.push(Event { kind: "playbook".into(), text: format!("Playbook run #{} ({}) finished: {}", run.id, run.playbook, run.goal.chars().take(60).collect::<String>()) });
    } else if run.status == "failed" {
        events.push(Event { kind: "error".into(), text: format!("Playbook run #{} failed; see hubcodes playbook runs", run.id) });
    }
}

fn agent_dir(t: &Tmux, agent: &str) -> Option<String> {
    let p = t.pane_by_name(agent).ok()?;
    p.worktree.clone().or_else(|| crate::sessiondef::pane_cwd(&p.id))
}

/// Built-in playbooks (overridable by name).
pub fn default_playbooks() -> Vec<Playbook> {
    vec![Playbook {
        name: "feature".into(),
        description: "Plan, build with tests, review, merge".into(),
        steps: vec![
            Step { name: "plan".into(), role: "orchestrator".into(), prompt: "Write a short implementation plan for: {goal}. Put it in the shared notes with `hubcodes note add`, then run the task done command with a one-line summary.".into(), gate: "human".into(), ..Default::default() },
            Step { name: "build".into(), role: "software engineer".into(), prompt: "Implement the plan in the shared notes (hubcodes note show) for: {goal}. Keep the diff small, run the tests yourself, then run the task done command with what you changed and how to verify.".into(), gate: "test".into(), retries: 2, ..Default::default() },
            Step { name: "review".into(), role: "reviewer".into(), prompt: "Review what {prev_agent} built for: {goal} ({prev_result}). Read the diff in its folder. If it is good, approve the task with `hubcodes task approve`; else send {prev_agent} the changes with `hubcodes send {prev_agent} \"…\"` and wait, then run the task done command.".into(), gate: "review".into(), ..Default::default() },
            Step { name: "merge".into(), action: "merge {prev_agent}".into(), ..Default::default() },
        ],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_and_parses() {
        let run = Run { id: 7, at: 0, updated: 0, playbook: "feature".into(), goal: "login".into(), status: "running".into(), step: 1, task: 0, merge: 0, attempt: 0, log: String::new(), created_by: "me".into(), cwd: String::new(), prev_result: "planned".into(), prev_agent: "lead".into() };
        let s = Step { name: "build".into(), ..Default::default() };
        assert_eq!(fill("{goal}/{prev_agent}/{prev_result}/{run}/{playbook}/{step}", &run, &s), "login/lead/planned/7/feature/build");
        let toml = r#"
[[playbook]]
name = "x"
[[playbook.step]]
name = "a"
role = "r"
prompt = "p"
gate = "test"
retries = 1
"#;
        #[derive(Deserialize)]
        struct C { #[serde(rename = "playbook")] playbooks: Vec<Playbook> }
        let c: C = ::toml::from_str(toml).unwrap();
        assert_eq!(c.playbooks[0].steps[0].gate, "test");
        assert_eq!(default_playbooks()[0].steps.len(), 4);
    }
}
