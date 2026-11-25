//! Files a person hands an agent with a message: a pasted screenshot, a
//! picked PDF, a log dropped from Finder. The agent CLIs read files by path
//! (Claude Code, Codex and opencode open images that way too), so a message
//! with attachments is the text plus the list of paths. A file pasted or
//! picked in the app is written here first; for an agent on another device
//! the file is copied there ([`crate::remote::push_file`]) and the message
//! carries the path on that device.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

/// Largest file the app takes (a screen recording is about this size).
pub const MAX_BYTES: usize = 100 * 1024 * 1024;

/// Where attachments live on this device.
pub fn dir() -> PathBuf {
    crate::db::state_dir().join("attachments")
}

/// A name safe on any filesystem and in a shell word: letters, digits,
/// `.`, `-`, `_`; the rest becomes `-`.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let s: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' }).collect();
    let s = s.trim_matches(['-', '.']).to_string();
    let s = if s.is_empty() { "file".to_string() } else { s };
    // Keep the extension when shortening.
    if s.len() <= 80 {
        return s;
    }
    match s.rsplit_once('.') {
        Some((stem, ext)) if ext.len() <= 8 => format!("{}.{ext}", &stem[..80 - ext.len() - 1]),
        _ => s[..80].to_string(),
    }
}

/// Write bytes the person pasted or picked; returns the file's path. Each
/// file gets its own time-stamped name, so two `image.png` pastes do not
/// overwrite each other.
pub fn save(name: &str, bytes: &[u8]) -> Result<PathBuf> {
    if bytes.is_empty() {
        return Err(anyhow!("{name} is empty"));
    }
    if bytes.len() > MAX_BYTES {
        return Err(anyhow!("{name} is {} MB; the limit is {} MB", bytes.len() / (1024 * 1024), MAX_BYTES / (1024 * 1024)));
    }
    let d = dir();
    std::fs::create_dir_all(&d).with_context(|| format!("create {}", d.display()))?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_millis()).unwrap_or(0);
    let path = d.join(format!("{stamp}-{}", safe_name(name)));
    std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

/// The message an agent gets: the text, then one path per line. Paths are
/// on the agent's own device.
pub fn with_files(text: &str, paths: &[String]) -> String {
    let text = text.trim();
    if paths.is_empty() {
        return text.to_string();
    }
    let list = paths.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n");
    let head = if paths.len() == 1 { "Attached file (open it with your file tools)" } else { "Attached files (open them with your file tools)" };
    if text.is_empty() {
        format!("{head}:\n{list}")
    } else {
        format!("{text}\n\n{head}:\n{list}")
    }
}

/// A local file that can be sent: exists, is a file, not over the limit.
pub fn check(path: &Path) -> Result<()> {
    let m = std::fs::metadata(path).with_context(|| format!("{} is not there", path.display()))?;
    if !m.is_file() {
        return Err(anyhow!("{} is a folder; attach the files in it", path.display()));
    }
    if m.len() as usize > MAX_BYTES {
        return Err(anyhow!("{} is {} MB; the limit is {} MB", path.display(), m.len() / (1024 * 1024), MAX_BYTES / (1024 * 1024)));
    }
    Ok(())
}

/// The paths `to` can open: local files as they are (absolute), copied to
/// the device first for an agent there (`device/agent`).
pub fn for_agent(peers: &[crate::remote::Peer], to: &str, files: &[PathBuf]) -> Result<Vec<String>> {
    let peer = to.split_once('/').and_then(|(device, _)| peers.iter().find(|p| p.name == device));
    files
        .iter()
        .map(|f| {
            let abs = if f.is_absolute() { f.clone() } else { std::env::current_dir()?.join(f) };
            match peer {
                Some(p) => crate::remote::push_file(p, &abs),
                None => check(&abs).map(|_| abs.display().to_string()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe_and_keep_their_extension() {
        assert_eq!(safe_name("Screen Shot 2026-11-17 at 10.02.png"), "Screen-Shot-2026-11-17-at-10.02.png");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("'; rm -rf ~'.txt"), "rm--rf---.txt");
        assert_eq!(safe_name(""), "file");
        let long = format!("{}.pdf", "a".repeat(200));
        let s = safe_name(&long);
        assert!(s.len() <= 80 && s.ends_with(".pdf"), "{s}");
    }

    #[test]
    fn the_message_lists_the_paths_after_the_text() {
        assert_eq!(with_files(" fix this ", &[]), "fix this");
        let m = with_files("what is wrong here?", &["/a/shot.png".into()]);
        assert_eq!(m, "what is wrong here?\n\nAttached file (open it with your file tools):\n- /a/shot.png");
        assert!(with_files("", &["/a".into(), "/b".into()]).starts_with("Attached files"));
    }

    #[test]
    fn saved_files_do_not_overwrite_each_other_and_empty_is_refused() {
        let a = save("image.png", b"one").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        let b = save("image.png", b"two").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read(&a).unwrap(), b"one");
        assert!(save("x.png", b"").is_err());
        assert!(check(&a).is_ok());
        assert!(check(a.parent().unwrap()).is_err());
        let _ = std::fs::remove_file(a);
        let _ = std::fs::remove_file(b);
    }
}
