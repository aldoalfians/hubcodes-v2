//! Alerts off the machine: the same text the desktop notification shows,
//! posted to a webhook so it reaches a phone or a team channel.
//!
//! `[alerts] webhook` is a Slack or Discord incoming webhook (JSON) or any
//! other URL, such as an ntfy topic, which gets the text as a plain body.
//! The URL itself is a secret and lives in the Keychain.

use std::time::Duration;

/// The request for `url`: content type and body.
pub fn request(url: &str, text: &str) -> (&'static str, String) {
    let text = format!("hubcodes: {text}");
    let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("").to_ascii_lowercase();
    if host == "hooks.slack.com" || host == "chat.googleapis.com" {
        ("application/json", serde_json::json!({ "text": text }).to_string())
    } else if host.ends_with("discord.com") || host.ends_with("discordapp.com") {
        ("application/json", serde_json::json!({ "content": text }).to_string())
    } else {
        ("text/plain; charset=utf-8", text)
    }
}

/// Post each alert, in the background: a slow or broken webhook never holds up the tick.
pub fn send(webhook: &str, alerts: Vec<String>) {
    let url = crate::mcp::expand_env(webhook).trim().to_string();
    if url.is_empty() || alerts.is_empty() || !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
        for a in alerts {
            let (kind, body) = request(&url, &a);
            let _ = agent.post(&url).header("Content-Type", kind).header("Title", "hubcodes").send(body);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_webhook_gets_its_own_body() {
        let (k, b) = request("https://hooks.slack.com/services/T/B/x", "o/a#7 needs you");
        assert_eq!((k, b.as_str()), ("application/json", r#"{"text":"hubcodes: o/a#7 needs you"}"#));
        let (_, b) = request("https://discord.com/api/webhooks/1/x", "hi");
        assert_eq!(b, r#"{"content":"hubcodes: hi"}"#);
        let (k, b) = request("https://ntfy.sh/my-topic", "hi");
        assert_eq!((k, b.as_str()), ("text/plain; charset=utf-8", "hubcodes: hi"));
    }
}
