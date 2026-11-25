//! Confine an agent's writes to its own folder. On macOS this uses
//! `sandbox-exec` with a generated profile: read anywhere, write only to the
//! agent's folder, its provider's config and cache dirs, and temp; network
//! allowed (the agent needs its API). With `[sandbox] command` any wrapper
//! (Docker, OrbStack, firejail) can be used instead: `{cmd}` and `{cwd}` are
//! filled.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SandboxConfig {
    /// `macos` (sandbox-exec) | `command` | `` (off)
    #[serde(default)]
    pub kind: String,
    /// For `command`: a wrapper with `{cmd}` and `{cwd}`; e.g.
    /// `docker run --rm -it -v {cwd}:{cwd} -w {cwd} my/agents sh -lc {cmd}`
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// Extra folders the agent may write (provider caches, package caches…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable: Vec<String>,
}

fn esc(p: &str) -> String {
    p.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The sandbox-exec profile for a folder.
pub fn macos_profile(cwd: &Path, extra: &[String]) -> String {
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
    let mut writable: Vec<String> = vec![
        cwd.display().to_string(),
        "/private/tmp".into(),
        "/private/var/folders".into(),
        "/dev".into(),
        format!("{home}/.claude"),
        format!("{home}/.claude.json"),
        format!("{home}/.codex"),
        format!("{home}/.config/opencode"),
        format!("{home}/.local/share/opencode"),
        format!("{home}/.gemini"),
        format!("{home}/.pi"),
        format!("{home}/.omp"),
        format!("{home}/.cache"),
        format!("{home}/.npm"),
        format!("{home}/.cargo/registry"),
        format!("{home}/.local/state/hubcodes"),
        format!("{home}/Library/Caches"),
        format!("{home}/Library/Keychains"),
    ];
    // Git worktrees keep their metadata in the main repo's .git.
    if let Some(root) = crate::git::repo_root(cwd) {
        writable.push(root.join(".git").display().to_string());
        writable.push(root.join(".hubcodes").display().to_string());
    }
    writable.extend(extra.iter().cloned());
    let rules = writable.iter().filter(|p| !p.is_empty()).map(|p| format!("  (subpath \"{}\")", esc(p))).collect::<Vec<_>>().join("\n");
    format!(
        "(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write*\n{rules}\n  (literal \"/dev/null\") (literal \"/dev/tty\") (regex #\"^/dev/ttys[0-9]+\") (regex #\"^/dev/fd/\"))\n(allow file-write-data (regex #\"^/dev/\"))\n"
    )
}

/// Wrap `cmd` so it runs confined to `cwd`. Returns `cmd` unchanged when off.
pub fn wrap(cfg: &SandboxConfig, cmd: &str, cwd: &Path) -> String {
    match cfg.kind.as_str() {
        "macos" => {
            let profile = macos_profile(cwd, &cfg.writable);
            let p = crate::db::state_dir().join("sandbox").join(format!("{}.sb", blake(&cwd.display().to_string())));
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&p, profile);
            format!("sandbox-exec -f '{}' /bin/sh -lc '{}'", p.display(), cmd.replace('\'', "'\\''"))
        }
        "command" if !cfg.command.trim().is_empty() => cfg.command.replace("{cwd}", &format!("'{}'", cwd.display())).replace("{cmd}", &format!("'{}'", cmd.replace('\'', "'\\''"))),
        _ => cmd.to_string(),
    }
}

fn blake(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_and_wrap() {
        let dir = std::env::temp_dir().join("hubcodes-sb");
        let prof = macos_profile(&dir, &["/opt/x".into()]);
        assert!(prof.contains("(deny file-write*)"));
        assert!(prof.contains("hubcodes-sb"));
        assert!(prof.contains("/opt/x"));
        let cfg = SandboxConfig { kind: "command".into(), command: "docker run -v {cwd}:{cwd} img sh -lc {cmd}".into(), writable: vec![] };
        let w = wrap(&cfg, "claude --model opus", &dir);
        assert!(w.starts_with("docker run -v '"));
        assert!(w.ends_with("sh -lc 'claude --model opus'"));
        assert_eq!(wrap(&SandboxConfig::default(), "x", &dir), "x");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn sandbox_exec_blocks_writes_outside() {
        let base = std::env::temp_dir().join(format!("hubcodes-core-tests-{}", std::process::id()));
        std::env::set_var("XDG_STATE_HOME", &base);
        let dir = base.join("sb-work");
        std::fs::create_dir_all(&dir).unwrap();
        let outside = std::env::temp_dir().join(format!("hubcodes-sb-outside-{}", std::process::id()));
        let _ = std::fs::remove_file(&outside);
        let cfg = SandboxConfig { kind: "macos".into(), command: String::new(), writable: vec![] };
        // /private/tmp is writable in the profile; the home dir (except caches/configs) is not.
        let home_target = dirs::home_dir().unwrap().join(format!(".hubcodes-sb-test-{}", std::process::id()));
        let cmd = wrap(&cfg, &format!("touch '{}' ; echo ok > '{}'", home_target.display(), dir.join("inside.txt").display()), &dir);
        let st = std::process::Command::new("/bin/sh").args(["-c", &cmd]).output().unwrap();
        assert!(!home_target.exists(), "write to home must be denied: {}", String::from_utf8_lossy(&st.stderr));
        assert!(dir.join("inside.txt").exists(), "write inside the folder must work: {}", String::from_utf8_lossy(&st.stderr));
    }
}
