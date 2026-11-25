//! Auto team: temporary agents for a role whose queue backs up. When tasks
//! (or review requests) wait for a role and none of its agents is free,
//! hubcodes starts one more, copied from an agent that already has the role
//! (provider, model, folder, account, links), named `<base>-N`; a software
//! engineer gets its own worktree on `agent/<name>`. The orchestrator can
//! ask for one too (`hubcodes hire`). Once the queue is empty and the extra
//! agent has been idle for a while it is removed; work it left in its
//! worktree stays on its branch and goes to another agent of the role as a
//! board task with a hand-over note. Agents a person started are never
//! removed.
//!
//! ```toml
//! [autoteam]
//! enabled = true
//! per_role = 2               # extra agents per role at most
//! roles = { reviewer = 3 }   # per-role override; orchestrators get 0 unless listed
//! max_agents = 8             # all agents here, extra or not
//! idle_minutes = 10          # remove an extra agent idle this long with nothing waiting
//! cooldown_minutes = 2       # between two hires for one role
//! worktree = true            # extra software engineers work on their own branch
//! ```

use crate::providers::Registry;
use crate::tasks::Task;
use crate::tmux::Tmux;
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AutoTeamConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "d_per_role")]
    pub per_role: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roles: BTreeMap<String, u32>,
    #[serde(default = "d_max")]
    pub max_agents: u32,
    #[serde(default = "d_idle")]
    pub idle_minutes: u32,
    #[serde(default = "d_cooldown")]
    pub cooldown_minutes: u32,
    #[serde(default = "d_true")]
    pub worktree: bool,
}

fn d_per_role() -> u32 {
    2
}
fn d_max() -> u32 {
    8
}
fn d_idle() -> u32 {
    10
}
fn d_cooldown() -> u32 {
    2
}
fn d_true() -> bool {
    true
}

impl Default for AutoTeamConfig {
    fn default() -> Self {
        Self { enabled: false, per_role: d_per_role(), roles: BTreeMap::new(), max_agents: d_max(), idle_minutes: d_idle(), cooldown_minutes: d_cooldown(), worktree: true }
    }
}

impl AutoTeamConfig {
    /// How many extra agents a role may get: its override, else 0 for an
    /// orchestrator (one team, one lead) and `per_role` for the rest.
    pub fn limit(&self, role: &str) -> u32 {
        if let Some((_, n)) = self.roles.iter().find(|(r, _)| r.trim().eq_ignore_ascii_case(role.trim())) {
            return *n;
        }
        if crate::prompts::kind(role) == "orchestrator" { 0 } else { self.per_role }
    }
}

/// An agent hubcodes added, as remembered per session.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Extra {
    pub role: String,
    /// The agent it was copied from.
    pub of: String,
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

fn path(session: &str) -> PathBuf {
    let safe: String = session.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    crate::db::state_dir().join("autoteam").join(format!("{safe}.json"))
}

/// The extra agents of a session, by name.
pub fn extras(session: &str) -> BTreeMap<String, Extra> {
    std::fs::read_to_string(path(session)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(session: &str, m: &BTreeMap<String, Extra>) -> Result<()> {
    let p = path(session);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(m)?)?;
    Ok(())
}

/// `dev` → `dev-2`; `Reviewer-3` → the first of `Reviewer-4`, `Reviewer-5`, … not taken.
pub fn next_name(base: &str, taken: &[String]) -> String {
    let stem = match base.rsplit_once('-') {
        Some((s, n)) if !s.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => s,
        _ => base,
    };
    (2..).map(|i| format!("{stem}-{i}")).find(|n| !taken.iter().any(|t| t.eq_ignore_ascii_case(n))).unwrap_or_else(|| format!("{stem}-x"))
}

/// Tasks waiting for `role` that no agent holds yet (held trackers excluded).
pub fn waiting(tasks: &[Task], role: &str, held: &[String]) -> usize {
    tasks.iter().filter(|t| t.status == "todo" && t.owner.is_empty() && t.role == role && !held.iter().any(|h| t.external.starts_with(h.as_str()))).count()
}

/// Commits on the worktree's branch that no other local branch has (so a
/// detached main checkout does not hide them). `None` when git cannot say,
/// which the caller treats as "has work".
fn unmerged_commits(dir: &Path) -> Option<u32> {
    let branch = std::process::Command::new("git").arg("-C").arg(dir).args(["symbolic-ref", "--short", "HEAD"]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(["rev-list", "--count", "HEAD", "--not", &format!("--exclude={branch}"), "--branches"]).output().ok().filter(|o| o.status.success())?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Start one more agent of `role`. `by` is who asked (`hubcodes` for the
/// queue rule, or the orchestrator's name).
pub fn hire(t: &Tmux, reg: &Registry, role: &str, reason: &str, by: &str, exe: &str) -> Result<String> {
    hire_with(t, reg, role, reason, by, exe, None)
}

/// [`hire`] with a folder put first on the agent's PATH (where the CLI is).
pub fn hire_with(t: &Tmux, reg: &Registry, role: &str, reason: &str, by: &str, exe: &str, path_prefix: Option<&str>) -> Result<String> {
    let cfg = &reg.autoteam;
    if !cfg.enabled {
        bail!("auto team is off: turn it on in Settings → Agents → Auto team (or [autoteam] enabled = true)");
    }
    let role = role.trim();
    if role.is_empty() {
        bail!("which role? hubcodes hire --role \"software engineer\"");
    }
    let panes = t.panes()?;
    let agents: Vec<&crate::tmux::Pane> = panes.iter().filter(|p| p.provider.as_deref().map(|x| x != "proxy" && x != "shell").unwrap_or(false) && !p.dead && !p.exited).collect();
    let mut extras = extras(&t.session);
    extras.retain(|n, _| agents.iter().any(|p| &p.name == n));
    let limit = cfg.limit(role);
    let mine = extras.values().filter(|e| e.role.eq_ignore_ascii_case(role)).count() as u32;
    if mine >= limit {
        bail!("{role} already has {mine} extra agent{} (the limit is {limit}); finish or reassign work instead", if mine == 1 { "" } else { "s" });
    }
    if cfg.max_agents > 0 && agents.len() as u32 >= cfg.max_agents {
        bail!("{} agents already run here (the limit is {}); no more are started", agents.len(), cfg.max_agents);
    }
    // Copy an agent a person started, else any agent of the role.
    let of = agents.iter().filter(|p| p.role.as_deref().map(|r| r.eq_ignore_ascii_case(role)).unwrap_or(false)).min_by_key(|p| extras.contains_key(&p.name)).ok_or_else(|| anyhow!("no {role} agent runs to copy from; start one first"))?;
    let cwd = crate::sessiondef::pane_cwd(&of.id).unwrap_or_default();
    let base_dir = of.worktree.as_deref().and_then(|w| crate::git::main_root(Path::new(w))).map(|p| p.display().to_string()).unwrap_or(cwd);
    // A name whose worktree or branch is still there holds someone's
    // left-over work: never start a new agent on top of it.
    let mut taken: Vec<String> = panes.iter().map(|p| p.name.clone()).collect();
    if let Some(root) = crate::git::main_root(Path::new(&base_dir)) {
        let stem = next_name(&of.name, &[]);
        let stem = stem.rsplit_once('-').map(|(s, _)| s).unwrap_or(&stem).to_string();
        for i in 2..50 {
            let n = format!("{stem}-{i}");
            let branch_exists = std::process::Command::new("git").arg("-C").arg(&root).args(["rev-parse", "--verify", "--quiet", &format!("refs/heads/agent/{n}")]).output().map(|o| o.status.success()).unwrap_or(false);
            if crate::git::worktree_dir(&root, &n).exists() || branch_exists {
                taken.push(n);
            }
        }
    }
    let name = next_name(&of.name, &taken);
    let worktree = (cfg.worktree && crate::prompts::kind(role) == "worker" && crate::git::repo_root(Path::new(&base_dir)).is_some()).then(|| format!("agent/{name}"));
    let agent = crate::team::TemplateAgent { name: name.clone(), provider: of.provider.clone().unwrap_or_default(), role: Some(role.to_string()), model: of.model.clone(), worktree: worktree.clone(), sandbox: false, device: None, account: of.account.clone() };
    let links: Vec<Vec<String>> = t.edges().unwrap_or_default().into_iter().filter_map(|(a, b, _)| if a == of.name { Some(vec![name.clone(), b]) } else if b == of.name { Some(vec![a, name.clone()]) } else { None }).collect();
    let tpl = crate::team::Template { name: "auto team".into(), description: String::new(), agents: vec![agent], links };
    let started = crate::team::start(t, reg, &tpl, (!base_dir.is_empty()).then_some(base_dir.as_str()), exe, path_prefix);
    if !started.spawned.contains(&name) {
        bail!("could not start {name}: {}", started.errors.join("; "));
    }
    let wt_dir = t.pane_by_name(&name).ok().and_then(|p| p.worktree);
    extras.insert(name.clone(), Extra { role: role.to_string(), of: of.name.clone(), at: crate::db::now(), worktree: wt_dir, reason: reason.trim().to_string() });
    save(&t.session, &extras)?;
    if let Ok(db) = crate::db::Db::open(&t.session) {
        let _ = db.add_event(&name, "autoteam", &format!("hired as {role} (copy of {}){}{}", of.name, if by == "hubcodes" { String::new() } else { format!(", asked by {by}") }, if reason.trim().is_empty() { String::new() } else { format!(": {}", reason.trim()) }));
    }
    Ok(name)
}

/// What [`release`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Released {
    /// The board task the left-over work went to.
    pub handed_to_task: Option<i64>,
    /// The branch kept for that work.
    pub kept_branch: Option<String>,
}

/// Remove an extra agent. Left-over work in its worktree (uncommitted
/// changes or commits not on the main branch) stays on its branch and goes
/// as a board task to an agent of the role that a person started (the one
/// it was copied from first), so the hand-over does not start another extra
/// agent; a clean worktree is removed.
pub fn release(t: &Tmux, reg: &Registry, name: &str) -> Result<Released> {
    let mut extras = extras(&t.session);
    let Some(x) = extras.get(name).cloned() else { bail!("{name} was not added by auto team; remove it yourself if you mean to") };
    let mut out = Released::default();
    let pane = t.pane_by_name(name).ok();
    if let Some(dir) = x.worktree.as_deref().map(Path::new).filter(|d| d.is_dir()) {
        // When git cannot tell, assume there is work: never force-remove it.
        let ch = crate::git::changes(dir).ok();
        let commits = unmerged_commits(dir);
        if ch.as_ref().map(|c| c.files > 0).unwrap_or(true) || commits.map(|n| n > 0).unwrap_or(true) {
            let ch = ch.unwrap_or_else(|| crate::git::Changes { branch: format!("agent/{name}"), ..Default::default() });
            let commits = commits.unwrap_or(0);
            let screen = pane.as_ref().and_then(|p| t.capture(&p.id, 400).ok()).unwrap_or_default();
            let db = crate::db::Db::open(&t.session)?;
            let tasks = crate::tasks::list(&db, false).unwrap_or_default();
            let messages = db.messages(200, None).unwrap_or_default();
            // The terminal text stays in a private file, not in the task.
            let file = crate::handoff::path(&t.session, name);
            let _ = crate::handoff::write_private(&file, &crate::handoff::note(name, Some(&x.role), name, "you", &tasks, &messages, &screen));
            let detail = format!(
                "{name} was an extra {role} and has been removed. Its work is on branch {branch} in {dir} ({files} changed file{fs} not committed, {commits} commit{cs} on no other branch). Open that folder, finish or check the work, commit it and get it merged. What it was doing (its messages and the end of its terminal) is in {note}.",
                note = file.display(),
                role = x.role,
                branch = ch.branch,
                dir = dir.display(),
                files = ch.files,
                fs = if ch.files == 1 { "" } else { "s" },
                cs = if commits == 1 { "" } else { "s" },
            );
            let panes = t.panes().unwrap_or_default();
            let kept = |n: &str| n != name && !extras.contains_key(n) && panes.iter().any(|p| p.name == n && !p.dead && !p.exited);
            let owner = if kept(&x.of) {
                x.of.clone()
            } else {
                panes.iter().find(|p| p.role.as_deref().map(|r| r.eq_ignore_ascii_case(&x.role)).unwrap_or(false) && kept(&p.name)).map(|p| p.name.clone()).unwrap_or_default()
            };
            let task = crate::tasks::add(&db, &format!("Finish {name}'s work on {}", ch.branch), &detail, &x.role, "hubcodes", &owner)?;
            if !owner.is_empty() {
                // Waits for its prompt when it is mid-turn.
                let _ = crate::relay::deliver(t, reg, "hubcodes", &owner, &crate::tasks::assignment_text(&task, "hubcodes"));
            }
            out.handed_to_task = Some(task.id);
            out.kept_branch = Some(ch.branch.clone());
        } else {
            let branch = ch.map(|c| c.branch).unwrap_or_default();
            let root = crate::git::main_root(dir);
            let _ = crate::git::remove_worktree(dir);
            // Its branch has nothing of its own: drop it too, so the name is free again.
            if let (Some(root), true) = (root, branch.starts_with("agent/")) {
                let _ = std::process::Command::new("git").arg("-C").arg(&root).args(["branch", "-D", &branch]).output();
            }
        }
    }
    // A task it still held goes back to the queue for the others.
    if let Ok(db) = crate::db::Db::open(&t.session) {
        for task in crate::tasks::list(&db, false).unwrap_or_default().into_iter().filter(|x| x.owner == name && x.status == "doing") {
            let _ = crate::tasks::assign(&db, task.id, "");
            let _ = crate::tasks::set_status(&db, task.id, "todo", None);
        }
        let _ = db.add_event(name, "autoteam", &match (&out.kept_branch, out.handed_to_task) {
            (Some(b), Some(id)) => format!("released; its work stays on {b} and went to task #{id}"),
            _ => "released".to_string(),
        });
    }
    if let Some(p) = pane {
        let _ = t.kill(&p.id);
    }
    let _ = t.remove_edges_of(name);
    let _ = crate::sessiondef::forget(&t.session, name);
    extras.remove(name);
    save(&t.session, &extras)?;
    Ok(out)
}

/// Brief line for an orchestrator while auto team is on.
pub fn brief_note(cfg: &AutoTeamConfig) -> Option<String> {
    cfg.enabled.then(|| format!(
        "Auto team is on: when work for a role piles up and its agents are all busy, hubcodes adds up to {} more of that role by itself and removes them when the queue is empty. You can ask for one too: hubcodes hire --role \"<role>\" --reason \"<why>\" (refused past the limit). Hand work to new teammates like any other.",
        cfg.per_role
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_limits_and_queues() {
        let taken = vec!["dev".to_string(), "dev-2".to_string(), "Reviewer-1".to_string(), "Reviewer-2".to_string(), "Reviewer-3".to_string()];
        assert_eq!(next_name("dev", &taken), "dev-3");
        assert_eq!(next_name("Reviewer-3", &taken), "Reviewer-4");
        assert_eq!(next_name("qa", &taken), "qa-2");
        let mut c = AutoTeamConfig::default();
        assert_eq!(c.limit("software engineer"), 2);
        assert_eq!(c.limit("orchestrator"), 0);
        c.roles.insert("Reviewer".into(), 3);
        assert_eq!(c.limit("reviewer"), 3);
        let task = |status: &str, owner: &str, role: &str, ext: &str| -> Task {
            serde_json::from_value(serde_json::json!({ "id": 1, "at": 0, "updated": 0, "title": "t", "detail": "", "status": status, "role": role, "owner": owner, "createdBy": "lead", "result": "", "external": ext })).unwrap()
        };
        let tasks = vec![task("todo", "", "reviewer", ""), task("todo", "", "reviewer", "reviews:o/a#1"), task("doing", "rv", "reviewer", ""), task("todo", "", "software engineer", "")];
        assert_eq!(waiting(&tasks, "reviewer", &[]), 2);
        assert_eq!(waiting(&tasks, "reviewer", &["reviews:".into()]), 1);
        assert!(brief_note(&c).is_none());
    }
}
