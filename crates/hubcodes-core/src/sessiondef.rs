//! The session as a definition that can be rebuilt: agents (provider, role,
//! model, folder, worktree), links, loops. Saved every few seconds to
//! `~/.local/state/hubcodes/<session>-def.json`; after a reboot the app (or
//! `hubcodes restore`) can start everything again, and providers that can
//! continue a conversation (`claude -c`, `codex resume --last`, `opencode
//! -c`) pick up where they were.

use crate::providers::Registry;
use crate::relay;
use crate::tmux::Tmux;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentDef {
    pub name: String,
    pub provider: String,
    pub role: Option<String>,
    pub model: Option<String>,
    pub cwd: String,
    pub worktree: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionDef {
    pub session: String,
    pub saved_at: i64,
    pub agents: Vec<AgentDef>,
    pub edges: Vec<(String, String, String)>,
    pub loops: std::collections::BTreeMap<String, relay::LoopState>,
}

pub fn path(session: &str) -> PathBuf {
    crate::db::state_dir().join(format!("{session}-def.json"))
}

/// Capture what runs now. Remote agents and proxies are not part of it.
pub fn capture(t: &Tmux) -> Result<SessionDef> {
    let agents = t
        .panes()?
        .into_iter()
        .filter(|p| p.provider.is_some() && p.provider.as_deref() != Some("proxy") && !p.dead && !p.exited)
        .map(|p| AgentDef {
            cwd: p.worktree.clone().or_else(|| pane_cwd(&p.id)).unwrap_or_default(),
            name: p.name,
            provider: p.provider.unwrap_or_default(),
            role: p.role,
            model: p.model,
            worktree: p.worktree,
            account: p.account,
        })
        .collect::<Vec<AgentDef>>();
    // One entry per name: a name can only run once, so duplicates (leftovers
    // of an older bug) would only make Restore spawn errors.
    let mut seen = std::collections::HashSet::new();
    let agents: Vec<AgentDef> = agents.into_iter().filter(|a| seen.insert(a.name.clone())).collect();
    Ok(SessionDef { session: t.session.clone(), saved_at: crate::db::now(), agents, edges: t.edges()?, loops: relay::loops(t) })
}

pub fn pane_cwd(pane: &str) -> Option<String> {
    let out = std::process::Command::new("tmux").args(["display-message", "-p", "-t", pane, "#{pane_current_path}"]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// Save when something changed (cheap: compares the JSON).
pub fn save(t: &Tmux) -> Result<bool> {
    let def = capture(t)?;
    if def.agents.is_empty() {
        return Ok(false);
    }
    let p = path(&t.session);
    let body = serde_json::to_string_pretty(&def)?;
    let same = std::fs::read_to_string(&p).map(|old| strip_time(&old) == strip_time(&body)).unwrap_or(false);
    if same {
        return Ok(false);
    }
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&p, body).with_context(|| format!("write {}", p.display()))?;
    Ok(true)
}

/// An agent removed on purpose leaves the saved session too, with its links
/// and loops, so Restore never brings it back. The save on the next tend
/// pass would skip an empty canvas, so this edits the file directly.
pub fn forget(session: &str, name: &str) -> Result<()> {
    let Some(mut def) = load_raw(session) else { return Ok(()) };
    let before = def.agents.len();
    def.agents.retain(|a| a.name != name);
    def.edges.retain(|(a, b, _)| a != name && b != name);
    def.loops.retain(|k, _| !k.split('|').any(|n| n == name));
    if def.agents.len() == before {
        return Ok(());
    }
    if def.agents.is_empty() {
        return discard(session);
    }
    std::fs::write(path(session), serde_json::to_string_pretty(&def)?).with_context(|| format!("write {}", path(session).display()))
}

/// Drop the saved session: the next start is an empty canvas.
pub fn discard(session: &str) -> Result<()> {
    match std::fs::remove_file(path(session)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("remove {}", path(session).display())),
    }
}

fn strip_time(s: &str) -> String {
    s.lines().filter(|l| !l.contains("\"savedAt\"")).collect::<Vec<_>>().join("\n")
}

pub fn load(session: &str) -> Option<SessionDef> {
    load_raw(session).map(|mut d| {
        let mut seen = std::collections::HashSet::new();
        d.agents.retain(|a| seen.insert(a.name.clone()));
        d
    })
}

fn load_raw(session: &str) -> Option<SessionDef> {
    serde_json::from_str(&std::fs::read_to_string(path(session)).ok()?).ok()
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub started: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

/// Restart the named agents with the current brief: their links, roles and
/// folders are kept (captured before the kill), the conversation is
/// continued where the provider can (`resume`). Names that are not running
/// are started like [`restore`] does.
pub fn restart(t: &Tmux, reg: &Registry, names: &[String], exe: &str, path_prefix: Option<&str>, resume: bool) -> Result<Restored> {
    let mut def = capture(t)?;
    def.agents.retain(|a| names.contains(&a.name));
    if def.agents.is_empty() {
        anyhow::bail!("none of {} is a saved or running agent", names.join(", "));
    }
    for p in t.panes().unwrap_or_default().into_iter().filter(|p| names.contains(&p.name)) {
        let _ = t.kill(&p.id);
    }
    Ok(restore(t, reg, &def, exe, path_prefix, resume))
}

/// What [`switch`] did: the start, and how the context came along
/// (`conversation`: the same provider continues it; `handoff`: a new
/// provider got the hand-over note).
#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Switched {
    pub restored: Restored,
    pub kept: String,
}

/// Run an agent on another provider or model, keeping its name, role,
/// folder, worktree and links (see [`crate::handoff`]). An empty model is
/// the provider's own default.
pub fn switch(t: &Tmux, reg: &Registry, name: &str, provider: &str, model: Option<&str>, exe: &str, path_prefix: Option<&str>) -> Result<Switched> {
    if reg.get(provider).is_none() {
        anyhow::bail!("unknown provider '{provider}'");
    }
    let mut def = capture(t)?;
    def.agents.retain(|a| a.name == name);
    let old = def.clone();
    let Some(a) = def.agents.first_mut() else { anyhow::bail!("{name} is not a running or saved agent") };
    let model = model.map(str::trim).filter(|m| !m.is_empty()).map(String::from);
    // A resume flag picks the folder's latest conversation, not this agent's:
    // with another agent of the same tool in the folder, hand over instead.
    let shared = t.panes().unwrap_or_default().iter().any(|p| p.name != name && p.provider.as_deref() == Some(a.provider.as_str()) && !p.dead && !p.exited && p.worktree.clone().or_else(|| pane_cwd(&p.id)).unwrap_or_default() == a.cwd);
    let same = a.provider == provider && !shared;
    let label = |p: &str, m: &Option<String>| format!("{p}{}", m.as_deref().map(|m| format!(" · {m}")).unwrap_or_default());
    let pending = crate::handoff::pending_path(&t.session, name);
    if !same {
        let screen = t.pane_by_name(name).ok().and_then(|p| t.capture(&p.id, 400).ok()).unwrap_or_default();
        let (tasks, messages) = match crate::db::Db::open(&t.session) {
            Ok(db) => (crate::tasks::list(&db, false).unwrap_or_default(), db.messages(200, None).unwrap_or_default()),
            Err(_) => (vec![], vec![]),
        };
        let (from, to) = (label(&a.provider, &a.model), label(provider, &model));
        let full = crate::handoff::note(name, a.role.as_deref(), &from, &to, &tasks, &messages, &screen);
        let file = crate::handoff::path(&t.session, name);
        let brief = crate::handoff::brief(name, a.role.as_deref(), &from, &to, &tasks, &file);
        crate::handoff::prepare(&t.session, name, &full, &brief).with_context(|| format!("write {}", file.display()))?;
        if a.provider != provider {
            // Accounts are logins of one tool; the new tool starts on its own.
            a.account = None;
        }
    }
    a.provider = provider.to_string();
    a.model = model;
    // Build the new launch command before anything is stopped: a tool that
    // cannot start leaves the agent running as it was.
    let tpl = crate::team::TemplateAgent { name: a.name.clone(), provider: a.provider.clone(), role: a.role.clone(), model: a.model.clone(), worktree: None, sandbox: false, device: None, account: a.account.clone() };
    if let Err(e) = crate::team::launch_command_in(reg, &t.session, &tpl, exe, path_prefix, Some(a.cwd.as_str())) {
        let _ = std::fs::remove_file(&pending);
        anyhow::bail!("{name} stays on {}: {provider} cannot start ({e:#})", label(&old.agents[0].provider, &old.agents[0].model));
    }
    for p in t.panes().unwrap_or_default().into_iter().filter(|p| p.name == name) {
        let _ = t.kill(&p.id);
    }
    let restored = restore(t, reg, &def, exe, path_prefix, same);
    let _ = std::fs::remove_file(&pending);
    if !restored.started.iter().any(|n| n == name) {
        // Put the agent back where it was rather than leave it gone.
        let back = restore(t, reg, &old, exe, path_prefix, true);
        anyhow::bail!(
            "{name} could not start on {provider} ({}); {}",
            restored.errors.join("; "),
            if back.started.iter().any(|n| n == name) { format!("it runs on {} again", label(&old.agents[0].provider, &old.agents[0].model)) } else { format!("restoring it failed too: {}", back.errors.join("; ")) }
        );
    }
    Ok(Switched { restored, kept: if same { "conversation".into() } else { "handoff".into() } })
}

/// Start every saved agent that is not running, with the provider's resume
/// flag when it has one, then the links and loops.
pub fn restore(t: &Tmux, reg: &Registry, def: &SessionDef, exe: &str, path_prefix: Option<&str>, resume: bool) -> Restored {
    let mut out = Restored::default();
    let panes = t.panes().unwrap_or_default();
    // A pane whose agent already quit (the shell prompt is back) is not
    // "running": close it so the restored one can take the name.
    let running: Vec<String> = panes.iter().filter(|p| !p.exited).map(|p| p.name.clone()).collect();
    for a in &def.agents {
        if running.contains(&a.name) {
            out.skipped.push(a.name.clone());
            continue;
        }
        for p in panes.iter().filter(|p| p.name == a.name && p.exited) {
            let _ = t.kill(&p.id);
        }
        let Some(p) = reg.get(&a.provider) else {
            out.errors.push(format!("{}: unknown provider {}", a.name, a.provider));
            continue;
        };
        let tpl = crate::team::TemplateAgent { name: a.name.clone(), provider: a.provider.clone(), role: a.role.clone(), model: a.model.clone(), worktree: None, sandbox: false, device: None, account: a.account.clone() };
        let fresh = match crate::team::launch_command_in(reg, &t.session, &tpl, exe, path_prefix, Some(a.cwd.as_str())) {
            Ok(c) => c,
            Err(e) => {
                out.errors.push(format!("{}: {e}", a.name));
                continue;
            }
        };
        let mut cmd = fresh.clone();
        if resume {
            if let Some(flag) = p.resume_flag.as_deref().filter(|f| !f.trim().is_empty()) {
                // The provider command is the last word group before hook args; append the flag right after it.
                let base = p.command.trim();
                if let Some(pos) = fresh.find(base) {
                    let mut with = fresh.clone();
                    with.insert_str(pos + base.len(), &format!(" {flag}"));
                    // Nothing to continue (new folder, history gone): the tool
                    // exits at once with an error, so fall back to a fresh start.
                    cmd = format!(
                        "_hc_t0=$(date +%s); {with}; _hc_s=$?; if [ $_hc_s -ne 0 ] && [ $(( $(date +%s) - _hc_t0 )) -lt 20 ]; then echo; echo '[hubcodes] nothing to continue here, starting fresh'; {fresh}; fi"
                    );
                }
            }
        }
        // `/` is what an app launched from Finder captures for agents started
        // without a folder; home is what they meant.
        let cwd = Some(a.cwd.as_str()).filter(|c| !c.is_empty() && *c != "/" && std::path::Path::new(c).is_dir());
        match t.spawn_with_account(&a.name, &a.provider, &cmd, cwd, a.role.as_deref(), a.model.as_deref(), a.account.as_deref()) {
            Ok(id) => {
                if let Some(w) = &a.worktree {
                    let _ = t.set_pane_option(&id, crate::tmux::OPT_WORKTREE, w);
                }
                out.started.push(a.name.clone());
            }
            Err(e) => out.errors.push(format!("{}: {e:#}", a.name)),
        }
    }
    for (a, b, label) in &def.edges {
        let _ = t.add_edge(a, b, label);
    }
    if !def.loops.is_empty() {
        let _ = relay::set_loops(t, &def.loops);
    }
    out
}
