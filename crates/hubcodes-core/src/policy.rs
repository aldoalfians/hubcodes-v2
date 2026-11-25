//! Auto-approve policy: which agent actions hubcodes answers by itself, so
//! "needs you" is left for the real decisions.
//!
//! ```toml
//! [policy]
//! mode = "auto"                      # off = every prompt waits for you; auto = apply the rules below
//! allow = ["git status*", "git diff*", "git log*", "cargo test*", "pnpm test", "npm test", "ls*", "cat *", "rg *"]
//! deny = ["rm -rf *", "git push --force*", "git push -f*", "git reset --hard*", "sudo *", "* | sh", "DROP TABLE*"]
//! allow_tools = ["Read", "Glob", "Grep", "WebFetch", "WebSearch"]   # Claude Code tools that never ask
//! deny_paths = ["~/.ssh/*", "*.env", "*.env.*", "*/secrets/*"]       # writes here are refused
//! ```
//!
//! Rules are globs (`*` = anything, `?` = one character). A rule without a
//! wildcard matches the command exactly or as its first words (`git status`
//! also matches `git status -s`). Compound commands (`a && b`, `a; b`,
//! `a | b`) are judged part by part: one denied part denies the whole, every
//! part must be allowed for an allow. Deny always wins over allow; anything
//! unmatched is asked as before.
//!
//! Two places apply it: Claude Code's `PreToolUse` hook answers before the
//! prompt is even shown (`hubcodes hook claude` prints the decision), and the
//! poll loop reads the screen of any other agent that is waiting and presses
//! its yes or no keys when a rule matches. A repository's `.hubcodes/team.toml`
//! can add `[policy]` rules on top of the user's.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyConfig {
    /// `off` | `auto`
    #[serde(default = "d_mode")]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny_paths: Vec<String>,
}

fn d_mode() -> String {
    "off".into()
}

impl Default for PolicyConfig {
    fn default() -> Self {
        PolicyConfig { mode: "off".into(), allow: Vec::new(), deny: Vec::new(), allow_tools: Vec::new(), deny_paths: Vec::new() }
    }
}

/// A starting point most teams can keep: read-only git and test commands
/// allowed, the destructive classics denied.
pub fn suggested() -> PolicyConfig {
    PolicyConfig {
        mode: "auto".into(),
        allow: ["git status*", "git diff*", "git log*", "git branch*", "git show*", "ls*", "cat *", "head *", "tail *", "rg *", "grep *", "find *", "cargo test*", "cargo build*", "cargo check*", "cargo clippy*", "pnpm test*", "pnpm run build", "npm test*", "npm run build", "pytest*", "go test*", "make test"].iter().map(|s| s.to_string()).collect(),
        deny: ["rm -rf *", "rm -r /*", "git push --force*", "git push -f*", "git reset --hard*", "git clean -f*", "git checkout -- .", "sudo *", "* | sh", "* | bash", "curl * | *", "DROP TABLE*", "DROP DATABASE*", "mkfs*", "dd if=*"].iter().map(|s| s.to_string()).collect(),
        allow_tools: ["Read", "Glob", "Grep", "WebFetch", "WebSearch", "TodoWrite", "Task"].iter().map(|s| s.to_string()).collect(),
        deny_paths: ["~/.ssh/*", "*/.ssh/*", "*.env", "*.env.*", "*/secrets/*", "*.pem", "*.key"].iter().map(|s| s.to_string()).collect(),
    }
}

impl PolicyConfig {
    pub fn is_on(&self) -> bool {
        self.mode.eq_ignore_ascii_case("auto")
    }

    /// The user's policy plus a repository's: the repo may turn it on or off
    /// and adds its own rules.
    pub fn merged(&self, repo: &PolicyConfig) -> PolicyConfig {
        let mut out = self.clone();
        if repo != &PolicyConfig::default() && !repo.mode.is_empty() {
            out.mode = repo.mode.clone();
        }
        for (dst, src) in [(&mut out.allow, &repo.allow), (&mut out.deny, &repo.deny), (&mut out.allow_tools, &repo.allow_tools), (&mut out.deny_paths, &repo.deny_paths)] {
            for r in src {
                if !dst.contains(r) {
                    dst.push(r.clone());
                }
            }
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "rule")]
pub enum Decision {
    Allow(String),
    Deny(String),
    Ask,
}

impl Decision {
    pub fn label(&self) -> &'static str {
        match self {
            Decision::Allow(_) => "allow",
            Decision::Deny(_) => "deny",
            Decision::Ask => "ask",
        }
    }
}

/// Glob match on the whole text: `*` any run, `?` one char. A pattern with
/// no wildcard also matches as the leading words of the text.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p = pattern.trim();
    let t = text.trim();
    if p.is_empty() {
        return false;
    }
    if !p.contains('*') && !p.contains('?') {
        return t == p || t.starts_with(&format!("{p} "));
    }
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
    let p = if !home.is_empty() && p.starts_with("~/") { format!("{home}/{}", &p[2..]) } else { p.to_string() };
    let t = if !home.is_empty() && t.starts_with("~/") { format!("{home}/{}", &t[2..]) } else { t.to_string() };
    wild(p.as_bytes(), t.as_bytes())
}

fn wild(p: &[u8], t: &[u8]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// Split `a && b | c; d` into its commands (quotes respected).
pub fn split_commands(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                    cur.push(c);
                } else if (c == '&' && chars.get(i + 1) == Some(&'&')) || (c == '|' && chars.get(i + 1) == Some(&'|')) {
                    out.push(std::mem::take(&mut cur));
                    i += 1;
                } else if c == '|' || c == ';' || c == '\n' {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.push(c);
                }
            }
        }
        i += 1;
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Judge a shell command.
pub fn decide_command(cfg: &PolicyConfig, cmd: &str) -> Decision {
    if !cfg.is_on() {
        return Decision::Ask;
    }
    let whole = cmd.trim();
    if whole.is_empty() {
        return Decision::Ask;
    }
    // Deny on the whole command or any part.
    for r in &cfg.deny {
        if glob_match(r, whole) || split_commands(whole).iter().any(|p| glob_match(r, p)) {
            return Decision::Deny(r.clone());
        }
    }
    let parts = split_commands(whole);
    let mut matched = Vec::new();
    for p in &parts {
        match cfg.allow.iter().find(|r| glob_match(r, p)) {
            Some(r) => matched.push(r.clone()),
            None => return Decision::Ask,
        }
    }
    if matched.is_empty() {
        return Decision::Ask;
    }
    matched.dedup();
    Decision::Allow(matched.join(", "))
}

/// Judge a Claude Code tool call (`PreToolUse` payload: tool name and input).
pub fn decide_tool(cfg: &PolicyConfig, tool: &str, input: &Value) -> Decision {
    if !cfg.is_on() {
        return Decision::Ask;
    }
    let path = input.get("file_path").or_else(|| input.get("path")).or_else(|| input.get("notebook_path")).and_then(|v| v.as_str()).unwrap_or("");
    let writes = matches!(tool, "Write" | "Edit" | "MultiEdit" | "NotebookEdit");
    if !path.is_empty() {
        if let Some(r) = cfg.deny_paths.iter().find(|r| glob_match(r, path)) {
            if writes {
                return Decision::Deny(format!("path {r}"));
            }
        }
    }
    if tool == "Bash" {
        let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        return decide_command(cfg, cmd);
    }
    if cfg.allow_tools.iter().any(|t| t.eq_ignore_ascii_case(tool)) {
        return Decision::Allow(format!("tool {tool}"));
    }
    Decision::Ask
}

/// The command an agent is asking about, from the last screen lines: the
/// first backticked span on a prompt line, else `run <cmd>?`, else `$ cmd`.
pub fn command_on_screen(lines: &[String]) -> Option<String> {
    let text: Vec<String> = lines.iter().rev().take(12).map(|l| strip_ansi(l)).collect();
    for l in &text {
        let lower = l.to_lowercase();
        let prompt_like = lower.contains("allow") || lower.contains("run") || lower.contains("execute") || lower.contains("permission") || lower.contains("(y/n)") || lower.contains("[y/n]");
        if !prompt_like {
            continue;
        }
        if let Some(a) = l.find('`') {
            if let Some(b) = l[a + 1..].find('`') {
                let c = l[a + 1..a + 1 + b].trim();
                if !c.is_empty() {
                    return Some(c.to_string());
                }
            }
        }
        if let Some(i) = lower.find(" run ") {
            let rest = l[i + 5..].trim().trim_end_matches('?').trim_end_matches("(y/n)").trim_end_matches("(y/N)").trim();
            if rest.len() > 2 {
                let rest = rest.trim_end_matches(['?', ':']).trim();
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }
        }
    }
    for l in &text {
        let t = l.trim();
        if let Some(rest) = t.strip_prefix("$ ") {
            if !rest.trim().is_empty() {
                return Some(rest.trim().to_string());
            }
        }
    }
    None
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                for n in chars.by_ref() {
                    if n == '\u{7}' {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_and_prefixes() {
        assert!(glob_match("git status*", "git status -s"));
        assert!(glob_match("git status", "git status"));
        assert!(glob_match("git status", "git status --short"));
        assert!(!glob_match("git status", "git statusx"));
        assert!(glob_match("* | sh", "curl x | sh"));
        assert!(glob_match("*.env", "/app/.env"));
        assert!(glob_match("*.env.*", "/app/.env.local"));
        assert!(!glob_match("rm -rf *", "rm file"));
        assert!(glob_match("cat ?", "cat a"));
    }

    #[test]
    fn compound_commands_and_precedence() {
        let p = suggested();
        assert_eq!(decide_command(&p, "git status").label(), "allow");
        assert_eq!(decide_command(&p, "git status && cargo test").label(), "allow");
        assert_eq!(decide_command(&p, "git status && rm -rf /tmp/x").label(), "deny");
        assert_eq!(decide_command(&p, "cargo test; sudo reboot").label(), "deny");
        assert_eq!(decide_command(&p, "npm run deploy").label(), "ask");
        assert_eq!(decide_command(&p, "git status | grep 'a && b'").label(), "allow");
        let off = PolicyConfig { mode: "off".into(), ..suggested() };
        assert_eq!(decide_command(&off, "git status").label(), "ask");
        assert_eq!(split_commands("a && b || c | d; e"), vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn tools_and_paths() {
        let p = suggested();
        assert_eq!(decide_tool(&p, "Read", &serde_json::json!({"file_path": "/x"})).label(), "allow");
        assert_eq!(decide_tool(&p, "Write", &serde_json::json!({"file_path": "/app/.env"})).label(), "deny");
        assert_eq!(decide_tool(&p, "Read", &serde_json::json!({"file_path": "/app/.env"})).label(), "allow", "reads of denied paths are not writes");
        assert_eq!(decide_tool(&p, "Write", &serde_json::json!({"file_path": "/app/src/a.rs"})).label(), "ask");
        assert_eq!(decide_tool(&p, "Bash", &serde_json::json!({"command": "cargo test -q"})).label(), "allow");
        let r = PolicyConfig { mode: "auto".into(), allow: vec!["npm run deploy".into()], ..Default::default() };
        let m = PolicyConfig { mode: "off".into(), ..suggested() }.merged(&r);
        assert!(m.is_on());
        assert!(m.allow.contains(&"npm run deploy".to_string()) && m.allow.contains(&"git status*".to_string()));
    }

    #[test]
    fn reads_the_command_off_the_screen() {
        let lines: Vec<String> = ["Reviewing 2 files…", "", "\x1b[33m?\x1b[0m Allow opencode to run `cargo clippy`? \x1b[2m(y/N)\x1b[0m "].iter().map(|s| s.to_string()).collect();
        assert_eq!(command_on_screen(&lines).as_deref(), Some("cargo clippy"));
        let codex: Vec<String> = vec!["Codex wants to run rm -rf build? (y/n)".into()];
        assert_eq!(command_on_screen(&codex).as_deref(), Some("rm -rf build"));
        assert_eq!(command_on_screen(&["hello".to_string()]), None);
    }
}
