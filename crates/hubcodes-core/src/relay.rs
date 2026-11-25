//! Message delivery with loops. One function, [`deliver`], is the path every
//! client uses to move text into an agent: it resolves local or remote
//! targets, prefixes `[from x]`, and when the two agents are joined by a
//! *loop* link it appends a trailer telling the receiver how to answer, and
//! counts the rounds so a loop cannot run away.
//!
//! Loop state lives in tmux session options next to the edges, so the CLI,
//! the TUI and the desktop app see the same numbers:
//!   `@hubcodes_loops`      JSON `{ "a|b": { rounds, max, paused, goal, done } }`
//!   `@hubcodes_awaiting`   JSON `{ "b": { "to": "a", "seq": 12, "nudged": false } }`

use crate::providers::Registry;
use crate::remote::{self, Peer};
use crate::tmux::Tmux;
use crate::db::Db;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const OPT_LOOPS: &str = "@hubcodes_loops";
pub const OPT_AWAITING: &str = "@hubcodes_awaiting";
pub const DEFAULT_MAX_ROUNDS: u32 = 20;
/// An agent ends a loop by starting its reply with this word.
pub const STOP_WORD: &str = "DONE";

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LoopState {
    pub rounds: u32,
    pub max: u32,
    pub paused: bool,
    pub done: bool,
    #[serde(default)]
    pub goal: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Awaiting {
    pub to: String,
    pub seq: u64,
    pub nudged: bool,
}

/// Unordered pair key, so `a→b` and `b→a` share one loop.
pub fn pair_key(a: &str, b: &str) -> String {
    if a <= b { format!("{a}|{b}") } else { format!("{b}|{a}") }
}

pub fn loops(t: &Tmux) -> BTreeMap<String, LoopState> {
    t.get_option(OPT_LOOPS).ok().filter(|s| !s.is_empty()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn set_loops(t: &Tmux, m: &BTreeMap<String, LoopState>) -> Result<()> {
    t.set_option(OPT_LOOPS, &serde_json::to_string(m)?)
}

pub fn awaiting(t: &Tmux) -> BTreeMap<String, Awaiting> {
    t.get_option(OPT_AWAITING).ok().filter(|s| !s.is_empty()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn set_awaiting(t: &Tmux, m: &BTreeMap<String, Awaiting>) -> Result<()> {
    t.set_option(OPT_AWAITING, &serde_json::to_string(m)?)
}

/// Turn the loop between `a` and `b` on (creating the link if needed) or off.
pub fn set_loop(t: &Tmux, a: &str, b: &str, on: bool, max: u32) -> Result<()> {
    let mut m = loops(t);
    let key = pair_key(a, b);
    if on {
        t.add_edge(a, b, "loop")?;
        let st = m.entry(key).or_default();
        st.max = max;
        st.paused = false;
        st.done = false;
    } else {
        m.remove(&key);
    }
    set_loops(t, &m)
}

/// Resume a paused loop with a fresh round counter.
pub fn reset_loop(t: &Tmux, a: &str, b: &str) -> Result<()> {
    let mut m = loops(t);
    if let Some(st) = m.get_mut(&pair_key(a, b)) {
        st.rounds = 0;
        st.paused = false;
        st.done = false;
    }
    set_loops(t, &m)
}

/// Where a message went and what the loop did with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub seq: u64,
    pub target: String,
    pub remote: bool,
    /// The agent was mid-turn; the text waits in the outbox until it is back at its prompt.
    pub queued: bool,
    pub loop_round: Option<(u32, u32)>,
    pub loop_event: Option<LoopEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoopEvent {
    Started,
    Done,
    /// Max rounds reached; the message was delivered without a trailer.
    Paused,
}

fn is_human(from: &str) -> bool {
    from.starts_with("human:")
}

/// `3/20`, or just `3` for a loop without a round limit (`max` 0).
pub fn round_text(round: u32, max: u32) -> String {
    if max == 0 { round.to_string() } else { format!("{round}/{max}") }
}

fn trailer(reply_to: &str, round: u32, max: u32) -> String {
    format!(
        "\n\n[hubcodes loop {}] When you finish this step, send the result to {reply_to} with:\n  hubcodes send {reply_to} \"<your result>\"\nIf the goal is fully met, send {reply_to} the single word {STOP_WORD} instead.",
        round_text(round, max)
    )
}

/// Deliver `text` from `from` to `to` (an agent name, or `peer/agent` on
/// another device). `from` is an agent name, `human:<user>`, or
/// `<device>/<agent>` when it came over SSH.
/// The orchestrator a person's message to `target` goes through first, or
/// `None` when it goes straight there: routing is off, the target leads
/// itself, is a plain shell, or no orchestrator is running in the session.
pub fn route_for(t: &Tmux, reg: &Registry, target: &str) -> Option<String> {
    if !reg.workspace.route_through_orchestrator {
        return None;
    }
    let leads = |role: &Option<String>| role.as_deref().is_some_and(|r| crate::prompts::kind(r) == "orchestrator");
    let panes = t.panes().ok()?;
    if target.contains('/') {
        // Another device's agent: its role is only known while the peer
        // cache is fresh, so route unless it is known to lead or be a shell.
        if let Some(agent) = remote::cached_agents(&t.session).into_iter().find(|a| a.name == target) {
            if leads(&agent.role) || agent.provider == "shell" {
                return None;
            }
        }
    } else {
        let p = panes.iter().find(|p| p.name == target)?;
        if leads(&p.role) || p.provider.as_deref().is_none_or(|v| v == "shell" || v == "proxy") {
            return None;
        }
    }
    let live: Vec<String> = panes.iter().filter(|p| !p.dead && leads(&p.role)).map(|p| p.name.clone()).collect();
    lead_in_group(target, &t.edges().unwrap_or_default(), &live)
}

/// The orchestrator among the agents `target` is connected to on the
/// canvas, through any chain of lines. An agent with no line to a lead is
/// on its own, and a person talks to it directly.
pub fn lead_in_group(target: &str, edges: &[(String, String, String)], leads: &[String]) -> Option<String> {
    let mut seen = vec![target.to_string()];
    let mut i = 0;
    while i < seen.len() {
        let at = seen[i].clone();
        for (a, b, _) in edges {
            let next = if *a == at { b } else if *b == at { a } else { continue };
            if !seen.contains(next) {
                seen.push(next.clone());
            }
        }
        i += 1;
    }
    seen.into_iter().skip(1).filter(|n| leads.contains(n)).min()
}

/// A person's message to `target`, which is on `lead`'s team: it stays with
/// `target`, with a note to check with the lead before taking it on, so the
/// two talk it over instead of the agent acting on its own.
pub fn routed_text(lead: &str, text: &str) -> String {
    format!(
        "{text}\n\n[hubcodes] You are on {lead}'s team. Before you act on this, tell {lead} what was asked and what you plan with `hubcodes send {lead} \"…\"`, and wait for its go-ahead. If it is outside your role, hand it to {lead} instead of doing it. A plain question about your own work you may answer straight away, then tell {lead} in one line."
    )
}

/// A person's message: to `to`, with the note from [`routed_text`] unless
/// `direct` or [`route_for`] finds no orchestrator `to` is linked to.
/// Returns the delivery and, when noted, that orchestrator.
pub fn deliver_from_person(t: &Tmux, reg: &Registry, from: &str, to: &str, text: &str, direct: bool, immediate: bool) -> Result<(Delivery, Option<String>)> {
    if !direct {
        if let Some(lead) = route_for(t, reg, to) {
            let d = deliver_inner(t, reg, from, to, &routed_text(&lead, text), immediate, false)?;
            return Ok((d, Some(lead)));
        }
    } else if let Ok(p) = t.pane_by_name(to) {
        // Sent past the lead on purpose: the agent's hook lets this one through.
        let _ = t.set_pane_option(&p.id, crate::checkin::OPT_DIRECT_ONCE, "1");
    }
    Ok((deliver_inner(t, reg, from, to, text, immediate, direct)?, None))
}

pub fn deliver(t: &Tmux, reg: &Registry, from: &str, to: &str, text: &str) -> Result<Delivery> {
    deliver_with(t, reg, from, to, text, false)
}

/// `immediate`: type it in even when the agent is mid-turn (`hubcodes send --now`).
pub fn deliver_with(t: &Tmux, reg: &Registry, from: &str, to: &str, text: &str, immediate: bool) -> Result<Delivery> {
    deliver_inner(t, reg, from, to, text, immediate, false)
}

/// `direct`: a person's message to an agent on another device that skips
/// that device's lead too (`--direct` travels with it).
fn deliver_inner(t: &Tmux, reg: &Registry, from: &str, to: &str, text: &str, immediate: bool, direct: bool) -> Result<Delivery> {
    let text = text.trim();
    if text.is_empty() {
        return Err(anyhow!("empty message"));
    }
    // A reply to a fan-out is counted (and the collector told when all are in).
    if !is_human(from) {
        if let Err(e) = crate::fanout::observe(t, reg, from, text) {
            eprintln!("[hubcodes] fan-out bookkeeping: {e:#}");
        }
    }
    let mut loops_map = loops(t);
    let key = pair_key(from, to);
    let mut loop_round = None;
    let mut loop_event = None;
    let mut body = text.to_string();
    let stop = text.split_whitespace().next().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).eq_ignore_ascii_case(STOP_WORD)).unwrap_or(false);

    // Loops are between agents; a human's kickoff goes through `kickoff`.
    if let Some(st) = loops_map.get_mut(&key).filter(|_| !is_human(from)) {
        if stop {
            st.done = true;
            loop_event = Some(LoopEvent::Done);
        } else if st.paused || st.done {
            // Delivered plainly; nobody is told to keep going.
        } else {
            st.rounds += 1;
            // 0 is a loop without a round limit: only DONE, a pause by
            // hand or a token budget stops it.
            let max = st.max;
            if max > 0 && st.rounds > max {
                st.paused = true;
                loop_event = Some(LoopEvent::Paused);
            } else {
                body.push_str(&trailer(from, st.rounds, max));
            }
            loop_round = Some((if max > 0 { st.rounds.min(max) } else { st.rounds }, max));
        }
        set_loops(t, &loops_map)?;
    }

    // Remote target: hand the whole thing to the peer's hubcodes over SSH.
    if let Some((peer_name, agent)) = to.split_once('/') {
        let peer = reg.peers.iter().find(|p| p.name == peer_name).ok_or_else(|| anyhow!("unknown device '{peer_name}'; add a [[peer]] in hubcodes.toml"))?;
        let from_remote = if is_human(from) { from.to_string() } else { format!("{}/{from}", reg.sharing.name) };
        let direct = direct && is_human(from);
        if let Err(e) = remote::send(peer, agent, &body, &from_remote, direct) {
            // The device is away: keep the message, every tend pass retries
            // it (for two days), the agent there gets it at its prompt.
            let seq = t.record_message_kind(from, to, text, "queued")?;
            remote::queue(&t.session, peer_name, &remote::send_args(agent, &body, &from_remote, direct), &format!("{e:#}")).with_context(|| format!("send to {to} via {}", peer.host))?;
            note_awaiting(t, from, to, seq, loop_round.is_some() && loop_event.is_none());
            return Ok(Delivery { seq, target: to.into(), remote: true, queued: true, loop_round, loop_event });
        }
        let seq = t.record_message(from, to, text)?;
        note_awaiting(t, from, to, seq, loop_round.is_some() && loop_event.is_none());
        return Ok(Delivery { seq, target: to.into(), remote: true, queued: false, loop_round, loop_event });
    }

    let pane = live_pane(t, to)?;
    crate::checkin::on_delivery(t, &pane.id, from);
    // A human's request to an orchestrator carries the delegation rule with
    // it, so it holds whatever prompt the agent was started with.
    if is_human(from) {
        if let Some(r) = crate::prompts::delegation_reminder(&t.session, to) {
            body.push_str("\n\n");
            body.push_str(&r);
        }
    }
    let payload = if pane.provider.as_deref().unwrap_or("shell") == "shell" { body.clone() } else { format!("[from {from}] {body}") };
    if !immediate && is_busy(&pane) {
        // Typed into a running turn the text lands inside a tool call or gets
        // lost; it waits for the prompt (flushed by the hook and by every tick).
        let seq = t.record_message_kind(from, to, text, "queued")?;
        if let Ok(db) = Db::open(&t.session) {
            db.outbox_add(seq as i64, from, to, &payload)?;
        }
        note_awaiting(t, from, to, seq, loop_round.is_some() && loop_event.is_none());
        return Ok(Delivery { seq, target: pane.id, remote: false, queued: true, loop_round, loop_event });
    }
    t.send_text(&pane.id, &payload)?;
    let seq = t.record_message(from, to, text)?;
    note_awaiting(t, from, to, seq, loop_round.is_some() && loop_event.is_none());
    Ok(Delivery { seq, target: pane.id, remote: false, queued: false, loop_round, loop_event })
}

/// Mid-turn according to the agent's own hooks (fresh `working` status).
/// Providers without hooks never look busy, so they get the text at once.
fn is_busy(pane: &crate::tmux::Pane) -> bool {
    match (pane.hook_status.as_deref(), pane.hook_at) {
        (Some("working"), Some(at)) => crate::db::now() - at < BUSY_MAX_SECS,
        _ => false,
    }
}

/// A `working` status older than this is treated as stale (the hook missed
/// the end of the turn); queued text is delivered anyway.
pub const BUSY_MAX_SECS: i64 = 15 * 60;

/// Deliver queued messages whose agent is back at its prompt. Returns the
/// (from, to) pairs delivered, for the activity feed.
pub fn flush_outbox(t: &Tmux) -> Result<Vec<(String, String)>> {
    let db = Db::open(&t.session)?;
    let pending = db.outbox_pending()?;
    if pending.is_empty() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for item in pending {
        let pane = match live_pane(t, &item.to) {
            Ok(p) => p,
            Err(e) => {
                db.outbox_mark(item.id, "failed", &format!("{e:#}"))?;
                db.set_message_kind(item.seq, "failed")?;
                let _ = db.add_event(&item.to, "delivery", &format!("could not deliver a message from {}: {e:#}", item.from));
                continue;
            }
        };
        if is_busy(&pane) {
            continue;
        }
        match t.send_text(&pane.id, &item.payload) {
            Ok(()) => {
                db.outbox_mark(item.id, "delivered", "")?;
                db.set_message_kind(item.seq, "message")?;
                // Re-announce so every canvas pulses the link now, not at enqueue time.
                let one_line = item.payload.replace(['\n', '\t'], " ");
                let _ = t.set_option(crate::tmux::OPT_LAST_MSG, &format!("{}\t{}\t{}\t{}", item.seq, item.from, item.to, one_line));
                out.push((item.from.clone(), item.to.clone()));
            }
            Err(e) => {
                db.outbox_mark(item.id, if item.attempts >= 5 { "failed" } else { "queued" }, &format!("{e:#}"))?;
            }
        }
    }
    Ok(out)
}

/// A message from `from` closes `from`'s own pending reply; a loop message
/// with a trailer opens one for `to`.
fn note_awaiting(t: &Tmux, from: &str, to: &str, seq: u64, expects_reply: bool) {
    let mut m = awaiting(t);
    let mut changed = m.remove(from).is_some();
    if expects_reply {
        m.insert(to.to_string(), Awaiting { to: from.to_string(), seq, nudged: false });
        changed = true;
    }
    if changed {
        let _ = set_awaiting(t, &m);
    }
}

/// Start a loop between `a` and `b` with `goal`: `a` gets the goal and is
/// told to report to `b`; from then on they alternate until one says DONE
/// or the round limit pauses it.
pub fn kickoff(t: &Tmux, reg: &Registry, human: &str, a: &str, b: &str, goal: &str, max: u32) -> Result<Delivery> {
    set_loop(t, a, b, true, max)?;
    let mut m = loops(t);
    let key = pair_key(a, b);
    let st = m.entry(key.clone()).or_default();
    st.rounds = 0;
    st.paused = false;
    st.done = false;
    st.goal = goal.to_string();
    st.max = max;
    set_loops(t, &m)?;
    let body = format!("{goal}{}", trailer(b, 0, max));
    let res = deliver_plain(t, reg, human, a, &body, goal)?;
    let mut aw = awaiting(t);
    aw.insert(a.to_string(), Awaiting { to: b.to_string(), seq: res.seq, nudged: false });
    set_awaiting(t, &aw)?;
    Ok(Delivery { loop_round: Some((0, max)), loop_event: Some(LoopEvent::Started), ..res })
}

/// The agent's pane, refusing one whose agent quit: its pane is a plain
/// shell now, and typing a message there would run it as a command.
fn live_pane(t: &Tmux, name: &str) -> Result<crate::tmux::Pane> {
    let pane = t.pane_by_name(name)?;
    if pane.exited {
        return Err(anyhow!("{name} is not running (its agent exited); restore the session or start it again"));
    }
    Ok(pane)
}

/// Deliver without any loop handling (used by kickoff, which built its own trailer).
fn deliver_plain(t: &Tmux, reg: &Registry, from: &str, to: &str, body: &str, logged: &str) -> Result<Delivery> {
    if let Some((peer_name, agent)) = to.split_once('/') {
        let peer: &Peer = reg.peers.iter().find(|p| p.name == peer_name).ok_or_else(|| anyhow!("unknown device '{peer_name}'"))?;
        remote::send(peer, agent, body, from, false)?;
        let seq = t.record_message(from, to, logged)?;
        return Ok(Delivery { seq, target: to.into(), remote: true, queued: false, loop_round: None, loop_event: None });
    }
    let pane = live_pane(t, to)?;
    let payload = if pane.provider.as_deref().unwrap_or("shell") == "shell" { body.to_string() } else { format!("[from {from}] {body}") };
    t.send_text(&pane.id, &payload)?;
    let seq = t.record_message(from, to, logged)?;
    Ok(Delivery { seq, target: pane.id, remote: false, queued: false, loop_round: None, loop_event: None })
}

/// Paste text into a local agent from hubcodes itself (no loop bookkeeping).
pub fn deliver_text(t: &Tmux, to: &str, text: &str) -> Result<()> {
    let pane = live_pane(t, to)?;
    let payload = if pane.provider.as_deref().unwrap_or("shell") == "shell" { text.to_string() } else { format!("[from hubcodes] {text}") };
    t.send_text(&pane.id, &payload)?;
    let _ = t.record_message_kind("hubcodes", to, text, "task");
    Ok(())
}

/// The agent's turn ended (hook). If it still owes a loop reply and has not
/// been reminded, remind it now instead of waiting for the idle timer.
pub fn turn_finished(t: &Tmux, agent: &str) -> Result<bool> {
    // Anything that waited for this prompt goes first.
    if let Ok(d) = flush_outbox(t) {
        if d.iter().any(|(_, to)| to == agent) {
            return Ok(false);
        }
    }
    let mut aw = awaiting(t);
    let Some(entry) = aw.get_mut(agent) else { return Ok(false) };
    if entry.nudged {
        return Ok(false);
    }
    let Ok(pane) = live_pane(t, agent) else { return Ok(false) };
    let text = nudge_text(&entry.to);
    t.send_text(&pane.id, &text)?;
    entry.nudged = true;
    set_awaiting(t, &aw)?;
    let _ = t.record_message_kind("hubcodes", agent, &text, "nudge");
    Ok(true)
}

/// Answer an agent's permission prompt: local pane keys, or the peer's
/// `hubcodes approve|deny` over SSH for `device/agent`.
pub fn answer(t: &Tmux, reg: &Registry, agent: &str, approve: bool, by: &str) -> Result<()> {
    if let Some((peer_name, name)) = agent.split_once('/') {
        let peer = reg.peers.iter().find(|p| p.name == peer_name).ok_or_else(|| anyhow!("unknown device '{peer_name}'"))?;
        remote::run(peer, &[if approve { "approve" } else { "deny" }, name], std::time::Duration::from_secs(15))?;
        return Ok(());
    }
    let pane = live_pane(t, agent)?;
    let provider = pane.provider.clone().unwrap_or_else(|| "shell".into());
    let keys = reg.get(&provider).map(|p| p.keys_for(approve)).unwrap_or_else(|| crate::providers::default_keys(&provider).0);
    t.send_keys(&pane.id, &keys)?;
    // Clear the hook status so the card stops saying "needs you" at once.
    let _ = t.set_pane_option(&pane.id, crate::hooks::OPT_STATUS, "working");
    let _ = t.set_pane_option(&pane.id, crate::hooks::OPT_STATUS_AT, &crate::db::now().to_string());
    if let Ok(db) = crate::db::Db::open(&t.session) {
        let _ = db.add_event(agent, "approval", &format!("{} by {by}", if approve { "approved" } else { "denied" }));
    }
    Ok(())
}

/// Text pasted into an agent that owes a reply and went quiet: one reminder.
pub fn nudge_text(reply_to: &str) -> String {
    format!("[hubcodes] {reply_to} is waiting for your answer. Send it with: hubcodes send {reply_to} \"<your result>\"  (or {STOP_WORD} if the goal is met)")
}

#[cfg(test)]
mod tests {

    #[test]
    fn only_an_agent_connected_to_a_lead_is_routed_and_to_that_lead() {
        let e = |a: &str, b: &str| (a.to_string(), b.to_string(), "link".to_string());
        let edges = vec![e("lead", "dev"), e("dev", "reviewer"), e("pm", "qa")];
        let leads = vec!["lead".to_string(), "pm".to_string()];
        assert_eq!(lead_in_group("dev", &edges, &leads).as_deref(), Some("lead"));
        assert_eq!(lead_in_group("reviewer", &edges, &leads).as_deref(), Some("lead"), "through dev");
        assert_eq!(lead_in_group("qa", &edges, &leads).as_deref(), Some("pm"), "its own lead, not the first by name");
        assert_eq!(lead_in_group("solo", &edges, &leads), None, "no line, no detour");
        assert_eq!(lead_in_group("lead", &edges, &leads), None, "a lead is not routed to itself");
    }

    #[test]
    fn a_person_s_message_stays_with_the_agent_and_asks_it_to_check_with_its_lead() {
        let t = routed_text("lead", "bisa ngoding?");
        assert!(t.starts_with("bisa ngoding?\n\n"));
        assert!(t.contains("hubcodes send lead"));
        assert!(t.contains("wait for its go-ahead"));
    }

    use super::*;

    #[test]
    fn pair_key_is_unordered() {
        assert_eq!(pair_key("a", "b"), pair_key("b", "a"));
        assert_eq!(pair_key("mini/x", "a"), "a|mini/x");
    }

    #[test]
    fn trailer_names_the_partner_and_round() {
        let t = trailer("planner", 3, 20);
        assert!(t.contains("loop 3/20"));
        assert!(t.contains("hubcodes send planner"));
        assert!(t.contains(STOP_WORD));
    }

    #[test]
    fn a_loop_without_a_round_limit_shows_only_the_round() {
        assert_eq!(round_text(3, 20), "3/20");
        assert_eq!(round_text(3, 0), "3");
        assert!(trailer("planner", 41, 0).contains("[hubcodes loop 41]"));
    }
}
