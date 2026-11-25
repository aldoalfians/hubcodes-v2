//! Housekeeping every client runs on its poll: hand out tasks to idle
//! agents, remind agents that owe a loop reply, pause loops over budget, fire
//! due triggers. The desktop app calls [`tend`] every few seconds; `hubcodes
//! watch` does the same from a terminal, so a headless machine behaves the
//! same as one with the app open.

use crate::db::Db;
use crate::providers::Registry;
use crate::relay;
use crate::tasks;
use crate::tmux::Tmux;
use crate::triggers::{self, Kind, Trigger};
use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// An agent as the caller sees it right now.
#[derive(Clone, Debug)]
pub struct AgentState {
    pub name: String,
    pub role: String,
    /// idle | working | waiting | offline
    pub status: String,
    pub quiet_secs: f32,
    pub remote: bool,
    pub dead: bool,
    /// Last non-empty screen line (for repeated-error detection).
    pub last_line: String,
    /// Folder whose diff is watched for stale loops.
    pub dir: String,
}

/// Something worth telling the user.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub kind: String,
    pub text: String,
}

/// Memory between polls (what was already nudged, dispatched, fired).
#[derive(Default, Debug)]
pub struct Memory {
    pub budget_hit: HashSet<String>,
    pub cron_fired: HashMap<String, i64>,
    pub prs_seen: HashMap<String, HashSet<u64>>,
    pub prs_primed: HashSet<String>,
    pub pr_checked_at: HashMap<String, i64>,
    /// Alerts already raised (agent or loop key → what), so each fires once per episode.
    pub alerted: HashMap<String, String>,
    /// Same last line seen N polls in a row.
    pub repeat: HashMap<String, (String, u32)>,
    /// Loop key → (rounds when the diff last changed, diff fingerprint).
    pub loop_diff: HashMap<String, (u32, u64)>,
    pub dead_seen: HashSet<String>,
    pub def_saved_at: i64,
    /// Tracker name → last pull (unix).
    pub tracker_polled: HashMap<String, i64>,
    /// Agent → screen hash the policy already answered.
    pub policy_seen: HashMap<String, u64>,
    /// When `[skills] sync` last ran (unix).
    pub skills_synced: i64,
    /// Provider or provider:model that ran out → skipped until (unix).
    pub resting: HashMap<String, i64>,
    /// Agent → when it was last moved by failover (unix).
    pub failed_over: HashMap<String, i64>,
    /// Agent → (what to tell it, when it was moved), sent once it has started.
    pub carry_on: HashMap<String, (String, i64)>,
    /// Role → when auto team last added an agent for it (unix).
    pub hired_at: HashMap<String, i64>,
    /// Extra agent → (screen fingerprint, since when it has not changed).
    pub idle_since: HashMap<String, (u64, i64)>,
    /// Agent → screen fingerprint that showed a limit on the last poll.
    pub limit_seen: HashMap<String, u64>,
    /// Pane → when it was first seen (unix), for the brief sent as a message.
    pub first_seen: HashMap<String, i64>,
}

/// Seconds an agent must sit idle before a reminder (the idle-timer path;
/// hooks remind at once through `relay::turn_finished`).
pub const NUDGE_AFTER_SECS: f32 = 20.0;

/// The agents of a session as a headless tender sees them: a fresh hook
/// report is the status, otherwise `idle` (no screen-change tracking here,
/// so a tender without hooks never mistakes a thinking agent for a stuck
/// one). Used by `hubcodes watch` and the daemon.
fn local_agent_states(t: &Tmux) -> Vec<AgentState> {
    t.panes()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.provider.is_some() && p.provider.as_deref() != Some("proxy"))
        .map(|p| {
            let fresh = p.hook_at.map(|at| crate::db::now() - at < crate::hooks::FRESH_SECS).unwrap_or(false);
            let last_line = t.capture(&p.id, 12).unwrap_or_default().iter().rev().find(|l| !l.trim().is_empty()).map(|l| crate::policy::strip_ansi(l)).unwrap_or_default();
            AgentState {
                dir: p.worktree.clone().unwrap_or_default(),
                dead: p.dead || p.exited,
                last_line,
                name: p.name,
                role: p.role.unwrap_or_default(),
                status: if p.dead || p.exited { "offline".into() } else if fresh { p.hook_status.unwrap_or_else(|| "idle".into()) } else { "idle".into() },
                quiet_secs: 30.0,
                remote: false,
            }
        })
        .collect()
}

/// Ask the other devices again when the cache is older than the adaptive
/// interval (3 s busy, 15 s quiet, 60 s unreachable). The app has its own
/// poll thread; `hubcodes watch` and hubcodesd call this every pass.
pub fn refresh_peers_if_due(session: &str, reg: &Registry) {
    if reg.peers.is_empty() {
        return;
    }
    let cache = crate::remote::cached(session);
    let wait = crate::remote::next_poll_secs(cache.as_ref(), cache.is_none()) as i64;
    if cache.as_ref().map(|c| crate::db::now() - c.at >= wait).unwrap_or(true) {
        crate::remote::refresh_cache(session, &reg.peers);
    }
}

/// Agents on this device (hook status when fresh, else idle) plus the other
/// devices' agents from the peer cache, so `hubcodes watch` and the daemon
/// hand tasks across devices the way the app does.
pub fn agent_states(t: &Tmux) -> Vec<AgentState> {
    let mut out = local_agent_states(t);
    for a in crate::remote::cached_agents(&t.session) {
        out.push(AgentState { name: a.name, role: a.role.unwrap_or_default(), status: a.status, quiet_secs: a.quiet_secs, remote: true, dead: a.dead, last_line: String::new(), dir: String::new() });
    }
    out
}

pub fn tend(t: &Tmux, reg: &Registry, agents: &[AgentState], mem: &mut Memory) -> Vec<Event> {
    let mut events = Vec::new();
    for (from, to) in crate::relay::flush_outbox(t).unwrap_or_default() {
        events.push(Event { kind: "delivery".into(), text: format!("Delivered the queued message from {from} to {to}") });
    }
    // Reports for another device's board that waited for it to come back.
    if !reg.peers.is_empty() {
        for f in crate::remote::flush_queue(&t.session, &reg.peers) {
            match f {
                crate::remote::Flushed::Delivered(c) => events.push(Event { kind: "delivery".into(), text: format!("Delivered to {}: hubcodes {}", c.peer, c.args.iter().take(3).cloned().collect::<Vec<_>>().join(" ")) }),
                crate::remote::Flushed::Expired(c) => events.push(Event { kind: "error".into(), text: format!("Gave up on {} after two days: hubcodes {} ({})", c.peer, c.args.iter().take(3).cloned().collect::<Vec<_>>().join(" "), c.error) }),
            }
        }
    }
    events.extend(nudge(t, agents).unwrap_or_default());
    events.extend(dispatch(t, reg, agents).unwrap_or_default());
    events.extend(budget(t, reg, agents, mem).unwrap_or_default());
    events.extend(fire_triggers(t, reg, mem));
    events.extend(merges(t));
    events.extend(alerts(t, reg, agents, mem));
    events.extend(auto_answer(t, reg, agents, mem));
    events.extend(failover(t, reg, agents, mem));
    events.extend(autoteam(t, reg, agents, mem));
    events.extend(brief_by_message(t, reg, agents, mem));
    crate::playbook::continue_after_tests(t, reg);
    events.extend(crate::playbook::tick(t, reg));
    // Skills added anywhere (a pull, a download, another tool) reach every tool within a minute.
    if reg.skills.sync && crate::db::now() - mem.skills_synced >= 60 {
        mem.skills_synced = crate::db::now();
        for c in crate::skills::ensure_builtin().into_iter().chain(crate::skills::sync(&reg.skills)) {
            events.push(Event { kind: "skill".into(), text: format!("Skills: {c}") });
        }
    }
    let tracked = crate::tracker::tick(t, &reg.trackers, &mut mem.tracker_polled, false);
    events.extend(review_news(t, reg, agents, &tracked));
    events.extend(tracked);
    // Session definition for restore after a reboot (cheap: skipped when unchanged).
    let now = crate::db::now();
    if now - mem.def_saved_at >= 15 {
        mem.def_saved_at = now;
        let _ = crate::sessiondef::save(t);
    }
    crate::notify::send(&reg.alerts.webhook, events.iter().filter(|e| e.kind == "alert").map(|e| e.text.clone()).collect());
    events
}

/// One queued merge per tick.
fn merges(t: &Tmux) -> Vec<Event> {
    let Ok(db) = Db::open(&t.session) else { return vec![] };
    match crate::merge::tick(t, &db) {
        Ok(Some(m)) => vec![Event { kind: if m.status == "merged" { "merge".into() } else { "error".into() }, text: format!("Merge {} → {}: {}", m.branch, m.into, m.status.replace('_', " ")) }],
        Ok(None) => vec![],
        Err(e) => vec![Event { kind: "error".into(), text: format!("Merge queue: {e:#}") }],
    }
}

/// Things that are off: idle with a task, a loop going round without the
/// code changing, the same error on screen poll after poll, an agent that
/// exited. Each fires once until the condition clears.
fn alerts(t: &Tmux, reg: &Registry, agents: &[AgentState], mem: &mut Memory) -> Vec<Event> {
    let mut events = Vec::new();
    let cfg = &reg.alerts;
    let db = Db::open(&t.session).ok();
    let doing: HashMap<String, String> = db
        .as_ref()
        .and_then(|d| tasks::list(d, false).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|x| x.status == "doing" && !x.owner.is_empty())
        .map(|x| (x.owner, format!("#{} {}", x.id, x.title)))
        .collect();
    // A task held by an agent on another device that is not reachable now
    // (its Mac is off, or the agent is gone): nothing will come back for it.
    for (owner, task) in doing.iter().filter(|(o, _)| o.contains('/')) {
        let seen = agents.iter().any(|a| a.remote && &a.name == owner && !a.dead);
        if seen {
            mem.alerted.remove(&format!("away:{owner}"));
        } else {
            let device = owner.split('/').next().unwrap_or(owner);
            raise(mem, db.as_ref(), &mut events, format!("away:{owner}"), task, format!("{owner} holds task {task} but {device} is unreachable (or the agent is gone). Reassign it from the board, or wait for {device}."));
        }
    }
    for a in agents.iter().filter(|a| !a.remote) {
        // Idle with a task.
        if cfg.idle_minutes > 0 {
            if let Some(task) = doing.get(&a.name) {
                if a.status == "idle" && a.quiet_secs >= cfg.idle_minutes as f32 * 60.0 {
                    raise(mem, db.as_ref(), &mut events, format!("idle:{}", a.name), task, format!("{} has been idle {} min while holding task {task}. Nudge it or reassign the task.", a.name, cfg.idle_minutes));
                } else if a.status != "idle" {
                    mem.alerted.remove(&format!("idle:{}", a.name));
                }
            }
        }
        // Same error line, poll after poll.
        if cfg.repeated_error_polls > 0 {
            let l = a.last_line.to_lowercase();
            let looks_error = l.contains("error") || l.contains("panic") || l.contains("traceback") || l.contains("exception") || l.contains("failed");
            let count = {
                let entry = mem.repeat.entry(a.name.clone()).or_insert((String::new(), 0));
                if looks_error && entry.0 == a.last_line {
                    entry.1 += 1;
                } else {
                    *entry = (a.last_line.clone(), if looks_error { 1 } else { 0 });
                }
                entry.1
            };
            if count >= cfg.repeated_error_polls {
                let line = a.last_line.chars().take(100).collect::<String>();
                raise(mem, db.as_ref(), &mut events, format!("repeat:{}", a.name), &line, format!("{} keeps showing the same error: {}", a.name, line));
            } else if count == 0 {
                mem.alerted.remove(&format!("repeat:{}", a.name));
            }
        }
        // Exited.
        if a.dead {
            if mem.dead_seen.insert(a.name.clone()) {
                raise(mem, db.as_ref(), &mut events, format!("dead:{}", a.name), "dead", format!("{} exited. Remove it, or restore the session to start it again.", a.name));
            }
        } else {
            mem.dead_seen.remove(&a.name);
            mem.alerted.remove(&format!("dead:{}", a.name));
        }
    }
    // Loops that go round without the diff changing.
    if cfg.stale_loop_rounds > 0 {
        let loops = relay::loops(t);
        for (key, st) in loops.iter().filter(|(_, s)| !s.paused && !s.done && s.rounds > 0) {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for name in key.split('|') {
                if let Some(a) = agents.iter().find(|a| a.name == name) {
                    if !a.dir.is_empty() {
                        if let Ok(c) = crate::git::changes(std::path::Path::new(&a.dir)) {
                            (c.files, c.insertions, c.deletions, c.status).hash(&mut h);
                        }
                    }
                }
            }
            let fp = h.finish();
            let (since, changed) = {
                let e = mem.loop_diff.entry(key.clone()).or_insert((st.rounds, fp));
                if e.1 != fp {
                    *e = (st.rounds, fp);
                    (st.rounds, true)
                } else {
                    (e.0, false)
                }
            };
            if changed {
                mem.alerted.remove(&format!("stale:{key}"));
            } else if st.rounds.saturating_sub(since) >= cfg.stale_loop_rounds {
                raise(mem, db.as_ref(), &mut events, format!("stale:{key}"), "stale", format!("Loop {} went {} rounds without any file changing. It may be talking in circles; check the goal or pause it.", key.replace('|', " ↔ "), st.rounds - since));
            }
        }
    }
    events
}

/// Policy on the screen: an agent that is waiting with a recognisable
/// command gets its yes or no keys when a rule decides it.
fn auto_answer(t: &Tmux, reg: &Registry, agents: &[AgentState], mem: &mut Memory) -> Vec<Event> {
    let mut events = Vec::new();
    for a in agents.iter().filter(|a| !a.remote && !a.dead && a.status == "waiting") {
        let Ok(pane) = t.pane_by_name(&a.name) else { continue };
        let Ok(lines) = t.capture(&pane.id, 14) else { continue };
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        lines.hash(&mut h);
        let fp = h.finish();
        if mem.policy_seen.get(&a.name) == Some(&fp) {
            continue;
        }
        let policy = if a.dir.is_empty() { reg.policy.clone() } else { reg.for_dir(std::path::Path::new(&a.dir)).policy };
        if !policy.is_on() {
            continue;
        }
        let Some(cmd) = crate::policy::command_on_screen(&lines) else { continue };
        let decision = crate::policy::decide_command(&policy, &cmd);
        let approve = match &decision {
            crate::policy::Decision::Allow(_) => true,
            crate::policy::Decision::Deny(_) => false,
            crate::policy::Decision::Ask => continue,
        };
        mem.policy_seen.insert(a.name.clone(), fp);
        let rule = match &decision { crate::policy::Decision::Allow(r) | crate::policy::Decision::Deny(r) => r.clone(), _ => String::new() };
        match crate::relay::answer(t, reg, &a.name, approve, "policy") {
            Ok(()) => {
                if let Ok(db) = Db::open(&t.session) {
                    let _ = db.add_event(&a.name, "policy", &format!("{} `{}` (rule: {rule})", if approve { "allowed" } else { "denied" }, cmd));
                }
                events.push(Event { kind: "policy".into(), text: format!("{}: {} `{}` (rule {rule})", a.name, if approve { "allowed" } else { "denied" }, cmd.chars().take(60).collect::<String>()) });
            }
            Err(e) => events.push(Event { kind: "error".into(), text: format!("Policy could not answer {}: {e:#}", a.name) }),
        }
    }
    events
}

/// Seconds a moved agent gets to start before it is told to carry on, and
/// before its screen is read for a limit again.
const FAILOVER_SETTLE_SECS: i64 = 20;

/// `[failover]`: an agent whose tool says it ran out moves to the next
/// provider or model in the chain (see [`crate::failover`]).
fn failover(t: &Tmux, reg: &Registry, agents: &[AgentState], mem: &mut Memory) -> Vec<Event> {
    let mut events = Vec::new();
    let now = crate::db::now();
    // Moved agents that have had time to start are told to carry on.
    let due: Vec<String> = mem.carry_on.iter().filter(|(_, (_, at))| now - at >= FAILOVER_SETTLE_SECS).map(|(n, _)| n.clone()).collect();
    for name in due {
        if let Some((text, _)) = mem.carry_on.remove(&name) {
            if let Err(e) = relay::deliver(t, reg, "hubcodes", &name, &text) {
                events.push(Event { kind: "error".into(), text: format!("Failover: could not tell {name} to carry on: {e:#}") });
            }
        }
    }
    let cfg = &reg.failover;
    if !cfg.enabled || cfg.chain.is_empty() {
        return events;
    }
    mem.resting.retain(|_, until| *until > now);
    let db = Db::open(&t.session).ok();
    for a in agents.iter().filter(|a| !a.remote && !a.dead && a.status != "working") {
        if mem.failed_over.get(&a.name).map(|at| now - at < FAILOVER_SETTLE_SECS * 3).unwrap_or(false) {
            continue;
        }
        let Ok(pane) = t.pane_by_name(&a.name) else { continue };
        let Some(provider) = pane.provider.clone().filter(|p| p != "shell") else { continue };
        let Ok(lines) = t.capture(&pane.id, 20) else { continue };
        let Some(sign) = crate::failover::limit_hit(&lines) else {
            mem.alerted.remove(&format!("limit:{}", a.name));
            mem.limit_seen.remove(&a.name);
            continue;
        };
        // The same screen on two polls in a row: the tool is stopped on it,
        // not printing it in passing.
        let fp = fingerprint(&lines);
        if mem.limit_seen.insert(a.name.clone(), fp) != Some(fp) {
            continue;
        }
        let model = pane.model.clone().filter(|m| !m.is_empty());
        let from = crate::failover::key(&provider, model.as_deref());
        mem.resting.insert(crate::failover::rests(&provider, model.as_deref(), &sign), now + cfg.rest_minutes.max(1) as i64 * 60);
        let resting = |k: &str| mem.resting.contains_key(k);
        let usable = |p: &str| reg.get(p).map(|x| x.installed()).unwrap_or(false);
        let Some((to_p, to_m)) = crate::failover::next(&cfg.chain, &provider, model.as_deref(), &resting, &usable) else {
            raise(mem, db.as_ref(), &mut events, format!("limit:{}", a.name), &from, format!("{} ran out on {from} ({sign}) and nothing in the failover chain is free. It waits until the limit resets.", a.name));
            continue;
        };
        let to = crate::failover::key(&to_p, to_m.as_deref());
        let (exe, prefix) = cli_exe();
        mem.failed_over.insert(a.name.clone(), now);
        match crate::sessiondef::switch(t, reg, &a.name, &to_p, to_m.as_deref(), &exe, prefix.as_deref()) {
            Ok(s) if s.restored.errors.is_empty() => {
                let text = format!("{} ran out on {from} ({sign}); moved to {to}{}.", a.name, if s.kept == "conversation" { ", same conversation" } else { " with a hand-over note" });
                if let Some(d) = &db {
                    let _ = d.add_event(&a.name, "failover", &text);
                }
                mem.carry_on.insert(a.name.clone(), (crate::failover::carry_on(&from, &to, s.kept == "conversation"), now));
                events.push(Event { kind: "alert".into(), text });
            }
            Ok(s) => events.push(Event { kind: "error".into(), text: format!("Failover of {} to {to}: {}", a.name, s.restored.errors.join("; ")) }),
            Err(e) => events.push(Event { kind: "error".into(), text: format!("Failover of {} to {to}: {e:#}", a.name) }),
        }
    }
    events
}

/// `[autoteam]`: one more agent for a role whose tasks wait while all its
/// agents are busy; extra agents idle past `idle_minutes` with nothing
/// waiting are removed (see [`crate::autoteam`]).
fn autoteam(t: &Tmux, reg: &Registry, agents: &[AgentState], mem: &mut Memory) -> Vec<Event> {
    let cfg = &reg.autoteam;
    if !cfg.enabled {
        return vec![];
    }
    let mut events = Vec::new();
    let now = crate::db::now();
    let Ok(db) = Db::open(&t.session) else { return events };
    let open = tasks::list(&db, false).unwrap_or_default();
    let held = crate::tracker::held(&reg.trackers);
    let busy: HashSet<String> = open.iter().filter(|x| x.status == "doing").map(|x| x.owner.clone()).collect();
    let local: Vec<&AgentState> = agents.iter().filter(|a| !a.remote && !a.dead).collect();
    let extras = crate::autoteam::extras(&t.session);
    // Hire: a role with work waiting and no free agent of its own.
    let mut roles: Vec<&str> = local.iter().map(|a| a.role.as_str()).filter(|r| !r.is_empty()).collect();
    roles.sort();
    roles.dedup();
    for role in roles {
        let waiting = crate::autoteam::waiting(&open, role, &held);
        let free = local.iter().any(|a| a.role == role && a.status == "idle" && !busy.contains(&a.name));
        let cooled = mem.hired_at.get(role).map(|at| now - at >= cfg.cooldown_minutes.max(1) as i64 * 60).unwrap_or(true);
        if waiting == 0 || free || !cooled || cfg.limit(role) == 0 {
            continue;
        }
        mem.hired_at.insert(role.to_string(), now);
        let (exe, prefix) = cli_exe();
        let reason = format!("{waiting} {role} task{} waiting, every {role} busy", if waiting == 1 { "" } else { "s" });
        match crate::autoteam::hire_with(t, reg, role, &reason, "hubcodes", &exe, prefix.as_deref()) {
            Ok(name) => {
                mem.alerted.remove(&format!("autoteam:{role}"));
                events.push(Event { kind: "alert".into(), text: format!("Auto team added {name} ({role}): {reason}.") })
            }
            // Each different reason once, until a hire works again.
            Err(e) => {
                let why = format!("{e:#}");
                raise(mem, Some(&db), &mut events, format!("autoteam:{role}"), &why, format!("Auto team could not add a {role}: {why}"))
            }
        }
    }
    // Release: extra agents idle long enough while their role has nothing
    // waiting. "Idle" also needs a screen that has not changed for the whole
    // window: without hooks a tender sees every agent as idle, and a long
    // build or test must not be cut off.
    for (name, x) in &extras {
        let Some(a) = local.iter().find(|a| &a.name == name) else {
            mem.idle_since.remove(name);
            continue;
        };
        if a.status != "idle" || busy.contains(name) || crate::autoteam::waiting(&open, &x.role, &held) > 0 {
            mem.idle_since.remove(name);
            continue;
        }
        let Some(fp) = t.pane_by_name(name).ok().and_then(|p| t.capture(&p.id, 60).ok()).map(|l| fingerprint(&l)) else { continue };
        let since = match mem.idle_since.get(name) {
            Some((f, s)) if *f == fp => *s,
            _ => {
                mem.idle_since.insert(name.clone(), (fp, now));
                now
            }
        };
        if now - since < cfg.idle_minutes.max(1) as i64 * 60 {
            continue;
        }
        mem.idle_since.remove(name);
        match crate::autoteam::release(t, reg, name) {
            Ok(r) => events.push(Event {
                kind: if r.kept_branch.is_some() { "alert".into() } else { "agent".into() },
                text: match (&r.kept_branch, r.handed_to_task) {
                    (Some(b), Some(id)) => format!("Auto team removed {name}; its unfinished work stays on {b} and went to task #{id}."),
                    _ => format!("Auto team removed {name}: idle, nothing waiting for {}.", x.role),
                },
            }),
            Err(e) => events.push(Event { kind: "error".into(), text: format!("Auto team could not remove {name}: {e:#}") }),
        }
    }
    events
}

/// Tools with no system-prompt flag (gemini, agy) learn their role from their
/// first message: once the pane has been up a while and sits idle, its role
/// prompt and brief go in once, marked on the pane so a restart of this
/// tender never sends it twice.
pub const OPT_BRIEF_SENT: &str = "@hubcodes_brief_sent";
const BRIEF_AFTER_SECS: i64 = 15;

fn brief_by_message(t: &Tmux, reg: &Registry, agents: &[AgentState], mem: &mut Memory) -> Vec<Event> {
    let mut events = Vec::new();
    let now = crate::db::now();
    let Ok(panes) = t.panes() else { return events };
    mem.first_seen.retain(|id, _| panes.iter().any(|p| &p.id == id));
    for p in panes.iter().filter(|p| !p.dead && !p.exited) {
        let (Some(provider), Some(role)) = (p.provider.as_deref(), p.role.as_deref().filter(|r| !r.trim().is_empty())) else { continue };
        let Some(prov) = reg.get(provider) else { continue };
        if prov.role_flag.is_some() || provider == "shell" || provider == "proxy" {
            continue;
        }
        let since = *mem.first_seen.entry(p.id.clone()).or_insert(now);
        if now - since < BRIEF_AFTER_SECS || !t.pane_option(&p.id, OPT_BRIEF_SENT).trim().is_empty() {
            continue;
        }
        if agents.iter().find(|a| a.name == p.name).map(|a| a.status != "idle").unwrap_or(false) {
            continue;
        }
        let cwd = p.worktree.clone().or_else(|| crate::sessiondef::pane_cwd(&p.id));
        let extra = crate::prompts::start_extra(&reg.memory, &t.session, &p.name, Some(role), cwd.as_deref());
        let text = format!("{}\n\nThat is your role for this session; keep to it. Reply with the single word: ok", crate::providers::role_prompt(role, &extra));
        let _ = t.set_pane_option(&p.id, OPT_BRIEF_SENT, &now.to_string());
        match relay::deliver(t, reg, "hubcodes", &p.name, &text) {
            Ok(_) => events.push(Event { kind: "agent".into(), text: format!("{} ({provider}) was told its role as {role}.", p.name) }),
            Err(e) => events.push(Event { kind: "error".into(), text: format!("Could not tell {} its role: {e:#}", p.name) }),
        }
    }
    events
}

/// The hubcodes CLI that agents' hooks call, and its folder for PATH: next
/// to this program (the app bundle ships it), else this program itself.
fn cli_exe() -> (String, Option<String>) {
    let exe = std::env::current_exe().ok().map(|p| p.with_file_name("hubcodes")).filter(|p| p.exists()).map(|p| p.display().to_string()).unwrap_or_else(|| crate::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into()));
    let prefix = std::path::Path::new(&exe).parent().map(|d| d.display().to_string()).filter(|d| !d.is_empty());
    (exe, prefix)
}

fn fingerprint(lines: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    lines.hash(&mut h);
    h.finish()
}

/// Raise an alert once per (key, what); logged as an event.
fn raise(mem: &mut Memory, db: Option<&Db>, events: &mut Vec<Event>, key: String, what: &str, text: String) {
    if mem.alerted.get(&key).map(|w| w == what).unwrap_or(false) {
        return;
    }
    mem.alerted.insert(key, what.into());
    if let Some(d) = db {
        let _ = d.add_event("hubcodes", "alert", &text);
    }
    events.push(Event { kind: "alert".into(), text });
}

fn nudge(t: &Tmux, agents: &[AgentState]) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    let mut aw = relay::awaiting(t);
    let mut changed = false;
    for (who, entry) in aw.iter_mut() {
        if entry.nudged {
            continue;
        }
        let Some(a) = agents.iter().find(|a| &a.name == who && !a.remote) else { continue };
        if a.status == "idle" && a.quiet_secs >= NUDGE_AFTER_SECS {
            if let Ok(p) = t.pane_by_name(who) {
                let text = relay::nudge_text(&entry.to);
                if t.send_text(&p.id, &text).is_ok() {
                    entry.nudged = true;
                    changed = true;
                    let _ = t.record_message_kind("hubcodes", who, &text, "nudge");
                    events.push(Event { kind: "nudge".into(), text: format!("Reminded {who} to answer {}", entry.to) });
                }
            }
        }
    }
    if changed {
        relay::set_awaiting(t, &aw)?;
    }
    Ok(events)
}

/// Unassigned `todo` tasks go to idle agents, role match first: the ones on
/// this device, then idle agents on the other devices (over SSH, with the
/// task id prefixed by this device's name so their `task done` comes back).
fn dispatch(t: &Tmux, reg: &Registry, agents: &[AgentState]) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    let db = Db::open(&t.session)?;
    let open = tasks::list(&db, false)?;
    if !open.iter().any(|x| x.status == "todo" && x.owner.is_empty()) {
        return Ok(events);
    }
    let held = crate::tracker::held(&reg.trackers);
    let busy: HashSet<String> = open.iter().filter(|x| x.status == "doing").map(|x| x.owner.clone()).collect();
    // A WIP limit on Doing holds the queue: nothing more is handed out while it is full.
    let wip = reg.board.wip_of("doing") as usize;
    let mut doing = open.iter().filter(|x| x.status == "doing").count();
    let idle = |a: &&AgentState| !a.dead && a.status == "idle" && a.quiet_secs >= 5.0 && !busy.contains(&a.name);
    for a in agents.iter().filter(idle).filter(|a| !a.remote) {
        if wip > 0 && doing >= wip {
            break;
        }
        let Some(task) = tasks::next_for(&db, &a.role, &held)? else { continue };
        let Ok(pane) = t.pane_by_name(&a.name) else { continue };
        let text = tasks::assignment_text(&task, &task.created_by);
        let payload = if pane.provider.as_deref().unwrap_or("shell") == "shell" { text.clone() } else { format!("[from hubcodes] {text}") };
        if t.send_text(&pane.id, &payload).is_ok() {
            tasks::assign(&db, task.id, &a.name)?;
            doing += 1;
            let _ = t.record_message_kind("hubcodes", &a.name, &format!("task #{}: {}", task.id, task.title), "task");
            events.push(Event { kind: "task".into(), text: format!("Task #{} “{}” → {}", task.id, task.title, a.name) });
        }
    }
    let home = reg.sharing.name.trim();
    for a in agents.iter().filter(idle).filter(|a| a.remote && a.name.contains('/')) {
        if home.is_empty() || (wip > 0 && doing >= wip) {
            break;
        }
        let Some(task) = tasks::next_for(&db, &a.role, &held)? else { continue };
        let text = tasks::assignment_text_on(&task, &task.created_by, home);
        match relay::deliver(t, reg, "hubcodes", &a.name, &text) {
            Ok(_) => {
                tasks::assign(&db, task.id, &a.name)?;
                doing += 1;
                let _ = t.record_message_kind("hubcodes", &a.name, &format!("task #{}: {}", task.id, task.title), "task");
                events.push(Event { kind: "task".into(), text: format!("Task #{} “{}” → {} (another device)", task.id, task.title, a.name) });
            }
            Err(e) => events.push(Event { kind: "error".into(), text: format!("Could not hand task #{} to {}: {e:#}", task.id, a.name) }),
        }
    }
    Ok(events)
}

/// Tell the orchestrators on this Mac when review requests come or go.
fn review_news(t: &Tmux, reg: &Registry, agents: &[AgentState], tracked: &[Event]) -> Vec<Event> {
    let changes: Vec<String> = tracked.iter().filter(|e| e.kind == "review").map(|e| e.text.clone()).collect();
    if changes.is_empty() {
        return Vec::new();
    }
    let leads: Vec<&AgentState> = agents.iter().filter(|a| !a.dead && !a.remote && crate::prompts::kind(&a.role) == "orchestrator").collect();
    let Ok(db) = Db::open(&t.session) else { return Vec::new() };
    let Some(text) = crate::tracker::review_digest(&db, &reg.trackers, &changes) else { return Vec::new() };
    let mut events = Vec::new();
    for a in leads {
        if let Err(e) = relay::deliver(t, reg, "hubcodes", &a.name, &text) {
            events.push(Event { kind: "error".into(), text: format!("Could not tell {} about review requests: {e:#}", a.name) });
        }
    }
    events
}

/// Pause loops of agents over their token budget, once.
fn budget(t: &Tmux, reg: &Registry, agents: &[AgentState], mem: &mut Memory) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    let per_agent = reg.budget.max_tokens_per_agent;
    let per_loop = reg.budget.max_tokens_per_loop;
    if per_agent <= 0 && per_loop <= 0 {
        return Ok(events);
    }
    let db = Db::open(&t.session)?;
    let usage: HashMap<String, i64> = db.all_usage()?.into_iter().map(|(a, u)| (a, u.input_tokens + u.output_tokens)).collect();
    let mut loops = relay::loops(t);
    let mut changed = false;
    for a in agents {
        let used = *usage.get(&a.name).unwrap_or(&0);
        if per_agent > 0 && used > per_agent && !mem.budget_hit.contains(&a.name) {
            mem.budget_hit.insert(a.name.clone());
            for (key, st) in loops.iter_mut() {
                if key.split('|').any(|n| n == a.name) && !st.paused {
                    st.paused = true;
                    changed = true;
                }
            }
            let _ = db.add_event(&a.name, "budget", &format!("{used} tokens > {per_agent}"));
            events.push(Event { kind: "budget".into(), text: format!("{} used {} tokens (limit {}); its loops are paused", a.name, used, per_agent) });
        }
    }
    if per_loop > 0 {
        for (key, st) in loops.iter_mut() {
            if st.paused || st.done {
                continue;
            }
            let total: i64 = key.split('|').map(|n| *usage.get(n).unwrap_or(&0)).sum();
            if total > per_loop && !mem.budget_hit.contains(key) {
                mem.budget_hit.insert(key.clone());
                st.paused = true;
                changed = true;
                let _ = db.add_event(key, "budget", &format!("{total} tokens > {per_loop}"));
                events.push(Event { kind: "budget".into(), text: format!("Loop {} used {} tokens (limit {}); paused", key.replace('|', " ↔ "), total, per_loop) });
            }
        }
    }
    if changed {
        relay::set_loops(t, &loops)?;
    }
    Ok(events)
}

/// Cron and PR triggers. Webhooks arrive through [`run_action`] from the server.
fn fire_triggers(t: &Tmux, reg: &Registry, mem: &mut Memory) -> Vec<Event> {
    let mut events = Vec::new();
    let Some((mi, h, d, mo, w, unix)) = triggers::local_now() else { return events };
    let minute_key = unix / 60;
    for tr in reg.triggers.iter().filter(|x| x.enabled) {
        match triggers::kind(tr) {
            Some(Kind::Cron(c)) => {
                if c.matches(mi, h, d, mo, w) && mem.cron_fired.get(&tr.name) != Some(&minute_key) {
                    mem.cron_fired.insert(tr.name.clone(), minute_key);
                    events.push(run_action(t, reg, tr, &tr.action));
                }
            }
            Some(Kind::Pr(repo)) => {
                if unix - mem.pr_checked_at.get(&repo).copied().unwrap_or(0) < 60 {
                    continue;
                }
                mem.pr_checked_at.insert(repo.clone(), unix);
                let prs = triggers::open_prs(&repo);
                let seen = mem.prs_seen.entry(repo.clone()).or_default();
                if !mem.prs_primed.contains(&repo) {
                    mem.prs_primed.insert(repo.clone());
                    seen.extend(prs.iter().map(|p| p.0));
                    continue;
                }
                for (n, title, url) in prs {
                    if seen.insert(n) {
                        let action = triggers::fill(&tr.action, &[("number", &n.to_string()), ("title", &title), ("url", &url)]);
                        events.push(run_action(t, reg, tr, &action));
                    }
                }
            }
            _ => {}
        }
    }
    events
}

/// Execute a trigger's action (CLI words) in-process.
pub fn run_action(t: &Tmux, reg: &Registry, tr: &Trigger, action: &str) -> Event {
    let w = triggers::words(action);
    let me = format!("trigger:{}", tr.name);
    let res: Result<String> = (|| {
        match w.as_slice() {
            [cmd, to, text @ ..] if cmd == "send" => {
                let text = text.join(" ");
                relay::deliver(t, reg, &me, to, &text)?;
                Ok(format!("sent to {to}"))
            }
            [l, s, a, b, rest @ ..] if l == "loop" && s == "start" => {
                let (goal, max) = split_max(rest, reg.loop_cfg.max_rounds);
                relay::kickoff(t, reg, &me, a, b, &goal, max)?;
                Ok(format!("loop {a} ↔ {b} started"))
            }
            [tm, s, name, rest @ ..] if tm == "team" && s == "start" => {
                let tpl = reg.templates.iter().find(|x| &x.name == name).ok_or_else(|| anyhow::anyhow!("no template {name}"))?;
                let cwd = rest.first().cloned();
                let exe = crate::hooks::self_exe().unwrap_or_else(|_| "hubcodes".into());
                let st = crate::team::start(t, reg, tpl, cwd.as_deref(), &exe, None);
                Ok(format!("team {name}: {} started, {} skipped", st.spawned.len(), st.skipped.len()))
            }
            [ta, s, title, rest @ ..] if ta == "task" && s == "add" => {
                let db = Db::open(&t.session)?;
                let task = tasks::add(&db, title, &rest.join(" "), "", &me, "")?;
                Ok(format!("task #{} added", task.id))
            }
            _ => Err(anyhow::anyhow!("unknown action: {action}")),
        }
    })();
    match res {
        Ok(s) => {
            if let Ok(db) = Db::open(&t.session) {
                let _ = db.add_event(&tr.name, "trigger", &s);
            }
            Event { kind: "trigger".into(), text: format!("Trigger {}: {s}", tr.name) }
        }
        Err(e) => Event { kind: "error".into(), text: format!("Trigger {} failed: {e:#}", tr.name) },
    }
}

fn split_max(rest: &[String], default: u32) -> (String, u32) {
    let mut goal = Vec::new();
    let mut max = default;
    let mut it = rest.iter();
    while let Some(w) = it.next() {
        if w == "--max" {
            max = it.next().and_then(|m| m.parse().ok()).unwrap_or(default);
        } else {
            goal.push(w.clone());
        }
    }
    (goal.join(" "), max)
}
