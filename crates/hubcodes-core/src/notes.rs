//! Shared notes: one markdown file per session that every agent and the
//! human can read and append to (`hubcodes note show|add|clear`). Agents are
//! told about it in their role prompt; it replaces long messages back and
//! forth with a board everyone sees.

use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn path(session: &str) -> PathBuf {
    crate::db::state_dir().join(format!("{session}-notes.md"))
}

pub fn read(session: &str) -> String {
    std::fs::read_to_string(path(session)).unwrap_or_default()
}

/// Append a dated, attributed entry.
pub fn add(session: &str, by: &str, text: &str) -> Result<()> {
    let p = path(session);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let stamp = std::process::Command::new("date")
        .arg("+%Y-%m-%d %H:%M")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let mut existing = read(session);
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    let entry = format!("{existing}- **{by}** ({stamp}): {}\n", text.trim().replace('\n', "\n  "));
    std::fs::write(&p, entry).with_context(|| format!("write {}", p.display()))
}

/// Replace the whole file (the desktop notes card edits in place).
pub fn write(session: &str, text: &str) -> Result<()> {
    let p = path(session);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&p, text).with_context(|| format!("write {}", p.display()))
}

pub fn clear(session: &str) -> Result<()> {
    let p = path(session);
    if p.exists() {
        std::fs::remove_file(&p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_read() {
        let dir = std::env::temp_dir().join(format!("hubcodes-core-tests-{}", std::process::id()));
        std::env::set_var("XDG_STATE_HOME", &dir);
        let _ = clear("n");
        add("n", "planner", "token store lives in src/auth").unwrap();
        add("n", "human:me", "ship by friday").unwrap();
        let t = read("n");
        assert!(t.contains("- **planner**"));
        assert!(t.lines().count() == 2);
        clear("n").unwrap();
        assert_eq!(read("n"), "");
    }
}
