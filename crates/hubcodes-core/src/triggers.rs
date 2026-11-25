//! Triggers: run an action on a schedule, when a new pull request appears,
//! or when a webhook hits the server.
//!
//! ```toml
//! [[trigger]]
//! name = "nightly-review"
//! on = "cron 0 9 * * 1-5"            # minute hour day month weekday, local time
//! action = "loop start lead reviewer \"Review every open PR and summarise\""
//!
//! [[trigger]]
//! name = "pr-review"
//! on = "pr owner/repo"               # new pull requests, via `gh pr list`
//! action = "send reviewer \"Review PR #{number}: {title} {url}\""
//!
//! [[trigger]]
//! name = "deploy-hook"
//! on = "webhook deploy"              # POST /webhook/deploy on the hubcodes server
//! action = "send ops \"Deploy webhook: {body}\""
//! ```
//!
//! Actions are hubcodes CLI words (`send`, `loop start`, `team start`,
//! `task add`), run through the same code as the CLI.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Trigger {
    pub name: String,
    /// `cron <5 fields>` | `pr <owner/repo>` | `webhook <name>`
    pub on: String,
    pub action: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// A five-field cron expression (minute hour day month weekday), each field
/// `*`, `n`, `a-b`, `a,b,c`, or `*/k`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cron {
    minute: HashSet<u32>,
    hour: HashSet<u32>,
    day: HashSet<u32>,
    month: HashSet<u32>,
    weekday: HashSet<u32>,
}

fn parse_field(f: &str, min: u32, max: u32) -> Option<HashSet<u32>> {
    let mut out = HashSet::new();
    for part in f.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().ok()?),
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (a.parse().ok()?, b.parse().ok()?)
        } else {
            let v: u32 = range.parse().ok()?;
            (v, if step > 1 { max } else { v })
        };
        if lo < min || hi > max || lo > hi || step == 0 {
            return None;
        }
        let mut v = lo;
        while v <= hi {
            out.insert(v);
            v += step;
        }
    }
    Some(out)
}

impl Cron {
    pub fn parse(expr: &str) -> Option<Cron> {
        let f: Vec<&str> = expr.split_whitespace().collect();
        if f.len() != 5 {
            return None;
        }
        Some(Cron {
            minute: parse_field(f[0], 0, 59)?,
            hour: parse_field(f[1], 0, 23)?,
            day: parse_field(f[2], 1, 31)?,
            month: parse_field(f[3], 1, 12)?,
            weekday: parse_field(f[4], 0, 7)?,
        })
    }

    /// Does `(minute, hour, day, month, weekday 0=Sunday)` match?
    pub fn matches(&self, minute: u32, hour: u32, day: u32, month: u32, weekday: u32) -> bool {
        self.minute.contains(&minute)
            && self.hour.contains(&hour)
            && self.day.contains(&day)
            && self.month.contains(&month)
            && (self.weekday.contains(&weekday) || (weekday == 0 && self.weekday.contains(&7)))
    }
}

/// Local time fields now, via `date` (no chrono dependency).
pub fn local_now() -> Option<(u32, u32, u32, u32, u32, i64)> {
    let out = std::process::Command::new("date").arg("+%M %H %d %m %w %s").output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let mut it = s.split_whitespace().map(|x| x.parse::<i64>().ok());
    let (mi, h, d, mo, w, unix) = (it.next()??, it.next()??, it.next()??, it.next()??, it.next()??, it.next()??);
    Some((mi as u32, h as u32, d as u32, mo as u32, w as u32, unix))
}

/// What kind of trigger `on` describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Cron(Box<Cron>),
    Pr(String),
    Webhook(String),
}

pub fn kind(t: &Trigger) -> Option<Kind> {
    let (k, rest) = t.on.trim().split_once(char::is_whitespace)?;
    match k {
        "cron" => Cron::parse(rest.trim()).map(|c| Kind::Cron(Box::new(c))),
        "pr" => Some(Kind::Pr(rest.trim().to_string())),
        "webhook" => Some(Kind::Webhook(rest.trim().to_string())),
        _ => None,
    }
}

/// Open PR numbers and titles via `gh pr list --json`.
pub fn open_prs(repo: &str) -> Vec<(u64, String, String)> {
    let out = std::process::Command::new("gh")
        .args(["pr", "list", "--repo", repo, "--state", "open", "--json", "number,title,url", "--limit", "50"])
        .output();
    let Ok(out) = out else { return vec![] };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return vec![] };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some((
                        p.get("number")?.as_u64()?,
                        p.get("title")?.as_str()?.to_string(),
                        p.get("url")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Fill `{number}`, `{title}`, `{url}`, `{body}` in an action.
pub fn fill(action: &str, vars: &[(&str, &str)]) -> String {
    let mut s = action.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), &v.replace('"', "'"));
    }
    s
}

/// Split an action string into CLI words, honouring double quotes.
pub fn words(action: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in action.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_fields() {
        let c = Cron::parse("0 9 * * 1-5").unwrap();
        assert!(c.matches(0, 9, 11, 11, 3));
        assert!(!c.matches(0, 9, 11, 11, 0));
        assert!(!c.matches(1, 9, 11, 11, 3));
        let e = Cron::parse("*/15 8-18 * * *").unwrap();
        assert!(e.matches(30, 12, 1, 1, 6));
        assert!(!e.matches(31, 12, 1, 1, 6));
        assert!(Cron::parse("60 * * * *").is_none());
        assert!(Cron::parse("* * * *").is_none());
    }

    #[test]
    fn kinds_and_words() {
        let t = Trigger { name: "x".into(), on: "pr owner/repo".into(), action: "send r \"PR #{number}: {title}\"".into(), enabled: true };
        assert_eq!(kind(&t), Some(Kind::Pr("owner/repo".into())));
        let a = fill(&t.action, &[("number", "7"), ("title", "fix \"quotes\"")]);
        assert_eq!(words(&a), vec!["send", "r", "PR #7: fix 'quotes'"]);
        assert_eq!(words("loop start a b \"two words\" --max 5"), vec!["loop", "start", "a", "b", "two words", "--max", "5"]);
    }
}
