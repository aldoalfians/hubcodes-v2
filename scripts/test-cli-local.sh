#!/usr/bin/env bash
# The CLI against a throwaway session, config and state: task show/edit/place,
# board columns, peer add/check/rm (host = local), log filters, --json
# everywhere, completions, exit codes. Never touches your own config or
# sessions. Run after `cargo build -p hubcodes`.
set -euo pipefail
ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
BIN=${HUBCODES_BIN:-"$ROOT_DIR/target/debug/hubcodes"}
[ -x "$BIN" ] || { echo "build first: cargo build -p hubcodes" >&2; exit 2; }
TMP=$(mktemp -d /tmp/hubcli.XXXXXX)
S="hubcli-$$"; S2="hubcli2-$$"
fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "ok   $*"; }
cleanup() { tmux kill-session -t "=$S" 2>/dev/null || true; tmux kill-session -t "=$S2" 2>/dev/null || true; rm -rf "$TMP"; }
trap cleanup EXIT
mkdir -p "$TMP/config/hubcodes" "$TMP/state"
printf '[sharing]\nname = "cli"\n\n[[provider]]\nname = "fake"\ncommand = "cat"\nglyph = "~"\nrole_flag = ""\n' > "$TMP/config/hubcodes/hubcodes.toml"
H() { XDG_CONFIG_HOME="$TMP/config" XDG_STATE_HOME="$TMP/state" HUBCODES_SESSION="$S" "$BIN" "$@"; }
J() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
# expect <pattern> <command...>: the command's whole output must contain the pattern
# (captured first: with pipefail, `| grep -q` would report the writer's SIGPIPE as a failure).
expect() { local pat=$1; shift; local out; out=$("$@" 2>&1) || true; grep -q -- "$pat" <<<"$out" || fail "$* → ${out:0:300}"; }
refuse() { if "$@" >/dev/null 2>&1; then fail "should be refused: $*"; fi; }

# --json on the readers
H spawn dev -p fake -r "software engineer" >/dev/null
[ "$(H ls --json | J "d['selfName'] + ' ' + str(len(d['agents']))")" = "cli 1" ] || fail "ls --json lacks selfName/agents"
[ "$(H ls --json | J "str(d['version']).startswith('0.')")" = "True" ] || fail "ls --json lacks version"
H task add "first" --role "software engineer" >/dev/null
H task add "second" >/dev/null
H task add "third" >/dev/null
[ "$(H task list --json | J "len(d)")" = "3" ] || fail "task list --json"
[ "$(H task columns --json | J "','.join(c['id'] for c in d)")" = "todo,doing,review,done" ] || fail "task columns --json"
H loop status --json | J "d['loops']" >/dev/null || fail "loop status --json"
H usage --json | J "d['limits']['perAgent']" >/dev/null || fail "usage --json"
H note add "hello" >/dev/null; [ "$(H note show --json | J "'hello' in d['text']")" = "True" ] || fail "note show --json"
H team list --json | J "d[0]['name']" >/dev/null || fail "team list --json"
H prompt list --json | J "d[0]['name']" >/dev/null || fail "prompt list --json"
H kb list --json | J "len(d)" >/dev/null || fail "kb list --json"
H tracker list --json | J "len(d)" >/dev/null || fail "tracker list --json"
H policy show --json | J "d['mode']" >/dev/null || fail "policy show --json"
H merge --list --json | J "len(d)" >/dev/null || fail "merge --list --json"
H daemon status --json | J "d['installed']" >/dev/null || fail "daemon status --json"
H role list --json | J "len(d)" >/dev/null || fail "role list --json"
[ "$(H providers --json | J "'fake' in [p['name'] for p in d]")" = "True" ] || fail "providers --json"
[ "$(H doctor --json | J "d['facts'][0][0]")" = "hubcodes" ] || fail "doctor --json"
pass "--json on every reader"

# task show / edit / place
H task edit 2 --title "second, renamed" --role tests >/dev/null
expect "second, renamed" H task show 2
[ "$(H task show 2 --json | J "d['task']['role']")" = "tests" ] || fail "task show --json"
H task place 3 todo --before 1 >/dev/null
[ "$(H task list --json | J "[t['id'] for t in d if t['status']=='todo'][0]")" = "3" ] || fail "task place before"
H log --task 2 --json | J "len(d)" >/dev/null || fail "log --task"
pass "task show/edit/place, log --task"

# board columns
expect "backlog → todo" H board add "Backlog" --before todo
expect "doing \[2\]" H board wip doing 2
H board rename backlog "Later" >/dev/null
[ "$(H board columns --json | J "d[0]['label']")" = "Later" ] || fail "board rename"
H task move 1 backlog >/dev/null
expect "moved to To do" H board rm backlog
refuse H board rm todo
pass "board add/wip/rename/rm"

# peer add/check/rm with a local second session that names us back
mkdir -p "$TMP/b/config/hubcodes" "$TMP/b/state" "$TMP/b/bin"
printf '#!/bin/sh\nexec env XDG_CONFIG_HOME="%s" XDG_STATE_HOME="%s" HUBCODES_SESSION="%s" "%s" "$@"\n' "$TMP/b/config" "$TMP/b/state" "$S2" "$BIN" > "$TMP/b/bin/hubcodes"; chmod +x "$TMP/b/bin/hubcodes"
printf '[sharing]\nname = "other"\n\n[[provider]]\nname = "fake"\ncommand = "cat"\nglyph = "~"\nrole_flag = ""\n\n[[peer]]\nname = "cli"\nhost = "local"\nsession = "%s"\n' "$S" > "$TMP/b/config/hubcodes/hubcodes.toml"
"$TMP/b/bin/hubcodes" spawn rev -p fake -r reviewer >/dev/null
expect "added other" H peer add other local --session "$S2" --hubcodes "$TMP/b/bin/hubcodes" --allow send,task
expect "paired" H peer check other
[ "$(H peer check --json | J "','.join(d['devices'][0]['allow'])")" = "send,task" ] || fail "peer check --json allow"
expect "nothing queued" H peer queue
expect "removed other" H peer rm other
refuse H peer check other
pass "peer add/check/queue/rm"

# a wrong name on the far side is reported
H peer add wrongname local --session "$S2" --hubcodes "$TMP/b/bin/hubcodes" >/dev/null
expect 'calls itself "other"' H peer check wrongname
H peer rm wrongname >/dev/null
pass "pairing issues are named"

# team start --only and exit codes
H team start full-stack --only lead,dev --cwd "$TMP" >/dev/null 2>&1 || true
expect "lead" H ls
[ "$(H ls --json | J "'reviewer' in [a['name'] for a in d['agents']]")" = "False" ] || fail "team start --only started reviewer"
refuse H team start full-stack --only nobody
pass "team start --only"

# MCP over stdio: initialize, tools, a call, a resource, a prompt (as agent dev)
mcp_out=$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"task_columns","arguments":{}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"task_show","arguments":{"id":2}}}' \
  '{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"hubcodes://board"}}' \
  '{"jsonrpc":"2.0","id":6,"method":"prompts/list","params":{}}' \
  '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"agents","arguments":{}}}' \
  | HUBCODES_AGENT=dev XDG_CONFIG_HOME="$TMP/config" XDG_STATE_HOME="$TMP/state" HUBCODES_SESSION="$S" "$BIN" mcp)
line() { sed -n "${1}p" <<<"$mcp_out"; }
[ "$(line 1 | J "d['result']['serverInfo']['name']")" = "hubcodes" ] || fail "mcp initialize: $(line 1)"
[ "$(line 2 | J "'spawn' in [t['name'] for t in d['result']['tools']] and len(d['result']['tools'])")" -ge 25 ] || fail "mcp tools/list short: $(line 2 | head -c 200)"
grep -q "todo" <<<"$(line 3)" || fail "mcp task_columns: $(line 3)"
grep -q "second, renamed" <<<"$(line 4)" || fail "mcp task_show: $(line 4)"
grep -q "To do" <<<"$(line 5)" || fail "mcp resources/read board: $(line 5)"
[ "$(line 6 | J "len(d['result']['prompts']) > 0")" = "True" ] || fail "mcp prompts/list"
grep -q "dev" <<<"$(line 7)" || fail "mcp agents: $(line 7)"
pass "mcp stdio: initialize, tools, task_show, board resource, prompts"
# The MCP server lives as long as its agent: a device added to hubcodes.toml
# afterwards must reach it without a restart.
mkdir -p "$TMP/mcpcfg/hubcodes"; printf '[sharing]\nname = "mac"\n' > "$TMP/mcpcfg/hubcodes/hubcodes.toml"
mcp_late=$(python3 - "$BIN" "$TMP" "$S" <<'PY'
import json, os, subprocess, sys, time
bin_, tmp, sess = sys.argv[1:]
env = dict(os.environ, XDG_CONFIG_HOME=f"{tmp}/mcpcfg", XDG_STATE_HOME=f"{tmp}/state", HUBCODES_AGENT="dev", HUBCODES_SESSION=sess)
p = subprocess.Popen([bin_, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env=env)
def peers(i):
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": i, "method": "tools/call", "params": {"name": "peers", "arguments": {}}}) + "\n"); p.stdin.flush()
    return json.loads(p.stdout.readline())["result"]["content"][0]["text"]
a = peers(1); time.sleep(1.1)
open(f"{tmp}/mcpcfg/hubcodes/hubcodes.toml", "a").write('\n[[peer]]\nname = "late"\nhost = "nowhere.invalid"\n')
b = peers(2); p.stdin.close(); p.wait()
print(a, "|", b)
PY
)
grep -q "no other devices | late @" <<<"$mcp_late" || fail "mcp did not pick up a device added after it started: $mcp_late"
pass "mcp picks up a device added to hubcodes.toml after it started"

# accounts: folders per login, env in the launch command, account on the pane
H account add fake work >/dev/null
expect "work" H account list fake
[ "$(H account list fake --json | J "[a['name'] for a in d]")" = "['default', 'work']" ] || fail "account list --json"
refuse H account add fake "bad name"
refuse H account rm fake default
H spawn acc1 -p fake --account work >/dev/null
[ "$(H ls --json | J "[a['account'] for a in d['agents'] if a['name']=='acc1'][0]")" = "work" ] || fail "spawn --account not on the pane"
refuse H spawn acc2 -p fake --account "no/slash"
H account rm fake work >/dev/null
expect "default" H account list fake
# the real isolation variable for a known provider
H account add claude work >/dev/null 2>&1
CL=$(H account list claude --json | J "[a['env'] for a in d if a['name']=='work'][0][0][0]")
[ "$CL" = "CLAUDE_CONFIG_DIR" ] || fail "claude account env: $CL"
pass "accounts: add/list/rm, spawn --account, CLAUDE_CONFIG_DIR"

# completions
expect "_hubcodes" H completions zsh
expect "hubcodes" H completions bash
pass "completions"

# A person's message to a role goes through the orchestrator first.
H kill lead >/dev/null 2>&1 || true
H spawn lead -p fake -r orchestrator >/dev/null
sleep 1
expect "sent to dev" H send dev "no line to the lead yet"
H send dev "unrouted" | grep -q "checks with" && fail "dev has no line to the lead but was routed"
H connect lead dev >/dev/null
expect "it checks with lead" H send dev "please refactor auth"
sleep 1
tmux capture-pane -p -J -t "=$S:dev" | grep -q "please refactor auth" || fail "dev did not get the person's message"
tmux capture-pane -p -J -t "=$S:dev" | grep -q "tell lead what was asked" || fail "dev was not asked to check with lead"
tmux capture-pane -p -J -t "=$S:lead" | grep -q "please refactor auth" && fail "the message went to lead instead of dev"
expect "sent to dev" H send dev "direct note" --direct
# A file goes with the message as a path the agent can open.
printf 'stack trace' > "$TMP/crash.log"
expect "sent to dev" H send dev "see the log" --direct --file "$TMP/crash.log"
sleep 0.3
tmux capture-pane -p -J -t "=$S:dev" | grep -q -- "- $TMP/crash.log" || fail "dev did not get the attached file's path"
out=$(H send dev "x" --file "$TMP/missing.png" 2>&1) && fail "a missing attachment was sent"
echo "$out" | grep -q "is not there" || fail "a missing attachment was not refused: $out"
HUBCODES_AGENT=lead H send dev "from the lead" | grep -q "checks with" && fail "an agent's message was routed"
expect "sent to lead" H send lead "to the lead itself"

# Enforced, not just asked: a person's prompt marks dev until lead answers.
hook() { echo "$2" | HUBCODES_AGENT="$1" H hook claude; }
DEVP=$(tmux list-panes -t "=$S:dev" -F '#{pane_id}' | head -1)
mark() { tmux show-options -pqv -t "$DEVP" @hubcodes_check_with; }
# The --direct note above reached dev's prompt first: no check-in for it.
hook dev '{"hook_event_name":"UserPromptSubmit","prompt":"[from human:me] direct note"}' | grep -q "typed this" && fail "a --direct message made dev check with lead"
out=$(hook dev '{"hook_event_name":"UserPromptSubmit","prompt":"delete the logs"}'); echo "$out" | grep -q "A person typed this to you directly" || fail "dev's prompt hook did not add the check-in note: $out"
[ "$(mark)" = lead ] || fail "dev was not marked to check with lead"
hook dev '{"hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"x"},"cwd":"/tmp"}' | grep -q '"deny"' || fail "dev could write before lead answered"
hook dev '{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"hubcodes send lead \"asked\""},"cwd":"/tmp"}' | grep -q '"deny"' && fail "dev could not ask lead"
hook dev '{"hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"file_path":"x"},"cwd":"/tmp"}' | grep -q '"deny"' && fail "dev could not read"
HUBCODES_AGENT=lead H send dev "go ahead" >/dev/null
[ -z "$(mark)" ] || fail "lead's answer did not lift the mark"
hook dev '{"hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"x"},"cwd":"/tmp"}' | grep -q '"deny"' && fail "dev was refused after lead answered"
H send dev "straight please" --direct >/dev/null
hook dev '{"hook_event_name":"UserPromptSubmit","prompt":"[from human:me] straight please"}' >/dev/null
[ -z "$(mark)" ] || fail "--direct still made dev check with lead"
hook dev '{"hook_event_name":"UserPromptSubmit","prompt":"[from hubcodes] task #1"}' >/dev/null
[ -z "$(mark)" ] || fail "a hubcodes message made dev check with lead"
hook dev '{"hook_event_name":"UserPromptSubmit","prompt":"one more"}' >/dev/null
H disconnect lead dev >/dev/null
hook dev '{"hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"x"},"cwd":"/tmp"}' | grep -q '"deny"' && fail "dev stayed blocked after its line to lead was removed"
[ -z "$(mark)" ] || fail "the mark outlived the line to lead"
hook dev '{"hook_event_name":"UserPromptSubmit","prompt":"no lead now"}' | grep -q "typed this" && fail "an unlinked dev was told to check with lead"
H connect lead dev >/dev/null
hook dev '{"hook_event_name":"Stop"}' >/dev/null  # the fake prompts above left dev "working"
pass "a person's request blocks dev's edits until lead answers (--direct, agents, unlinked: no block)"

python3 - "$TMP/config/hubcodes/hubcodes.toml" <<'PY'
import sys; p = sys.argv[1]; s = open(p).read()
if "route_through_orchestrator" in s: s = s.replace("route_through_orchestrator = true", "route_through_orchestrator = false")
elif "[workspace]\n" in s: s = s.replace("[workspace]\n", "[workspace]\nroute_through_orchestrator = false\n", 1)
else: s += "\n[workspace]\nroute_through_orchestrator = false\n"
open(p, "w").write(s)
PY
expect "sent to dev" H send dev "routing off"
pass "a person's message stays with the agent, which checks with the lead it is linked to (unlinked, --direct, agents and the lead itself: no note)"

echo "all cli checks passed"
