//! Per-repository settings in `<repo>/.hubcodes/team.toml`: the team that
//! fits this codebase, how to test it, how to merge, where its docs are.
//! Loaded on top of the user's `hubcodes.toml` whenever an agent is started
//! in (or a command runs from) that repository, so `git clone` + `hubcodes
//! team start` gives the right team on any machine.
//!
//! ```toml
//! [[template]]                      # the repo's teams; the first is the default
//! name = "hermina"
//! agents = [{ name = "lead", provider = "claude", role = "orchestrator" }, { name = "api", provider = "claude", role = "software engineer", worktree = "agent/api" }]
//! links = [["lead", "api", "loop"]]
//!
//! [test]
//! command = "pnpm test"             # else detected from Cargo.toml / package.json / pyproject / go.mod
//!
//! [merge]
//! into = "main"
//! test = true                       # run [test] before fast-forwarding main
//! push = false                      # push main after a successful merge
//!
//! [board]
//! review_required = true            # an agent's `task done` lands in Review until a reviewer or you approve
//!
//! [[kb]]                            # docs that live in this repo
//! name = "docs"
//! kind = "folder"
//! path = "docs"
//! ```

use crate::git;
use crate::kb::KbSource;
use crate::team::Template;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TestConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeConfig {
    #[serde(default = "d_main")]
    pub into: String,
    #[serde(default = "yes")]
    pub test: bool,
    #[serde(default)]
    pub push: bool,
}

fn d_main() -> String {
    "main".into()
}
fn yes() -> bool {
    true
}

impl Default for MergeConfig {
    fn default() -> Self {
        MergeConfig { into: "main".into(), test: true, push: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BoardConfig {
    #[serde(default)]
    pub review_required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RepoConfig {
    #[serde(default, rename = "template", skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<Template>,
    #[serde(default)]
    pub test: TestConfig,
    #[serde(default)]
    pub merge: MergeConfig,
    #[serde(default)]
    pub board: BoardConfig,
    #[serde(default, rename = "kb", skip_serializing_if = "Vec::is_empty")]
    pub kbs: Vec<KbSource>,
    #[serde(default, rename = "playbook", skip_serializing_if = "Vec::is_empty")]
    pub playbooks: Vec<crate::playbook::Playbook>,
    #[serde(default, skip_serializing_if = "is_default_policy")]
    pub policy: crate::policy::PolicyConfig,
    /// Where it was read from (not serialized).
    #[serde(skip)]
    pub root: Option<PathBuf>,
}

fn is_default_policy(p: &crate::policy::PolicyConfig) -> bool {
    p == &crate::policy::PolicyConfig::default()
}

pub const FILE: &str = ".hubcodes/team.toml";

/// The repo config for a folder (walks up to the git root), default when none.
pub fn load(dir: &Path) -> RepoConfig {
    let Some(root) = git::main_root(dir) else { return RepoConfig::default() };
    load_at(&root)
}

pub fn load_at(root: &Path) -> RepoConfig {
    let path = root.join(FILE);
    let mut cfg: RepoConfig = std::fs::read_to_string(&path).ok().and_then(|t| toml::from_str(&t).map_err(|e| eprintln!("[hubcodes] {}: {e}", path.display())).ok()).unwrap_or_default();
    // Relative kb paths are relative to the repo.
    for k in &mut cfg.kbs {
        if k.kind == "folder" && !k.path.is_empty() && !Path::new(&k.path).is_absolute() {
            k.path = root.join(&k.path).display().to_string();
        }
    }
    cfg.root = Some(root.to_path_buf());
    cfg
}

pub fn save_at(root: &Path, cfg: &RepoConfig) -> Result<PathBuf> {
    let path = root.join(FILE);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let body = toml::to_string_pretty(cfg).context("serialize team.toml")?;
    std::fs::write(&path, format!("# hubcodes team settings for this repository. Docs: docs/team-guide.md\n\n{body}"))?;
    Ok(path)
}

/// Detect how to run the tests when `[test] command` is empty.
pub fn detect_test_command(dir: &Path) -> Option<String> {
    if dir.join("Cargo.toml").exists() {
        return Some("cargo test".into());
    }
    if dir.join("package.json").exists() {
        let pm = if dir.join("pnpm-lock.yaml").exists() { "pnpm" } else if dir.join("yarn.lock").exists() { "yarn" } else if dir.join("bun.lockb").exists() || dir.join("bun.lock").exists() { "bun" } else { "npm" };
        let has_test = std::fs::read_to_string(dir.join("package.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v.get("scripts")?.get("test").cloned()).is_some();
        return Some(if has_test { format!("{pm} test") } else { format!("{pm} run build") });
    }
    if dir.join("pyproject.toml").exists() || dir.join("pytest.ini").exists() || dir.join("setup.py").exists() {
        return Some("pytest -q".into());
    }
    if dir.join("go.mod").exists() {
        return Some("go test ./...".into());
    }
    if dir.join("Makefile").exists() && std::fs::read_to_string(dir.join("Makefile")).map(|m| m.lines().any(|l| l.starts_with("test:"))).unwrap_or(false) {
        return Some("make test".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_save_detect() {
        let dir = std::env::temp_dir().join(format!("hubcodes-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::process::Command::new("git").args(["-C", dir.to_str().unwrap(), "init", "-q"]).status().unwrap();
        assert_eq!(load(&dir), RepoConfig { root: Some(dir.canonicalize().unwrap()), ..Default::default() });
        std::fs::write(dir.join("package.json"), r#"{"scripts":{"test":"vitest"}}"#).unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(detect_test_command(&dir).as_deref(), Some("pnpm test"));
        let cfg = RepoConfig { merge: MergeConfig { into: "develop".into(), test: true, push: false }, board: BoardConfig { review_required: true }, ..Default::default() };
        save_at(&dir, &cfg).unwrap();
        let back = load_at(&dir);
        assert_eq!(back.merge.into, "develop");
        assert!(back.board.review_required);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
