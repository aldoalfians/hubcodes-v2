//! Merge queue: bring an agent's branch back into the target branch, one at
//! a time, with the tests in between. Works in its own checkout
//! (`<repo>/.hubcodes/worktrees/_merge`) so nobody's working tree moves.
//! Conflicts and failing tests become a task for the branch's owner.
//! Queue rows live in SQLite (`merges`); `tend` processes the next queued one.

use crate::db::{now, Db};
use crate::git;
use crate::repo;
use crate::tasks;
use crate::testrun;
use crate::tmux::Tmux;
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Merge {
    pub id: i64,
    pub at: i64,
    pub updated: i64,
    pub repo: String,
    pub branch: String,
    pub into: String,
    pub agent: String,
    /// queued | merging | merged | conflict | tests_failed | failed
    pub status: String,
    pub log: String,
}

fn ensure_table(db: &Db) -> Result<()> {
    db.conn().execute_batch(
        "CREATE TABLE IF NOT EXISTS merges (
           id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, updated INTEGER NOT NULL,
           repo TEXT NOT NULL, branch TEXT NOT NULL, into_branch TEXT NOT NULL, agent TEXT NOT NULL,
           status TEXT NOT NULL DEFAULT 'queued', log TEXT NOT NULL DEFAULT '');",
    )?;
    Ok(())
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Merge> {
    Ok(Merge { id: r.get(0)?, at: r.get(1)?, updated: r.get(2)?, repo: r.get(3)?, branch: r.get(4)?, into: r.get(5)?, agent: r.get(6)?, status: r.get(7)?, log: r.get(8)? })
}

const COLS: &str = "id, at, updated, repo, branch, into_branch, agent, status, log";

pub fn enqueue(db: &Db, repo: &Path, branch: &str, into: &str, agent: &str) -> Result<Merge> {
    ensure_table(db)?;
    let root = git::main_root(repo).ok_or_else(|| anyhow!("{} is not a git repository", repo.display()))?;
    if branch == into {
        return Err(anyhow!("{branch} is the target branch"));
    }
    db.conn().execute(
        "INSERT INTO merges (at, updated, repo, branch, into_branch, agent) VALUES (?1, ?1, ?2, ?3, ?4, ?5)",
        params![now(), root.display().to_string(), branch, into, agent],
    )?;
    get(db, db.conn().last_insert_rowid())?.ok_or_else(|| anyhow!("merge vanished"))
}

pub fn get(db: &Db, id: i64) -> Result<Option<Merge>> {
    ensure_table(db)?;
    db.conn().query_row(&format!("SELECT {COLS} FROM merges WHERE id = ?1"), params![id], row).optional().map_err(Into::into)
}

pub fn list(db: &Db, limit: usize) -> Result<Vec<Merge>> {
    ensure_table(db)?;
    let mut st = db.conn().prepare(&format!("SELECT {COLS} FROM merges ORDER BY id DESC LIMIT ?1"))?;
    let rows = st.query_map(params![limit as i64], row)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn set(db: &Db, id: i64, status: &str, log: &str) -> Result<()> {
    db.conn().execute("UPDATE merges SET status = ?2, log = ?3, updated = ?4 WHERE id = ?1", params![id, status, log.chars().rev().take(6000).collect::<String>().chars().rev().collect::<String>(), now()])?;
    Ok(())
}

fn g(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().context("git")?;
    if !out.status.success() {
        return Err(anyhow!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The merge checkout on `into`, created or refreshed.
fn merge_worktree(root: &Path, into: &str) -> Result<PathBuf> {
    let dir = root.join(".hubcodes").join("worktrees").join("_merge");
    if !dir.join(".git").exists() {
        std::fs::create_dir_all(dir.parent().unwrap())?;
        // `into` may be checked out in the main worktree; a detached checkout of it avoids the lock.
        g(root, &["worktree", "add", "--detach", &dir.display().to_string(), into])?;
    }
    let _ = g(&dir, &["merge", "--abort"]);
    g(&dir, &["checkout", "-q", "--detach", into])?;
    let _ = g(&dir, &["reset", "-q", "--hard"]);
    Ok(dir)
}

/// Process one queued merge end to end. Returns the final row.
pub fn process(t: &Tmux, db: &Db, id: i64) -> Result<Merge> {
    let m = get(db, id)?.ok_or_else(|| anyhow!("no merge #{id}"))?;
    if m.status != "queued" {
        return Ok(m);
    }
    set(db, id, "merging", "")?;
    let root = PathBuf::from(&m.repo);
    let rc = repo::load_at(&root);
    let mut log = String::new();
    let result: Result<String> = (|| {
        let wt = merge_worktree(&root, &m.into)?;
        log.push_str(&format!("checkout {} at {}\n", m.into, g(&wt, &["rev-parse", "--short", "HEAD"])?));
        match g(&wt, &["merge", "--no-ff", "--no-edit", &m.branch]) {
            Ok(o) => log.push_str(&format!("{o}\n")),
            Err(e) => {
                let conflicts = g(&wt, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
                let _ = g(&wt, &["merge", "--abort"]);
                return Err(anyhow!("CONFLICT {}\n{}", e, conflicts));
            }
        }
        if rc.merge.test {
            if let Some(cmd) = testrun::command_for(&wt, None) {
                let r = testrun::run(&wt, &cmd, std::time::Duration::from_secs(900))?;
                log.push_str(&format!("tests: {} ({})\n{}\n", r.summary, cmd, r.tail));
                if !r.ok {
                    let _ = g(&wt, &["reset", "-q", "--hard", &m.into]);
                    return Err(anyhow!("TESTS_FAILED {}", r.summary));
                }
            } else {
                log.push_str("tests: no command found, skipped\n");
            }
        }
        // Fast-forward the real branch ref to the merge commit.
        let sha = g(&wt, &["rev-parse", "HEAD"])?;
        g(&root, &["update-ref", &format!("refs/heads/{}", m.into), &sha])?;
        log.push_str(&format!("{} → {}\n", m.into, &sha[..8]));
        // If the main worktree has `into` checked out, bring its working copy along (no local changes expected there).
        if g(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|b| b == m.into).unwrap_or(false) {
            let _ = g(&root, &["reset", "-q", "--hard", &sha]);
        }
        if rc.merge.push {
            match g(&root, &["push", "origin", &m.into]) {
                Ok(_) => log.push_str("pushed\n"),
                Err(e) => log.push_str(&format!("push failed: {e}\n")),
            }
        }
        Ok(sha[..8].to_string())
    })();
    match result {
        Ok(sha) => {
            set(db, id, "merged", &log)?;
            let _ = t.record_message_kind("hubcodes", &m.agent, &format!("merged {} into {} ({sha})", m.branch, m.into), "merge");
            let _ = db.add_event(&m.agent, "merge", &format!("merged {} into {} ({sha})", m.branch, m.into));
        }
        Err(e) => {
            let msg = format!("{e:#}");
            let status = if msg.starts_with("CONFLICT") { "conflict" } else if msg.starts_with("TESTS_FAILED") { "tests_failed" } else { "failed" };
            log.push_str(&msg);
            set(db, id, status, &log)?;
            let _ = db.add_event(&m.agent, "merge", &format!("{status}: {} into {}", m.branch, m.into));
            // Hand the problem back as a task for the branch owner.
            let title = match status {
                "conflict" => format!("Resolve conflicts merging {} into {}", m.branch, m.into),
                "tests_failed" => format!("Fix tests on {} before merging into {}", m.branch, m.into),
                _ => format!("Merge of {} into {} failed", m.branch, m.into),
            };
            let detail = msg.lines().take(12).collect::<Vec<_>>().join("\n");
            if let Ok(task) = tasks::add(db, &title, &format!("{detail}\n\nWhen fixed, run: hubcodes merge {}", m.branch), "", "hubcodes", &m.agent) {
                if let Ok(p) = t.pane_by_name(&m.agent) {
                    let text = tasks::assignment_text(&task, "hubcodes");
                    let payload = if p.provider.as_deref().unwrap_or("shell") == "shell" { text } else { format!("[from hubcodes] {text}") };
                    let _ = t.send_text(&p.id, &payload);
                }
            }
        }
    }
    get(db, id)?.ok_or_else(|| anyhow!("merge vanished"))
}

/// Called by tend: process the oldest queued merge, if none is merging.
pub fn tick(t: &Tmux, db: &Db) -> Result<Option<Merge>> {
    ensure_table(db)?;
    let merging: i64 = db.conn().query_row("SELECT count(*) FROM merges WHERE status = 'merging'", [], |r| r.get(0))?;
    if merging > 0 {
        return Ok(None);
    }
    let next: Option<i64> = db.conn().query_row("SELECT id FROM merges WHERE status = 'queued' ORDER BY id LIMIT 1", [], |r| r.get(0)).optional()?;
    match next {
        Some(id) => process(t, db, id).map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        assert!(Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success(), "git {args:?}");
    }

    #[test]
    fn merges_clean_branch_and_reports_conflict() {
        let base = std::env::temp_dir().join(format!("hubcodes-core-tests-{}", std::process::id()));
        std::env::set_var("XDG_STATE_HOME", &base);
        let dir = base.join("merge-repo");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "t@t"]);
        git(&dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        // Agent branch with a clean change.
        let wt = git::add_worktree(&dir, "api", "agent/api").unwrap();
        std::fs::write(wt.join("b.txt"), "two\n").unwrap();
        git(&wt, &["add", "."]);
        git(&wt, &["commit", "-q", "-m", "api work"]);
        let _ = std::fs::remove_file(crate::db::path_for("merge"));
        let db = Db::open("merge").unwrap();
        let t = Tmux::named("hubcodes-merge-test-no-session");
        let m = enqueue(&db, &dir, "agent/api", "main", "api").unwrap();
        let done = process(&t, &db, m.id).unwrap();
        assert_eq!(done.status, "merged", "{}", done.log);
        assert!(dir.join("b.txt").exists(), "main worktree brought along");
        // Conflicting branch.
        let wt2 = git::add_worktree(&dir, "web", "agent/web").unwrap();
        std::fs::write(wt2.join("a.txt"), "web version\n").unwrap();
        git(&wt2, &["commit", "-qam", "web"]);
        std::fs::write(dir.join("a.txt"), "main version\n").unwrap();
        git(&dir, &["commit", "-qam", "main moves"]);
        let m2 = enqueue(&db, &dir, "agent/web", "main", "web").unwrap();
        let r2 = process(&t, &db, m2.id).unwrap();
        assert_eq!(r2.status, "conflict", "{}", r2.log);
        assert!(r2.log.contains("a.txt"));
        let open = tasks::list(&db, false).unwrap();
        assert!(open.iter().any(|x| x.title.starts_with("Resolve conflicts") && x.owner == "web"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
