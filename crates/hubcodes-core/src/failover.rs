//! When a provider runs out (a usage limit, no credit left, a quota), move
//! the agent to the next provider or model in `[failover] chain` instead of
//! leaving it stuck. The move is [`crate::sessiondef::switch`]: the agent
//! keeps its name, role, folder and links; on the same provider the
//! conversation continues, on another one it starts with a hand-over note.
//! Then it is told to carry on.
//!
//! ```toml
//! [failover]
//! enabled = true
//! chain = ["claude:opus", "claude:sonnet", "codex", "opencode"]   # provider or provider:model, in order
//! rest_minutes = 60                                              # how long a provider that ran out is skipped
//! ```

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FailoverConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub chain: Vec<String>,
    #[serde(default = "d_rest")]
    pub rest_minutes: u32,
}

fn d_rest() -> u32 {
    60
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self { enabled: false, chain: Vec::new(), rest_minutes: d_rest() }
    }
}

/// `claude:sonnet` → (`claude`, Some(`sonnet`)); `codex` → (`codex`, None).
pub fn parse(entry: &str) -> (String, Option<String>) {
    match entry.trim().split_once(':') {
        Some((p, m)) if !m.trim().is_empty() => (p.trim().to_string(), Some(m.trim().to_string())),
        Some((p, _)) => (p.trim().to_string(), None),
        None => (entry.trim().to_string(), None),
    }
}

/// The key a provider (and model) rests under once it ran out.
pub fn key(provider: &str, model: Option<&str>) -> String {
    match model.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => format!("{provider}:{m}"),
        None => provider.to_string(),
    }
}

/// The tool's own words for "you ran out", as the last lines of its screen
/// show them, or `None`. Only the tools' own status sentences count (not
/// API error codes, which code and tests print too, and not a passing 429
/// the tool retries by itself), and only at the bottom of the screen, so a
/// file or diff that mentions limits further up does not trigger it. The
/// caller also wants the same screen on two polls in a row.
pub fn limit_hit(lines: &[String]) -> Option<String> {
    const SIGNS: &[&str] = &[
        "usage limit reached",
        "you've hit your limit",
        "you've hit your usage limit",
        "you have hit your usage limit",
        "hit your session limit",
        "-hour limit reached",
        "weekly limit reached",
        "opus limit reached",
        "credit balance is too low",
    ];
    let tail: Vec<&String> = lines.iter().rev().filter(|l| !l.trim().is_empty()).take(10).collect();
    tail.into_iter().rev().find_map(|l| {
        let plain = crate::policy::strip_ansi(l);
        let low = plain.to_lowercase().replace('’', "'");
        SIGNS.iter().any(|s| low.contains(s)).then(|| plain.trim().chars().take(160).collect())
    })
}

/// The next entry after the agent's own in the chain (wrapping round),
/// skipping itself, what is resting and what `usable` rejects (a provider
/// that is not installed). An agent whose provider is not in the chain
/// starts from the top.
pub fn next(chain: &[String], provider: &str, model: Option<&str>, resting: &dyn Fn(&str) -> bool, usable: &dyn Fn(&str) -> bool) -> Option<(String, Option<String>)> {
    let entries: Vec<(String, Option<String>)> = chain.iter().map(|e| parse(e)).filter(|(p, _)| !p.is_empty()).collect();
    let me = key(provider, model);
    let start = entries.iter().position(|(p, m)| key(p, m.as_deref()) == me).or_else(|| entries.iter().position(|(p, m)| p == provider && m.is_none())).map(|i| i + 1).unwrap_or(0);
    (0..entries.len()).map(|i| &entries[(start + i) % entries.len()]).find(|(p, m)| {
        let k = key(p, m.as_deref());
        k != me && !resting(&k) && !resting(p) && usable(p)
    }).cloned()
}

/// What rests after `sign`: only that model when the tool names a limit of
/// one model (Opus), else the whole provider (a session or weekly limit, no
/// credit) so its other models are not tried in vain.
pub fn rests(provider: &str, model: Option<&str>, sign: &str) -> String {
    if sign.to_lowercase().contains("opus") { key(provider, model) } else { provider.to_string() }
}

/// What the agent is told once it runs on the replacement.
pub fn carry_on(from: &str, to: &str, conversation: bool) -> String {
    if conversation {
        format!("{from} ran out, so hubcodes moved you to {to} in the same conversation. Carry on with what you were doing.")
    } else {
        format!("{from} ran out, so hubcodes moved you to {to}. Your brief has a hand-over note with your open tasks, recent messages and the end of the previous terminal. Read it and carry on where it stopped.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn limits_are_read_from_the_bottom_of_the_screen() {
        assert!(limit_hit(&lines(&["working…", "Claude usage limit reached. Your limit will reset at 7pm", "❯"])).unwrap().contains("usage limit reached"));
        assert!(limit_hit(&lines(&["■ You’ve hit your usage limit. Try again in 3 days"])).is_some());
        assert!(limit_hit(&lines(&["\x1b[31m5-hour limit reached ∙ resets 11pm\x1b[0m"])).is_some());
        assert!(limit_hit(&lines(&["Credit balance is too low"])).is_some());
        // API codes in output or code are not the tool saying it.
        assert!(limit_hit(&lines(&["fixed the insufficient_quota handling", "rate_limit_exceeded: retrying", "RESOURCE_EXHAUSTED", "❯"])).is_none());
        assert!(limit_hit(&lines(&["all good", "❯"])).is_none());
        // A mention far above the status lines is not the tool saying it.
        let mut v = lines(&["// handles usage limit reached from the API"]);
        v.extend((0..12).map(|i| format!("line {i}")));
        assert!(limit_hit(&v).is_none());
    }

    #[test]
    fn the_chain_moves_on_and_wraps() {
        let chain: Vec<String> = ["claude:opus", "claude:sonnet", "codex", "opencode"].iter().map(|s| s.to_string()).collect();
        let none = |_: &str| false;
        let all = |_: &str| true;
        assert_eq!(next(&chain, "claude", Some("opus"), &none, &all), Some(("claude".into(), Some("sonnet".into()))));
        assert_eq!(next(&chain, "claude", Some("sonnet"), &none, &all), Some(("codex".into(), None)));
        assert_eq!(next(&chain, "opencode", None, &none, &all), Some(("claude".into(), Some("opus".into()))));
        // Not in the chain: from the top.
        assert_eq!(next(&chain, "gemini", None, &none, &all), Some(("claude".into(), Some("opus".into()))));
        // Resting and missing ones are skipped.
        let resting = |k: &str| k == "codex";
        let usable = |p: &str| p != "opencode";
        assert_eq!(next(&chain, "claude", Some("sonnet"), &resting, &usable), Some(("claude".into(), Some("opus".into()))));
        let tired = |k: &str| k.starts_with("claude") || k == "codex";
        assert_eq!(next(&chain, "claude", Some("sonnet"), &tired, &usable), None);
        assert_eq!(parse(" codex : "), ("codex".into(), None));
        // A provider-wide rest skips its models too.
        let claude_out = |k: &str| k == "claude";
        assert_eq!(next(&chain, "claude", Some("opus"), &claude_out, &all), Some(("codex".into(), None)));
        assert_eq!(rests("claude", Some("opus"), "Opus weekly limit reached"), "claude:opus");
        assert_eq!(rests("claude", Some("opus"), "5-hour limit reached"), "claude");
    }
}
