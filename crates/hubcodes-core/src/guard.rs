//! The orchestrator guard. A start brief tells an orchestrator to delegate,
//! but a prompt is advice: a long-running or resumed orchestrator keeps an
//! older prompt, and a model under pressure "just does it". So the rule is
//! also enforced where the tools run: the provider hooks (Claude Code
//! `PreToolUse`, the opencode plugin, the pi / omp extensions) ask
//! `hubcodes guard` before a tool runs, and an orchestrator is refused file
//! edits, builds, tests and git writes with a reason that names the
//! engineers to send the work to. Reads, searches, `hubcodes …` commands
//! and plain shell inspection stay allowed, so it can still check results.

use crate::prompts;
use serde_json::Value;

/// Tool names that write files, across providers (Claude Code, opencode,
/// pi, omp, codex).
const WRITE_TOOLS: &[&str] = &["write", "edit", "multiedit", "notebookedit", "patch", "apply_patch", "str_replace_editor", "str_replace_based_edit_tool", "create_file", "write_file", "edit_file"];
/// Tool names that run a shell command.
const SHELL_TOOLS: &[&str] = &["bash", "shell", "exec", "execute", "run", "terminal", "command", "shell_command", "run_command", "local_shell", "container.exec"];

/// Why a shell command counts as doing the work instead of coordinating.
fn command_reason(cmd: &str) -> Option<&'static str> {
    let c = cmd.trim();
    if c.is_empty() {
        return None;
    }
    // Anything that goes through hubcodes itself is coordination.
    if c.starts_with("hubcodes ") || c.contains("| hubcodes ") || c.contains("&& hubcodes ") {
        return None;
    }
    let lower = c.to_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    let first = first.rsplit('/').next().unwrap_or(first);
    let word = |w: &str| lower.split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-' && ch != '.').any(|t| t == w);
    let has = |needle: &str| lower.contains(needle);

    // Build / test runners.
    let runners = ["cargo", "pnpm", "npm", "npx", "yarn", "bun", "make", "pytest", "go", "gradle", "mvn", "dotnet", "tsc", "vitest", "jest", "mocha", "cmake", "ninja", "swift", "xcodebuild", "flutter", "mix", "rake", "bundle"];
    // `cargo --version`, `npm ls`, `go env` and the like only look.
    let readonly_sub = ["--version", "-v", "-V", "--help", "-h", "help", "list", "ls", "view", "info", "outdated", "why", "metadata", "tree", "search", "env", "version", "config", "audit", "licenses", "--print"];
    let runner_at = |segment: &str| -> bool {
        let mut it = segment.split_whitespace();
        let Some(head) = it.next() else { return false };
        let head = head.rsplit('/').next().unwrap_or(head);
        if !runners.contains(&head) {
            return false;
        }
        !it.next().map(|sub| readonly_sub.contains(&sub)).unwrap_or(false)
    };
    for segment in lower.split([';', '|', '&']).map(|seg| seg.trim_start()) {
        if runner_at(segment) {
            return Some("builds and tests are the engineers' and the tester's job");
        }
    }
    // Git writes (reads such as status / log / diff / show stay allowed).
    if first == "git" || has("&& git ") || has("; git ") {
        let git_writes = ["commit", "push", "add", "checkout", "switch", "stash", "merge", "rebase", "reset", "cherry-pick", "revert", "rm", "mv", "apply", "am", "tag", "branch -d", "branch -D", "worktree add", "worktree remove", "pull", "fetch --prune", "restore", "clean"];
        if git_writes.iter().any(|w| has(&format!("git {w}"))) {
            return Some("git writes (commit, push, checkout, add, merge…) belong to the engineer who owns the branch");
        }
        return None;
    }
    // File changes from the shell.
    if word("sed") && (has(" -i") || has("--in-place")) {
        return Some("editing files is the engineers' job");
    }
    let editors = ["rm", "mv", "cp", "touch", "mkdir", "rmdir", "tee", "truncate", "chmod", "chown", "ln", "patch", "dd", "install", "perl -pi", "perl -i"];
    if editors.iter().any(|e| first == *e || has(&format!("&& {e} ")) || has(&format!("; {e} ")) || has(&format!("| {e} "))) {
        return Some("changing files is the engineers' job");
    }
    // Redirection into a file (not `2>&1`, not `>/dev/null`, not `<`).
    let mut rest = lower.as_str();
    while let Some(i) = rest.find('>') {
        let before = rest[..i].trim_end();
        let after = rest[i + 1..].trim_start_matches('>').trim_start();
        let target = after.split_whitespace().next().unwrap_or("");
        let fd_dup = after.starts_with('&');
        let to_null = target.starts_with("/dev/null");
        let is_2_1 = before.ends_with('2') && fd_dup;
        if !fd_dup && !to_null && !target.is_empty() && !is_2_1 {
            return Some("writing files is the engineers' job");
        }
        rest = &rest[i + 1..];
    }
    None
}

/// The tool input's shell command, whatever the provider calls the field.
fn command_of(input: &Value) -> Option<String> {
    for k in ["command", "cmd", "script", "shell_command"] {
        match input.get(k) {
            Some(Value::String(s)) => return Some(s.clone()),
            Some(Value::Array(a)) => return Some(a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" ")),
            _ => {}
        }
    }
    None
}

/// `Some(reason)` when an agent with this role must not run this tool.
/// `team` is the roster, for naming the engineers in the reason.
pub fn decide(role: Option<&str>, tool: &str, input: &Value, team: &[(String, Option<String>)], me: &str) -> Option<String> {
    let role = role.map(str::trim).filter(|r| !r.is_empty())?;
    if prompts::kind(role) != "orchestrator" {
        return None;
    }
    let why = work_reason(tool, input)?;
    let engineers: Vec<&str> = team.iter().filter(|(n, r)| n != me && r.as_deref().map(|r| prompts::kind(r) == "worker").unwrap_or(false)).map(|(n, _)| n.as_str()).collect();
    // An orchestrator on another Mac takes whole parts of the work.
    let peer_leads: Vec<&str> = team.iter().filter(|(n, r)| n != me && n.contains('/') && r.as_deref().map(|r| prompts::kind(r) == "orchestrator").unwrap_or(false)).map(|(n, _)| n.as_str()).collect();
    let to = if engineers.is_empty() && peer_leads.is_empty() {
        "an engineer (start one with hubcodes spawn <name> -p <provider> -r \"software engineer\" or ask the human)".to_string()
    } else if engineers.is_empty() {
        format!("{} (the orchestrator on that device; it splits the part there)", peer_leads.join(" or "))
    } else {
        engineers.join(" or ")
    };
    let more = if !engineers.is_empty() && !peer_leads.is_empty() { format!(" A whole part can go to {} on the other device.", peer_leads.join(" or ")) } else { String::new() };
    Some(format!("hubcodes: you are the orchestrator ({role}); {why}. Send it to {to} with hubcodes send <name> \"goal, files, done-when\" and wait for the [from <name>] reply.{more}"))
}

/// Why a tool call is doing the work rather than looking or coordinating:
/// a file edit, a build or test, a git write. `None` for reads, searches,
/// plain shell inspection and `hubcodes …` commands.
pub fn work_reason(tool: &str, input: &Value) -> Option<&'static str> {
    let t = tool.trim().to_lowercase();
    let t = t.rsplit("__").next().unwrap_or(&t).to_string();
    if WRITE_TOOLS.contains(&t.as_str()) {
        Some("editing files is the engineers' job")
    } else if SHELL_TOOLS.contains(&t.as_str()) {
        command_reason(&command_of(input).unwrap_or_default())
    } else {
        None
    }
}

/// What the shims and the Claude hook print: `{"allow": bool, "reason": "…"}`.
pub fn verdict(reason: Option<String>) -> Value {
    match reason {
        Some(r) => serde_json::json!({ "allow": false, "reason": r }),
        None => serde_json::json!({ "allow": true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn team() -> Vec<(String, Option<String>)> {
        vec![("lead".into(), Some("orchestrator".into())), ("dev".into(), Some("software engineer".into())), ("rev".into(), Some("reviewer".into()))]
    }

    #[test]
    fn orchestrator_is_refused_edits_builds_and_git_writes() {
        let t = team();
        let d = decide(Some("orchestrator"), "Write", &json!({"file_path": "a.rs"}), &t, "lead").unwrap();
        assert!(d.contains("Send it to dev"), "{d}");
        assert!(decide(Some("Tech Lead"), "edit", &json!({}), &t, "lead").is_some());
        assert!(decide(Some("orchestrator"), "mcp__x__multiedit", &json!({}), &t, "lead").is_some());
        for cmd in ["cargo test", "pnpm build", "cd app && npm run build", "git commit -m x", "git push origin main", "git checkout -b feat", "sed -i '' s/a/b/ f.rs", "rm -rf dist", "echo hi > out.txt", "cat a | tee b", "mkdir -p x", "make"] {
            assert!(decide(Some("orchestrator"), "Bash", &json!({"command": cmd}), &t, "lead").is_some(), "{cmd} should be refused");
        }
    }

    #[test]
    fn orchestrator_may_read_inspect_and_use_hubcodes() {
        let t = team();
        for cmd in ["hubcodes send dev \"do x\"", "hubcodes task add \"x\" --role \"software engineer\"", "git status", "git log --oneline -5", "git diff", "ls -la", "cat src/main.rs", "grep -rn foo src", "cargo --version 2>&1 | head", "echo done >/dev/null", "rg TODO", "hubcodes ls --json | jq ."] {
            assert!(decide(Some("orchestrator"), "Bash", &json!({"command": cmd}), &t, "lead").is_none(), "{cmd} should be allowed");
        }
        assert!(decide(Some("orchestrator"), "Read", &json!({"file_path": "a.rs"}), &t, "lead").is_none());
        assert!(decide(Some("orchestrator"), "Grep", &json!({}), &t, "lead").is_none());
    }

    #[test]
    fn other_roles_are_never_guarded() {
        let t = team();
        assert!(decide(Some("software engineer"), "Write", &json!({}), &t, "dev").is_none());
        assert!(decide(Some("reviewer"), "Bash", &json!({"command": "cargo test"}), &t, "rev").is_none());
        assert!(decide(None, "Write", &json!({}), &t, "x").is_none());
    }

    #[test]
    fn an_orchestrator_on_another_mac_is_a_delegation_target() {
        let t = vec![("lead".to_string(), Some("orchestrator".to_string())), ("mini/lead".to_string(), Some("orchestrator".to_string()))];
        let d = decide(Some("orchestrator"), "Write", &json!({}), &t, "lead").unwrap();
        assert!(d.contains("Send it to mini/lead"), "{d}");
        let t2 = vec![("lead".to_string(), Some("orchestrator".to_string())), ("dev".to_string(), Some("software engineer".to_string())), ("mini/lead".to_string(), Some("orchestrator".to_string()))];
        let d2 = decide(Some("orchestrator"), "Write", &json!({}), &t2, "lead").unwrap();
        assert!(d2.contains("Send it to dev") && d2.contains("A whole part can go to mini/lead"), "{d2}");
    }

    #[test]
    fn reason_without_engineers_says_how_to_get_one() {
        let d = decide(Some("orchestrator"), "Write", &json!({}), &[("lead".into(), Some("orchestrator".into()))], "lead").unwrap();
        assert!(d.contains("hubcodes spawn"));
        assert_eq!(verdict(None), json!({"allow": true}));
    }
}
