//! Accounts connected once and picked by trackers, knowledge sources and
//! triggers: an Atlassian site (Jira and Confluence), a Plane workspace,
//! GitHub through the `gh` login. The token is a Keychain reference
//! ([`crate::secrets`]); a tracker that picks the connection copies its site,
//! email and the reference, so rotating the token in one place updates all.
//!
//! ```toml
//! [[connection]]
//! name = "hubstudio"
//! kind = "atlassian"                 # atlassian | plane | github
//! url = "https://hubstudio.atlassian.net"
//! email = "me@hubstudio.id"
//! token = "${keychain:connection:hubstudio}"   # or "twg": sign in through the twg CLI
//! ```

use crate::mcp::expand_env;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Connection {
    pub name: String,
    /// atlassian | plane | github
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token: String,
    /// Plane workspace slug.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workspace: String,
}

/// The Keychain name for a connection's token.
pub fn secret_name(name: &str) -> String {
    format!("connection:{}", name.trim())
}

/// Where to make a token for this kind of account.
pub fn token_page(kind: &str) -> &'static str {
    match kind {
        "atlassian" => "https://id.atlassian.com/manage-profile/security/api-tokens",
        "plane" => "https://app.plane.so/profile/api-tokens",
        "github" => "https://github.com/settings/tokens",
        _ => "",
    }
}

/// One thing a picker can offer: a value and what to show for it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

fn choice(value: impl Into<String>, label: impl Into<String>) -> Choice {
    Choice { value: value.into(), label: label.into() }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into()
}

fn get_json(url: &str, auth: (&str, &str)) -> Result<Value> {
    match agent().get(url).header(auth.0, auth.1).header("Accept", "application/json").call() {
        Ok(mut r) => Ok(r.body_mut().read_json::<Value>()?),
        Err(ureq::Error::StatusCode(code)) => Err(anyhow!(
            "HTTP {code} from {}{}",
            url.split('?').next().unwrap_or(url),
            if code == 401 || code == 403 { " (check the email and token)" } else if code == 404 { " (check the address)" } else { "" }
        )),
        Err(e) => Err(anyhow!("{url}: {e}")),
    }
}

fn basic(email: &str, token: &str) -> String {
    format!("Basic {}", b64(format!("{}:{}", expand_env(email), expand_env(token)).as_bytes()))
}

/// Small standard base64, without another dependency.
pub fn base64(raw: &[u8]) -> String {
    b64(raw)
}

/// The reverse of [`base64`]; `None` on a character outside the alphabet.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        acc = acc << 6 | T.iter().position(|t| *t == c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn b64(raw: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in raw.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            out.push(if i <= chunk.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

fn site(url: &str) -> String {
    expand_env(url).trim().trim_end_matches('/').to_string()
}

/// Atlassian: who the token belongs to, as proof the connection works.
pub fn atlassian_me(c: &Connection) -> Result<String> {
    if crate::twg::is_twg(&c.token) {
        return crate::twg::me(&c.url);
    }
    let v = get_json(&format!("{}/rest/api/3/myself", site(&c.url)), ("Authorization", &basic(&c.email, &c.token)))?;
    Ok(v.get("displayName").and_then(|d| d.as_str()).unwrap_or("signed in").to_string())
}

/// Jira projects the account can see.
pub fn jira_projects(c: &Connection) -> Result<Vec<Choice>> {
    if crate::twg::is_twg(&c.token) {
        return Ok(crate::twg::projects(&c.url)?.into_iter().map(|(k, n)| choice(k.clone(), format!("{k} · {n}"))).collect());
    }
    let v = get_json(&format!("{}/rest/api/3/project/search?maxResults=100&orderBy=name", site(&c.url)), ("Authorization", &basic(&c.email, &c.token)))?;
    Ok(v.get("values").and_then(|x| x.as_array()).cloned().unwrap_or_default().iter().filter_map(|p| Some(choice(p.get("key")?.as_str()?, format!("{} · {}", p.get("key")?.as_str()?, p.get("name").and_then(|n| n.as_str()).unwrap_or(""))))).collect())
}

/// The statuses a Jira project's issues can be in (every issue type), in workflow order.
pub fn jira_statuses(c: &Connection, project: &str) -> Result<Vec<Choice>> {
    if crate::twg::is_twg(&c.token) {
        return Ok(crate::twg::statuses(&c.url, project)?.into_iter().map(|n| choice(n.clone(), n)).collect());
    }
    let v = get_json(&format!("{}/rest/api/3/project/{}/statuses", site(&c.url), project.trim()), ("Authorization", &basic(&c.email, &c.token)))?;
    let mut seen = Vec::<String>::new();
    for t in v.as_array().cloned().unwrap_or_default() {
        for s in t.get("statuses").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
            if let Some(n) = s.get("name").and_then(|n| n.as_str()) {
                if !seen.iter().any(|x| x == n) {
                    seen.push(n.to_string());
                }
            }
        }
    }
    Ok(seen.into_iter().map(|n| choice(n.clone(), n)).collect())
}

/// Confluence spaces the account can see.
pub fn confluence_spaces(c: &Connection) -> Result<Vec<Choice>> {
    if crate::twg::is_twg(&c.token) {
        return Ok(crate::twg::spaces(&c.url)?.into_iter().map(|(k, n)| choice(k.clone(), format!("{k} · {n}"))).collect());
    }
    let v = get_json(&format!("{}/wiki/rest/api/space?limit=200", site(&c.url)), ("Authorization", &basic(&c.email, &c.token)))?;
    Ok(v.get("results").and_then(|x| x.as_array()).cloned().unwrap_or_default().iter().filter_map(|s| Some(choice(s.get("key")?.as_str()?, format!("{} · {}", s.get("key")?.as_str()?, s.get("name").and_then(|n| n.as_str()).unwrap_or(""))))).collect())
}

fn plane_api(c: &Connection) -> String {
    let u = site(&c.url);
    let u = if u.is_empty() { "https://api.plane.so".to_string() } else { u };
    format!("{u}/api/v1/workspaces/{}", expand_env(&c.workspace).trim())
}

/// Plane projects in the workspace (value = project id).
pub fn plane_projects(c: &Connection) -> Result<Vec<Choice>> {
    let v = get_json(&format!("{}/projects/", plane_api(c)), ("X-API-Key", &expand_env(&c.token)))?;
    let list = v.get("results").and_then(|x| x.as_array()).or_else(|| v.as_array()).cloned().unwrap_or_default();
    Ok(list.iter().filter_map(|p| Some(choice(p.get("id")?.as_str()?, format!("{} · {}", p.get("identifier").and_then(|i| i.as_str()).unwrap_or(""), p.get("name").and_then(|n| n.as_str()).unwrap_or(""))))).collect())
}

/// A Plane project's states, by name.
pub fn plane_states(c: &Connection, project: &str) -> Result<Vec<Choice>> {
    let v = get_json(&format!("{}/projects/{}/states/", plane_api(c), project.trim()), ("X-API-Key", &expand_env(&c.token)))?;
    let list = v.get("results").and_then(|x| x.as_array()).or_else(|| v.as_array()).cloned().unwrap_or_default();
    Ok(list.iter().filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(|n| choice(n, n))).collect())
}

fn gh(args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("gh").args(args).output().map_err(|e| anyhow!("gh is not installed ({e}); brew install gh, then gh auth login"))?;
    if !out.status.success() {
        return Err(anyhow!("gh {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim().lines().last().unwrap_or("failed")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// The GitHub login `gh` uses here.
pub fn github_me() -> Result<String> {
    Ok(gh(&["api", "user", "--jq", ".login"])?.trim().to_string())
}

/// Repositories of the signed-in user and their organisations.
pub fn github_repos() -> Result<Vec<Choice>> {
    let out = gh(&["repo", "list", "--limit", "200", "--json", "nameWithOwner,description"])?;
    let mut list: Vec<Choice> = serde_json::from_str::<Vec<Value>>(&out)?.iter().filter_map(|r| r.get("nameWithOwner")?.as_str().map(|n| choice(n, n))).collect();
    if let Ok(orgs) = gh(&["api", "user/orgs", "--jq", ".[].login"]) {
        for org in orgs.lines().map(str::trim).filter(|o| !o.is_empty()).take(10) {
            if let Ok(o) = gh(&["repo", "list", org, "--limit", "200", "--json", "nameWithOwner"]) {
                for r in serde_json::from_str::<Vec<Value>>(&o).unwrap_or_default() {
                    if let Some(n) = r.get("nameWithOwner").and_then(|x| x.as_str()) {
                        if !list.iter().any(|c| c.value == n) {
                            list.push(choice(n, n));
                        }
                    }
                }
            }
        }
    }
    Ok(list)
}

/// A repository's labels.
pub fn github_labels(repo: &str) -> Result<Vec<Choice>> {
    let out = gh(&["label", "list", "-R", repo.trim(), "--limit", "200", "--json", "name"])?;
    Ok(serde_json::from_str::<Vec<Value>>(&out)?.iter().filter_map(|l| l.get("name")?.as_str().map(|n| choice(n, n))).collect())
}

/// Check a connection and say who it signs in as.
pub fn check(c: &Connection) -> Result<String> {
    match c.kind.as_str() {
        "atlassian" => atlassian_me(c),
        "plane" => plane_projects(c).map(|p| format!("{} project(s)", p.len())),
        "github" => github_me(),
        k => Err(anyhow!("unknown connection kind '{k}'")),
    }
}

/// What a picker asks for: `projects`, `statuses` (needs `project`),
/// `spaces`, `repos`, `labels` (needs `repo`).
pub fn choices(c: &Connection, what: &str, arg: &str) -> Result<Vec<Choice>> {
    match (c.kind.as_str(), what) {
        ("atlassian", "projects") => jira_projects(c),
        ("atlassian", "statuses") => jira_statuses(c, arg),
        ("atlassian", "spaces") => confluence_spaces(c),
        ("plane", "projects") => plane_projects(c),
        ("plane", "statuses") => plane_states(c, arg),
        ("github", "repos") => github_repos(),
        ("github", "labels") => github_labels(arg),
        (k, w) => Err(anyhow!("a {k} connection has no {w}")),
    }
}

/// The JSON a picker gets (for the desktop and `hubcodes connection`).
pub fn choices_json(c: &Connection, what: &str, arg: &str) -> Value {
    match choices(c, what, arg) {
        Ok(v) => json!({ "ok": true, "choices": v }),
        Err(e) => json!({ "ok": false, "error": format!("{e:#}"), "choices": [] }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_matches_the_standard_encoding() {
        // RFC 4648 test vectors. No "Basic <user:pass>" literal here: secret
        // scanners flag it even when the pair is made up.
        for (raw, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(b64(raw.as_bytes()), enc);
            assert_eq!(base64_decode(enc).unwrap(), raw.as_bytes());
        }
        let bin: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64(&bin)).unwrap(), bin);
        assert!(base64_decode("a$").is_none());
        assert_eq!(basic("a", "b"), format!("Basic {}", b64(b"a:b")));
    }

    #[test]
    fn secret_names_and_token_pages() {
        assert_eq!(secret_name(" hub "), "connection:hub");
        assert!(token_page("atlassian").starts_with("https://id.atlassian.com/"));
        assert_eq!(token_page("other"), "");
    }
}
