//! Redaction of secret-shaped text before it leaves for a guest viewer.
//!
//! A preview is whatever the agent printed, so a `.env` it cat'ed or a token
//! it echoed would be visible to everyone the canvas is shared with. Before
//! the server sends previews, messages or history rows to anyone below the
//! owner, every token that looks like a credential is replaced with
//! `[redacted]`. This is a safety net, not a guarantee: a secret with an
//! unusual shape passes through. Mark the agent private when that matters.
//!
//! PEM blocks are handled line by line: [`redact_lines`] blanks every line
//! from a `-----BEGIN … PRIVATE KEY-----` marker through the matching END
//! marker; [`redact`] on its own only blanks the marker lines, since a body
//! line of a key is plain base64 and a 40-character hex git sha must stay
//! readable.

use regex::Regex;
use std::sync::OnceLock;

pub const MASK: &str = "[redacted]";

struct Rule {
    re: Regex,
    rep: &'static str,
}

/// Patterns in the order they run. The `key=value` rule goes first so that
/// `api_key=sk-…` ends as `api_key=[redacted]` and not `api_key=[redacted]`
/// twice; the bare `Bearer` rule runs after it and leaves an already masked
/// value alone (`[` is outside its token class).
fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let r = |re: &str, rep: &'static str| Rule { re: Regex::new(re).expect("redact pattern"), rep };
        vec![
            // password=…, "secret": "…", GITHUB_TOKEN=…, x-api-key: …, Authorization: Bearer …
            r(
                r#"(?i)\b([a-z0-9_.-]*(?:password|passwd|secret|token|api[_-]?key|auth)[a-z0-9_.-]*)(["']?\s*[:=]\s*["']?)((?:bearer\s+)?)([^\s"',;]+)"#,
                "${1}${2}${3}[redacted]",
            ),
            // OpenAI / Anthropic (`sk-…`, `sk-ant-…`, `sk-proj-…`)
            r(r"\bsk-[A-Za-z0-9_-]{16,}", MASK),
            // GitHub tokens
            r(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}", MASK),
            r(r"\bgithub_pat_[A-Za-z0-9_]{20,}", MASK),
            // AWS access key id
            r(r"\bAKIA[0-9A-Z]{16}\b", MASK),
            // Slack
            r(r"\bxox[abpr]-[A-Za-z0-9-]{10,}", MASK),
            // Google API key
            r(r"\bAIza[0-9A-Za-z_-]{30,}", MASK),
            // JWT: three base64url parts
            r(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+", MASK),
            // Bare bearer token
            r(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{8,}", "Bearer [redacted]"),
        ]
    })
}

fn pem_marker() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"-----(BEGIN|END) [A-Z ]*PRIVATE KEY-----").expect("pem pattern"))
}

/// Replace every secret-shaped token in one line with `[redacted]`. A PEM
/// marker line is blanked whole.
pub fn redact(line: &str) -> String {
    if pem_marker().is_match(line) {
        return MASK.to_string();
    }
    let mut out = std::borrow::Cow::Borrowed(line);
    for rule in rules() {
        if rule.re.is_match(&out) {
            out = std::borrow::Cow::Owned(rule.re.replace_all(&out, rule.rep).into_owned());
        }
    }
    out.into_owned()
}

/// [`redact`] over a screen or a message, blanking whole PEM private-key
/// blocks (BEGIN through END markers, inclusive).
pub fn redact_lines(lines: &[String]) -> Vec<String> {
    let mut in_pem = false;
    lines
        .iter()
        .map(|l| {
            if let Some(m) = pem_marker().captures(l) {
                in_pem = &m[1] == "BEGIN";
                return MASK.to_string();
            }
            if in_pem {
                return MASK.to_string();
            }
            redact(l)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_prefixes_are_masked() {
        assert_eq!(redact("key sk-abcdefghijklmnopqrstuvwxyz0123"), "key [redacted]");
        assert_eq!(redact("ANTHROPIC sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz"), "ANTHROPIC [redacted]");
        assert_eq!(redact("gh auth: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"), "gh auth: [redacted]");
        assert_eq!(redact("github_pat_11ABCDEFG0123456789_abcdefghijklmnop"), "[redacted]");
        assert_eq!(redact("aws AKIAIOSFODNN7EXAMPLE used"), "aws [redacted] used");
        assert_eq!(redact("slack xoxb-123456789012-abcdefghij"), "slack [redacted]");
        assert_eq!(redact("google AIzaSyA1234567890abcdefghijklmnopqrstuv"), "google [redacted]");
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        assert_eq!(redact(&format!("jwt={jwt}")), "jwt=[redacted]");
        assert_eq!(redact(&format!("got {jwt} back")), "got [redacted] back");
    }

    #[test]
    fn key_value_pairs_keep_the_key() {
        assert_eq!(redact("DB_PASSWORD=hunter2"), "DB_PASSWORD=[redacted]");
        assert_eq!(redact("aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"), "aws_secret_access_key = [redacted]");
        assert_eq!(redact(r#"  "token": "abc123","#), r#"  "token": "[redacted]","#);
        assert_eq!(redact("x-api-key: 0123456789"), "x-api-key: [redacted]");
        assert_eq!(redact("--api_key=foo"), "--api_key=[redacted]");
        assert_eq!(redact("Authorization: Bearer abcdef.ghijkl"), "Authorization: Bearer [redacted]");
        assert_eq!(redact("curl -H 'Authorization: Bearer abcdefghijkl'"), "curl -H 'Authorization: Bearer [redacted]'");
        // A masked value is not masked twice.
        assert_eq!(redact("OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz0123"), "OPENAI_API_KEY=[redacted]");
    }

    #[test]
    fn ordinary_lines_are_untouched() {
        for l in [
            "$ cargo test -p hubcodes-core",
            "commit 3f2a9c1e4b7d8a6f5c0e1d2b3a4f5e6d7c8b9a0f",
            "https://github.com/hubstudio/hubcodes/pull/12",
            "open https://example.com/docs?page=2#auth",
            "the token budget is fine",
            "sk-tool is a shell helper",
            "~/code/api ❯",
            "",
        ] {
            assert_eq!(redact(l), l, "{l:?} changed");
        }
    }

    #[test]
    fn pem_blocks_are_blanked_between_markers() {
        let lines: Vec<String> = [
            "cat id_rsa",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW",
            "QyNTUxOQAAACB5v5R",
            "-----END OPENSSH PRIVATE KEY-----",
            "done",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let out = redact_lines(&lines);
        assert_eq!(out, vec!["cat id_rsa", MASK, MASK, MASK, MASK, "done"]);
        // Single-line: only the markers go; a body line alone is indistinguishable from any base64.
        assert_eq!(redact("-----BEGIN RSA PRIVATE KEY-----"), MASK);
        assert_eq!(redact(&lines[2]), lines[2]);
        // A certificate is not a secret.
        assert_eq!(redact("-----BEGIN CERTIFICATE-----"), "-----BEGIN CERTIFICATE-----");
    }
}
