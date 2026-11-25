//! Run a repository's tests for an agent and keep the verdict where the
//! card, the merge queue and the transcript can see it:
//! pane option `@hubcodes_test` = `pass|fail <unix> <summary>`.

use crate::db::{now, Db};
use crate::repo;
use crate::tmux::Tmux;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const OPT_TEST: &str = "@hubcodes_test";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub command: String,
    pub at: i64,
    pub secs: u64,
    /// Last lines of output.
    pub tail: String,
    /// One line: `12 passed`, `3 failed`…
    pub summary: String,
}

/// Which command to run in `dir`: `[test] command` from the repo, else detected.
pub fn command_for(dir: &Path, override_cmd: Option<&str>) -> Option<String> {
    if let Some(c) = override_cmd.filter(|c| !c.trim().is_empty()) {
        return Some(c.to_string());
    }
    let cfg = repo::load(dir);
    if !cfg.test.command.is_empty() {
        return Some(cfg.test.command);
    }
    repo::detect_test_command(dir)
}

fn summarize(out: &str, ok: bool) -> String {
    let mut candidates: Vec<&str> = out
        .lines()
        .rev()
        .filter(|l| {
            let l = l.to_lowercase();
            l.contains("passed") || l.contains("failed") || l.contains("test result") || l.contains("tests") || l.contains("ok") && l.contains("test")
        })
        .take(1)
        .collect();
    if let Some(c) = candidates.pop() {
        return c.trim().chars().take(120).collect();
    }
    if ok { "passed".into() } else { "failed".into() }
}

/// Run the tests (max `limit`), capture output.
pub fn run(dir: &Path, command: &str, limit: Duration) -> Result<TestResult> {
    let t0 = Instant::now();
    let mut child = Command::new("/bin/sh").args(["-lc", command]).current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| anyhow!("run {command}: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut a = String::new();
        let mut b = String::new();
        if let Some(mut s) = stdout {
            let _ = s.read_to_string(&mut a);
        }
        if let Some(mut s) = stderr {
            let _ = s.read_to_string(&mut b);
        }
        format!("{a}\n{b}")
    });
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if t0.elapsed() > limit {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let out = reader.join().unwrap_or_default();
    let ok = status.map(|s| s.success()).unwrap_or(false);
    let tail: String = out.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    let summary = if status.is_none() { format!("timed out after {} s", limit.as_secs()) } else { summarize(&out, ok) };
    Ok(TestResult { ok, command: command.into(), at: now(), secs: t0.elapsed().as_secs(), tail: tail.chars().rev().take(4000).collect::<String>().chars().rev().collect(), summary })
}

/// Run for an agent and record: pane option, event, transcript line.
pub fn run_for_agent(t: &Tmux, agent: &str, dir: &Path, override_cmd: Option<&str>) -> Result<TestResult> {
    let cmd = command_for(dir, override_cmd).ok_or_else(|| anyhow!("no test command: add [test] command to {} or pass one", repo::FILE))?;
    let r = run(dir, &cmd, Duration::from_secs(900))?;
    if let Ok(p) = t.pane_by_name(agent) {
        let _ = t.set_pane_option(&p.id, OPT_TEST, &format!("{} {} {}", if r.ok { "pass" } else { "fail" }, r.at, r.summary.replace('\n', " ")));
    }
    if let Ok(db) = Db::open(&t.session) {
        let _ = db.add_event(agent, "test", &format!("{} · {} · {}", if r.ok { "pass" } else { "fail" }, cmd, r.summary));
    }
    let _ = t.record_message_kind("hubcodes", agent, &format!("tests {}: {} ({})", if r.ok { "passed" } else { "FAILED" }, r.summary, cmd), "test");
    Ok(r)
}

/// Parse `@hubcodes_test`.
pub fn parse_opt(v: &str) -> Option<(bool, i64, String)> {
    let mut it = v.splitn(3, ' ');
    let ok = it.next()? == "pass";
    let at = it.next()?.parse().ok()?;
    Some((ok, at, it.next().unwrap_or("").to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_summarizes() {
        let dir = std::env::temp_dir();
        let r = run(&dir, "echo '3 passed, 0 failed'; true", Duration::from_secs(10)).unwrap();
        assert!(r.ok);
        assert_eq!(r.summary, "3 passed, 0 failed");
        let f = run(&dir, "echo boom; exit 1", Duration::from_secs(10)).unwrap();
        assert!(!f.ok);
        assert_eq!(f.summary, "failed");
        let t = run(&dir, "sleep 5", Duration::from_millis(300)).unwrap();
        assert!(!t.ok && t.summary.contains("timed out"));
        assert_eq!(parse_opt("pass 1700000000 12 passed"), Some((true, 1700000000, "12 passed".into())));
    }
}
