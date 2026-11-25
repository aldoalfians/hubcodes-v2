//! A teammate checks with its lead before acting on what a person typed.
//!
//! The brief asks for it, but a prompt is advice (a test showed a model
//! writing the file at once), so it is also enforced where the tools run.
//! When a person's message reaches an agent that is linked to an
//! orchestrator (typed into its terminal, untagged, or sent as
//! `[from human:…]`), the agent's `UserPromptSubmit` hook marks the pane
//! with that lead. While the mark is there the tool guard refuses work
//! (file edits, builds, tests, git writes) with a reason that says to ask
//! the lead first; reads, searches and `hubcodes send` stay allowed. The
//! lead's next message to the agent lifts the mark, and so does removing
//! the line to the lead or turning `route_through_orchestrator` off.
//! `--direct` (⌥Enter) marks the next prompt as exempt.

use crate::providers::Registry;
use crate::relay;
use crate::tmux::Tmux;
use serde_json::Value;

/// The lead this pane waits on before working.
pub const OPT_CHECK_WITH: &str = "@hubcodes_check_with";
/// The next prompt was sent with `--direct`: no check-in for it.
pub const OPT_DIRECT_ONCE: &str = "@hubcodes_direct_once";

/// A prompt typed or relayed by a person, not by another agent or hubcodes.
pub fn from_person(prompt: &str) -> bool {
    let p = prompt.trim_start();
    !p.starts_with('[') || p.starts_with("[from human:")
}

/// The agent's prompt hook: marks the pane when a person's message needs a
/// check-in and returns the line added to the agent's context; clears the
/// mark when the message is the lead's.
pub fn on_prompt(t: &Tmux, reg: &Registry, agent: &str, prompt: &str) -> Option<String> {
    let pane = t.pane_by_name(agent).ok()?;
    let waiting_on = t.pane_option(&pane.id, OPT_CHECK_WITH);
    if !waiting_on.is_empty() && prompt.trim_start().starts_with(&format!("[from {waiting_on}]")) {
        let _ = t.unset_pane_option(&pane.id, OPT_CHECK_WITH);
        return None;
    }
    if !from_person(prompt) {
        return None;
    }
    if t.pane_option(&pane.id, OPT_DIRECT_ONCE) == "1" {
        let _ = t.unset_pane_option(&pane.id, OPT_DIRECT_ONCE);
        return None;
    }
    let lead = relay::route_for(t, reg, agent)?;
    t.set_pane_option(&pane.id, OPT_CHECK_WITH, &lead).ok()?;
    Some(format!(
        "[hubcodes] A person typed this to you directly. You are on {lead}'s team: before you edit files, build or commit, tell {lead} what was asked and what you plan with `hubcodes send {lead} \"…\"`, and wait for its [from {lead}] reply. Until then hubcodes refuses those tools. A plain question about your own work you may answer now."
    ))
}

/// Why `agent` may not run this tool yet: it owes its lead a check-in.
pub fn reason(t: &Tmux, reg: &Registry, agent: &str, tool: &str, input: &Value) -> Option<String> {
    let pane = t.pane_by_name(agent).ok()?;
    let lead = t.pane_option(&pane.id, OPT_CHECK_WITH);
    if lead.is_empty() {
        return None;
    }
    // The line to the lead is gone, or routing is off: nothing to wait for.
    if relay::route_for(t, reg, agent).as_deref() != Some(lead.as_str()) {
        let _ = t.unset_pane_option(&pane.id, OPT_CHECK_WITH);
        return None;
    }
    crate::guard::work_reason(tool, input)?;
    Some(format!(
        "hubcodes: a person asked you this directly and you are on {lead}'s team, so check with {lead} before editing files, building or committing. Run hubcodes send {lead} \"asked: …; plan: …\" and wait for its [from {lead}] reply."
    ))
}

/// A message from `from` reached `pane`: the lead's lifts the mark.
pub fn on_delivery(t: &Tmux, pane_id: &str, from: &str) {
    if !from.is_empty() && t.pane_option(pane_id, OPT_CHECK_WITH) == from {
        let _ = t.unset_pane_option(pane_id, OPT_CHECK_WITH);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_person_s_prompt_is_untagged_or_from_human() {
        assert!(from_person("tolong bikin file hello.txt"));
        assert!(from_person("[from human:aldo] bisa ngoding?"));
        assert!(!from_person("[from lead] go ahead"));
        assert!(!from_person("[from hubcodes] task #3"));
        assert!(!from_person("[room auth] dev: done"));
    }
}
