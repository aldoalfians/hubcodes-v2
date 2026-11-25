//! Issue trackers: tickets flow onto the board and their status flows back.
//! Any tracker behind one small interface (`list`, `transition`, `comment`):
//! Jira and Plane natively, GitHub issues through `gh`, and anything else
//! through a command or an MCP server.
//!
//! ```toml
//! [[tracker]]
//! name = "jira"
//! kind = "jira"                                   # jira | plane | github | github-review | github-feedback | command | mcp
//! url = "https://hubstudio.atlassian.net"
//! email = "${JIRA_EMAIL}"
//! token = "${JIRA_TOKEN}"                         # or "twg": sign in through the twg CLI, no email
//! query = "project = PE AND status = \"Selected for Development\" AND assignee = currentUser() ORDER BY priority"
//! instruction = "/jira-task {key}"                # what the agent is told; {key} {title} {url} {description}
//! role = "software engineer"                      # which idle agents take these
//! on_start = "In Development"                     # transition when an agent picks it up ("" = none)
//! on_done = "In Review"                           # transition when the task is done or approved
//! comment_on_done = true                          # post the agent's result as a comment
//! limit = 2                                       # at most this many open board tasks from this tracker
//! poll_minutes = 2
//!
//! [[tracker]]
//! name = "plane"
//! kind = "plane"
//! url = "https://api.plane.so"
//! api_key = "${PLANE_API_KEY}"
//! workspace = "hubstudio"
//! project = "<project uuid>"
//! query = "Todo"                                  # state name
//! on_start = "In Progress"
//! on_done = "Done"
//!
//! [[tracker]]
//! name = "gh"
//! kind = "github"
//! repo = "hubstudio/app"
//! query = "--label ready --assignee @me"          # gh issue list filters
//! on_start = "in-progress"                        # labels added
//! on_done = "closed"                              # a label, or `closed`
//!
//! [[tracker]]
//! name = "reviews"
//! kind = "github-review"                          # PRs waiting for your review
//! repo = ""                                       # empty: every repository; or owner/repo
//! role = "reviewer"                               # one PR per idle reviewer; the rest queue
//! limit = 10
//! auto_approve = false                            # true: approve with "LGTM" when the reviewer finds nothing blocking
//!
//! [[tracker]]
//! name = "feedback"
//! kind = "github-feedback"                        # new comments on your own open PRs
//! role = "pr-fixer"                               # one round per PR at a time
//! limit = 10
//!
//! [[tracker]]
//! name = "any"
//! kind = "command"                                # JSON on stdin: {"op":"list"} | {"op":"transition","key","to"} | {"op":"comment","key","text"}
//! run = "my-tracker-cli"                          # list prints a JSON array of {key,title,url,description}
//! ```
//!
//! Every poll: new issues from `query` become board tasks (title `KEY title`,
//! detail = the filled instruction) up to `limit`; when a task is picked up
//! the issue moves to `on_start`; when it is done (or approved, when the repo
//! requires a review) the issue moves to `on_done` and gets the result as a
//! comment. The task carries `external = "<tracker>:<key>"`, so nothing is
//! imported twice.

use crate::db::Db;
use crate::mcp::{self, McpTarget};
use crate::tasks;
use crate::tend::Event;
use crate::tmux::Tmux;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tracker {
    pub name: String,
    /// jira | plane | github | github-review | github-feedback | command | mcp
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workspace: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub project: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub query: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub instruction: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on_start: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on_done: String,
    #[serde(default = "yes")]
    pub comment_on_done: bool,
    #[serde(default = "d_limit")]
    pub limit: u32,
    #[serde(default = "d_poll")]
    pub poll_minutes: u32,
    // command
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run: String,
    // mcp
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub list_tool: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub list_args: Value,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transition_tool: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub transition_args: Value,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment_tool: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub comment_args: Value,
    /// Feedback trackers: ignore feedback older than this (`2026-11-21` or
    /// RFC 3339); set to the day the tracker is added, so old threads on
    /// open pull requests do not all start at once.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub since: String,
    /// Feedback trackers: logins whose comments never start a round
    /// (`auto-reviewer, other`), for automated reviewers on a person's account.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ignore: String,
    /// Review trackers: review again when the author pushes after your
    /// review, looking at what changed since.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub rereview: bool,
    /// Review trackers: when the reviewer ends with `VERDICT: approve`,
    /// approve the pull request on GitHub with a "LGTM" comment instead of
    /// waiting for a person to press Approve.
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_approve: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn d_limit() -> u32 {
    2
}
fn d_poll() -> u32 {
    2
}

impl Default for Tracker {
    fn default() -> Self {
        Tracker {
            name: String::new(),
            kind: "jira".into(),
            url: String::new(),
            email: String::new(),
            token: String::new(),
            api_key: String::new(),
            workspace: String::new(),
            project: String::new(),
            repo: String::new(),
            query: String::new(),
            instruction: String::new(),
            role: String::new(),
            on_start: String::new(),
            on_done: String::new(),
            comment_on_done: true,
            limit: 2,
            poll_minutes: 2,
            run: String::new(),
            command: String::new(),
            args: Vec::new(),
            headers: BTreeMap::new(),
            list_tool: String::new(),
            list_args: Value::Null,
            since: String::new(),
            ignore: String::new(),
            rereview: true,
            auto_approve: false,
            transition_tool: String::new(),
            transition_args: Value::Null,
            comment_tool: String::new(),
            comment_args: Value::Null,
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub key: String,
    pub title: String,
    pub url: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub ms: u64,
    pub issues: Vec<Issue>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(25))).build().into()
}

fn base64(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = s.as_bytes();
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn http_error(url: &str, e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::StatusCode(code) => anyhow!("HTTP {code} from {url}{}", if code == 401 || code == 403 { " (check the credentials)" } else if code == 404 { " (check the URL / project)" } else { "" }),
        e => anyhow!("{url}: {e}"),
    }
}

fn read_json(mut resp: ureq::http::Response<ureq::Body>, url: &str) -> Result<Value> {
    let text = resp.body_mut().read_to_string()?;
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).with_context(|| format!("parse {url}"))
}

// ---------------------------------------------------------------- jira

fn jira_base(tr: &Tracker) -> String {
    mcp::expand_env(&tr.url).trim_end_matches('/').to_string()
}

fn jira_auth(tr: &Tracker) -> String {
    format!("Basic {}", base64(&format!("{}:{}", mcp::expand_env(&tr.email), mcp::expand_env(&tr.token))))
}

fn jira_get(tr: &Tracker, url: &str) -> Result<Value> {
    let resp = agent().get(url).header("Authorization", &jira_auth(tr)).header("Accept", "application/json").call().map_err(|e| http_error(url, e))?;
    read_json(resp, url)
}

fn jira_post(tr: &Tracker, url: &str, body: &Value) -> Result<Value> {
    let resp = agent().post(url).header("Authorization", &jira_auth(tr)).header("Accept", "application/json").header("Content-Type", "application/json").send(body.to_string().as_bytes()).map_err(|e| http_error(url, e))?;
    read_json(resp, url)
}

fn adf_text(v: &Value) -> String {
    fn walk(v: &Value, out: &mut String) {
        if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
            out.push_str(t);
        }
        if let Some(c) = v.get("content").and_then(|c| c.as_array()) {
            for x in c {
                walk(x, out);
                if matches!(x.get("type").and_then(|t| t.as_str()), Some("paragraph") | Some("heading") | Some("listItem")) {
                    out.push('\n');
                }
            }
        }
    }
    let mut s = String::new();
    walk(v, &mut s);
    s.trim().to_string()
}

fn jira_list(tr: &Tracker) -> Result<Vec<Issue>> {
    let base = jira_base(tr);
    let jql = if tr.query.trim().is_empty() { "assignee = currentUser() AND statusCategory != Done ORDER BY priority".to_string() } else { tr.query.clone() };
    let v = if crate::twg::is_twg(&tr.token) {
        json!({ "issues": crate::twg::issues(&base, &jql, 20, "summary,description,status")? })
    } else {
        jira_get(tr, &format!("{base}/rest/api/3/search/jql?jql={}&maxResults=20&fields=summary,description,status", url_encode(&jql)))?
    };
    Ok(v.get("issues")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|i| {
                    let key = i.get("key")?.as_str()?.to_string();
                    let f = i.get("fields")?;
                    Some(Issue {
                        url: format!("{base}/browse/{key}"),
                        title: f.get("summary").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                        description: adf_text(f.get("description").unwrap_or(&Value::Null)).chars().take(2000).collect(),
                        key,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

fn jira_transition(tr: &Tracker, key: &str, to: &str) -> Result<()> {
    let base = jira_base(tr);
    if crate::twg::is_twg(&tr.token) {
        return crate::twg::transition(&base, key, to);
    }
    let v = jira_get(tr, &format!("{base}/rest/api/3/issue/{key}/transitions"))?;
    let id = v
        .get("transitions")
        .and_then(|a| a.as_array())
        .and_then(|a| a.iter().find(|x| x.get("name").and_then(|n| n.as_str()).map(|n| n.eq_ignore_ascii_case(to)).unwrap_or(false) || x.get("to").and_then(|t| t.get("name")).and_then(|n| n.as_str()).map(|n| n.eq_ignore_ascii_case(to)).unwrap_or(false)))
        .and_then(|x| x.get("id"))
        .and_then(|i| i.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("{key}: no transition to '{to}' from its current status"))?;
    jira_post(tr, &format!("{base}/rest/api/3/issue/{key}/transitions"), &json!({ "transition": { "id": id } }))?;
    Ok(())
}

fn jira_comment(tr: &Tracker, key: &str, text: &str) -> Result<()> {
    let base = jira_base(tr);
    if crate::twg::is_twg(&tr.token) {
        return crate::twg::comment(&base, key, text);
    }
    let paragraphs: Vec<Value> = text.split('\n').map(|l| json!({ "type": "paragraph", "content": if l.is_empty() { vec![] } else { vec![json!({ "type": "text", "text": l })] } })).collect();
    jira_post(tr, &format!("{base}/rest/api/3/issue/{key}/comment"), &json!({ "body": { "type": "doc", "version": 1, "content": paragraphs } }))?;
    Ok(())
}

// ---------------------------------------------------------------- plane

fn plane_base(tr: &Tracker) -> String {
    let u = mcp::expand_env(&tr.url);
    let u = if u.trim().is_empty() { "https://api.plane.so".to_string() } else { u };
    format!("{}/api/v1/workspaces/{}/projects/{}", u.trim_end_matches('/'), mcp::expand_env(&tr.workspace), mcp::expand_env(&tr.project))
}

fn plane_call(tr: &Tracker, method: &str, url: &str, body: Option<&Value>) -> Result<Value> {
    let key = mcp::expand_env(&tr.api_key);
    let a = agent();
    let resp = match (method, body) {
        ("GET", _) => a.get(url).header("X-API-Key", &key).header("Accept", "application/json").call(),
        ("PATCH", Some(b)) => a.patch(url).header("X-API-Key", &key).header("Content-Type", "application/json").send(b.to_string().as_bytes()),
        (_, Some(b)) => a.post(url).header("X-API-Key", &key).header("Content-Type", "application/json").send(b.to_string().as_bytes()),
        _ => a.post(url).header("X-API-Key", &key).send_empty(),
    }
    .map_err(|e| http_error(url, e))?;
    read_json(resp, url)
}

fn plane_states(tr: &Tracker) -> Result<Vec<(String, String)>> {
    let v = plane_call(tr, "GET", &format!("{}/states/", plane_base(tr)), None)?;
    let arr = v.get("results").and_then(|r| r.as_array()).cloned().or_else(|| v.as_array().cloned()).unwrap_or_default();
    Ok(arr.iter().filter_map(|s| Some((s.get("id")?.as_str()?.to_string(), s.get("name")?.as_str()?.to_string()))).collect())
}

fn plane_state_id(tr: &Tracker, name: &str) -> Result<String> {
    plane_states(tr)?.into_iter().find(|(_, n)| n.eq_ignore_ascii_case(name)).map(|(id, _)| id).ok_or_else(|| anyhow!("Plane: no state called '{name}'"))
}

fn plane_list(tr: &Tracker) -> Result<Vec<Issue>> {
    let states = plane_states(tr)?;
    let want: Vec<&str> = tr.query.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let ids: Vec<&String> = states.iter().filter(|(_, n)| want.is_empty() || want.iter().any(|w| w.eq_ignore_ascii_case(n))).map(|(id, _)| id).collect();
    let base = plane_base(tr);
    let v = plane_call(tr, "GET", &format!("{base}/issues/?per_page=100&order_by=-created_at"), None)?;
    let arr = v.get("results").and_then(|r| r.as_array()).cloned().or_else(|| v.as_array().cloned()).unwrap_or_default();
    let web = format!("{}/{}/projects/{}/issues", mcp::expand_env(&tr.url).trim_end_matches('/').replace("api.plane.so", "app.plane.so"), mcp::expand_env(&tr.workspace), mcp::expand_env(&tr.project));
    Ok(arr
        .iter()
        .filter(|i| i.get("state").and_then(|s| s.as_str()).map(|s| ids.iter().any(|id| id.as_str() == s)).unwrap_or(false))
        .filter_map(|i| {
            let id = i.get("id")?.as_str()?.to_string();
            let seq = i.get("sequence_id").and_then(|s| s.as_i64()).unwrap_or(0);
            Some(Issue {
                key: id.clone(),
                title: format!("#{seq} {}", i.get("name").and_then(|n| n.as_str()).unwrap_or("")),
                url: format!("{web}/{id}"),
                description: i.get("description_stripped").and_then(|d| d.as_str()).unwrap_or("").chars().take(2000).collect(),
            })
        })
        .collect())
}

fn plane_transition(tr: &Tracker, key: &str, to: &str) -> Result<()> {
    let state = plane_state_id(tr, to)?;
    plane_call(tr, "PATCH", &format!("{}/issues/{key}/", plane_base(tr)), Some(&json!({ "state": state })))?;
    Ok(())
}

fn plane_comment(tr: &Tracker, key: &str, text: &str) -> Result<()> {
    let html = format!("<p>{}</p>", text.replace('&', "&amp;").replace('<', "&lt;").replace('\n', "<br/>"));
    plane_call(tr, "POST", &format!("{}/issues/{key}/comments/", plane_base(tr)), Some(&json!({ "comment_html": html })))?;
    Ok(())
}

// ---------------------------------------------------------------- github (gh)

pub(crate) fn gh(args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("gh").args(args).output().map_err(|e| anyhow!("gh: {e} (is GitHub CLI installed?)"))?;
    if !out.status.success() {
        return Err(anyhow!("gh {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn github_list(tr: &Tracker) -> Result<Vec<Issue>> {
    let mut args = vec!["issue", "list", "--repo", &tr.repo, "--state", "open", "--json", "number,title,url,body", "--limit", "30"];
    let extra = crate::triggers::words(&tr.query);
    let extra_refs: Vec<&str> = extra.iter().map(|s| s.as_str()).collect();
    args.extend(extra_refs);
    let v: Value = serde_json::from_str(&gh(&args)?)?;
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| {
                    Some(Issue {
                        key: format!("#{}", i.get("number")?.as_u64()?),
                        title: i.get("title")?.as_str()?.to_string(),
                        url: i.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                        description: i.get("body").and_then(|b| b.as_str()).unwrap_or("").chars().take(2000).collect(),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

fn github_transition(tr: &Tracker, key: &str, to: &str) -> Result<()> {
    let n = key.trim_start_matches('#');
    if to.eq_ignore_ascii_case("closed") {
        gh(&["issue", "close", n, "--repo", &tr.repo])?;
    } else {
        gh(&["issue", "edit", n, "--repo", &tr.repo, "--add-label", to])?;
    }
    Ok(())
}

fn github_comment(tr: &Tracker, key: &str, text: &str) -> Result<()> {
    gh(&["issue", "comment", key.trim_start_matches('#'), "--repo", &tr.repo, "--body", text])?;
    Ok(())
}

// ---------------------------------------------------------------- github review requests

/// Open pull requests that ask the signed-in `gh` user for a review. Key is
/// `owner/repo#N`. `repo` narrows to one repository (or `owner/*` to one
/// owner); `query` takes extra `gh search prs` flags.
fn github_review_list(tr: &Tracker) -> Result<Vec<Issue>> {
    let mut args: Vec<String> = ["search", "prs", "--review-requested=@me", "--state=open", "--json", "number,title,url,repository,author,isDraft", "--limit", "50"].map(String::from).to_vec();
    let repo = tr.repo.trim();
    if let Some(owner) = repo.strip_suffix("/*") {
        args.push(format!("--owner={owner}"));
    } else if !repo.is_empty() {
        args.push(format!("--repo={repo}"));
    }
    args.extend(crate::triggers::words(&tr.query));
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let v: Value = serde_json::from_str(&gh(&refs)?)?;
    Ok(review_issues(&v))
}

/// Pull requests you reviewed whose author pushed since: what is left to
/// look at, with the time of the newest commit.
const REVIEWED: &str = "query($q: String!, $me: String!) { search(query: $q, type: ISSUE, first: 50) { nodes { ... on PullRequest { number title url isDraft author { login } repository { nameWithOwner } commits(last: 1) { nodes { commit { oid committedDate } } } reviews(last: 1, author: $me) { nodes { submittedAt commit { oid } } } } } } }";

fn github_rereview_list(tr: &Tracker) -> Result<Vec<(Issue, i64)>> {
    let me = crate::feedback::me();
    if me.is_empty() {
        return Ok(Vec::new());
    }
    let q = format!("is:pr is:open reviewed-by:@me -author:@me archived:false{}", crate::feedback::scope(&tr.repo));
    let v: Value = serde_json::from_str(&gh(&["api", "graphql", "-f", &format!("query={REVIEWED}"), "-f", &format!("q={q}"), "-f", &format!("me={me}")])?)?;
    Ok(rereview_issues(&v, &me, crate::db::now()))
}

/// New commits after your last review, quiet for a few minutes (a push of
/// several commits is one re-review).
pub fn rereview_issues(v: &Value, me: &str, now: i64) -> Vec<(Issue, i64)> {
    let s = |v: &Value, p: &str| v.pointer(p).and_then(|x| x.as_str()).unwrap_or("").to_string();
    v.pointer("/data/search/nodes")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .filter(|p| !p.get("isDraft").and_then(|d| d.as_bool()).unwrap_or(false) && !s(p, "/author/login").eq_ignore_ascii_case(me))
        .filter_map(|p| {
            let head = p.pointer("/commits/nodes/0/commit")?;
            let review = p.pointer("/reviews/nodes/0")?;
            let (head_oid, head_at) = (s(head, "/oid"), crate::feedback::ts(&s(head, "/committedDate")));
            let (seen_oid, seen_at) = (s(review, "/commit/oid"), crate::feedback::ts(&s(review, "/submittedAt")));
            if head_oid.is_empty() || head_oid == seen_oid || head_at <= seen_at || head_at > now - crate::feedback::QUIET_SECS {
                return None;
            }
            let repo = s(p, "/repository/nameWithOwner");
            let author = s(p, "/author/login");
            let short = |o: &str| o.chars().take(10).collect::<String>();
            Some((
                Issue {
                    key: format!("{repo}#{}", p.get("number")?.as_u64()?),
                    title: s(p, "/title"),
                    url: s(p, "/url"),
                    description: format!(
                        "Pull request by @{author} in {repo}. You reviewed it at {}; @{author} has pushed since. Review again, only what changed: `git diff {}..HEAD` in the checkout. Say which of your earlier points are now addressed and what is new.",
                        short(&seen_oid),
                        if seen_oid.is_empty() { "<your last reviewed commit>".to_string() } else { short(&seen_oid) }
                    ),
                },
                head_at,
            ))
        })
        .collect()
}

fn review_issues(v: &Value) -> Vec<Issue> {
    let Some(a) = v.as_array() else { return Vec::new() };
    a.iter()
        .filter(|p| !p.get("isDraft").and_then(|d| d.as_bool()).unwrap_or(false))
        .filter_map(|p| {
            let repo = p.pointer("/repository/nameWithOwner").and_then(|r| r.as_str())?;
            let author = p.pointer("/author/login").and_then(|r| r.as_str()).unwrap_or("someone");
            Some(Issue {
                key: format!("{repo}#{}", p.get("number")?.as_u64()?),
                title: p.get("title")?.as_str()?.to_string(),
                url: p.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                description: format!("Pull request by @{author} in {repo}."),
            })
        })
        .collect()
}

fn split_pr(key: &str) -> Result<(&str, &str)> {
    key.rsplit_once('#').ok_or_else(|| anyhow!("'{key}' is not owner/repo#N"))
}

fn github_review_comment(key: &str, text: &str) -> Result<()> {
    let (repo, n) = split_pr(key)?;
    gh(&["pr", "comment", n, "--repo", repo, "--body", text])?;
    Ok(())
}

pub const REVIEW_INSTRUCTION: &str = "Review pull request {key}: {title}\n{url}\n\n{description}\n\nFirst run `hubcodes review checkout {key}`: it prints a folder with this pull request checked out, separate from every other review and from your own work. Read and run things there only, and do not commit or push from it. Read the description, the linked ticket and the diff (`gh pr view` / `gh pr diff`), and run the affected tests if you can.\n\nThen post one review in English. Mark every finding: 🔴 blocks merging (a bug, data loss, a security hole, a broken contract, a merge conflict), 🟡 should be fixed but does not block, `needs_validation` when it depends on a fact you could not check (name the missing fact). A finding about a changed line goes on that line as an inline comment: what is wrong, why, and the smallest fix. Post it with `gh api repos/OWNER/REPO/pulls/N/reviews -X POST --input review.json`, where review.json is `{\"event\": \"COMMENT\", \"body\": \"...\", \"comments\": [{\"path\": \"src/a.go\", \"line\": 42, \"side\": \"RIGHT\", \"body\": \"🔴 ...\"}]}` (`line` is the line number in the new file and must be in the diff; a finding that is not on a changed line goes in `body` only). The body lists what you ran (build, tests: passed, failed, not run), a count line such as `2 🔴, 4 🟡, 1 needs_validation`, and every finding numbered with its marker, ending in `(inline)` when it has an inline comment. Nothing found: post nothing. Do not approve or request changes on GitHub yourself.\n\nEnd your `hubcodes task done` result with the count line, then a last line `VERDICT: approve` when there is no 🔴, 🟡 or `needs_validation` finding and the tests you ran passed, or `VERDICT: changes` otherwise. On approve hubcodes approves with LGTM, or shows the person an Approve button.";

/// `synced` of a review a person approved on GitHub from hubcodes.
pub const APPROVED: &str = "approved";
/// `synced` of a pull request a person told hubcodes to leave alone.
pub const IGNORED: &str = "ignored";

/// The reviewer's result ends with `VERDICT: approve`.
pub fn verdict_approve(result: &str) -> bool {
    result.lines().rev().map(str::trim).find(|l| !l.is_empty()).map(|l| l.to_ascii_lowercase().replace(['*', '`'], "")).map(|l| l.starts_with("verdict:") && l["verdict:".len()..].trim() == "approve").unwrap_or(false)
}

/// The result ends with a verdict line, `VERDICT: approve` or `VERDICT: changes`.
pub fn has_verdict(result: &str) -> bool {
    result.lines().rev().map(str::trim).find(|l| !l.is_empty()).map(|l| l.to_ascii_lowercase().replace(['*', '`'], "")).map(|l| l.starts_with("verdict:") && matches!(l["verdict:".len()..].trim(), "approve" | "changes")).unwrap_or(false)
}

/// A pull request review a tracker handed out (not a feedback round).
pub fn is_review_task(t: &tasks::Task) -> bool {
    pr_of(t).is_some() && (t.title.starts_with("Review ") || t.title.starts_with("Re-review "))
}

/// `reviews:owner/repo#12`: the tracker name and the pull request.
fn pr_of(t: &tasks::Task) -> Option<(&str, &str)> {
    let (name, key) = t.external.split_once(':')?;
    let (repo, n) = key.rsplit_once('#')?;
    (repo.contains('/') && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then_some((name, key))
}

/// A finished review whose agent found nothing blocking, not yet approved.
pub fn ready_to_approve(t: &tasks::Task) -> bool {
    pr_of(t).is_some() && (t.title.starts_with("Review ") || t.title.starts_with("Re-review ")) && t.status == "done" && matches!(t.synced.as_str(), "" | "started" | "done") && verdict_approve(&t.result)
}

/// The comment that goes with an approval.
pub const APPROVE_BODY: &str = "LGTM";

/// Approve the pull request on GitHub with a "LGTM" comment, as the `gh`
/// sign-in: because a person pressed the button, or because the review
/// tracker has `auto_approve` and the reviewer found nothing blocking.
pub fn approve(db: &Db, id: i64, by: &str) -> Result<String> {
    let t = tasks::get(db, id)?.ok_or_else(|| anyhow!("no task #{id}"))?;
    if !ready_to_approve(&t) {
        return Err(anyhow!("task #{id} is not a finished review that recommends approving"));
    }
    let (_, key) = pr_of(&t).ok_or_else(|| anyhow!("task #{id} is not a pull request"))?;
    let (repo, n) = split_pr(key)?;
    gh(&["pr", "review", n, "--repo", repo, "--approve", "--body", APPROVE_BODY])?;
    tasks::set_synced(db, id, APPROVED)?;
    let _ = db.add_event(by, "review", &format!("approved {key} on GitHub (review task #{id})"));
    Ok(format!("Approved {key}"))
}

/// Stop handling a pull request: its queued task is set aside, no further
/// rounds or reviews start, and its checkouts go. Not while an agent works on it.
pub fn ignore(db: &Db, id: i64, by: &str) -> Result<String> {
    let t = tasks::get(db, id)?.ok_or_else(|| anyhow!("no task #{id}"))?;
    let (_, key) = pr_of(&t).ok_or_else(|| anyhow!("task #{id} is not a pull request"))?;
    if t.status != "done" && !(t.status == "todo" && t.owner.is_empty()) {
        return Err(anyhow!("{} is working on {key}; wait for it to finish", if t.owner.is_empty() { "an agent" } else { &t.owner }));
    }
    let key = key.to_string();
    if t.status != "done" {
        tasks::set_status(db, id, "done", Some("Ignored by a person"))?;
    }
    tasks::set_synced(db, id, IGNORED)?;
    let _ = crate::reviews::cleanup(&key);
    let _ = crate::reviews::cleanup_branch(&key);
    let _ = db.add_event(by, "review", &format!("ignoring {key} from now on (task #{id})"));
    Ok(format!("Ignoring {key}"))
}

/// What a review tracker does with this poll's list: which PRs are new, which
/// queued ones were withdrawn (reviewed by someone else, closed, merged, no
/// longer asking you), and which finished reviews may come back. A PR leaves
/// the list once your review is in, so seeing it again means a new request.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReviewPlan {
    pub import: Vec<Issue>,
    /// Queued tasks (not started) whose PR no longer asks for you.
    pub withdraw: Vec<i64>,
    /// Finished tasks whose PR has left the list: a later request is new work.
    pub release: Vec<i64>,
}

pub fn review_plan(prefix: &str, tasks: &[tasks::Task], issues: Vec<Issue>, again: Vec<(Issue, i64)>) -> ReviewPlan {
    let mut plan = ReviewPlan::default();
    // A re-review of a pull request that is also asking for you is the request.
    let again: Vec<(Issue, i64)> = again.into_iter().filter(|(a, _)| !issues.iter().any(|i| i.key == a.key)).collect();
    let listed = |t: &tasks::Task| issues.iter().map(|i| &i.key).chain(again.iter().map(|(i, _)| &i.key)).any(|k| t.external == format!("{prefix}{k}"));
    for t in tasks.iter().filter(|t| t.external.starts_with(prefix)) {
        if listed(t) {
            continue;
        }
        if t.status == "todo" && t.owner.is_empty() {
            plan.withdraw.push(t.id);
        } else if t.status == "done" && t.synced != "released" && t.synced != IGNORED {
            plan.release.push(t.id);
        }
    }
    for i in issues {
        let ext = format!("{prefix}{}", i.key);
        let held = tasks.iter().any(|t| t.external == ext && !(t.status == "done" && t.synced == "released"));
        if !held {
            plan.import.push(i);
        }
    }
    // Pushed after a review this tracker ran (not old reviews made by hand):
    // once, unless a review task already started after that push.
    for (i, pushed) in again {
        let ext = format!("{prefix}{}", i.key);
        let ours = tasks.iter().any(|t| t.external == ext) && !tasks.iter().any(|t| t.external == ext && t.synced == IGNORED);
        let busy = tasks.iter().any(|t| t.external == ext && (t.status != "done" || t.at >= pushed));
        if ours && !busy {
            plan.import.push(i);
        }
    }
    plan
}

// ---------------------------------------------------------------- command / mcp

fn run_json(cmd: &str, input: &Value) -> Result<String> {
    use std::io::Write;
    let mut child = std::process::Command::new("/bin/sh").args(["-lc", &mcp::expand_env(cmd)]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().map_err(|e| anyhow!("run {cmd}: {e}"))?;
    if let Some(mut s) = child.stdin.take() {
        let _ = s.write_all(input.to_string().as_bytes());
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(anyhow!("{cmd} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn issues_from(text: &str) -> Vec<Issue> {
    let v: Value = serde_json::from_str(text.trim()).unwrap_or(Value::Null);
    let arr = v.as_array().cloned().or_else(|| v.get("issues").and_then(|a| a.as_array()).cloned()).or_else(|| v.get("results").and_then(|a| a.as_array()).cloned()).or_else(|| v.get("items").and_then(|a| a.as_array()).cloned()).unwrap_or_default();
    arr.iter()
        .filter_map(|i| {
            let key = i.get("key").or_else(|| i.get("id")).or_else(|| i.get("number")).map(|k| k.as_str().map(String::from).unwrap_or_else(|| k.to_string()))?;
            Some(Issue {
                key,
                title: i.get("title").or_else(|| i.get("summary")).or_else(|| i.get("name")).and_then(|t| t.as_str()).unwrap_or("").to_string(),
                url: i.get("url").or_else(|| i.get("link")).and_then(|u| u.as_str()).unwrap_or("").to_string(),
                description: i.get("description").or_else(|| i.get("body")).and_then(|d| d.as_str()).unwrap_or("").chars().take(2000).collect(),
            })
        })
        .collect()
}

fn mcp_target(tr: &Tracker) -> McpTarget {
    McpTarget { url: tr.url.clone(), command: tr.command.clone(), args: tr.args.clone(), headers: tr.headers.clone(), env: BTreeMap::new() }
}

// ---------------------------------------------------------------- interface

pub fn list(tr: &Tracker) -> Result<Vec<Issue>> {
    match tr.kind.as_str() {
        "jira" => jira_list(tr),
        "plane" => plane_list(tr),
        "github" => github_list(tr),
        "github-review" => {
            let mut all = github_review_list(tr)?;
            if tr.rereview {
                let again = github_rereview_list(tr)?;
                all.extend(again.into_iter().map(|(i, _)| i).filter(|a| !all.iter().any(|i| i.key == a.key)).collect::<Vec<_>>());
            }
            Ok(all)
        }
        "github-feedback" => crate::feedback::list(tr),
        "command" => Ok(issues_from(&run_json(&tr.run, &json!({ "op": "list", "query": tr.query }))?)),
        "mcp" => {
            let mut c = mcp::Client::connect(&mcp_target(tr))?;
            let tool = if tr.list_tool.is_empty() { "list_issues" } else { &tr.list_tool };
            let args = if tr.list_args.is_null() { json!({ "query": "{query}" }) } else { tr.list_args.clone() };
            let (text, raw) = c.call_tool(tool, mcp::fill_args(&args, &[("query", &tr.query)]))?;
            let mut v = issues_from(&text);
            if v.is_empty() {
                v = issues_from(&raw.to_string());
            }
            Ok(v)
        }
        other => Err(anyhow!("unknown tracker kind '{other}'")),
    }
}

pub fn transition(tr: &Tracker, key: &str, to: &str) -> Result<()> {
    if to.trim().is_empty() {
        return Ok(());
    }
    match tr.kind.as_str() {
        "jira" => jira_transition(tr, key, to),
        "plane" => plane_transition(tr, key, to),
        "github" => github_transition(tr, key, to),
        // A review request, or feedback on your own pull request, has no status to move.
        "github-review" | "github-feedback" => Ok(()),
        "command" => run_json(&tr.run, &json!({ "op": "transition", "key": key, "to": to })).map(|_| ()),
        "mcp" => {
            if tr.transition_tool.is_empty() {
                return Ok(());
            }
            let mut c = mcp::Client::connect(&mcp_target(tr))?;
            let args = if tr.transition_args.is_null() { json!({ "key": "{key}", "status": "{to}" }) } else { tr.transition_args.clone() };
            c.call_tool(&tr.transition_tool, mcp::fill_args(&args, &[("key", key), ("to", to)])).map(|_| ())
        }
        other => Err(anyhow!("unknown tracker kind '{other}'")),
    }
}

pub fn comment(tr: &Tracker, key: &str, text: &str) -> Result<()> {
    match tr.kind.as_str() {
        "jira" => jira_comment(tr, key, text),
        "plane" => plane_comment(tr, key, text),
        "github" => github_comment(tr, key, text),
        "github-review" | "github-feedback" => github_review_comment(key, text),
        "command" => run_json(&tr.run, &json!({ "op": "comment", "key": key, "text": text })).map(|_| ()),
        "mcp" => {
            if tr.comment_tool.is_empty() {
                return Ok(());
            }
            let mut c = mcp::Client::connect(&mcp_target(tr))?;
            let args = if tr.comment_args.is_null() { json!({ "key": "{key}", "text": "{text}" }) } else { tr.comment_args.clone() };
            c.call_tool(&tr.comment_tool, mcp::fill_args(&args, &[("key", key), ("text", text)])).map(|_| ())
        }
        other => Err(anyhow!("unknown tracker kind '{other}'")),
    }
}

/// Reach the tracker and show what the query returns.
pub fn check(tr: &Tracker) -> Check {
    let t0 = std::time::Instant::now();
    match list(tr) {
        Ok(issues) => Check { name: tr.name.clone(), ok: true, detail: format!("{} issue{} match the query", issues.len(), if issues.len() == 1 { "" } else { "s" }), ms: t0.elapsed().as_millis() as u64, issues: issues.into_iter().take(10).collect() },
        Err(e) => Check { name: tr.name.clone(), ok: false, detail: format!("{e:#}"), ms: t0.elapsed().as_millis() as u64, issues: vec![] },
    }
}

/// What the agent is told for an issue.
pub fn instruction_for(tr: &Tracker, i: &Issue) -> String {
    let tpl = if !tr.instruction.trim().is_empty() {
        tr.instruction.clone()
    } else if tr.kind == "github-review" {
        REVIEW_INSTRUCTION.to_string()
    } else if tr.kind == "github-feedback" {
        crate::feedback::INSTRUCTION.to_string()
    } else {
        "Work on {key}: {title}\n{url}\n\n{description}".to_string()
    };
    let mut s = tpl.replace("{key}", &i.key).replace("{title}", &i.title).replace("{url}", &i.url).replace("{description}", &i.description);
    if !tpl.contains("{url}") && !i.url.is_empty() {
        // `/pr-review` alone: the URL is its argument, on the same line.
        let sep = if tpl.trim_start().starts_with('/') && !tpl.trim().contains('\n') { " " } else { "\n" };
        s = format!("{}{sep}{}", s.trim_end(), i.url);
    }
    // Own review instructions (`/pr-review`) still need the verdict line the
    // Approve button and `auto_approve` read.
    if tr.kind == "github-review" && !s.to_ascii_uppercase().contains("VERDICT:") {
        s = format!("{}\n\n{VERDICT_RULE}", s.trim_end());
    }
    s
}

/// Added to a review instruction that does not ask for a verdict itself.
pub const VERDICT_RULE: &str = "Mark every finding 🔴 (blocks merging), 🟡 (should be fixed) or `needs_validation` (name the missing fact), and put a finding about a changed line on that line as an inline review comment. Do not approve or request changes on GitHub yourself, and do not stop to ask the person whether to approve: hubcodes approves a clean review (or shows them an Approve button). End your `hubcodes task done` result with a count line such as `2 🔴, 4 🟡, 1 needs_validation`, then a last line `VERDICT: approve` when there is no 🔴, 🟡 or `needs_validation` finding and the tests you ran passed, or `VERDICT: changes` otherwise.";

/// `name:` prefixes of the trackers that are off: their tasks already on the
/// board are not handed to agents until the tracker is on again.
pub fn held(trackers: &[Tracker]) -> Vec<String> {
    trackers.iter().filter(|t| !t.enabled && !t.name.is_empty()).map(|t| format!("{}:", t.name)).collect()
}

/// Import new issues and push status back. Called from the poll loop;
/// `polled` remembers when each tracker was last asked.
pub fn tick(t: &Tmux, trackers: &[Tracker], polled: &mut std::collections::HashMap<String, i64>, force: bool) -> Vec<Event> {
    let mut events = Vec::new();
    let Ok(db) = Db::open(&t.session) else { return events };
    let now = crate::db::now();
    for tr in trackers.iter().filter(|x| x.enabled && !x.name.is_empty()) {
        // Status back to the tracker first (cheap, every tick).
        events.extend(push(&db, tr));
        let last = polled.get(&tr.name).copied().unwrap_or(0);
        if !force && now - last < (tr.poll_minutes.max(1) as i64) * 60 {
            continue;
        }
        polled.insert(tr.name.clone(), now);
        events.extend(pull(&db, tr));
    }
    events
}

/// New issues → board tasks, up to the tracker's limit of open ones.
pub fn pull(db: &Db, tr: &Tracker) -> Vec<Event> {
    if tr.kind == "github-review" {
        return pull_reviews(db, tr);
    }
    if tr.kind == "github-feedback" {
        return crate::feedback::pull(db, tr);
    }
    let mut events = Vec::new();
    let open: Vec<tasks::Task> = tasks::list(db, true).unwrap_or_default().into_iter().filter(|x| x.external.starts_with(&format!("{}:", tr.name))).collect();
    let open_count = open.iter().filter(|x| x.status != "done").count() as u32;
    if open_count >= tr.limit.max(1) {
        return events;
    }
    let issues = match list(tr) {
        Ok(i) => i,
        Err(e) => {
            events.push(Event { kind: "error".into(), text: format!("Tracker {}: {e:#}", tr.name) });
            return events;
        }
    };
    let mut room = tr.limit.max(1) - open_count;
    for i in issues {
        if room == 0 {
            break;
        }
        let ext = format!("{}:{}", tr.name, i.key);
        if open.iter().any(|x| x.external == ext) {
            continue;
        }
        let title = format!("{} {}", i.key, i.title).chars().take(140).collect::<String>();
        let detail = instruction_for(tr, &i);
        match tasks::add_external(db, &title, &detail, &tr.role, &format!("tracker:{}", tr.name), &ext, &i.url) {
            Ok(task) => {
                room -= 1;
                let _ = db.add_event("hubcodes", "tracker", &format!("imported {} as task #{}", i.key, task.id));
                events.push(Event { kind: "task".into(), text: format!("{}: {} → task #{}", tr.name, i.key, task.id) });
            }
            Err(e) => events.push(Event { kind: "error".into(), text: format!("Tracker {}: {e:#}", tr.name) }),
        }
    }
    events
}

/// Review requests: queue new ones (oldest first, up to `limit` waiting or
/// in progress), drop queued ones that no longer ask for you, and let a
/// re-request after a finished review in again.
fn pull_reviews(db: &Db, tr: &Tracker) -> Vec<Event> {
    let mut events = Vec::new();
    let issues = match github_review_list(tr) {
        Ok(i) => i,
        Err(e) => return vec![Event { kind: "error".into(), text: format!("Tracker {}: {e:#}", tr.name) }],
    };
    // A failed re-review search must not withdraw anything: skip this poll.
    let again = if tr.rereview {
        match github_rereview_list(tr) {
            Ok(a) => a,
            Err(e) => return vec![Event { kind: "error".into(), text: format!("Tracker {}: {e:#}", tr.name) }],
        }
    } else {
        Vec::new()
    };
    let prefix = format!("{}:", tr.name);
    let mine: Vec<tasks::Task> = tasks::list(db, true).unwrap_or_default().into_iter().filter(|x| x.external.starts_with(&prefix)).collect();
    let mut plan = review_plan(&prefix, &mine, issues, again);
    for id in &plan.withdraw {
        if tasks::remove(db, *id).is_ok() {
            let _ = db.add_event("hubcodes", "tracker", &format!("review task #{id} withdrawn: the pull request no longer asks for you"));
            events.push(Event { kind: "review".into(), text: format!("{}: task #{id} withdrawn (no longer requested)", tr.name) });
        }
    }
    for id in &plan.release {
        let _ = tasks::set_synced(db, *id, "released");
        // Reviewed and off GitHub's list: its checkout is no longer needed.
        if let Some(t) = mine.iter().find(|t| t.id == *id) {
            let _ = crate::reviews::cleanup(&t.external[prefix.len()..]);
        }
    }
    let open = mine.iter().filter(|t| t.status != "done" && !plan.withdraw.contains(&t.id)).count() as u32;
    let room = tr.limit.max(1).saturating_sub(open) as usize;
    // gh lists newest first; the queue goes oldest first.
    plan.import.reverse();
    for i in plan.import.into_iter().take(room) {
        let ext = format!("{prefix}{}", i.key);
        let verb = if i.description.contains("Review again") { "Re-review" } else { "Review" };
        let title = format!("{verb} {} {}", i.key, i.title).chars().take(140).collect::<String>();
        match tasks::add_external(db, &title, &instruction_for(tr, &i), &tr.role, &format!("tracker:{}", tr.name), &ext, &i.url) {
            Ok(task) => {
                let _ = db.add_event("hubcodes", "tracker", &format!("review request {} queued as task #{}", i.key, task.id));
                events.push(Event { kind: "review".into(), text: format!("{}: review {} → task #{}", tr.name, i.key, task.id) });
            }
            Err(e) => events.push(Event { kind: "error".into(), text: format!("Tracker {}: {e:#}", tr.name) }),
        }
    }
    events
}

/// What the orchestrator is told when review requests come or go: what
/// changed and the queue now. For its information; reviewers take the
/// requests on their own.
pub fn review_digest(db: &Db, trackers: &[Tracker], changes: &[String]) -> Option<String> {
    let names: Vec<&str> = trackers.iter().filter(|t| t.enabled && (t.kind == "github-review" || t.kind == "github-feedback")).map(|t| t.name.as_str()).collect();
    if names.is_empty() || changes.is_empty() {
        return None;
    }
    let open: Vec<tasks::Task> = tasks::list(db, false).unwrap_or_default();
    Some(digest_text(&names, &open, changes))
}

fn digest_text(names: &[&str], open: &[tasks::Task], changes: &[String]) -> String {
    let open: Vec<&tasks::Task> = open.iter().filter(|t| t.status != "done" && names.iter().any(|n| t.external.starts_with(&format!("{n}:")))).collect();
    let waiting: Vec<String> = open.iter().filter(|t| t.status == "todo" && t.owner.is_empty()).map(|t| format!("#{}", t.id)).collect();
    let reviewing: Vec<String> = open.iter().filter(|t| t.status != "todo" || !t.owner.is_empty()).map(|t| format!("#{} → {}", t.id, if t.owner.is_empty() { "?" } else { &t.owner })).collect();
    let list = |v: &[String]| if v.is_empty() { String::new() } else { format!(" ({})", v.join(", ")) };
    format!(
        "[hubcodes] GitHub reviews and feedback: {}. Queue now: {} waiting{}, {} in progress{}. Agents take them oldest first on their own, so there is nothing to assign. To change the order: `hubcodes task place <id> todo --before <id>`. For your information; no reply needed.",
        changes.join("; "),
        waiting.len(),
        list(&waiting),
        reviewing.len(),
        list(&reviewing)
    )
}

/// Transitions and comments for tasks whose status moved.
pub fn push(db: &Db, tr: &Tracker) -> Vec<Event> {
    let mut events = Vec::new();
    let prefix = format!("{}:", tr.name);
    for task in tasks::list(db, true).unwrap_or_default().into_iter().filter(|x| x.external.starts_with(&prefix)) {
        let key = &task.external[prefix.len()..];
        if task.status == "doing" && task.synced.is_empty() {
            let r = transition(tr, key, &tr.on_start);
            let _ = tasks::set_synced(db, task.id, "started");
            match r {
                Ok(()) if !tr.on_start.is_empty() => events.push(Event { kind: "tracker".into(), text: format!("{key} → {}", tr.on_start) }),
                Ok(()) => {}
                Err(e) => events.push(Event { kind: "error".into(), text: format!("{key}: {e:#}") }),
            }
        }
        if task.status == "done" && matches!(task.synced.as_str(), "" | "started") {
            let _ = tasks::set_synced(db, task.id, "done");
            let mut errs = Vec::new();
            if tr.comment_on_done && !task.result.trim().is_empty() {
                if let Err(e) = comment(tr, key, &format!("hubcodes · {}: {}", if task.owner.is_empty() { "done" } else { &task.owner }, task.result)) {
                    errs.push(format!("comment: {e:#}"));
                }
            }
            if let Err(e) = transition(tr, key, &tr.on_done) {
                errs.push(format!("{e:#}"));
            }
            // Once, when the review finishes: a failure leaves the Approve button.
            if tr.kind == "github-review" && tr.auto_approve && ready_to_approve(&task) {
                match approve(db, task.id, "hubcodes") {
                    Ok(_) => events.push(Event { kind: "review".into(), text: format!("{key}: nothing blocking, approved on GitHub (LGTM)") }),
                    Err(e) => errs.push(format!("auto-approve: {e:#}")),
                }
            }
            if errs.is_empty() {
                let _ = db.add_event("hubcodes", "tracker", &format!("{key} done{}", if tr.on_done.is_empty() { String::new() } else { format!(" → {}", tr.on_done) }));
                events.push(Event { kind: "tracker".into(), text: format!("{key} {}", if tr.on_done.is_empty() { "done, result posted".into() } else { format!("→ {}", tr.on_done) }) });
            } else {
                events.push(Event { kind: "error".into(), text: format!("{key}: {}", errs.join("; ")) });
            }
        }
    }
    events
}

/// The tracker without its secrets, for `tracker list --json` and for other
/// devices asking what this one polls. A `${ENV_VAR}` reference is not a
/// secret and stays, so the reader can still see how it is configured.
pub fn redacted(tr: &Tracker) -> Tracker {
    let keep = |v: &str| if v.is_empty() || (v.starts_with("${") && v.ends_with('}')) { v.to_string() } else { "(set)".to_string() };
    Tracker {
        email: keep(&tr.email),
        token: keep(&tr.token),
        api_key: keep(&tr.api_key),
        headers: tr.headers.iter().map(|(k, v)| (k.clone(), keep(v))).collect(),
        ..tr.clone()
    }
}

/// What a tracker reads: its kind, the place (site, project, repository,
/// command) and the query. Two trackers with the same source import the
/// same issues, so two devices polling it put every ticket on two boards.
pub fn source(tr: &Tracker) -> String {
    let norm = |v: &str| v.trim().trim_end_matches('/').to_lowercase().trim_start_matches("https://").trim_start_matches("http://").to_string();
    let place = match tr.kind.as_str() {
        "jira" => norm(&tr.url),
        "plane" => format!("{}/{}/{}", norm(&tr.url), norm(&tr.workspace), norm(&tr.project)),
        "github" | "github-review" | "github-feedback" => norm(&tr.repo),
        "command" => tr.run.trim().to_string(),
        _ => format!("{}|{}|{}", norm(&tr.url), tr.command.trim(), tr.list_tool.trim()),
    };
    let query = tr.query.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{}|{}|{}", tr.kind, place, query)
}

/// A tracker on another device that reads the same issues as one here.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Overlap {
    /// The tracker here.
    pub tracker: String,
    pub device: String,
    /// Its name on that device.
    pub name: String,
    pub enabled: bool,
}

/// Which of `mine` another device already polls. `theirs` is each device
/// with its trackers (from `hubcodes tracker list --json` there).
pub fn overlaps(mine: &[Tracker], theirs: &[(String, Vec<Tracker>)]) -> Vec<Overlap> {
    let mut out = Vec::new();
    for m in mine {
        let src = source(m);
        for (device, list) in theirs {
            for t in list.iter().filter(|t| source(t) == src) {
                out.push(Overlap { tracker: m.name.clone(), device: device.clone(), name: t.name.clone(), enabled: t.enabled });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: i64, ext: &str, status: &str, owner: &str, synced: &str) -> tasks::Task {
        serde_json::from_value(json!({ "id": id, "at": 0, "updated": 0, "title": "t", "detail": "", "status": status, "role": "reviewer", "owner": owner, "createdBy": "tracker:rv", "result": "", "external": ext, "synced": synced })).unwrap()
    }

    fn pr(key: &str) -> Issue {
        Issue { key: key.into(), title: "Fix login".into(), url: format!("https://github.com/{}", key.replace('#', "/pull/")), description: String::new() }
    }

    #[test]
    fn review_requests_queue_withdraw_and_come_back() {
        let p = "rv:";
        let have = vec![
            task(1, "rv:o/a#1", "doing", "rev1", ""),        // being reviewed, still listed
            task(2, "rv:o/a#2", "todo", "", ""),             // queued, request withdrawn
            task(3, "rv:o/a#3", "done", "rev1", "done"),     // reviewed, left the list
            task(4, "rv:o/a#4", "done", "rev2", "released"), // reviewed earlier, asked again
            task(5, "rv:o/a#5", "done", "rev2", "done"),     // reviewed, still listed (review not posted)
            task(6, "other:o/a#9", "todo", "", ""),          // another tracker's
        ];
        let plan = review_plan(p, &have, vec![pr("o/a#1"), pr("o/a#4"), pr("o/a#5"), pr("o/b#7")], vec![]);
        assert_eq!(plan.import.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["o/a#4", "o/b#7"]);
        assert_eq!(plan.withdraw, [2]);
        assert_eq!(plan.release, [3]);
    }

    #[test]
    fn only_a_finished_review_that_says_approve_gets_the_button() {
        assert!(verdict_approve("2 nits, nothing blocking\n\n**VERDICT: approve**\n"));
        assert!(!verdict_approve("VERDICT: changes"));
        assert!(!verdict_approve("VERDICT: approve\nbut actually the migration is wrong"), "the verdict is the last line");
        let mut t = task(1, "rv:o/a#1", "done", "rev1", "done");
        t.title = "Review o/a#1 Fix login".into();
        t.result = "Looks right.\nVERDICT: approve".into();
        assert!(ready_to_approve(&t));
        for (synced, status) in [(APPROVED, "done"), (IGNORED, "done"), ("released", "done"), ("", "doing")] {
            let mut x = t.clone();
            x.synced = synced.into();
            x.status = status.into();
            assert!(!ready_to_approve(&x), "{synced} {status}");
        }
        let mut fb = t.clone();
        fb.title = "Feedback round 1 on o/a#1".into();
        assert!(!ready_to_approve(&fb), "a feedback round is never approved");
        assert!(REVIEW_INSTRUCTION.contains("VERDICT: approve") && REVIEW_INSTRUCTION.contains("Do not approve"));
    }

    #[test]
    fn an_ignored_pull_request_never_comes_back() {
        let p = "rv:";
        let mut gone = task(1, "rv:o/a#1", "done", "", IGNORED);
        gone.at = 100;
        let plan = review_plan(p, &[gone.clone()], vec![pr("o/a#1")], vec![]);
        assert!(plan.import.is_empty(), "asked again: still ignored");
        let plan = review_plan(p, &[gone.clone()], vec![], vec![(pr("o/a#1"), 200)]);
        assert!(plan.import.is_empty() && plan.release.is_empty(), "pushed again: still ignored, and kept ignored");
    }

    #[test]
    fn a_push_after_our_review_comes_back_once() {
        let p = "rv:";
        let t = |id, ext: &str, status: &str, at| {
            let mut x = task(id, ext, status, "rev1", "done");
            x.at = at;
            x
        };
        let have = vec![
            t(1, "rv:o/a#1", "done", 100),  // reviewed at 100, pushed at 200: again
            t(2, "rv:o/a#2", "done", 300),  // re-review already ran after the push
            t(3, "rv:o/a#3", "doing", 100), // still reviewing
            t(4, "rv:o/a#5", "done", 100),  // asked again by request: that wins
        ];
        let again = vec![(pr("o/a#1"), 200), (pr("o/a#2"), 200), (pr("o/a#3"), 200), (pr("o/a#4"), 200), (pr("o/a#5"), 200)];
        let plan = review_plan(p, &have, vec![pr("o/a#5")], again);
        // o/a#4 was reviewed by hand, not by this tracker: left alone.
        assert_eq!(plan.import.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["o/a#1"]);
        assert!(plan.release.is_empty(), "every one is still listed");
    }

    #[test]
    fn rereviews_wait_for_a_quiet_push_and_skip_our_own() {
        let node = |n: u64, author: &str, head: &str, at: &str, seen: &str, seen_at: &str| json!({ "number": n, "title": "T", "url": "u", "isDraft": false, "author": { "login": author }, "repository": { "nameWithOwner": "o/a" }, "commits": { "nodes": [{ "commit": { "oid": head, "committedDate": at } }] }, "reviews": { "nodes": [{ "submittedAt": seen_at, "commit": { "oid": seen } }] } });
        let v = json!({ "data": { "search": { "nodes": [
            node(1, "sam", "bbb", "2026-11-21T10:00:00Z", "aaa", "2026-11-21T09:00:00Z"),
            node(2, "sam", "aaa", "2026-11-21T08:00:00Z", "aaa", "2026-11-21T09:00:00Z"),
            node(3, "sam", "ccc", "2026-11-21T10:58:00Z", "aaa", "2026-11-21T09:00:00Z"),
            node(4, "me", "ddd", "2026-11-21T10:00:00Z", "aaa", "2026-11-21T09:00:00Z")
        ] } } });
        let now = crate::feedback::ts("2026-11-21T11:00:00Z");
        let got = rereview_issues(&v, "me", now);
        assert_eq!(got.iter().map(|(i, _)| i.key.as_str()).collect::<Vec<_>>(), ["o/a#1"], "2 is reviewed at its head, 3 was pushed 2 minutes ago, 4 is mine");
        assert!(got[0].0.description.contains("git diff aaa..HEAD"), "{}", got[0].0.description);
    }

    #[test]
    fn the_orchestrator_hears_what_changed_and_the_queue() {
        let open = vec![task(7, "rv:o/a#1", "doing", "rev1", ""), task(8, "rv:o/a#2", "todo", "", ""), task(9, "jira:PE-1", "todo", "", "")];
        let text = digest_text(&["rv"], &open, &["rv: review o/a#2 → task #8".into()]);
        assert!(text.contains("rv: review o/a#2 → task #8"), "{text}");
        assert!(text.contains("1 waiting (#8), 1 in progress (#7 → rev1)"), "{text}");
        assert!(!text.contains("#9"), "other trackers' tasks stay out: {text}");
    }

    #[test]
    fn review_requests_parse_from_gh_search() {
        let v = json!([
            { "number": 12, "title": "Add export", "url": "https://github.com/o/a/pull/12", "repository": { "nameWithOwner": "o/a" }, "author": { "login": "dev" }, "isDraft": false },
            { "number": 13, "title": "WIP", "url": "u", "repository": { "nameWithOwner": "o/a" }, "author": { "login": "dev" }, "isDraft": true }
        ]);
        let got = review_issues(&v);
        assert_eq!(got.len(), 1, "drafts wait until they are ready");
        assert_eq!(got[0].key, "o/a#12");
        assert_eq!(split_pr(&got[0].key).unwrap(), ("o/a", "12"));
        let tr = Tracker { name: "rv".into(), kind: "github-review".into(), ..Tracker::default() };
        let text = instruction_for(&tr, &got[0]);
        assert!(text.starts_with("Review pull request o/a#12: Add export"), "{text}");
        assert!(text.contains("pulls/N/reviews") && text.contains("\"side\": \"RIGHT\"") && text.contains("🔴") && text.contains("Do not approve"), "{text}");
    }

    #[test]
    fn instruction_and_parsing() {
        let tr = Tracker { name: "j".into(), instruction: "/jira-task {key}".into(), ..Default::default() };
        let i = Issue { key: "PE-1".into(), title: "Login".into(), url: "https://x/browse/PE-1".into(), description: "d".into() };
        assert_eq!(instruction_for(&tr, &i), "/jira-task PE-1 https://x/browse/PE-1", "a one-line skill command takes the URL as an argument");
        let multi = Tracker { instruction: "Fix {key}\nCarefully".into(), ..Default::default() };
        assert_eq!(instruction_for(&multi, &i), "Fix PE-1\nCarefully\nhttps://x/browse/PE-1");
        let d = Tracker::default();
        assert!(instruction_for(&d, &i).starts_with("Work on PE-1: Login"));
        let v = issues_from(r#"[{"key":"A-1","summary":"s","link":"u"},{"id":7,"title":"t"}]"#);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].key, "7");
        assert_eq!(adf_text(&json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]})), "hi");
    }

    #[test]
    fn the_same_source_on_another_device_is_an_overlap() {
        let jira = |name: &str, url: &str, q: &str| Tracker { name: name.into(), kind: "jira".into(), url: url.into(), query: q.into(), ..Tracker::default() };
        let mine = vec![jira("jira", "https://hubstudio.atlassian.net/", "project = PE  AND status = Todo")];
        let theirs = vec![
            ("mini".to_string(), vec![jira("pe", "HTTPS://hubstudio.atlassian.net", "project = PE AND status = Todo"), jira("other", "https://hubstudio.atlassian.net", "project = PMP")]),
            ("old".to_string(), vec![]),
        ];
        let o = overlaps(&mine, &theirs);
        assert_eq!(o, vec![Overlap { tracker: "jira".into(), device: "mini".into(), name: "pe".into(), enabled: true }]);
        let gh = |repo: &str| Tracker { name: "gh".into(), kind: "github".into(), repo: repo.into(), ..Tracker::default() };
        assert!(overlaps(&[gh("hubstudio/app")], &[("mini".into(), vec![gh("hubstudio/web")])]).is_empty());
    }

    #[test]
    fn secrets_do_not_leave_the_device() {
        let mut tr = Tracker { email: "a@b.c".into(), token: "${JIRA_TOKEN}".into(), api_key: "plane_live_x".into(), ..Tracker::default() };
        tr.headers.insert("Authorization".into(), "Bearer x".into());
        let r = redacted(&tr);
        assert_eq!((r.email.as_str(), r.token.as_str(), r.api_key.as_str()), ("(set)", "${JIRA_TOKEN}", "(set)"));
        assert_eq!(r.headers["Authorization"], "(set)");
        assert_eq!(source(&r), source(&tr));
    }

    #[test]
    fn own_review_instruction_gets_the_verdict_rule() {
        let tr = Tracker { name: "reviews".into(), kind: "github-review".into(), instruction: "/pr-review".into(), ..Tracker::default() };
        let i = Issue { key: "o/r#1".into(), title: "T".into(), url: "https://github.com/o/r/pull/1".into(), description: String::new() };
        let s = instruction_for(&tr, &i);
        assert!(s.starts_with("/pr-review https://github.com/o/r/pull/1\n\n"), "{s}");
        assert!(s.ends_with(VERDICT_RULE));
        let plain = Tracker { instruction: String::new(), ..tr.clone() };
        assert_eq!(instruction_for(&plain, &i).matches("VERDICT: approve").count(), 1, "the built-in text asks once");
    }


    #[test]
    fn verdict_lines() {
        assert!(has_verdict("2 🟡\nVERDICT: changes"));
        assert!(has_verdict("ok\n**VERDICT: approve**\n"));
        assert!(!has_verdict("No defects found."));
        assert!(!has_verdict("VERDICT: maybe"));
    }

}
