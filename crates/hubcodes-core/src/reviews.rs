//! One checkout per pull request under review, so reviewers running in
//! parallel never share a working tree (with each other or with anyone's
//! own work).
//!
//! Layout under `~/.local/state/hubcodes/reviews/<owner>__<repo>/`:
//! `repo/` is a partial clone (`gh repo clone … --filter=blob:none`, so
//! private repositories work with the `gh` sign-in), and `pr-<N>/` is a
//! detached worktree at the pull request's head. A re-request moves the
//! same worktree to the new head. The worktree goes when the review task is
//! finished and the pull request no longer asks for you.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

pub fn root() -> PathBuf {
    crate::db::state_dir().join("reviews")
}

/// `owner/repo#12` → (`owner/repo`, 12).
pub fn parse(key: &str) -> Result<(String, u64)> {
    let (repo, n) = key.trim().rsplit_once('#').ok_or_else(|| anyhow!("'{key}' is not owner/repo#N"))?;
    let n: u64 = n.parse().map_err(|_| anyhow!("'{key}': '{n}' is not a pull request number"))?;
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) && s != "." && s != "..";
    match repo.split_once('/') {
        Some((o, r)) if ok(o) && ok(r) => Ok((repo.to_string(), n)),
        _ => Err(anyhow!("'{key}' is not owner/repo#N")),
    }
}

fn base(repo: &str) -> PathBuf {
    root().join(repo.replace('/', "__"))
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().context("run git")?;
    if !out.status.success() {
        return Err(anyhow!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A lock directory per repository: two reviewers of the same repository
/// clone and fetch one after the other. A lock older than ten minutes is
/// taken to be left over from a crash.
struct Lock(PathBuf);

impl Lock {
    fn take(dir: &Path) -> Result<Lock> {
        std::fs::create_dir_all(dir)?;
        let p = dir.join(".lock");
        let start = Instant::now();
        loop {
            match std::fs::create_dir(&p) {
                Ok(()) => return Ok(Lock(p)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|age| age > Duration::from_secs(600));
                    if stale {
                        let _ = std::fs::remove_dir(&p);
                        continue;
                    }
                    if start.elapsed() > Duration::from_secs(300) {
                        return Err(anyhow!("{} is busy (another review is cloning or fetching it)", dir.display()));
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn ensure_clone(repo: &str, dir: &Path) -> Result<PathBuf> {
    let clone = dir.join("repo");
    if !clone.join(".git").exists() {
        let _ = std::fs::remove_dir_all(&clone);
        let out = Command::new("gh")
            .args(["repo", "clone", repo])
            .arg(&clone)
            .args(["--", "--filter=blob:none", "--quiet"])
            .output()
            .map_err(|e| anyhow!("gh: {e} (is GitHub CLI installed?)"))?;
        if !out.status.success() {
            return Err(anyhow!("gh repo clone {repo}: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
    }
    Ok(clone)
}

/// Check the pull request out in its own folder (detached at its head, for
/// reading) and return it.
pub fn checkout(key: &str) -> Result<PathBuf> {
    let (repo, n) = parse(key)?;
    let dir = base(&repo);
    let _lock = Lock::take(&dir)?;
    let clone = ensure_clone(&repo, &dir)?;
    prepare(&clone, &dir.join(format!("pr-{n}")), n)
}

/// Your own pull request on its branch, in a folder of its own (`fix-<N>`),
/// for pushing fixes. Returns the folder and a note when local work was kept.
pub fn checkout_branch(key: &str) -> Result<(PathBuf, Option<String>)> {
    let (repo, n) = parse(key)?;
    let ns = n.to_string();
    let head = crate::tracker::gh(&["pr", "view", &ns, "--repo", &repo, "--json", "headRefName,isCrossRepository", "--jq", "[.headRefName, .isCrossRepository] | @tsv"])?;
    let (branch, cross) = head.trim().split_once('\t').ok_or_else(|| anyhow!("{key}: no head branch"))?;
    if cross == "true" {
        return Err(anyhow!("{key} comes from a fork; push its fixes from the fork"));
    }
    let dir = base(&repo);
    let _lock = Lock::take(&dir)?;
    let clone = ensure_clone(&repo, &dir)?;
    prepare_branch(&clone, &dir.join(format!("fix-{n}")), branch)
}

/// Put `wt` on `branch` as it is on the remote. Local commits that were not
/// pushed, or uncommitted changes, are kept and reported instead of reset.
fn prepare_branch(clone: &Path, wt: &Path, branch: &str) -> Result<(PathBuf, Option<String>)> {
    if branch.is_empty() || branch.starts_with('-') || branch.contains("..") {
        return Err(anyhow!("'{branch}' is not a branch name"));
    }
    let remote = format!("refs/remotes/origin/{branch}");
    git(clone, &["fetch", "--quiet", "--force", "origin", &format!("+refs/heads/{branch}:{remote}")])?;
    if wt.join(".git").exists() {
        let dirty = !git(wt, &["status", "--porcelain"])?.is_empty();
        let ahead = git(wt, &["rev-list", "--count", &format!("{remote}..HEAD")])?.trim() != "0";
        if dirty || ahead {
            return Ok((wt.to_path_buf(), Some(format!("kept local work in {} (uncommitted changes or commits not pushed yet); push or reset it before the next round", wt.display()))));
        }
        git(wt, &["checkout", "--quiet", "-B", branch, &remote])?;
    } else {
        let _ = git(clone, &["worktree", "prune"]);
        let wt_s = wt.to_string_lossy().to_string();
        git(clone, &["worktree", "add", "--quiet", "-B", branch, &wt_s, &remote])?;
    }
    git(wt, &["branch", "--quiet", "--set-upstream-to", &format!("origin/{branch}")])?;
    Ok((wt.to_path_buf(), None))
}

/// Remove your pull request's `fix-<N>` folder (after it is merged or closed).
pub fn cleanup_branch(key: &str) -> Result<bool> {
    let (repo, n) = parse(key)?;
    let dir = base(&repo);
    let wt = dir.join(format!("fix-{n}"));
    if !wt.exists() {
        return Ok(false);
    }
    let _lock = Lock::take(&dir)?;
    let clone = dir.join("repo");
    let wt_s = wt.to_string_lossy().to_string();
    let branch = git(&wt, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let _ = git(&clone, &["worktree", "remove", "--force", &wt_s]);
    let _ = std::fs::remove_dir_all(&wt);
    let _ = git(&clone, &["worktree", "prune"]);
    if !branch.is_empty() && branch != "HEAD" {
        let _ = git(&clone, &["branch", "-q", "-D", &branch]);
    }
    Ok(true)
}

/// Fetch the pull request's head into `clone` and put `wt` on it.
fn prepare(clone: &Path, wt: &Path, n: u64) -> Result<PathBuf> {
    let r = format!("refs/hubcodes/pr-{n}");
    git(clone, &["fetch", "--quiet", "--force", "origin", &format!("+refs/pull/{n}/head:{r}")])?;
    let head = git(clone, &["rev-parse", &r])?;
    if wt.join(".git").exists() {
        git(wt, &["checkout", "--quiet", "--force", "--detach", &head])?;
        git(wt, &["clean", "-fdq"])?;
    } else {
        let _ = git(clone, &["worktree", "prune"]);
        let wt_s = wt.to_string_lossy().to_string();
        git(clone, &["worktree", "add", "--quiet", "--force", "--detach", &wt_s, &head])?;
    }
    Ok(wt.to_path_buf())
}

/// Remove the pull request's folder; the clone stays for the next review.
pub fn cleanup(key: &str) -> Result<bool> {
    let (repo, n) = parse(key)?;
    let dir = base(&repo);
    let wt = dir.join(format!("pr-{n}"));
    if !wt.exists() {
        return Ok(false);
    }
    let _lock = Lock::take(&dir)?;
    remove(&dir.join("repo"), &wt, n);
    Ok(true)
}

fn remove(clone: &Path, wt: &Path, n: u64) {
    let wt_s = wt.to_string_lossy().to_string();
    let _ = git(clone, &["worktree", "remove", "--force", &wt_s]);
    let _ = std::fs::remove_dir_all(wt);
    let _ = git(clone, &["worktree", "prune"]);
    let _ = git(clone, &["update-ref", "-d", &format!("refs/hubcodes/pr-{n}")]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_checked_before_they_become_paths() {
        assert_eq!(parse("o/a#12").unwrap(), ("o/a".into(), 12));
        assert_eq!(parse("Hub-Studio/raciq.be#7").unwrap(), ("Hub-Studio/raciq.be".into(), 7));
        for bad in ["o/a", "o#1", "../x#1", "o/..#1", "o/a/b#1", "o/a#x", "o/a b#1"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert_eq!(base("o/a").file_name().unwrap(), "o__a");
    }

    /// Two pull requests get two folders at their own heads; a new push
    /// moves the folder; cleanup removes it and leaves the clone.
    #[test]
    fn each_pull_request_gets_its_own_worktree() {
        let tmp = std::env::temp_dir().join(format!("hubcodes-reviews-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let origin = tmp.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let g = |d: &Path, a: &[&str]| git(d, a).unwrap();
        g(&origin, &["init", "-q", "-b", "main"]);
        g(&origin, &["config", "user.email", "t@t"]);
        g(&origin, &["config", "user.name", "t"]);
        let commit = |file: &str| {
            std::fs::write(origin.join(file), file).unwrap();
            g(&origin, &["add", "."]);
            g(&origin, &["commit", "-qm", file]);
            g(&origin, &["rev-parse", "HEAD"])
        };
        commit("base");
        let one = commit("one");
        g(&origin, &["update-ref", "refs/pull/1/head", &one]);
        g(&origin, &["reset", "-q", "--hard", "HEAD~1"]);
        let two = commit("two");
        g(&origin, &["update-ref", "refs/pull/2/head", &two]);

        let clone = tmp.join("o__a").join("repo");
        let out = Command::new("git").args(["clone", "-q"]).arg(&origin).arg(&clone).output().unwrap();
        assert!(out.status.success());
        let w1 = prepare(&clone, &tmp.join("o__a/pr-1"), 1).unwrap();
        let w2 = prepare(&clone, &tmp.join("o__a/pr-2"), 2).unwrap();
        assert!(w1.join("one").exists() && !w1.join("two").exists());
        assert!(w2.join("two").exists() && !w2.join("one").exists());

        // The author pushes again: the same folder moves to the new head, stray files go.
        std::fs::write(w1.join("scratch.txt"), "x").unwrap();
        g(&origin, &["checkout", "-q", &one]);
        let again = commit("one-more");
        g(&origin, &["update-ref", "refs/pull/1/head", &again]);
        prepare(&clone, &w1, 1).unwrap();
        assert!(w1.join("one-more").exists() && !w1.join("scratch.txt").exists());

        remove(&clone, &w1, 1);
        assert!(!w1.exists() && w2.exists() && clone.join(".git").exists());
        assert!(git(&clone, &["rev-parse", "--verify", "-q", "refs/hubcodes/pr-1"]).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn your_pull_request_gets_its_branch_and_local_work_is_kept() {
        let tmp = std::env::temp_dir().join(format!("hubcodes-fix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let origin = tmp.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let g = |d: &Path, a: &[&str]| git(d, a).unwrap();
        g(&origin, &["init", "-q", "-b", "main"]);
        for (k, v) in [("user.email", "t@t"), ("user.name", "t")] {
            g(&origin, &["config", k, v]);
        }
        let commit = |d: &Path, file: &str| {
            std::fs::write(d.join(file), file).unwrap();
            g(d, &["add", "."]);
            g(d, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", file]);
        };
        commit(&origin, "base");
        g(&origin, &["checkout", "-q", "-b", "feature/x"]);
        commit(&origin, "f1");
        g(&origin, &["checkout", "-q", "main"]);
        let clone = tmp.join("o__a/repo");
        assert!(Command::new("git").args(["clone", "-q"]).arg(&origin).arg(&clone).status().unwrap().success());
        let wt = tmp.join("o__a/fix-7");
        let (dir, note) = prepare_branch(&clone, &wt, "feature/x").unwrap();
        assert!(note.is_none() && dir.join("f1").exists());
        assert_eq!(g(&wt, &["rev-parse", "--abbrev-ref", "HEAD"]), "feature/x");
        assert_eq!(g(&wt, &["rev-parse", "--abbrev-ref", "@{u}"]), "origin/feature/x");

        // Someone pushes to the branch: the next round starts from it.
        g(&origin, &["checkout", "-q", "feature/x"]);
        commit(&origin, "f2");
        g(&origin, &["checkout", "-q", "main"]);
        assert!(prepare_branch(&clone, &wt, "feature/x").unwrap().1.is_none());
        assert!(wt.join("f2").exists());

        // A commit that was not pushed is kept, not reset.
        commit(&wt, "local");
        let (_, note) = prepare_branch(&clone, &wt, "feature/x").unwrap();
        assert!(note.unwrap().contains("kept local work"));
        assert!(wt.join("local").exists());
        assert!(prepare_branch(&clone, &wt, "--upload-pack=x").is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
