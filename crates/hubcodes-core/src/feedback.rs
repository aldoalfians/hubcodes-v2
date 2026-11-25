//! Review feedback on your own pull requests, round after round.
//!
//! `kind = "github-feedback"`: every open pull request you authored is read
//! for feedback from other people (reviews with a body, inline comments,
//! conversation comments) newer than both its last commit and the last
//! round's task. Anything new becomes one task per pull request (a "round")
//! for `role` (default `pr-fixer`). The agent fixes what is clear, pushes to
//! the pull request's branch, and answers each thread. The next comment from
//! a reviewer starts the next round; your own comments and replies (the `gh`
//! sign-in) never do, and neither do bots (`…[bot]`). A pull request that is
//! merged or closed leaves the queue and its checkout is removed.

use crate::db::Db;
use crate::tasks;
use crate::tend::Event;
use crate::tracker::{gh, instruction_for, Issue, Tracker};
use anyhow::Result;
use serde_json::Value;
use std::sync::OnceLock;

pub const INSTRUCTION: &str = "Address review feedback on your pull request {key}: {title}\n{url}\n\nNew since the last round:\n{description}\n\nFirst run `hubcodes review checkout {key} --branch`: it prints a folder on the pull request's branch, up to date with GitHub and separate from everyone else's work. Work only there. You decide what to do with each point: fix it, or explain why not. When two comments disagree, choose the better one, apply it, and say in both threads which you followed and why, mentioning both authors. A point from @checks is a failed check: read its log with the command given, fix the cause (not the test), and push. A point from @conflicts means the branch no longer merges: `git fetch origin <base>` and `git merge origin/<base>`, resolve each conflict keeping both sides' intent, run the tests, commit the merge and push; never rebase. Run the affected tests, commit with messages that say what was fixed, and push with `git push origin HEAD` (never another branch, never --force). Then answer every point that has a reply command in its own thread, saying what changed and in which commit: no such point goes without a reply. Only when a point truly needs a person (a product decision, access you do not have, a merge order, a conflict you cannot resolve safely) say so in its thread if it has one, and begin your `hubcodes task done` result with `NEEDS HUMAN:` followed by those points and their links.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub who: String,
    pub at: i64,
    pub url: String,
    pub body: String,
    /// `path:line` for an inline comment.
    pub place: String,
    /// The command that answers this point.
    pub reply: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pr {
    pub issue: Issue,
    pub last_commit: i64,
    /// Every commit's time, to tell rounds that changed code from rounds that only talked.
    pub commits: Vec<i64>,
    pub items: Vec<Item>,
}

/// `2026-11-21T07:17:46Z` → unix seconds (GitHub always answers in UTC).
pub fn ts(s: &str) -> i64 {
    let b = s.as_bytes();
    if b.len() < 19 {
        return 0;
    }
    let n = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
    let (y, m, d) = (n(0..4), n(5..7), n(8..10));
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + n(11..13) * 3600 + n(14..16) * 60 + n(17..19)
}

pub(crate) fn me() -> String {
    static ME: OnceLock<String> = OnceLock::new();
    ME.get_or_init(|| gh(&["api", "user", "--jq", ".login"]).map(|s| s.trim().to_string()).unwrap_or_default()).clone()
}

fn counts(who: &str, me: &str) -> bool {
    !who.is_empty() && !who.eq_ignore_ascii_case(me) && !who.ends_with("[bot]")
}

fn clip(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 1500 {
        format!("{}…", s.chars().take(1500).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Feedback items from `gh pr view --json commits,reviews,comments` and the
/// inline comments (`gh api …/pulls/N/comments`, one JSON object a line).
pub fn parse(repo: &str, number: u64, url: &str, view: &Value, inline: &str, me: &str) -> (Vec<i64>, Vec<Item>) {
    let s = |v: &Value, p: &str| v.pointer(p).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let mut commits: Vec<i64> = view.get("commits").and_then(|c| c.as_array()).into_iter().flatten().map(|c| ts(&s(c, "/committedDate"))).collect();
    commits.sort_unstable();
    let general = format!("gh pr comment {number} --repo {repo} --body '…'");
    let mut items = Vec::new();
    for r in view.get("reviews").and_then(|x| x.as_array()).into_iter().flatten() {
        let who = s(r, "/author/login");
        let body = s(r, "/body");
        if counts(&who, me) && !body.trim().is_empty() && s(r, "/state") != "APPROVED" {
            items.push(Item { who, at: ts(&s(r, "/submittedAt")), url: url.to_string(), body: clip(&body), place: format!("review ({})", s(r, "/state").to_lowercase().replace('_', " ")), reply: general.clone() });
        }
    }
    for c in view.get("comments").and_then(|x| x.as_array()).into_iter().flatten() {
        let who = s(c, "/author/login");
        if counts(&who, me) {
            items.push(Item { who, at: ts(&s(c, "/createdAt")), url: s(c, "/url"), body: clip(&s(c, "/body")), place: "conversation".into(), reply: general.clone() });
        }
    }
    for line in inline.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(c) = serde_json::from_str::<Value>(line) else { continue };
        let who = s(&c, "/user/login");
        if !counts(&who, me) {
            continue;
        }
        let id = c.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
        let root = c.get("in_reply_to_id").and_then(|x| x.as_u64()).unwrap_or(id);
        let line_no = c.get("line").and_then(|x| x.as_u64()).or_else(|| c.get("original_line").and_then(|x| x.as_u64()));
        items.push(Item {
            who,
            at: ts(&s(&c, "/created_at")),
            url: s(&c, "/html_url"),
            body: clip(&s(&c, "/body")),
            place: match line_no { Some(n) => format!("{}:{n}", s(&c, "/path")), None => s(&c, "/path") },
            reply: format!("gh api repos/{repo}/pulls/{number}/comments/{root}/replies -f body='…'"),
        });
    }
    items.sort_by_key(|i| i.at);
    (commits, items)
}

/// Who a failed check and a merge conflict are "from": not people, so they
/// get no reply command, and `ignore = "checks"` (or `"conflicts"`) turns them off.
pub const CHECKS: &str = "checks";
pub const CONFLICTS: &str = "conflicts";

/// The command that prints a failed check's log, from its link.
fn log_command(repo: &str, url: &str) -> String {
    let after = |k: &str| url.split(k).nth(1).map(|r| r.split(['/', '?', '#']).next().unwrap_or("").to_string()).filter(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()));
    match (after("/actions/runs/"), after("/job/")) {
        (_, Some(job)) => format!("gh run view --repo {repo} --job {job} --log-failed"),
        (Some(run), None) => format!("gh run view {run} --repo {repo} --log-failed"),
        _ => format!("open {url}"),
    }
}

/// Points that come from GitHub itself rather than a person: failed checks
/// on the head commit (once every check has finished) and a conflict with
/// the base branch. A conflict is dated just after the later of the head
/// commit and the base branch's tip, so it starts one round, and another
/// only when the base moves again.
pub fn status_items(repo: &str, node: &Value, since: i64) -> (i64, Vec<Item>) {
    let s = |v: &Value, p: &str| v.pointer(p).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let url = s(node, "/url");
    let head = node.pointer("/commits/nodes/0/commit").cloned().unwrap_or(Value::Null);
    let head_at = ts(&s(&head, "/committedDate"));
    let mut items = Vec::new();
    let rollup = head.get("statusCheckRollup").cloned().unwrap_or(Value::Null);
    let finished = !matches!(s(&rollup, "/state").as_str(), "" | "PENDING" | "EXPECTED");
    for c in rollup.pointer("/contexts/nodes").and_then(|x| x.as_array()).into_iter().flatten().filter(|_| finished) {
        let (name, outcome, at, link) = if s(c, "/__typename") == "StatusContext" {
            (s(c, "/context"), s(c, "/state"), s(c, "/createdAt"), s(c, "/targetUrl"))
        } else {
            if s(c, "/status") != "COMPLETED" {
                continue;
            }
            (s(c, "/name"), s(c, "/conclusion"), s(c, "/completedAt"), s(c, "/detailsUrl"))
        };
        if !matches!(outcome.as_str(), "FAILURE" | "ERROR" | "TIMED_OUT" | "STARTUP_FAILURE" | "ACTION_REQUIRED") {
            continue;
        }
        let link = if link.is_empty() { url.clone() } else { link };
        items.push(Item {
            who: CHECKS.into(),
            at: ts(&at).max(head_at + 1),
            body: format!("{name} failed ({}). Its log: {}", outcome.to_lowercase().replace('_', " "), log_command(repo, &link)),
            url: link,
            place: format!("check {name}"),
            reply: String::new(),
        });
    }
    if s(node, "/mergeable") == "CONFLICTING" {
        let base = s(node, "/baseRefName");
        let base_at = ts(&s(node, "/baseRef/target/committedDate"));
        items.push(Item {
            who: CONFLICTS.into(),
            at: head_at.max(base_at).max(since) + 1,
            url: url.clone(),
            body: format!("This branch conflicts with {base}. Merge it in: `git fetch origin {base} && git merge origin/{base}`, resolve, test, commit, push."),
            place: format!("merge with {base}"),
            reply: String::new(),
        });
    }
    (head_at, items)
}

/// Your open pull requests with their checks and mergeability, in one call.
const SEARCH: &str = "query($q: String!) { search(query: $q, type: ISSUE, first: 50) { nodes { ... on PullRequest { number title url updatedAt baseRefName mergeable repository { nameWithOwner } baseRef { target { ... on Commit { committedDate } } } commits(last: 1) { nodes { commit { committedDate statusCheckRollup { state contexts(first: 60) { nodes { __typename ... on CheckRun { name status conclusion completedAt detailsUrl } ... on StatusContext { context state targetUrl createdAt } } } } } } } } } } }";

/// The search words for a tracker's `repo`: one repository, an owner's (`owner/*`), or all.
pub fn scope(repo: &str) -> String {
    let repo = repo.trim();
    if let Some(owner) = repo.strip_suffix("/*") {
        format!(" user:{owner}")
    } else if !repo.is_empty() {
        format!(" repo:{repo}")
    } else {
        String::new()
    }
}

/// What the agent is shown for this round's points.
pub fn describe(items: &[Item]) -> String {
    items
        .iter()
        .map(|i| {
            let reply = if i.reply.is_empty() { String::new() } else { format!("\n  reply: {}", i.reply) };
            format!("- @{} on {} ({})\n  {}{reply}", i.who, i.place, i.url, i.body.replace('\n', "\n  "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One pull request's feedback, re-read only when GitHub says it changed
/// (`updatedAt`): the daemon polls every few minutes and most pull requests
/// are quiet.
type Cache = std::collections::HashMap<String, (String, Pr)>;

fn cache() -> &'static std::sync::Mutex<Cache> {
    static C: OnceLock<std::sync::Mutex<Cache>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn read_pr(repo: &str, n: u64, url: &str, title: &str, me: &str) -> Result<Pr> {
    let ns = n.to_string();
    let view: Value = serde_json::from_str(&gh(&["pr", "view", &ns, "--repo", repo, "--json", "commits,reviews,comments"])?)?;
    let inline = gh(&["api", &format!("repos/{repo}/pulls/{n}/comments?per_page=100"), "--paginate", "--jq", ".[]"])?;
    let (commits, items) = parse(repo, n, url, &view, &inline, me);
    Ok(Pr { issue: Issue { key: format!("{repo}#{n}"), title: title.to_string(), url: url.to_string(), description: String::new() }, last_commit: commits.last().copied().unwrap_or(0), commits, items })
}

fn list_prs(tr: &Tracker) -> Result<Vec<Pr>> {
    let q = format!("is:pr is:open author:@me archived:false{}", scope(&tr.repo));
    let found: Value = serde_json::from_str(&gh(&["api", "graphql", "-f", &format!("query={SEARCH}"), "-f", &format!("q={q}")])?)?;
    let me = me();
    let since = since(&tr.since);
    let ignore = ignored(&tr.ignore);
    let mut status: std::collections::HashMap<String, (i64, Vec<Item>)> = Default::default();
    let wanted: Vec<(String, u64, String, String, String)> = found
        .pointer("/data/search/nodes")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let g = |k: &str| p.pointer(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let updated = g("/updatedAt");
            // Nothing has happened on it since the cut-off: no feedback to read.
            if since > 0 && ts(&updated) <= since {
                return None;
            }
            let repo = g("/repository/nameWithOwner");
            let n = p.get("number")?.as_u64()?;
            status.insert(format!("{repo}#{n}"), status_items(&repo, p, since));
            Some((repo, n, g("/url"), g("/title"), updated))
        })
        .filter(|w| !w.0.is_empty())
        .collect();
    let stale: Vec<&(String, u64, String, String, String)> = {
        let c = cache().lock().unwrap();
        wanted.iter().filter(|w| c.get(&format!("{}|{}#{}", tr.name, w.0, w.1)).map(|(u, _)| u != &w.4).unwrap_or(true)).collect()
    };
    // Read the changed ones a few at a time.
    let mut fresh: Vec<(String, String, Result<Pr>)> = Vec::new();
    for chunk in stale.chunks(6) {
        let got: Vec<(String, String, Result<Pr>)> = std::thread::scope(|s| {
            let hs: Vec<_> = chunk.iter().map(|w| s.spawn(|| (format!("{}|{}#{}", tr.name, w.0, w.1), w.4.clone(), read_pr(&w.0, w.1, &w.2, &w.3, &me)))).collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        fresh.extend(got);
    }
    // One pull request that cannot be read is tried again next poll; only
    // when none can be read is it an error.
    let mut c = cache().lock().unwrap();
    let mut first_err = None;
    let tried = fresh.len();
    for (k, updated, r) in fresh {
        match r {
            Ok(pr) => {
                c.insert(k, (updated, pr));
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    if let Some(e) = first_err.filter(|_| tried > 0 && c.is_empty()) {
        return Err(e);
    }
    let mut out = Vec::new();
    for w in &wanted {
        if let Some((_, pr)) = c.get(&format!("{}|{}#{}", tr.name, w.0, w.1)) {
            let mut pr = pr.clone();
            pr.issue.title = w.3.clone();
            // Checks and mergeability change without touching `updatedAt`: always fresh.
            if let Some((head, extra)) = status.remove(&format!("{}#{}", w.0, w.1)) {
                pr.last_commit = pr.last_commit.max(head);
                pr.items.extend(extra);
                pr.items.sort_by_key(|i| i.at);
            }
            pr.items.retain(|i| i.at > since && !ignore.iter().any(|x| x.eq_ignore_ascii_case(&i.who)));
            out.push(pr);
        }
    }
    Ok(out)
}

/// Logins whose comments never start a round (`@a, b c`).
pub fn ignored(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c.is_whitespace()).map(|x| x.trim().trim_start_matches('@').to_string()).filter(|x| !x.is_empty()).collect()
}

/// `2026-11-21` or a full RFC 3339 time → unix seconds; empty → 0.
fn since(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() {
        0
    } else if s.len() == 10 {
        ts(&format!("{s}T00:00:00Z"))
    } else {
        ts(s)
    }
}

/// The pull requests with feedback newer than their last commit, as issues
/// (for `tracker test`).
pub fn list(tr: &Tracker) -> Result<Vec<Issue>> {
    Ok(list_prs(tr)?
        .into_iter()
        .filter_map(|p| {
            let new: Vec<Item> = p.items.into_iter().filter(|i| i.at > p.last_commit).collect();
            (!new.is_empty()).then(|| Issue { description: describe(&new), title: format!("{} ({} new)", p.issue.title, new.len()), ..p.issue })
        })
        .collect())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// New rounds: the pull request with only its new points, and the round number.
    pub import: Vec<(Issue, usize)>,
    /// Queued rounds of pull requests that were merged or closed.
    pub withdraw: Vec<i64>,
    /// Finished rounds of pull requests that are gone: their checkout goes.
    pub release: Vec<i64>,
    /// Pull requests paused for a person: (key, last round's task), told once.
    pub stalled: Vec<(String, i64)>,
}

/// New feedback waits this long after its last comment, so a review that
/// arrives as many comments becomes one round.
pub const QUIET_SECS: i64 = 300;
/// `synced` of a paused round a person chose to continue from.
pub const RESUMED: &str = "resumed";

/// Let a paused pull request have another round without a commit.
pub fn resume(db: &Db, id: i64) -> Result<()> {
    let t = tasks::get(db, id)?.ok_or_else(|| anyhow::anyhow!("no task #{id}"))?;
    if t.synced != "stalled" {
        anyhow::bail!("task #{id} is not paused");
    }
    tasks::set_synced(db, id, RESUMED)
}

/// Rounds in a row that ended without a commit before a pull request is
/// paused: the reviewer and the agent are going round in circles.
pub const STALL_ROUNDS: usize = 2;

/// A round starts when a pull request has feedback newer than its last
/// commit and than its last round, quiet for [`QUIET_SECS`], no round of it
/// is open, and it is not stalled. A pull request is stalled when its last
/// [`STALL_ROUNDS`] rounds each ended without a commit; a new commit (yours,
/// say) lifts that.
pub fn plan(prefix: &str, have: &[tasks::Task], prs: Vec<Pr>, now: i64) -> Plan {
    let mut out = Plan::default();
    let listed = |t: &tasks::Task| prs.iter().any(|p| t.external == format!("{prefix}{}", p.issue.key));
    for t in have.iter().filter(|t| t.external.starts_with(prefix) && !listed(t)) {
        if t.status == "todo" && t.owner.is_empty() {
            out.withdraw.push(t.id);
        } else if t.status == "done" && t.synced != "released" && t.synced != crate::tracker::IGNORED {
            out.release.push(t.id);
        }
    }
    for p in prs {
        let ext = format!("{prefix}{}", p.issue.key);
        let mut rounds: Vec<&tasks::Task> = have.iter().filter(|t| t.external == ext).collect();
        if rounds.iter().any(|t| t.status != "done" || t.synced == crate::tracker::IGNORED) {
            continue;
        }
        rounds.sort_by_key(|t| (t.at, t.id));
        let since = rounds.last().map(|t| t.at).unwrap_or(0).max(p.last_commit);
        let new: Vec<Item> = p.items.into_iter().filter(|i| i.at > since).collect();
        if new.is_empty() {
            continue;
        }
        // A person said "go on" after a pause: one more round before it can pause again.
        let resumed = rounds.last().map(|t| t.synced == RESUMED).unwrap_or(false);
        if rounds.len() >= STALL_ROUNDS && !resumed {
            let tail = &rounds[rounds.len() - STALL_ROUNDS..];
            let talked_only = tail.iter().enumerate().all(|(k, r)| {
                let end = tail.get(k + 1).map(|n| n.at).unwrap_or(i64::MAX);
                !p.commits.iter().any(|c| *c >= r.at && *c < end)
            });
            if talked_only {
                let last = tail[tail.len() - 1];
                if last.synced != "stalled" {
                    out.stalled.push((p.issue.key.clone(), last.id));
                }
                continue;
            }
        }
        if new.iter().any(|i| i.at > now - QUIET_SECS) {
            continue;
        }
        out.import.push((Issue { description: describe(&new), title: format!("{} ({} new)", p.issue.title, new.len()), ..p.issue }, rounds.len() + 1));
    }
    out
}

/// Finished rounds whose agent asked for a person (`NEEDS HUMAN:` at the
/// start of the result) and who has not been told yet.
pub fn asks(prefix: &str, have: &[tasks::Task]) -> Vec<(i64, String, String)> {
    have.iter()
        .filter(|t| t.external.starts_with(prefix) && t.status == "done" && matches!(t.synced.as_str(), "" | "started" | "done") && t.result.trim_start().to_uppercase().starts_with("NEEDS HUMAN"))
        .map(|t| (t.id, t.external[prefix.len()..].to_string(), t.result.trim().chars().take(400).collect()))
        .collect()
}

pub fn pull(db: &Db, tr: &Tracker) -> Vec<Event> {
    let mut events = Vec::new();
    let prs = match list_prs(tr) {
        Ok(p) => p,
        Err(e) => return vec![Event { kind: "error".into(), text: format!("Tracker {}: {e:#}", tr.name) }],
    };
    let prefix = format!("{}:", tr.name);
    let mine: Vec<tasks::Task> = tasks::list(db, true).unwrap_or_default().into_iter().filter(|x| x.external.starts_with(&prefix)).collect();
    for (id, key, what) in asks(&prefix, &mine) {
        let _ = tasks::set_synced(db, id, "asked");
        let text = format!("{key} needs you (task #{id}): {what}");
        let _ = db.add_event("hubcodes", "alert", &text);
        events.push(Event { kind: "alert".into(), text: text.clone() });
        events.push(Event { kind: "review".into(), text });
    }
    let plan = plan(&prefix, &mine, prs, crate::db::now());
    for (key, id) in &plan.stalled {
        let _ = tasks::set_synced(db, *id, "stalled");
        let _ = db.add_event("hubcodes", "alert", &format!("feedback on {key} paused: {STALL_ROUNDS} rounds in a row ended without a commit"));
        events.push(Event { kind: "alert".into(), text: format!("{key}: feedback paused after {STALL_ROUNDS} rounds without a commit; a new commit resumes it") });
        events.push(Event { kind: "review".into(), text: format!("{}: {key} paused after {STALL_ROUNDS} rounds without a commit; the reviewer and the agent disagree, so a person decides (a new commit resumes it)", tr.name) });
    }
    for id in &plan.withdraw {
        if tasks::remove(db, *id).is_ok() {
            events.push(Event { kind: "review".into(), text: format!("{}: task #{id} withdrawn (pull request merged or closed)", tr.name) });
        }
    }
    for id in &plan.release {
        let _ = tasks::set_synced(db, *id, "released");
        if let Some(t) = mine.iter().find(|t| t.id == *id) {
            let _ = crate::reviews::cleanup_branch(&t.external[prefix.len()..]);
        }
    }
    let open = mine.iter().filter(|t| t.status != "done" && !plan.withdraw.contains(&t.id)).count() as u32;
    let room = tr.limit.max(1).saturating_sub(open) as usize;
    for (issue, round) in plan.import.into_iter().take(room) {
        let ext = format!("{prefix}{}", issue.key);
        let title = format!("Feedback round {round} on {} {}", issue.key, issue.title).chars().take(140).collect::<String>();
        match tasks::add_external(db, &title, &instruction_for(tr, &issue), &tr.role, &format!("tracker:{}", tr.name), &ext, &issue.url) {
            Ok(task) => {
                let _ = db.add_event("hubcodes", "tracker", &format!("feedback round {round} on {} queued as task #{}", issue.key, task.id));
                events.push(Event { kind: "review".into(), text: format!("{}: feedback round {round} on {} → task #{}", tr.name, issue.key, task.id) });
            }
            Err(e) => events.push(Event { kind: "error".into(), text: format!("Tracker {}: {e:#}", tr.name) }),
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn since_takes_a_day_or_a_time() {
        assert_eq!(since(""), 0);
        assert_eq!(since("2026-11-21"), ts("2026-11-21T00:00:00Z"));
        assert_eq!(since("2026-11-21T07:17:46Z"), 1_795_245_466);
    }

    #[test]
    fn github_times_read_as_unix_seconds() {
        assert_eq!(ts("1970-01-01T00:00:00Z"), 0);
        assert_eq!(ts("2026-11-21T07:17:46Z"), 1_795_245_466);
        assert_eq!(ts("2000-03-01T00:00:00Z"), 951_868_800);
        assert_eq!(ts(""), 0);
    }

    #[test]
    fn feedback_from_others_counts_mine_and_bots_do_not() {
        let view = json!({
            "commits": [{ "committedDate": "2026-11-21T07:00:00Z" }, { "committedDate": "2026-11-21T09:09:17Z" }],
            "reviews": [
                { "author": { "login": "rev-a" }, "state": "COMMENTED", "submittedAt": "2026-11-21T10:00:00Z", "body": "1. criteria_clinic_keys is flat" },
                { "author": { "login": "rev-a" }, "state": "COMMENTED", "submittedAt": "2026-11-21T10:00:01Z", "body": "" },
                { "author": { "login": "lead" }, "state": "APPROVED", "submittedAt": "2026-11-21T10:00:02Z", "body": "LGTM" },
                { "author": { "login": "aldo" }, "state": "COMMENTED", "submittedAt": "2026-11-21T10:00:03Z", "body": "thanks" }
            ],
            "comments": [
                { "author": { "login": "vercel[bot]" }, "createdAt": "2026-11-21T10:01:00Z", "body": "Preview ready", "url": "u1" },
                { "author": { "login": "sam" }, "createdAt": "2026-11-21T10:02:00Z", "body": "Depends on #103", "url": "u2" }
            ]
        });
        let inline = [
            json!({ "id": 11, "user": { "login": "rev-a" }, "created_at": "2026-11-21T10:00:00Z", "body": "drafts dropped", "html_url": "u3", "path": "src/a.tsx", "line": 82, "in_reply_to_id": null }),
            json!({ "id": 12, "user": { "login": "sam" }, "created_at": "2026-11-21T10:03:00Z", "body": "still broken", "html_url": "u4", "path": "src/a.tsx", "line": null, "original_line": 80, "in_reply_to_id": 11 }),
            json!({ "id": 13, "user": { "login": "aldo" }, "created_at": "2026-11-21T10:04:00Z", "body": "Fixed in abc", "html_url": "u5", "path": "src/a.tsx", "line": 82, "in_reply_to_id": 11 }),
        ]
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let (commits, items) = parse("o/a", 107, "https://github.com/o/a/pull/107", &view, &inline, "aldo");
        assert_eq!(commits, [ts("2026-11-21T07:00:00Z"), ts("2026-11-21T09:09:17Z")]);
        let who: Vec<&str> = items.iter().map(|i| i.who.as_str()).collect();
        assert_eq!(who, ["rev-a", "rev-a", "sam", "sam"], "{items:#?}");
        let inline_reply = items.iter().find(|i| i.body == "still broken").unwrap();
        assert_eq!(inline_reply.place, "src/a.tsx:80");
        assert_eq!(inline_reply.reply, "gh api repos/o/a/pulls/107/comments/11/replies -f body='…'", "a reply answers the thread's first comment");
        assert!(items[0].reply.starts_with("gh pr comment 107 --repo o/a"));
        let text = describe(&items);
        assert!(text.contains("- @rev-a on src/a.tsx:82 (u3)\n  drafts dropped\n  reply: gh api repos/o/a/pulls/107/comments/11/replies"), "{text}");
    }

    fn task(id: i64, ext: &str, status: &str, owner: &str, at: i64) -> tasks::Task {
        serde_json::from_value(json!({ "id": id, "at": at, "updated": at, "title": "t", "detail": "", "status": status, "role": "pr-fixer", "owner": owner, "createdBy": "tracker:fb", "result": "", "external": ext })).unwrap()
    }

    fn pr(key: &str, last_commit: i64, ats: &[i64]) -> Pr {
        let items = ats.iter().map(|a| Item { who: "sam".into(), at: *a, url: "u".into(), body: format!("point at {a}"), place: "conversation".into(), reply: "r".into() }).collect();
        Pr { issue: Issue { key: key.into(), title: "T".into(), url: "u".into(), description: String::new() }, last_commit, commits: vec![last_commit], items }
    }

    #[test]
    fn rounds_follow_new_feedback_and_never_overlap() {
        let p = "fb:";
        let have = vec![
            task(1, "fb:o/a#1", "doing", "fix1", 100), // round open: wait for it
            task(2, "fb:o/a#2", "done", "fix1", 100),  // round 1 done
            task(3, "fb:o/a#3", "done", "fix1", 100),  // done, nothing new since
            task(4, "fb:o/a#9", "todo", "", 100),      // merged before it started
            task(5, "fb:o/a#8", "done", "fix1", 100),  // merged after its round
        ];
        let prs = vec![
            pr("o/a#1", 0, &[150]),
            pr("o/a#2", 120, &[90, 110, 130]), // only 130 is after the round and the fix commit
            pr("o/a#3", 140, &[90, 135]),      // 135 came before the last commit: answered by it
            pr("o/a#4", 50, &[40, 60]),        // first round: everything after the last commit
            pr("o/a#5", 50, &[]),              // no feedback
        ];
        let plan = plan(p, &have, prs, 10_000);
        let got: Vec<(String, usize, String)> = plan.import.iter().map(|(i, r)| (i.key.clone(), *r, i.description.clone())).collect();
        assert_eq!(got.len(), 2, "{got:#?}");
        assert_eq!((got[0].0.as_str(), got[0].1), ("o/a#2", 2));
        assert!(got[0].2.contains("point at 130") && !got[0].2.contains("point at 110"));
        assert_eq!((got[1].0.as_str(), got[1].1), ("o/a#4", 1));
        assert!(got[1].2.contains("point at 60") && !got[1].2.contains("point at 40"));
        assert_eq!(plan.withdraw, [4]);
        assert_eq!(plan.release, [5]);
    }

    #[test]
    fn a_review_that_is_still_arriving_waits_until_it_is_quiet() {
        let plan = plan("fb:", &[], vec![pr("o/a#1", 0, &[9_000, 9_900])], 10_000);
        assert!(plan.import.is_empty(), "the last comment is 100 s old");
        let plan = super::plan("fb:", &[], vec![pr("o/a#1", 0, &[9_000, 9_600])], 10_000);
        assert_eq!(plan.import[0].0.description.matches("point at").count(), 2, "both comments in one round");
    }

    #[test]
    fn rounds_that_only_talk_pause_the_pull_request_until_a_commit() {
        let p = "fb:";
        // Round 1 at 100 pushed a fix at 150; rounds 2 (200) and 3 (300) only replied.
        let mut have = vec![task(1, "fb:o/a#1", "done", "f", 100), task(2, "fb:o/a#1", "done", "f", 200), task(3, "fb:o/a#1", "done", "f", 300)];
        let mut talking = pr("o/a#1", 150, &[400]);
        talking.commits = vec![50, 150];
        let plan1 = plan(p, &have, vec![talking.clone()], 10_000);
        assert!(plan1.import.is_empty());
        assert_eq!(plan1.stalled, [("o/a#1".to_string(), 3)]);
        // Told once.
        have[2].synced = "stalled".into();
        assert!(plan(p, &have, vec![talking.clone()], 10_000).stalled.is_empty());
        // A person chooses to go on: one more round.
        let mut go_on = have.clone();
        go_on[2].synced = RESUMED.into();
        assert_eq!(plan(p, &go_on, vec![talking.clone()], 10_000).import.len(), 1);
        // A person pushes at 500 and a reviewer answers at 600: rounds resume.
        talking.commits.push(500);
        talking.last_commit = 500;
        talking.items.push(Item { who: "sam".into(), at: 600, url: "u".into(), body: "next".into(), place: "conversation".into(), reply: "r".into() });
        let plan3 = plan(p, &have, vec![talking], 10_000);
        assert_eq!(plan3.import.len(), 1);
        assert_eq!(plan3.import[0].1, 4);
        // A round that pushed is progress, not a stall.
        let mut fixing = pr("o/a#2", 0, &[400]);
        fixing.commits = vec![250];
        let have2 = vec![task(4, "fb:o/a#2", "done", "f", 100), task(5, "fb:o/a#2", "done", "f", 200)];
        assert_eq!(plan(p, &have2, vec![fixing], 10_000).import.len(), 1);
    }

    #[test]
    fn an_ignored_pull_request_gets_no_more_rounds() {
        let have = vec![task(1, "fb:o/a#1", "done", "f", 100)];
        let mut ignored = have.clone();
        ignored[0].synced = crate::tracker::IGNORED.into();
        assert_eq!(plan("fb:", &have, vec![pr("o/a#1", 0, &[200])], 10_000).import.len(), 1);
        assert!(plan("fb:", &ignored, vec![pr("o/a#1", 0, &[200])], 10_000).import.is_empty());
        assert!(plan("fb:", &ignored, vec![], 10_000).release.is_empty(), "stays ignored after it is merged");
        ignored[0].result = "NEEDS HUMAN: x".into();
        assert!(asks("fb:", &ignored).is_empty(), "no alert for one you set aside");
    }

    #[test]
    fn an_agent_that_asks_for_a_person_is_heard_once() {
        let mut asked = task(1, "fb:o/a#1", "done", "f", 100);
        asked.result = "NEEDS HUMAN: merge order with #103 (https://…)".into();
        let mut fine = task(2, "fb:o/a#2", "done", "f", 100);
        fine.result = "Fixed 3 points in abc123".into();
        let got = asks("fb:", &[asked.clone(), fine]);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].0, got[0].1.as_str()), (1, "o/a#1"));
        assert!(got[0].2.contains("merge order"));
        asked.synced = "asked".into();
        assert!(asks("fb:", &[asked]).is_empty());
    }

    #[test]
    fn failed_checks_and_conflicts_become_points_without_a_reply() {
        let node = json!({
            "url": "https://github.com/o/a/pull/7", "mergeable": "CONFLICTING", "baseRefName": "main",
            "baseRef": { "target": { "committedDate": "2026-11-21T12:00:00Z" } },
            "commits": { "nodes": [{ "commit": { "committedDate": "2026-11-21T10:00:00Z", "statusCheckRollup": { "state": "FAILURE", "contexts": { "nodes": [
                { "__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "FAILURE", "completedAt": "2026-11-21T10:05:00Z", "detailsUrl": "https://github.com/o/a/actions/runs/11/job/22" },
                { "__typename": "CheckRun", "name": "lint", "status": "COMPLETED", "conclusion": "SUCCESS", "completedAt": "2026-11-21T10:04:00Z", "detailsUrl": "x" },
                { "__typename": "StatusContext", "context": "ci/legacy", "state": "ERROR", "createdAt": "2026-11-21T10:06:00Z", "targetUrl": "https://ci.example/9" }
            ] } } } }] }
        });
        let (head, items) = status_items("o/a", &node, 0);
        assert_eq!(head, ts("2026-11-21T10:00:00Z"));
        let got: Vec<(&str, &str)> = items.iter().map(|i| (i.who.as_str(), i.place.as_str())).collect();
        assert_eq!(got, [(CHECKS, "check test"), (CHECKS, "check ci/legacy"), (CONFLICTS, "merge with main")]);
        assert!(items[0].body.contains("gh run view --repo o/a --job 22 --log-failed"), "{}", items[0].body);
        assert!(items[1].body.contains("open https://ci.example/9"));
        assert_eq!(items[2].at, ts("2026-11-21T12:00:00Z") + 1, "dated after the base moved");
        assert!(items.iter().all(|i| i.reply.is_empty()));
        assert!(!describe(&items).contains("reply:"));
        // Still running: wait for every check before saying what failed.
        let mut running = node.clone();
        running["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"] = json!("PENDING");
        running["mergeable"] = json!("MERGEABLE");
        assert!(status_items("o/a", &running, 0).1.is_empty());
    }

    #[test]
    fn log_commands_follow_the_check_link() {
        assert_eq!(log_command("o/a", "https://github.com/o/a/actions/runs/11"), "gh run view 11 --repo o/a --log-failed");
        assert_eq!(log_command("o/a", "https://github.com/o/a/actions/runs/11/job/22?pr=7"), "gh run view --repo o/a --job 22 --log-failed");
        assert_eq!(scope("o/*"), " user:o");
        assert_eq!(scope(" o/a "), " repo:o/a");
        assert_eq!(scope(""), "");
    }

    #[test]
    fn ignored_logins_are_read_from_a_loose_list() {
        assert_eq!(ignored("@auto-reviewer, rev-a  bot"), ["auto-reviewer", "rev-a", "bot"]);
        assert!(ignored("").is_empty());
    }

    #[test]
    fn the_agent_is_told_to_work_on_the_branch_and_answer_each_point() {
        let tr = Tracker { name: "fb".into(), kind: "github-feedback".into(), ..Tracker::default() };
        let i = Issue { key: "o/a#107".into(), title: "Settings page".into(), url: "u".into(), description: "- @sam on conversation (u)\n  fix it\n  reply: r".into() };
        let text = instruction_for(&tr, &i);
        assert!(text.contains("hubcodes review checkout o/a#107 --branch"), "{text}");
        assert!(text.contains("- @sam on conversation") && text.contains("never --force"), "{text}");
        assert!(text.contains("no such point goes without a reply") && text.contains("NEEDS HUMAN:") && text.contains("When two comments disagree"), "{text}");
        assert!(text.contains("@checks is a failed check") && text.contains("never rebase"), "{text}");
    }
}
