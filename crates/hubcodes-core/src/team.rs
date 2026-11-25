//! Team templates: a named set of agents (provider, role, model) with links
//! and loops, started with one command or one click. Saved in hubcodes.toml:
//!
//! ```toml
//! [[template]]
//! name = "full-stack"
//! agents = [
//!   { name = "lead",     provider = "claude", role = "orchestrator", model = "opus" },
//!   { name = "dev",      provider = "claude", role = "software engineer", worktree = "agent/dev" },
//!   { name = "reviewer", provider = "opencode", role = "reviewer" },
//! ]
//! links = [["lead", "web"], ["lead", "api"], ["api", "reviewer", "loop"]]
//! ```

use crate::providers::Registry;
use crate::relay;
use crate::tmux::Tmux;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TemplateAgent {
    pub name: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Branch for an own worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Confine writes to its folder (`[sandbox]` in hubcodes.toml decides how).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sandbox: bool,
    /// Start it on another device (a `[[peer]]` name); it shows as `device/name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Account (a separate login of the tool) it runs under; `None` = the tool's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Template {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub agents: Vec<TemplateAgent>,
    /// `[from, to]` or `[from, to, "loop"]`.
    #[serde(default)]
    pub links: Vec<Vec<String>>,
}

pub fn default_templates() -> Vec<Template> {
    let a = |name: &str, provider: &str, role: &str| TemplateAgent { name: name.into(), provider: provider.into(), role: Some(role.into()), model: None, worktree: None, sandbox: false, device: None, account: None };
    let l = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    vec![
        Template {
            name: "pair".into(),
            description: "Builder and reviewer in a loop".into(),
            agents: vec![a("builder", "claude", "software engineer"), a("reviewer", "codex", "reviewer")],
            links: vec![l(&["builder", "reviewer", "loop"])],
        },
        Template {
            name: "full-stack".into(),
            description: "Orchestrator, software engineer, reviewer".into(),
            agents: vec![a("lead", "claude", "orchestrator"), a("dev", "claude", "software engineer"), a("reviewer", "codex", "reviewer")],
            links: vec![l(&["lead", "dev"]), l(&["dev", "reviewer", "loop"])],
        },
    ]
}

/// The agent with its model replaced when `models` names it.
fn with_model(a: &TemplateAgent, models: &std::collections::BTreeMap<String, String>) -> TemplateAgent {
    match models.get(&a.name) {
        Some(m) => TemplateAgent { model: Some(m.trim().to_string()).filter(|m| !m.is_empty()), ..a.clone() },
        None => a.clone(),
    }
}

/// What `start` did, agent by agent.
#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub spawned: Vec<String>,
    pub skipped: Vec<String>,
    pub links: usize,
    pub errors: Vec<String>,
}

/// Build the launch command for one agent the way the CLI and the desktop do.
pub fn launch_command(reg: &Registry, session: &str, a: &TemplateAgent, exe: &str, path_prefix: Option<&str>) -> Result<String> {
    launch_command_in(reg, session, a, exe, path_prefix, None)
}

/// [`launch_command`] for an agent that starts in `cwd` (where its repository's role files are looked up).
pub fn launch_command_in(reg: &Registry, session: &str, a: &TemplateAgent, exe: &str, path_prefix: Option<&str>, cwd: Option<&str>) -> Result<String> {
    let p = reg.get(&a.provider).ok_or_else(|| anyhow!("unknown provider '{}'", a.provider))?;
    let hook_kind = p.hook_kind.clone().unwrap_or_else(|| a.provider.clone());
    let account = a.account.as_deref().map(str::trim).filter(|x| !x.is_empty()).unwrap_or(crate::accounts::DEFAULT);
    let paths = crate::accounts::paths(&a.provider, p.hook_kind.as_deref(), account);
    crate::accounts::prepare(&a.provider, p.hook_kind.as_deref(), account)?;
    let hook_args = crate::hooks::launch_args_account(&hook_kind, session, exe, &p.command, &paths).unwrap_or_default();
    let hook_part = hook_args.iter().map(|x| format!("'{}'", x.replace('\'', "'\\''"))).collect::<Vec<_>>().join(" ");
    let path_part = path_prefix.map(|d| format!("PATH='{}':\"$PATH\" ", d.replace('\'', "'\\''"))).unwrap_or_default();
    let account_part = crate::accounts::env_prefix(&a.provider, p.hook_kind.as_deref(), account);
    Ok(format!(
        "{path_part}{account_part}HUBCODES_AGENT='{}' HUBCODES_SESSION='{session}' HUBCODES_ROLE='{}' HUBCODES_MODEL='{}' HUBCODES_ACCOUNT='{account}' {}{}{}",
        a.name,
        a.role.as_deref().unwrap_or(""),
        a.model.as_deref().unwrap_or(""),
        p.launch_command_with(a.model.as_deref(), a.role.as_deref(), &crate::prompts::start_extra(&reg.memory, session, &a.name, a.role.as_deref(), cwd)),
        if hook_part.is_empty() { "" } else { " " },
        hook_part
    ))
}

/// Start every agent of the template that does not exist yet, then the links.
pub fn start(t: &Tmux, reg: &Registry, tpl: &Template, cwd: Option<&str>, exe: &str, path_prefix: Option<&str>) -> Started {
    start_with(t, reg, tpl, cwd, exe, path_prefix, &std::collections::BTreeMap::new())
}

/// [`start`] with the model chosen per agent on this machine: `models` maps
/// an agent name to the model id to use instead of the template's (an empty
/// id means the provider's own default). A template travels between Macs
/// whose tools carry different catalogs, so the model is settled where the
/// team starts, not where it was saved.
pub fn start_with(t: &Tmux, reg: &Registry, tpl: &Template, cwd: Option<&str>, exe: &str, path_prefix: Option<&str>, models: &std::collections::BTreeMap<String, String>) -> Started {
    let mut out = Started::default();
    let existing: Vec<String> = t.panes().map(|ps| ps.into_iter().map(|p| p.name).collect()).unwrap_or_default();
    for a in tpl.agents.iter().map(|a| with_model(a, models)) {
        let a = &a;
        if existing.contains(&a.name) {
            out.skipped.push(a.name.clone());
            continue;
        }
        if let Some(device) = a.device.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            let Some(peer) = reg.peers.iter().find(|p| p.name == device) else {
                out.errors.push(format!("{}: unknown device {device}", a.name));
                continue;
            };
            match crate::remote::spawn_account(peer, &a.name, &a.provider, None, a.role.as_deref(), a.model.as_deref(), a.account.as_deref()) {
                Ok(full) => out.spawned.push(full),
                Err(e) => out.errors.push(format!("{}: {e:#}", a.name)),
            }
            continue;
        }
        let cmd = match launch_command_in(reg, &t.session, a, exe, path_prefix, cwd) {
            Ok(c) => c,
            Err(e) => {
                out.errors.push(format!("{}: {e}", a.name));
                continue;
            }
        };
        let mut dir = cwd.map(|s| s.to_string());
        let mut wt = None;
        if let Some(branch) = &a.worktree {
            let base = dir.clone().unwrap_or_else(|| std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default());
            match crate::git::add_worktree(std::path::Path::new(&base), &a.name, branch) {
                Ok(p) => {
                    wt = Some(p.display().to_string());
                    dir = wt.clone();
                }
                Err(e) => out.errors.push(format!("{}: worktree: {e:#}", a.name)),
            }
        }
        match t.spawn_with_account(&a.name, &a.provider, &cmd, dir.as_deref(), a.role.as_deref(), a.model.as_deref(), a.account.as_deref()) {
            Ok(id) => {
                if let Some(w) = wt {
                    let _ = t.set_pane_option(&id, crate::tmux::OPT_WORKTREE, &w);
                }
                out.spawned.push(a.name.clone());
            }
            Err(e) => out.errors.push(format!("{}: {e:#}", a.name)),
        }
    }
    for l in &tpl.links {
        if l.len() < 2 {
            continue;
        }
        let (from, to) = (&l[0], &l[1]);
        let is_loop = l.get(2).map(|k| k == "loop").unwrap_or(false);
        let r = if is_loop { relay::set_loop(t, from, to, true, reg.loop_cfg.max_rounds) } else { t.add_edge(from, to, "link") };
        match r {
            Ok(()) => out.links += 1,
            Err(e) => out.errors.push(format!("{from}→{to}: {e:#}")),
        }
    }
    out
}

/// Capture the running session as a template (agents + links + loops).
pub fn capture(t: &Tmux, name: &str) -> Result<Template> {
    let loops = relay::loops(t);
    let agents = t
        .panes()?
        .into_iter()
        .filter(|p| p.provider.is_some() && p.provider.as_deref() != Some("proxy"))
        .map(|p| TemplateAgent {
            name: p.name,
            provider: p.provider.unwrap_or_else(|| "shell".into()),
            role: p.role,
            model: p.model,
            worktree: None,
            sandbox: false,
            device: None,
            account: p.account,
        })
        .collect();
    let links = t
        .edges()?
        .into_iter()
        .map(|(a, b, _)| {
            let mut v = vec![a.clone(), b.clone()];
            if loops.contains_key(&relay::pair_key(&a, &b)) {
                v.push("loop".into());
            }
            v
        })
        .collect();
    Ok(Template { name: name.into(), description: String::new(), agents, links })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_chosen_at_start_replaces_the_templates() {
        let a = TemplateAgent { name: "dev".into(), provider: "claude".into(), role: None, model: Some("opus".into()), worktree: None, sandbox: false, device: None, account: None };
        let mut m = std::collections::BTreeMap::new();
        assert_eq!(with_model(&a, &m).model.as_deref(), Some("opus"));
        m.insert("dev".to_string(), "sonnet".to_string());
        assert_eq!(with_model(&a, &m).model.as_deref(), Some("sonnet"));
        m.insert("dev".to_string(), "  ".to_string());
        assert_eq!(with_model(&a, &m).model, None, "an empty choice means the provider default");
    }
}
