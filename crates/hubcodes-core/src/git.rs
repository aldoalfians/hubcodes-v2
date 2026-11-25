//! Git around an agent: its own worktree and branch, and what it changed.
//! Every call shells out to `git`, so nothing here assumes libgit2.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().context("run git")?;
    if !out.status.success() {
        return Err(anyhow!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Top of the checkout `dir` is in (a worktree's own root), if any.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    git(dir, &["rev-parse", "--show-toplevel"]).ok().map(|s| PathBuf::from(s.trim()))
}

/// The main checkout's root, even when `dir` is inside one of its worktrees
/// (where hubcodes keeps `.hubcodes/worktrees/`).
pub fn main_root(dir: &Path) -> Option<PathBuf> {
    let common = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok().map(|s| PathBuf::from(s.trim()))?;
    if common.file_name().map(|f| f == ".git").unwrap_or(false) {
        common.parent().map(|p| p.to_path_buf())
    } else {
        repo_root(dir)
    }
}

/// Where hubcodes keeps worktrees for a repo: `<repo>/.hubcodes/worktrees/<agent>`.
/// Git ignores it through `.git/info/exclude`, so the repo stays clean.
pub fn worktree_dir(root: &Path, agent: &str) -> PathBuf {
    root.join(".hubcodes").join("worktrees").join(agent)
}

fn exclude_hubcodes_dir(root: &Path) {
    let exclude = root.join(".git").join("info").join("exclude");
    if let Ok(text) = std::fs::read_to_string(&exclude) {
        if text.lines().any(|l| l.trim() == ".hubcodes/") {
            return;
        }
    }
    if let Some(p) = exclude.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(&exclude) {
        use std::io::Write;
        let _ = writeln!(f, ".hubcodes/");
    }
}

/// Create (or reuse) a worktree on `branch` for `agent`. The branch is
/// created from the current HEAD when it does not exist. Returns the path.
pub fn add_worktree(repo_dir: &Path, agent: &str, branch: &str) -> Result<PathBuf> {
    let root = main_root(repo_dir).ok_or_else(|| anyhow!("{} is not inside a git repository", repo_dir.display()))?;
    exclude_hubcodes_dir(&root);
    let dir = worktree_dir(&root, agent);
    if dir.join(".git").exists() {
        return Ok(dir);
    }
    if let Some(p) = dir.parent() {
        std::fs::create_dir_all(p)?;
    }
    let exists = git(&root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok();
    let dir_s = dir.display().to_string();
    if exists {
        git(&root, &["worktree", "add", &dir_s, branch])?;
    } else {
        git(&root, &["worktree", "add", "-b", branch, &dir_s])?;
    }
    Ok(dir)
}

/// Remove an agent's worktree (keeps the branch so work is never lost).
pub fn remove_worktree(dir: &Path) -> Result<()> {
    let root = main_root(dir).ok_or_else(|| anyhow!("not a worktree"))?;
    git(&root, &["worktree", "remove", "--force", &dir.display().to_string()])?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub branch: String,
    pub files: u32,
    pub insertions: u32,
    pub deletions: u32,
    /// `M src/x.rs`, `?? new.txt` …
    pub status: Vec<String>,
    pub ahead: u32,
    pub behind: u32,
}

/// Branch and diff summary for `dir` (working tree vs HEAD, untracked included).
pub fn changes(dir: &Path) -> Result<Changes> {
    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_default();
    let status: Vec<String> = git(dir, &["status", "--porcelain"])?.lines().map(|l| l.to_string()).collect();
    let shortstat = git(dir, &["diff", "--shortstat", "HEAD"]).unwrap_or_default();
    let num = |key: &str| -> u32 {
        shortstat
            .split(',')
            .find(|p| p.contains(key))
            .and_then(|p| p.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    };
    let (mut ahead, mut behind) = (0, 0);
    if let Ok(ab) = git(dir, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]) {
        let mut it = ab.split_whitespace();
        ahead = it.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        behind = it.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    }
    Ok(Changes {
        branch,
        files: status.len() as u32,
        insertions: num("insertion"),
        deletions: num("deletion"),
        status,
        ahead,
        behind,
    })
}

/// Unified diff of the working tree (plus untracked files as additions), capped.
pub fn diff(dir: &Path, max_bytes: usize) -> Result<String> {
    let mut out = git(dir, &["diff", "HEAD"]).unwrap_or_default();
    if let Ok(untracked) = git(dir, &["ls-files", "--others", "--exclude-standard"]) {
        for f in untracked.lines().filter(|l| !l.is_empty()) {
            if out.len() > max_bytes {
                break;
            }
            if let Ok(d) = git(dir, &["diff", "--no-index", "--", "/dev/null", f]) {
                out.push_str(&d);
            } else if let Ok(text) = std::fs::read_to_string(dir.join(f)) {
                out.push_str(&format!("diff --git a/{f} b/{f}\nnew file\n+++ b/{f}\n"));
                for l in text.lines().take(400) {
                    out.push('+');
                    out.push_str(l);
                    out.push('\n');
                }
            }
        }
    }
    if out.len() > max_bytes {
        out.truncate(max_bytes);
        out.push_str("\n… (truncated)\n");
    }
    Ok(out)
}

pub fn commit_all(dir: &Path, message: &str) -> Result<String> {
    git(dir, &["add", "-A"])?;
    git(dir, &["commit", "-m", message])?;
    git(dir, &["rev-parse", "--short", "HEAD"]).map(|s| s.trim().to_string())
}

/// `gh pr create --fill --web` after pushing the branch; returns gh's output.
pub fn open_pr(dir: &Path) -> Result<String> {
    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
    let _ = git(dir, &["push", "-u", "origin", &branch]);
    let out = Command::new("gh").arg("pr").args(["create", "--fill", "--web"]).current_dir(dir).output().context("run gh (GitHub CLI)")?;
    if !out.status.success() {
        return Err(anyhow!("gh pr create: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_and_changes_on_a_temp_repo() {
        let dir = std::env::temp_dir().join(format!("hubcodes-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]).unwrap();
        git(&dir, &["config", "user.email", "t@t"]).unwrap();
        git(&dir, &["config", "user.name", "t"]).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(&dir, &["add", "."]).unwrap();
        git(&dir, &["commit", "-q", "-m", "init"]).unwrap();
        let wt = add_worktree(&dir, "builder", "feat/x").unwrap();
        assert!(wt.ends_with(".hubcodes/worktrees/builder"));
        assert_eq!(main_root(&wt).unwrap(), dir.canonicalize().unwrap(), "main root from inside a worktree");
        let nested = add_worktree(&wt, "second", "feat/y").unwrap();
        assert!(nested.starts_with(dir.canonicalize().unwrap().join(".hubcodes")), "no nested worktrees: {}", nested.display());
        remove_worktree(&nested).unwrap();
        assert_eq!(add_worktree(&dir, "builder", "feat/x").unwrap(), wt, "reused");
        std::fs::write(wt.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(wt.join("new.txt"), "n\n").unwrap();
        let c = changes(&wt).unwrap();
        assert_eq!(c.branch, "feat/x");
        assert_eq!((c.files, c.insertions), (2, 1));
        assert!(diff(&wt, 10_000).unwrap().contains("+two"));
        let sha = commit_all(&wt, "work").unwrap();
        assert_eq!(sha.len(), 7);
        assert_eq!(changes(&wt).unwrap().files, 0);
        remove_worktree(&wt).unwrap();
        assert!(!wt.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
