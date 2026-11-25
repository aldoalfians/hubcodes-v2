//! Saved prompts: `/review`, `/test`, `/handoff` in a composer expand to
//! text from `[[prompt]]` in hubcodes.toml, with variables filled from the
//! agent they are sent to.
//!
//! Variables: `{agent}`, `{role}`, `{model}`, `{cwd}`, `{branch}`,
//! `{changes}` (one-line summary), `{last_diff}` (unified diff, capped),
//! `{goal}` (the loop goal, if any), `{notes}` (the shared notes), `{date}`.

use crate::git;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Prompt {
    /// Typed as `/name`.
    pub name: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

pub fn default_prompts() -> Vec<Prompt> {
    let p = |name: &str, description: &str, text: &str| Prompt { name: name.into(), text: text.into(), description: description.into() };
    vec![
        p("review", "Review the agent's current changes", "Review the changes on branch {branch} in {cwd} for correctness, missing tests and anything that would surprise a reviewer. Be concrete: file and line.\n\n{last_diff}"),
        p("test", "Run the tests and fix failures", "Run the test suite in {cwd}. If anything fails, fix it, rerun, and report what changed. Branch: {branch}."),
        p("handoff", "Summarise for the next agent", "Write a handoff for the next agent: what you changed on {branch}, what is left, and the exact commands to verify. Keep it under 20 lines. Changes so far: {changes}."),
        p("status", "One-paragraph status", "In one paragraph: what you are doing, what is blocked, what you need from a human."),
        p("plan", "Plan before coding", "Before writing code, list the steps you will take and the files you will touch in {cwd}. Then wait for a go."),
    ]
}

/// Everything the variables can read, gathered once per expansion.
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub agent: String,
    pub role: String,
    pub model: String,
    pub cwd: String,
    pub goal: String,
    pub notes: String,
}

const DIFF_CAP: usize = 12_000;

pub fn expand(text: &str, ctx: &Context) -> String {
    let dir = Path::new(&ctx.cwd);
    let lazy_git = text.contains("{branch}") || text.contains("{changes}") || text.contains("{last_diff}");
    let changes = if lazy_git && !ctx.cwd.is_empty() { git::changes(dir).ok() } else { None };
    let branch = changes.as_ref().map(|c| c.branch.clone()).unwrap_or_else(|| "(no git)".into());
    let summary = changes
        .as_ref()
        .map(|c| format!("{} files, +{} −{}", c.files, c.insertions, c.deletions))
        .unwrap_or_else(|| "(no git)".into());
    let last_diff = if text.contains("{last_diff}") && changes.is_some() {
        git::diff(dir, DIFF_CAP).unwrap_or_default()
    } else {
        String::new()
    };
    let date = std::process::Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    text.replace("{agent}", &ctx.agent)
        .replace("{role}", &ctx.role)
        .replace("{model}", &ctx.model)
        .replace("{cwd}", &ctx.cwd)
        .replace("{branch}", &branch)
        .replace("{changes}", &summary)
        .replace("{last_diff}", &last_diff)
        .replace("{goal}", &ctx.goal)
        .replace("{notes}", &ctx.notes)
        .replace("{date}", &date)
}

/// `/review tighten the tests` → (prompt text with variables filled, trailing words appended).
pub fn apply_slash(input: &str, prompts: &[Prompt], ctx: &Context) -> Option<String> {
    let rest = input.trim().strip_prefix('/')?;
    let (name, extra) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let p = prompts.iter().find(|p| p.name == name)?;
    let mut out = expand(&p.text, ctx);
    if !extra.trim().is_empty() {
        out.push_str("\n\n");
        out.push_str(extra.trim());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_and_slash() {
        let ctx = Context { agent: "builder".into(), role: "backend".into(), cwd: String::new(), ..Default::default() };
        assert_eq!(expand("hi {agent} ({role}) on {branch}", &ctx), "hi builder (backend) on (no git)");
        let ps = default_prompts();
        let out = apply_slash("/status please", &ps, &ctx).unwrap();
        assert!(out.starts_with("In one paragraph"));
        assert!(out.ends_with("please"));
        assert!(apply_slash("/nope", &ps, &ctx).is_none());
        assert!(apply_slash("plain text", &ps, &ctx).is_none());
    }
}

// ----- what an agent is told when it starts -----

/// Version of the generated brief. Stored on every pane at spawn
/// (`@hubcodes_brief_v`); an agent started with an older version still
/// carries the old text, so the app offers to restart it.
/// 4: teammates check what a person types to them with their lead.
pub const BRIEF_VERSION: &str = "5";

/// Which kind of job a role name describes, for the start brief.
pub fn kind(role: &str) -> &'static str {
    let r = role.to_lowercase();
    if r.contains("orchestr") || r.contains("lead") || r.contains("manager") || r.contains("coordinator") || r.contains("planner") {
        "orchestrator"
    } else if r.contains("review") {
        "reviewer"
    } else if r.contains("test") || r.contains("qa") {
        "tester"
    } else {
        "worker"
    }
}

/// The part of an agent's system prompt that says how the team works: what
/// its role does and does not do, who else is on the team, and how to hand
/// work over. Without it the orchestrator just does everything itself.
pub fn start_brief(name: &str, role: Option<&str>, team: &[(String, Option<String>)]) -> String {
    start_brief_linked(name, role, team, &[])
}

/// [`start_brief`] that also knows which teammates are linked to this
/// agent on the canvas: an orchestrator is told to hand work along its
/// links first, and its engineer list is the linked engineers when there
/// are any.
pub fn start_brief_linked(name: &str, role: Option<&str>, team: &[(String, Option<String>)], links: &[String]) -> String {
    start_brief_with(name, role, team, links, None)
}

/// What each kind of role is told about its job, before the live team facts
/// (who runs, the lead, links) hubcodes adds. `{lead}` becomes the
/// orchestrator's name, or "your orchestrator". Settings → Roles shows this
/// text to edit; a role file that replaces it keeps the team facts.
pub fn default_job(kind: &str) -> String {
    match kind {
        "orchestrator" => "Your job is to coordinate the team, not to do the work yourself. You must not edit files, write code, or run builds and tests on your own, and hubcodes refuses those tools for you; the engineers do that and the reviewer checks it. For every request from the human: split it into clear tasks, send each one to the right teammate with hubcodes send <name> \"goal, files, how we know it is done\" (or hubcodes task add \"...\" --role \"<role>\" so an idle agent with that role picks it up), then wait for their replies, which arrive as [from <name>] ... . Check what came back, pass finished work to the reviewer, and report a short status to the human. Only touch the code yourself when no teammate can.",
        "reviewer" => "Your job is to review what the engineers send you, not to rewrite it. Reply with findings (file:line, what is wrong, the fix) to the sender and to {lead} with hubcodes send <name> \"...\"; say clearly when it is approved.",
        "tester" => "Your job is to run and write tests for what the engineers send you and report the results (what passed, what failed, how to reproduce) with hubcodes send <name> \"...\" to the sender and to {lead}.",
        _ => "Do the work your orchestrator sends you, in your own area of the code. When it is done, report back with hubcodes send {lead} \"done: what changed, how to verify\" (and hubcodes task done <id> \"...\" when it came as a task). When a brief is unclear, ask instead of guessing.",
    }
    .to_string()
}

/// [`start_brief_linked`] with the job text replaced (a role file in replace mode).
pub fn start_brief_with(name: &str, role: Option<&str>, team: &[(String, Option<String>)], links: &[String], job: Option<String>) -> String {
    let role = role.map(str::trim).filter(|r| !r.is_empty());
    let mut s = String::new();
    let others: Vec<String> = team.iter().filter(|(n, _)| n != name).map(|(n, r)| match r.as_deref().filter(|r| !r.trim().is_empty()) { Some(r) => format!("{n} ({r})"), None => n.clone() }).collect();
    let reviewer = team.iter().find(|(n, r)| n != name && r.as_deref().map(|r| kind(r) == "reviewer").unwrap_or(false)).map(|(n, _)| n.clone());
    let all_engineers: Vec<String> = team.iter().filter(|(n, r)| n != name && r.as_deref().map(|r| kind(r) == "worker").unwrap_or(false)).map(|(n, _)| n.clone()).collect();
    let linked_engineers: Vec<String> = all_engineers.iter().filter(|e| links.contains(e)).cloned().collect();
    let engineers = if linked_engineers.is_empty() { all_engineers } else { linked_engineers };
    let k = role.map(kind).unwrap_or("worker");
    // The lead is the orchestrator this agent is linked to on the canvas. An
    // agent linked to none has no lead: it answers whoever asked, and is not
    // told to clear a human's request with an orchestrator it is not wired to.
    // Any orchestrator, not only the first one in the session.
    let my_lead = team.iter().find(|(n, r)| n != name && links.contains(n) && r.as_deref().map(|r| kind(r) == "orchestrator").unwrap_or(false)).map(|(n, _)| n.clone());
    let lead_word = my_lead.clone().unwrap_or_else(|| "<whoever asked>".into());
    s.push_str(&job.unwrap_or_else(|| default_job(k)).trim().replace("{lead}", &lead_word));
    if k == "orchestrator" {
        if !engineers.is_empty() {
            s.push_str(&format!(" Engineers available now: {}.", engineers.join(", ")));
            if engineers.iter().any(|e| e.contains('/')) {
                s.push_str(" A name like device/agent is a teammate on another device: hubcodes send works the same, its reply arrives as [from device/agent].");
            }
        }
        if let Some(r) = &reviewer {
            s.push_str(&format!(" Reviewer: {r}."));
        }
        let peer_leads: Vec<&str> = team.iter().filter(|(n, r)| n != name && n.contains('/') && r.as_deref().map(|r| kind(r) == "orchestrator").unwrap_or(false)).map(|(n, _)| n.as_str()).collect();
        if !peer_leads.is_empty() {
            s.push_str(&format!(" Orchestrators on other devices: {}. For a large request, hand a whole part to one of them with hubcodes send {} \"part: goal, repo and branch, how we know it is done\"; it splits that part among its own engineers there and reports back as [from {}].", peer_leads.join(", "), peer_leads[0], peer_leads[0]));
        }
        let linked: Vec<&String> = links.iter().filter(|l| team.iter().any(|(n, _)| n == *l && n != name)).collect();
        if !linked.is_empty() {
            s.push_str(&format!(" Linked to you on the canvas: {}; hand work along those links first.", linked.iter().map(|l| l.as_str()).collect::<Vec<_>>().join(", ")));
        }
    }
    if let Some(o) = my_lead.as_deref().filter(|_| role.map(kind).unwrap_or("worker") != "orchestrator") {
        s.push_str(&format!(" A message without a [from <name>] tag was typed by the human straight to you: before acting on it, tell {o} what was asked and what you plan with hubcodes send {o} \"...\" and wait for its go-ahead; hand anything outside your role to {o}."));
    }
    if !others.is_empty() {
        s.push_str(&format!(" Your team right now: {}. hubcodes ls shows the current team.", others.join(", ")));
    }
    s
}

/// Names linked to `name` on the canvas (either direction), for [`start_brief_linked`].
pub fn links_of(session: &str, name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (a, b, _) in crate::tmux::Tmux::named(session).edges().unwrap_or_default() {
        let other = if a == name { b } else if b == name { a } else { continue };
        if !out.contains(&other) {
            out.push(other);
        }
    }
    out
}

/// One short line appended to a human's message to an orchestrator, so the
/// rule holds whatever prompt the agent was started with (a resumed or
/// long-running orchestrator keeps its old system prompt). `None` for
/// every other role.
pub fn delegation_reminder(session: &str, to: &str) -> Option<String> {
    let team = team_roster(session);
    let (_, role) = team.iter().find(|(n, _)| n == to)?;
    if kind(role.as_deref()?) != "orchestrator" {
        return None;
    }
    let links = links_of(session, to);
    // Teammates to delegate to: everyone but another orchestrator on this
    // Mac; an orchestrator on another Mac counts, it takes whole parts.
    let mut mates: Vec<String> = team
        .iter()
        .filter(|(n, r)| n != to && r.as_deref().map(|r| kind(r) != "orchestrator" || n.contains('/')).unwrap_or(false))
        .map(|(n, r)| format!("{n} ({}{})", r.as_deref().unwrap_or(""), if n.contains('/') && r.as_deref().map(|r| kind(r) == "orchestrator").unwrap_or(false) { ", takes a whole part" } else { "" }))
        .collect();
    let linked: Vec<String> = team.iter().filter(|(n, _)| n != to && links.contains(n)).map(|(n, r)| format!("{n} ({})", r.as_deref().unwrap_or(""))).collect();
    if !linked.is_empty() {
        mates = linked;
    }
    if mates.is_empty() {
        return Some("[hubcodes] You are the orchestrator: do not do this yourself. No teammate is running yet; start one (hubcodes spawn <name> -p <provider> -r \"software engineer\") or ask the human for one, then delegate with hubcodes send <name> \"...\".".into());
    }
    Some(format!(
        "[hubcodes] You are the orchestrator: do not do this yourself. Delegate with hubcodes send <name> \"goal, files, done-when\" to {}; wait for their [from <name>] replies, then report back.",
        mates.join(", ")
    ))
}

/// `(name, role)` of every agent in the session, for [`start_brief`].
pub fn team_roster(session: &str) -> Vec<(String, Option<String>)> {
    let mut team: Vec<(String, Option<String>)> = crate::tmux::Tmux::named(session)
        .panes()
        .map(|ps| ps.into_iter().filter(|p| p.provider.as_deref().map(|v| v != "shell" && v != "proxy").unwrap_or(false)).map(|p| (p.name, p.role)).collect())
        .unwrap_or_default();
    // Teammates on other devices, as `device/agent`, while the peer cache is fresh.
    for a in crate::remote::cached_agents(session).into_iter().filter(|a| !a.dead) {
        if !team.iter().any(|(n, _)| n == &a.name) {
            team.push((a.name, a.role));
        }
    }
    team
}

// ----- role files: what the user adds to the brief, or puts in its place -----
//
// `<repo>/.hubcodes/roles/<slug>.md` (first), then `~/.config/hubcodes/roles/<slug>.md`.
// The file is appended to the generated brief. When its first line is
// `<!-- hubcodes: replace -->` it stands in for the generated brief instead.
// HTML comments are for the human and are never sent.

/// First line of a role file that makes it replace the generated brief.
pub const REPLACE_MARKER: &str = "<!-- hubcodes: replace -->";

/// `Software Engineer` → `software-engineer`: the file name a role is looked up under.
pub fn role_slug(role: &str) -> String {
    let mut slug = String::new();
    for c in role.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_string()
}

/// The user's role files: `~/.config/hubcodes/roles/` (next to hubcodes.toml).
pub fn user_roles_dir() -> PathBuf {
    crate::providers::user_config_path().parent().map(|d| d.join("roles")).unwrap_or_else(|| PathBuf::from("roles"))
}

/// The repository's role files for a folder: `<git root>/.hubcodes/roles/`
/// (the main checkout's, even from an agent worktree), if the folder is in a repo.
pub fn repo_roles_dir(cwd: &Path) -> Option<PathBuf> {
    git::main_root(cwd).map(|r| r.join(".hubcodes").join("roles"))
}

/// Where a role file is looked for, in order: the repository's, then the user's.
pub fn role_dirs(cwd: Option<&str>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = cwd.map(str::trim).filter(|c| !c.is_empty()).and_then(|c| repo_roles_dir(Path::new(c))).into_iter().collect();
    v.push(user_roles_dir());
    v
}

/// The role file for a role, first hit in [`role_dirs`] wins: `(path, raw text)`.
pub fn role_file(role: &str, cwd: Option<&str>) -> Option<(PathBuf, String)> {
    role_file_in(role, &role_dirs(cwd))
}

/// [`role_file`] over explicit folders.
pub fn role_file_in(role: &str, dirs: &[PathBuf]) -> Option<(PathBuf, String)> {
    let slug = role_slug(role);
    if slug.is_empty() {
        return None;
    }
    dirs.iter().map(|d| d.join(format!("{slug}.md"))).find_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
}

/// Does this role file replace the generated brief (first line is [`REPLACE_MARKER`])?
pub fn replaces(text: &str) -> bool {
    text.trim_start().starts_with(REPLACE_MARKER)
}

/// The file without its `<!-- … -->` comments, trimmed: what the agent is sent.
pub fn role_body(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        rest = match rest[i..].find("-->") {
            Some(j) => &rest[i + j + 3..],
            None => "",
        };
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// The generated brief with a role file layered on: the file's text appended,
/// or in the brief's place when the file replaces. No file → the brief as is.
pub fn layer_role_file(brief: &str, file: Option<&str>) -> String {
    let Some(text) = file else { return brief.to_string() };
    let body = role_body(text);
    if replaces(text) {
        body
    } else if body.is_empty() {
        brief.to_string()
    } else if brief.is_empty() {
        body
    } else {
        format!("{brief}\n\n{body}")
    }
}

/// Where the agent runs, when this Mac has other devices: its own device
/// name (so it does not take a `mac/lead` for itself when it is `lead` on
/// `mini`) and how to see the other devices' agents, which `hubcodes ls`
/// does not list.
pub fn devices_note(me_device: &str, name: &str, peers: &[&str]) -> Option<String> {
    if peers.is_empty() || name.is_empty() {
        return None;
    }
    let me_device = if me_device.is_empty() { "this device" } else { me_device };
    let first = peers[0];
    Some(format!(
        "You run on the device {me_device}. Other devices: {}. Their agents are named device/agent (for example {first}/lead), and to them you are {me_device}/{name}; a name without a device is always one here. hubcodes ls lists only this device; hubcodes peers lists every device and its agents right now, so check it before saying who else is on the team. Message them like local agents: hubcodes send {first}/<agent> \"…\". An orchestrator on another device takes whole parts of the work.",
        peers.join(", ")
    ))
}

/// What an agent of this role is told today: the generated brief with the
/// role file layered on, and the file that was used, if any.
pub fn effective_brief(session: &str, name: &str, role: Option<&str>, cwd: Option<&str>) -> (String, Option<PathBuf>) {
    let file = role.and_then(|r| role_file(r, cwd));
    let job = file.as_ref().filter(|(_, t)| replaces(t)).map(|(_, t)| role_body(t)).filter(|b| !b.is_empty());
    let replaced = job.is_some();
    let team = team_roster(session);
    let mut links = links_of(session, name);
    // The lead counts through a chain of lines too (reviewer → dev → lead),
    // the same way a person's message is routed (relay::lead_in_group).
    let leads: Vec<String> = team.iter().filter(|(n, r)| n != name && r.as_deref().map(|r| kind(r) == "orchestrator").unwrap_or(false)).map(|(n, _)| n.clone()).collect();
    if let Some(l) = crate::relay::lead_in_group(name, &crate::tmux::Tmux::named(session).edges().unwrap_or_default(), &leads) {
        if !links.contains(&l) {
            links.push(l);
        }
    }
    let mut brief = start_brief_with(name, role, &team, &links, job);
    if let Ok(reg) = crate::providers::Registry::load() {
        if role.map(kind) == Some("orchestrator") {
            if let Some(n) = crate::autoteam::brief_note(&reg.autoteam) {
                brief = if brief.is_empty() { n } else { format!("{brief}\n\n{n}") };
            }
        }
        if let Some(d) = devices_note(&reg.sharing.name, name, &reg.peers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()) {
            brief = if brief.is_empty() { d } else { format!("{brief}\n\n{d}") };
        }
    }
    // Replace mode already took the file as the job text; extend mode appends it.
    let text = if replaced { brief } else { layer_role_file(&brief, file.as_ref().map(|(_, t)| t.as_str())) };
    (text, file.map(|(p, _)| p))
}

/// A starting role file for `hubcodes role init`: the replace marker, how the
/// two modes work, then the generated brief for the role as text to edit.
pub fn role_template(role: &str) -> String {
    let brief = default_job(kind(role));
    format!(
        "{REPLACE_MARKER}\n<!--\nhubcodes role file for \"{role}\". Comments like this one are never sent to the agent.\n\nThe first line makes this file REPLACE the brief hubcodes generates for the role.\nDelete that line and the text below is APPENDED to the generated brief instead\n(the generated brief says what the job is, who is on the team right now and how to\nhand work over; keep only your additions in that mode).\n\nRepo file <repo>/.hubcodes/roles/<slug>.md wins over ~/.config/hubcodes/roles/<slug>.md.\n`hubcodes role show \"{role}\"` prints what the agent gets.\n-->\n\n{brief}\n",
        role = role.trim().replace('"', "'")
    )
}

/// Everything appended to the role prompt at start: the brief (generated,
/// with the role file layered on), then what the team remembers.
pub fn start_extra(cfg: &crate::memory::MemoryConfig, session: &str, name: &str, role: Option<&str>, cwd: Option<&str>) -> String {
    let (brief, _) = effective_brief(session, name, role, cwd);
    // An agent switched to another provider starts with the hand-over note.
    let brief = match crate::handoff::pending(session, name) {
        Some(h) if brief.is_empty() => h,
        Some(h) => format!("{brief}\n\n{h}"),
        None => brief,
    };
    let recall = crate::memory::recall_block(cfg, session, &crate::memory::start_query(name, role, cwd));
    match (brief.is_empty(), recall.is_empty()) {
        (true, _) => recall,
        (_, true) => brief,
        _ => format!("{brief}\n\n{recall}"),
    }
}

#[cfg(test)]
mod brief_tests {
    use super::*;

    #[test]
    fn orchestrator_is_told_to_delegate_and_who_to() {
        let team = vec![("lead".to_string(), Some("orchestrator".to_string())), ("dev".to_string(), Some("software engineer".to_string())), ("omp-1".to_string(), Some("reviewer".to_string()))];
        let s = start_brief("lead", Some("orchestrator"), &team);
        assert!(s.contains("not to do the work yourself"));
        assert!(s.contains("Engineers available now: dev."));
        assert!(s.contains("Reviewer: omp-1."));
        assert!(s.contains("Your team right now: dev (software engineer), omp-1 (reviewer)."));
        assert!(!s.contains("lead ("), "does not list itself");
    }

    #[test]
    fn orchestrator_prefers_linked_engineers() {
        let team = vec![("lead".to_string(), Some("orchestrator".to_string())), ("dev".to_string(), Some("software engineer".to_string())), ("dev2".to_string(), Some("backend engineer".to_string())), ("rev".to_string(), Some("reviewer".to_string()))];
        let s = start_brief_linked("lead", Some("orchestrator"), &team, &["dev2".to_string(), "rev".to_string()]);
        assert!(s.contains("Engineers available now: dev2."), "{s}");
        assert!(s.contains("Linked to you on the canvas: dev2, rev"));
        assert!(s.contains("hubcodes refuses those tools"));
        let s = start_brief_linked("lead", Some("orchestrator"), &team, &[]);
        assert!(s.contains("Engineers available now: dev, dev2."));
        assert!(!s.contains("Linked to you"));
    }

    #[test]
    fn a_teammate_checks_untagged_human_messages_with_its_lead() {
        let team = vec![("lead".to_string(), Some("orchestrator".to_string())), ("dev".to_string(), Some("software engineer".to_string()))];
        let s = start_brief_linked("dev", Some("software engineer"), &team, &["lead".to_string()]);
        assert!(s.contains("without a [from <name>] tag"));
        assert!(s.contains("hubcodes send lead"));
        let alone = start_brief_linked("dev", Some("software engineer"), &team, &["reviewer".to_string()]);
        assert!(!alone.contains("without a [from <name>] tag"), "not linked to the lead");
        let lead = start_brief_linked("lead", Some("orchestrator"), &team, &["dev".to_string()]);
        assert!(!lead.contains("without a [from <name>] tag"));
    }

    #[test]
    fn engineer_reports_back_to_the_orchestrator() {
        let team = vec![("lead".to_string(), Some("orchestrator".to_string())), ("dev".to_string(), Some("software engineer".to_string()))];
        let s = start_brief_linked("dev", Some("software engineer"), &team, &["lead".to_string()]);
        assert!(s.contains("hubcodes send lead \"done:"));
        assert!(!s.contains("not to do the work"));
        // Not linked to the lead: no lead to report to or clear requests with.
        let free = start_brief("rv", Some("reviewer"), &team);
        assert!(!free.contains("to lead") && !free.contains("send lead") && !free.contains("without a [from <name>] tag"), "{free}");
        assert!(free.contains("<whoever asked>"), "{free}");
        // Any name, and not only the first orchestrator of the session.
        let two = vec![("alpha".to_string(), Some("orchestrator".to_string())), ("koordinator".to_string(), Some("Lead".to_string())), ("dev".to_string(), Some("software engineer".to_string()))];
        let s = start_brief_linked("dev", Some("software engineer"), &two, &["koordinator".to_string()]);
        assert!(s.contains("hubcodes send koordinator \"done:") && !s.contains("alpha \""), "{s}");
    }

    #[test]
    fn no_role_means_worker_and_no_team_means_no_roster() {
        let s = start_brief("solo", None, &[]);
        assert!(s.starts_with("Do the work"));
        assert!(!s.contains("Your team right now"));
    }
}

#[cfg(test)]
mod role_file_tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hubcodes-roles-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn slug() {
        assert_eq!(role_slug("Software Engineer"), "software-engineer");
        assert_eq!(role_slug("  QA / tester  "), "qa-tester");
        assert_eq!(role_slug("orchestrator"), "orchestrator");
        assert_eq!(role_slug("front-end!!"), "front-end");
        assert_eq!(role_slug("---"), "");
        assert_eq!(role_slug(""), "");
    }

    #[test]
    fn no_file_leaves_the_brief_alone() {
        let repo = tmp("none");
        let user = tmp("none-user");
        assert!(role_file_in("reviewer", &[repo.clone(), user.clone()]).is_none());
        assert!(role_file_in("", std::slice::from_ref(&repo)).is_none(), "empty slug never reads a file");
        let brief = start_brief("r", Some("reviewer"), &[]);
        assert_eq!(layer_role_file(&brief, None), brief);
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&user);
    }

    #[test]
    fn file_extends_and_repo_wins_over_user() {
        let repo = tmp("extend");
        let user = tmp("extend-user");
        std::fs::write(user.join("software-engineer.md"), "<!-- note -->\nUser text.\n").unwrap();
        let brief = start_brief("dev", Some("Software Engineer"), &[]);
        let (p, t) = role_file_in("Software Engineer", &[repo.clone(), user.clone()]).unwrap();
        assert_eq!(p, user.join("software-engineer.md"));
        assert_eq!(layer_role_file(&brief, Some(&t)), format!("{brief}\n\nUser text."), "comments stripped, text appended");
        std::fs::write(repo.join("software-engineer.md"), "Repo text.").unwrap();
        let (p, t) = role_file_in("Software Engineer", &[repo.clone(), user.clone()]).unwrap();
        assert_eq!(p, repo.join("software-engineer.md"));
        let out = layer_role_file(&brief, Some(&t));
        assert!(out.starts_with(&brief) && out.ends_with("\n\nRepo text."));
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&user);
    }

    #[test]
    fn replace_marker_replaces_and_only_comments_leave_the_brief() {
        let brief = start_brief("lead", Some("orchestrator"), &[]);
        let text = format!("{REPLACE_MARKER}\n<!-- how-to -->\n\nOnly this.\n");
        assert!(replaces(&text));
        assert_eq!(layer_role_file(&brief, Some(&text)), "Only this.");
        assert_eq!(layer_role_file(&brief, Some("<!-- just a comment -->")), brief);
        assert!(!replaces("text first\n<!-- hubcodes: replace -->"), "marker must be first");
    }

    #[test]
    fn template_replaces_and_carries_the_generated_brief() {
        let t = role_template("Reviewer");
        assert!(replaces(&t));
        let out = layer_role_file("generated", Some(&t));
        assert_eq!(out, default_job("reviewer"), "the template is the default job text, ready to edit");
        assert!(!out.contains("<!--"));
    }

    #[test]
    fn a_replaced_job_keeps_the_live_team_facts() {
        let team = vec![("lead".to_string(), Some("orchestrator".to_string())), ("dev".to_string(), Some("software engineer".to_string()))];
        let s = start_brief_with("rv", Some("reviewer"), &team, &["lead".to_string()], Some("Review only migrations. Tell {lead} when done.".into()));
        assert!(s.starts_with("Review only migrations. Tell lead when done."), "{s}");
        assert!(s.contains("Your team right now: lead (orchestrator), dev (software engineer)"), "{s}");
        assert_eq!(start_brief("rv", Some("reviewer"), &team), start_brief_with("rv", Some("reviewer"), &team, &[], Some(default_job("reviewer"))), "the default text is what a role gets without a file");
    }

    #[test]
    fn an_agent_knows_its_device_and_how_to_see_the_others() {
        let d = devices_note("mini", "lead", &["mac"]).unwrap();
        assert!(d.contains("You run on the device mini"), "{d}");
        assert!(d.contains("to them you are mini/lead"), "{d}");
        assert!(d.contains("hubcodes peers"), "{d}");
        assert!(devices_note("mini", "lead", &[]).is_none());
    }
}
