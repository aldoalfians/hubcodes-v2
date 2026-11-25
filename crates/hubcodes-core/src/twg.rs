//! Atlassian through the `twg` CLI login (Atlassian's own OAuth sign-in), for
//! people who would rather not make an API token. An Atlassian connection,
//! a Jira tracker or a Confluence/Jira knowledge source with `token = "twg"`
//! runs its calls through `twg … -o json` instead of the REST API with an
//! email and token. Sign in once with `twg login` (or `twg setup`).
//!
//! ```toml
//! [[connection]]
//! name = "hubstudio"
//! kind = "atlassian"
//! url = "https://hubstudio.atlassian.net"
//! token = "twg"
//! ```

use anyhow::{anyhow, Result};
use serde_json::Value;

/// The token value that means "sign in through twg".
pub const MARK: &str = "twg";

pub fn is_twg(token: &str) -> bool {
    token.trim() == MARK
}

/// `https://team.atlassian.net/wiki/…` → `team.atlassian.net`, what `--site` takes.
pub fn host(url: &str) -> String {
    let u = crate::mcp::expand_env(url);
    let u = u.trim();
    let u = u.split_once("://").map(|(_, r)| r).unwrap_or(u);
    u.split('/').next().unwrap_or("").to_string()
}

/// Run `twg <args> -o json` for a site and return the payload's `data`.
pub fn run(site: &str, args: &[&str]) -> Result<Value> {
    let mut c = std::process::Command::new("twg");
    c.args(args).args(["-o", "json", "--output-summary", "none"]);
    let h = host(site);
    if !h.is_empty() {
        c.args(["--site", &h]);
    }
    let out = crate::twg::output_within(c, std::time::Duration::from_secs(120)).map_err(|e| anyhow!("{e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Option<Value> = stdout.find('{').and_then(|i| serde_json::from_str(&stdout[i..]).ok());
    if !out.status.success() || v.as_ref().and_then(|v| v.get("ok")).and_then(|o| o.as_bool()) == Some(false) {
        let msg = v
            .as_ref()
            .and_then(|v| v.get("error"))
            .and_then(|e| e.get("message").and_then(|m| m.as_str()).map(String::from).or_else(|| e.as_str().map(String::from)))
            .unwrap_or_else(|| String::from_utf8_lossy(&out.stderr).trim().lines().last().unwrap_or("failed").to_string());
        let hint = if msg.to_lowercase().contains("login") || msg.to_lowercase().contains("auth") { " (run: twg login)" } else { "" };
        return Err(anyhow!("twg {}: {msg}{hint}", args.iter().take(3).copied().collect::<Vec<_>>().join(" ")));
    }
    Ok(v.and_then(|mut v| v.get_mut("data").map(Value::take)).unwrap_or(Value::Null))
}

/// Run with no stdin (twg must never wait for typing) and give up after
/// `limit`, so a slow or stuck call cannot hold the tender.
pub(crate) fn output_within(mut c: std::process::Command, limit: std::time::Duration) -> Result<std::process::Output, String> {
    use std::process::Stdio;
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("twg is not installed ({e}); install the TWG CLI, then run: twg login"))?;
    // Read both pipes while waiting, so a large answer cannot fill a pipe and stall.
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let ro = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = so.as_mut() {
            let _ = std::io::Read::read_to_end(s, &mut b);
        }
        b
    });
    let re = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = se.as_mut() {
            let _ = std::io::Read::read_to_end(s, &mut b);
        }
        b
    });
    let t0 = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if t0.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("twg did not answer in {} s", limit.as_secs()));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => return Err(format!("twg: {e}")),
        }
    };
    Ok(std::process::Output { status, stdout: ro.join().unwrap_or_default(), stderr: re.join().unwrap_or_default() })
}

fn list(v: &Value, key: &str) -> Vec<Value> {
    v.get(key).and_then(|a| a.as_array()).or_else(|| v.as_array()).cloned().unwrap_or_default()
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

/// A site twg is signed in to on this computer.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// `https://team.atlassian.net`
    pub url: String,
    pub user: String,
}

/// The sites twg has a login for (`twg auth profile list`, no network), so
/// an account can be added from it without typing anything.
pub fn profiles() -> Result<Vec<Profile>> {
    Ok(profiles_from(&run("", &["auth", "profile", "list"])?))
}

fn profiles_from(v: &Value) -> Vec<Profile> {
    let mut out: Vec<Profile> = Vec::new();
    for p in list(v, "profiles") {
        let domain = match (s(&p, "domain"), s(&p, "site")) {
            ("", "") => continue,
            ("", site) => format!("{site}.atlassian.net"),
            (d, _) => d.to_string(),
        };
        let url = format!("https://{domain}");
        if !out.iter().any(|x| x.url == url) {
            out.push(Profile { name: s(&p, "name").to_string(), url, user: s(&p, "user").to_string() });
        }
    }
    out
}

/// Who twg signs in as.
pub fn me(site: &str) -> Result<String> {
    let v = run(site, &["whoami"])?;
    let name = s(&v, "name");
    Ok(if name.is_empty() { "signed in through twg".into() } else { format!("{name} (twg)") })
}

/// Jira projects: (key, name).
pub fn projects(site: &str) -> Result<Vec<(String, String)>> {
    Ok(list(&run(site, &["jira", "space", "query", "--limit", "100"])?, "projects").iter().map(|p| (s(p, "key").to_string(), s(p, "name").to_string())).filter(|(k, _)| !k.is_empty()).collect())
}

/// Jira issues for a JQL query, in the REST shape (`key`, `fields.summary`,
/// `fields.status`, `fields.description`, …).
pub fn issues(site: &str, jql: &str, limit: usize, fields: &str) -> Result<Vec<Value>> {
    let v = run(site, &["jira", "workitem", "query", "--jql", jql, "--limit", &limit.to_string(), "--fields", fields])?;
    Ok(list(&v, "issues").into_iter().map(rest_shape).collect())
}

/// One Jira issue in the REST shape.
pub fn issue(site: &str, key: &str) -> Result<Value> {
    let v = run(site, &["jira", "workitem", "get", key])?;
    list(&v, "issues").into_iter().next().map(rest_shape).ok_or_else(|| anyhow!("{key}: not found"))
}

/// twg lays an issue's fields out next to its key; the REST API nests them.
fn rest_shape(i: Value) -> Value {
    if i.get("fields").is_some() {
        return i;
    }
    serde_json::json!({ "key": i.get("key").cloned().unwrap_or(Value::Null), "fields": i })
}

/// The statuses seen on a project's issues, most recent first.
pub fn statuses(site: &str, project: &str) -> Result<Vec<String>> {
    let mut seen = Vec::<String>::new();
    for i in issues(site, &format!("project = \"{}\" ORDER BY updated DESC", project.trim().replace('"', "")), 100, "status")? {
        let n = i["fields"]["status"]["name"].as_str().unwrap_or("").to_string();
        if !n.is_empty() && !seen.contains(&n) {
            seen.push(n);
        }
    }
    Ok(seen)
}

/// Move an issue to a status (a transition or status name).
pub fn transition(site: &str, key: &str, to: &str) -> Result<()> {
    run(site, &["jira", "workitem", "transition", "--id", key, "--transition-id", to]).map(|_| ())
}

pub fn comment(site: &str, key: &str, text: &str) -> Result<()> {
    run(site, &["jira", "workitem", "comment", "create", "--issue-id", key, "--body", text, "--body-format", "plain"]).map(|_| ())
}

/// Confluence team spaces: (key, name).
pub fn spaces(site: &str) -> Result<Vec<(String, String)>> {
    Ok(list(&run(site, &["confluence", "space", "list", "--type", "global", "--limit", "250"])?, "results").iter().map(|p| (s(p, "key").to_string(), s(p, "name").to_string())).filter(|(k, _)| !k.is_empty()).collect())
}

/// Confluence search by CQL: (page id, title, excerpt).
pub fn search_pages(site: &str, cql: &str, limit: usize) -> Result<Vec<(String, String, String)>> {
    let v = run(site, &["confluence", "search", "query", "--cql", cql, "--limit", &limit.to_string()])?;
    Ok(list(&v, "results")
        .iter()
        .filter_map(|r| {
            let c = r.get("content")?;
            Some((s(c, "id").to_string(), s(c, "title").to_string(), s(r, "excerpt").to_string()))
        })
        .filter(|(id, _, _)| !id.is_empty())
        .collect())
}

/// A Confluence page as (title, markdown body).
pub fn page(site: &str, id: &str) -> Result<(String, String)> {
    let v = run(site, &["confluence", "content", "get", id, "--detail", "full", "--format", "md"])?;
    let b = &v["body"];
    let body = match b.get("outputFile").and_then(|f| f.as_str()) {
        Some(f) => std::fs::read_to_string(f).unwrap_or_default(),
        None => s(b, "value").to_string(),
    };
    Ok((s(&v, "title").to_string(), body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_hosts_and_shapes() {
        assert!(is_twg(" twg ") && !is_twg("${keychain:connection:x}"));
        assert_eq!(host("https://team.atlassian.net/wiki/spaces/X"), "team.atlassian.net");
        assert_eq!(host("team.atlassian.net"), "team.atlassian.net");
        let i = rest_shape(serde_json::json!({ "key": "PE-1", "summary": "s", "status": { "name": "Done" } }));
        assert_eq!(i["key"], "PE-1");
        assert_eq!(i["fields"]["summary"], "s");
        assert_eq!(i["fields"]["status"]["name"], "Done");
        let p = profiles_from(&serde_json::json!({ "profiles": [
            { "name": "a", "user": "me@x.test", "site": "team", "domain": "team.atlassian.net" },
            { "name": "b", "user": "me@x.test", "site": "team" },
            { "name": "c", "site": "other" },
            { "name": "d" }
        ] }));
        assert_eq!(p.iter().map(|x| x.url.as_str()).collect::<Vec<_>>(), ["https://team.atlassian.net", "https://other.atlassian.net"]);
        assert_eq!(p[0].user, "me@x.test");
    }
}
