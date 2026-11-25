//! One subscription per panel. Three Claude agents on three accounts must
//! not share one login, so each agent can carry an *account*: a name whose
//! credentials and settings live in a folder of their own under
//! `~/.local/state/hubcodes/accounts/<provider>/<account>/`, handed to the
//! tool through the environment variable it already honours:
//!
//! | provider | variable                         | what moves                      |
//! |----------|----------------------------------|---------------------------------|
//! | claude   | `CLAUDE_CONFIG_DIR`              | `.claude.json`, settings, login |
//! | codex    | `CODEX_HOME`                     | `auth.json`, `config.toml`      |
//! | gemini   | `GEMINI_CLI_HOME`                | `.gemini/` (oauth, settings)    |
//! | opencode | `XDG_DATA_HOME`                  | `opencode/auth.json`            |
//! | pi       | `PI_CODING_AGENT_DIR`            | the agent dir (auth, settings)  |
//! | omp      | `OMP_PROFILE`                    | `~/.omp/profiles/<account>/`    |
//!
//! `default` (or no account) is the tool's own home: nothing changes. The
//! first start under a new account asks for a login; after that it sticks.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

pub const DEFAULT: &str = "default";

/// Where accounts live for one provider.
pub fn base(provider: &str) -> PathBuf {
    crate::db::state_dir().join("accounts").join(provider)
}

/// The account's own folder (`None` for the default account).
pub fn dir(provider: &str, account: &str) -> Option<PathBuf> {
    let a = account.trim();
    (!a.is_empty() && a != DEFAULT).then(|| base(provider).join(a))
}

/// A valid account name: one word, letters, digits, `-`, `_`, `.`.
pub fn valid(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.len() <= 40 && n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) && !n.starts_with('.')
}

/// Accounts known for a provider: `default` first, then the folders.
pub fn list(provider: &str) -> Vec<String> {
    list_in(&base(provider))
}

pub fn list_in(base: &Path) -> Vec<String> {
    let mut out = vec![DEFAULT.to_string()];
    if let Ok(rd) = std::fs::read_dir(base) {
        let mut names: Vec<String> = rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| valid(n)).collect();
        names.sort();
        out.extend(names);
    }
    out
}

/// Create the folder for a new account; returns it.
pub fn add(provider: &str, account: &str) -> Result<PathBuf> {
    add_in(&base(provider), account)
}

pub fn add_in(base: &Path, account: &str) -> Result<PathBuf> {
    let a = account.trim();
    if !valid(a) {
        return Err(anyhow!("an account name is one word of letters, digits, - _ . (got “{a}”)"));
    }
    if a == DEFAULT {
        return Err(anyhow!("“default” is the tool's own login; it is always there"));
    }
    let p = base.join(a);
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

/// Delete an account's folder with its credentials.
pub fn remove(provider: &str, account: &str) -> Result<()> {
    remove_in(&base(provider), account)
}

pub fn remove_in(base: &Path, account: &str) -> Result<()> {
    let a = account.trim();
    if a == DEFAULT || a.is_empty() {
        return Err(anyhow!("the default account cannot be removed"));
    }
    if !valid(a) {
        return Err(anyhow!("no such account “{a}”"));
    }
    let p = base.join(a);
    if !p.is_dir() {
        return Err(anyhow!("no account “{a}” under {}", base.display()));
    }
    std::fs::remove_dir_all(&p)?;
    Ok(())
}

/// Environment that points the tool at the account's folder. Empty for the
/// default account and for providers without an isolation variable.
pub fn env(provider: &str, hook_kind: Option<&str>, account: &str) -> Vec<(String, String)> {
    let Some(d) = dir(provider, account) else { return Vec::new() };
    let kind = hook_kind.unwrap_or(provider);
    let p = d.display().to_string();
    match kind {
        "claude" => vec![("CLAUDE_CONFIG_DIR".into(), p)],
        "codex" => vec![("CODEX_HOME".into(), p)],
        "gemini" => vec![("GEMINI_CLI_HOME".into(), p)],
        "opencode" => vec![("XDG_DATA_HOME".into(), d.join("data").display().to_string())],
        "pi" => vec![("PI_CODING_AGENT_DIR".into(), d.join("agent").display().to_string())],
        "omp" => vec![("OMP_PROFILE".into(), account.trim().to_string())],
        _ => Vec::new(),
    }
}

/// The env as a `K='v' ` prefix for a launch command.
pub fn env_prefix(provider: &str, hook_kind: Option<&str>, account: &str) -> String {
    env(provider, hook_kind, account).into_iter().map(|(k, v)| format!("{k}='{}' ", v.replace('\'', "'\\''"))).collect()
}

/// Where the tool keeps its files under this account (what the status reader
/// and the shim writers look at). `None` = the tool's own home layout.
#[derive(Clone, Debug, Default)]
pub struct Paths {
    pub claude_dir: Option<PathBuf>,
    pub codex_dir: Option<PathBuf>,
    pub gemini_dir: Option<PathBuf>,
    pub opencode_data: Option<PathBuf>,
    pub pi_agent_dir: Option<PathBuf>,
    pub omp_profile: Option<String>,
}

pub fn paths(provider: &str, hook_kind: Option<&str>, account: &str) -> Paths {
    let mut p = Paths::default();
    let Some(d) = dir(provider, account) else { return p };
    match hook_kind.unwrap_or(provider) {
        "claude" => p.claude_dir = Some(d),
        "codex" => p.codex_dir = Some(d),
        "gemini" => p.gemini_dir = Some(d.join(".gemini")),
        "opencode" => p.opencode_data = Some(d.join("data")),
        "pi" => p.pi_agent_dir = Some(d.join("agent")),
        "omp" => p.omp_profile = Some(account.trim().to_string()),
        _ => {}
    }
    p
}

/// Create what the tool expects under the account before its first start
/// (so a login lands inside, not in the tool's default home).
pub fn prepare(provider: &str, hook_kind: Option<&str>, account: &str) -> Result<()> {
    let Some(d) = dir(provider, account) else { return Ok(()) };
    std::fs::create_dir_all(&d)?;
    match hook_kind.unwrap_or(provider) {
        "gemini" => std::fs::create_dir_all(d.join(".gemini"))?,
        "opencode" => std::fs::create_dir_all(d.join("data").join("opencode"))?,
        "pi" => std::fs::create_dir_all(d.join("agent"))?,
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_are_folders_and_env_points_at_them() {
        let base = std::env::temp_dir().join(format!("hubacct-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!(list_in(&base), vec!["default".to_string()]);
        add_in(&base, "work").unwrap();
        add_in(&base, "personal").unwrap();
        assert!(add_in(&base, "bad name").is_err());
        assert!(add_in(&base, "default").is_err());
        assert_eq!(list_in(&base), ["default", "personal", "work"].map(String::from).to_vec());
        remove_in(&base, "personal").unwrap();
        assert!(remove_in(&base, "default").is_err());
        assert_eq!(list_in(&base).len(), 2);
        let _ = std::fs::remove_dir_all(&base);

        assert!(env("claude", None, "default").is_empty());
        assert!(env("claude", None, "").is_empty());
        let e = env("claude", None, "work");
        assert_eq!(e[0].0, "CLAUDE_CONFIG_DIR");
        assert!(e[0].1.ends_with("/accounts/claude/work"));
        assert_eq!(env("omp", None, "work"), vec![("OMP_PROFILE".to_string(), "work".to_string())]);
        assert_eq!(env("mytool", Some("codex"), "x")[0].0, "CODEX_HOME");
        assert!(env("shell", None, "work").is_empty());
        assert!(env_prefix("codex", None, "work").starts_with("CODEX_HOME='"));
        assert!(paths("gemini", None, "work").gemini_dir.unwrap().ends_with("work/.gemini"));
    }
}
