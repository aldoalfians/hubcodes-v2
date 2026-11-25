//! Switching an agent's provider or model in place. The agent keeps its
//! name, role, folder, links and board tasks. With the same provider the
//! conversation itself is continued (the provider's resume flag, new model).
//! A new provider cannot read the old one's conversation, so it starts with a
//! hand-over note in its brief: who it replaces, its open tasks, its recent
//! messages and the end of the old terminal. The full note is a file
//! (`~/.local/state/hubcodes/handoff/`, readable by this user only); the
//! brief carries only a short pointer to it, so the launch command stays
//! small and the terminal text never shows in `ps`.

use crate::db::{self, MessageRow};
use crate::tasks::Task;
use std::path::PathBuf;

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn dir() -> PathBuf {
    db::state_dir().join("handoff")
}

/// The full note of an agent (kept; the newest replaces the last).
pub fn path(session: &str, name: &str) -> PathBuf {
    dir().join(format!("{}-{}.md", safe(session), safe(name)))
}

/// The short brief pointer, read by [`crate::prompts::start_extra`] while the
/// new agent's launch command is built, then removed.
pub fn pending_path(session: &str, name: &str) -> PathBuf {
    dir().join(format!("{}-{}.pending", safe(session), safe(name)))
}

/// The brief pointer waiting for this agent's next start, if any.
pub fn pending(session: &str, name: &str) -> Option<String> {
    std::fs::read_to_string(pending_path(session, name)).ok().filter(|s| !s.trim().is_empty())
}

/// Write a file readable by this user only.
pub fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    use std::io::Write;
    o.open(path)?.write_all(text.as_bytes())
}

/// Write the full note and the brief pointer for `name`'s next start.
/// Notes older than a week are cleared on the way.
pub fn prepare(session: &str, name: &str, full: &str, brief: &str) -> std::io::Result<PathBuf> {
    if let Ok(rd) = std::fs::read_dir(dir()) {
        let week = std::time::Duration::from_secs(7 * 86400);
        for e in rd.flatten() {
            if e.metadata().and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).map(|a| a > week).unwrap_or(false) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let p = path(session, name);
    write_private(&p, full)?;
    write_private(&pending_path(session, name), brief)?;
    Ok(p)
}

/// What the brief says: who is taken over, the open tasks, and where the
/// full note is. Small on purpose.
pub fn brief(name: &str, role: Option<&str>, from: &str, to: &str, tasks: &[Task], file: &std::path::Path) -> String {
    let mut out = format!(
        "## Hand-over\nYou take over as {name}{} from {from}; you now run on {to}. The name, role, folder and links stay the same. Before anything else, read {} : it has your open tasks, the recent messages and the end of the previous agent's terminal. Carry on where it stopped; do not redo finished work.",
        role.map(|r| format!(" ({r})")).unwrap_or_default(),
        file.display()
    );
    let open: Vec<&Task> = tasks.iter().filter(|t| t.owner == name).take(10).collect();
    if !open.is_empty() {
        out.push_str("\nYour open board tasks:");
        for t in open {
            out.push_str(&format!("\n- #{} [{}] {}", t.id, t.status, t.title.chars().take(120).collect::<String>()));
        }
    }
    out
}

/// How much of the old terminal goes along.
const SCREEN_CHARS: usize = 6000;

/// The hand-over note. `screen` is the old terminal, oldest line first;
/// `messages` are newest first, as the database returns them.
pub fn note(name: &str, role: Option<&str>, from: &str, to: &str, tasks: &[Task], messages: &[MessageRow], screen: &[String]) -> String {
    let mut out = format!(
        "## Hand-over\nYou take over as {name}{} from {from}; you now run on {to}. The name, role, folder and links stay the same, so the team keeps talking to you as before. Read this, then carry on where the previous agent stopped. Do not redo finished work; if something below is unclear, ask whoever gave the task.",
        role.map(|r| format!(" ({r})")).unwrap_or_default()
    );
    let open: Vec<&Task> = tasks.iter().filter(|t| t.owner == name).collect();
    if !open.is_empty() {
        out.push_str("\n\nYour open board tasks:");
        for t in open {
            out.push_str(&format!("\n- #{} [{}] {}", t.id, t.status, t.title));
        }
    }
    let mine: Vec<&MessageRow> = messages.iter().filter(|m| m.from == name || m.to == name).take(12).collect();
    if !mine.is_empty() {
        out.push_str("\n\nRecent messages (oldest first):");
        for m in mine.iter().rev() {
            let text: String = m.text.split_whitespace().collect::<Vec<_>>().join(" ");
            let text: String = text.chars().take(400).collect();
            out.push_str(&format!("\n- {} → {}: {}", m.from, m.to, crate::redact::redact(&text)));
        }
    }
    let lines: Vec<String> = screen.iter().map(|l| crate::redact::redact(crate::policy::strip_ansi(l).trim_end())).collect();
    let mut tail = String::new();
    for l in lines.iter().rev() {
        if tail.len() + l.len() + 1 > SCREEN_CHARS {
            break;
        }
        tail = if tail.is_empty() { l.clone() } else { format!("{l}\n{tail}") };
    }
    let tail = tail.trim_matches('\n');
    if !tail.trim().is_empty() {
        out.push_str(&format!("\n\nThe end of the previous agent's terminal (raw; the latest work is at the bottom):\n```\n{tail}\n```"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: i64, owner: &str, title: &str) -> Task {
        serde_json::from_value(serde_json::json!({ "id": id, "at": 0, "updated": 0, "title": title, "detail": "", "status": "doing", "role": "", "owner": owner, "createdBy": "lead", "result": "" })).unwrap()
    }

    fn msg(seq: i64, from: &str, to: &str, text: &str) -> MessageRow {
        MessageRow { seq, at: 0, from: from.into(), to: to.into(), text: text.into(), kind: "message".into() }
    }

    #[test]
    fn the_note_carries_tasks_messages_and_the_screen_end() {
        let tasks = [task(3, "fe", "Fix the login form"), task(4, "be", "Not mine")];
        let messages = [msg(2, "fe", "lead", "done with step one"), msg(1, "lead", "fe", "please fix the login form"), msg(0, "lead", "be", "other")];
        let screen: Vec<String> = vec!["\x1b[1mold line\x1b[0m".into(), "last line".into()];
        let n = note("fe", Some("frontend"), "claude · opus", "codex", &tasks, &messages, &screen);
        assert!(n.contains("take over as fe (frontend) from claude · opus; you now run on codex"));
        assert!(n.contains("#3 [doing] Fix the login form") && !n.contains("Not mine"));
        let (a, b) = (n.find("please fix").unwrap(), n.find("done with step one").unwrap());
        assert!(a < b, "messages read oldest first");
        assert!(!n.contains("other"));
        assert!(n.contains("old line\nlast line") && !n.contains('\x1b'));
    }

    #[test]
    fn a_long_screen_keeps_its_end() {
        let screen: Vec<String> = (0..2000).map(|i| format!("line {i}")).collect();
        let n = note("fe", None, "claude", "codex", &[], &[], &screen);
        assert!(n.contains("line 1999") && !n.contains("line 10\n"));
        assert!(n.len() < SCREEN_CHARS + 1000);
    }
}
