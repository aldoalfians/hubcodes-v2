#!/usr/bin/env bash
# Two "devices" on one Mac: device a (session hubpeerA-<pid>) and device b
# (session hubpeerB-<pid>), each with its own config and state, reaching each
# other as peers with host = "local" through a wrapper that sets that
# device's XDG dirs. Exercises what the real two-Mac setup relies on:
#   1. the peer cache and the roster (b/dev in a's orchestrator brief),
#   2. the guard naming b/dev as a delegation target,
#   3. cross-device dispatch (a's To do task handed to b/dev),
#   4. the report coming back (`task done a:1` on b lands on a's board),
#   5. the remote queue (report while a is down, delivered on the next tend),
#   6. max_agents refusing one more spawn on b,
#   7. the snapshot a peer reads: secrets redacted, private agents blank,
#   8. federation: a reaches b through b's listener (no shell hop), b's
#      allow list still applies, watch answers on a change, and `auto`
#      falls back when the listener is gone,
#   9. someone else's device (another Tailscale login) is view only: its
#      message waits for approval, its spawn is refused,
#  10. rooms: created on a, posts from b reach a and back, the transcript
#      stays on a, a post waits in the queue while a is down.
# Never touches the user's own sessions or config. Cleans up on exit.
set -euo pipefail
ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
BIN=${HUBCODES_BIN:-"$ROOT_DIR/target/debug/hubcodes"}
[ -x "$BIN" ] || { echo "build first: cargo build -p hubcodes" >&2; exit 2; }
TMP=$(mktemp -d /tmp/hubpeers.XXXXXX)
SA="hubpeerA-$$"; SB="hubpeerB-$$"
fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "ok   $*"; }
cleanup() {
  [ -n "${LPID:-}" ] && kill "$LPID" 2>/dev/null || true
  tmux kill-session -t "=$SA" 2>/dev/null || true
  tmux kill-session -t "=$SB" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

mkdev() { # name session peername peersession maxagents
  local d=$1 sess=$2 peer=$3 psess=$4 max=$5
  mkdir -p "$TMP/$d/config/hubcodes" "$TMP/$d/state" "$TMP/$d/bin"
  cat > "$TMP/$d/bin/hubcodes" <<W
#!/bin/sh
exec env XDG_CONFIG_HOME="$TMP/$d/config" XDG_STATE_HOME="$TMP/$d/state" HUBCODES_SESSION="$sess" "$BIN" "\$@"
W
  chmod +x "$TMP/$d/bin/hubcodes"
  printf '#!/bin/sh\necho "%s is off" >&2; exit 1\n' "$d" > "$TMP/$d/bin/hubcodes-down"; chmod +x "$TMP/$d/bin/hubcodes-down"
  cat > "$TMP/$d/config/hubcodes/hubcodes.toml" <<C
[sharing]
name = "$d"

[[provider]]
name = "fake"
command = "cat"
glyph = "~"
role_flag = ""

[[peer]]
name = "$peer"
host = "local"
session = "$psess"
hubcodes = "$TMP/$peer/bin/hubcodes"
max_agents = $max
C
}
mkdev a "$SA" b "$SB" 1   # a may start at most 1 agent on b
mkdev b "$SB" a "$SA" 0
A="$TMP/a/bin/hubcodes"; B="$TMP/b/bin/hubcodes"

# Agents: an orchestrator on a, an engineer on b. `cat` stands in for a tool:
# it echoes what it is sent, so the pane shows the assignment.
"$A" spawn lead -p fake -r orchestrator >/dev/null
"$B" spawn dev -p fake -r "software engineer" >/dev/null
sleep 1

# 1. Peer cache + roster.
"$A" peers | grep -q "^b " || fail "a does not see b (hubcodes peers)"
[ -f "$TMP/a/state/hubcodes/peers-$SA.json" ] || [ -f "$(find "$TMP/a/state" -name "peers-$SA.json" | head -1)" ] || fail "peer cache not written"
"$A" role show orchestrator 2>/dev/null | grep -q "b/dev" || fail "orchestrator brief does not list b/dev"
pass "b/dev is in a's orchestrator brief"

# 2. Guard names the remote engineer.
reason=$(HUBCODES_AGENT=lead "$A" guard --tool Write --input '{"file_path":"x.rs"}')
echo "$reason" | grep -q '"allow":false' || fail "guard allowed Write for the orchestrator: $reason"
echo "$reason" | grep -q "b/dev" || fail "guard reason does not name b/dev: $reason"
pass "guard refuses lead and points at b/dev"

# 3. Dispatch across devices. A remote agent counts as idle only after its
# screen sat still 5 s (measured on its own device), like a local one.
"$A" task add "build the store" --role "software engineer" >/dev/null
"$A" watch --once | grep -q "→ b/dev" && fail "task went to b/dev before it had been quiet 5 s"
sleep 6
"$A" watch --once | grep -q "→ b/dev" || fail "task was not handed to b/dev"
"$A" task list | grep -q "doing   b/dev" || fail "a's board does not show b/dev as owner"
sleep 1
tmux capture-pane -p -t "=$SB:dev" | grep -q "hubcodes task done a:1" || fail "b/dev did not receive the assignment with the a:1 id"
pass "a's task #1 went to b/dev with id a:1"

# 4. The report comes back.
HUBCODES_AGENT=dev "$B" task done a:1 "store built" | grep -q "on a done" || fail "task done a:1 was not forwarded"
"$A" task list --all | grep "#1 " | grep -q "done    b/dev" || fail "a's board did not record task #1 done by b/dev: $("$A" task list --all)"
pass "task done a:1 on b landed on a's board as b/dev"

# 5. Queue while a is down, delivered on the next tend.
"$A" task add "second one" --role "software engineer" >/dev/null
"$A" peers >/dev/null   # b/dev's screen changed when the first task arrived; measure quiet from here
sleep 6
"$A" watch --once | grep -q "→ b/dev" || fail "second task was not handed to b/dev"
sed -i '' "s|hubcodes = \"$TMP/a/bin/hubcodes\"|hubcodes = \"$TMP/a/bin/hubcodes-down\"|" "$TMP/b/config/hubcodes/hubcodes.toml"
HUBCODES_AGENT=dev "$B" task review a:2 "need the schema" | grep -q "queued (#1)" || fail "report was not queued while a is down"
qf=$(find "$TMP/b/state" -name "remote-queue-$SB.json" | head -1); [ -n "$qf" ] || fail "queue file missing"
sed -i '' "s|hubcodes = \"$TMP/a/bin/hubcodes-down\"|hubcodes = \"$TMP/a/bin/hubcodes\"|" "$TMP/b/config/hubcodes/hubcodes.toml"
"$B" watch --once | grep -q "Delivered to a" || fail "queued report was not delivered once a was back"
"$A" task list | grep "#2 " | grep -q "review" || fail "task #2 is not in review on a: $("$A" task list)"
pass "report queued while a was down, delivered on the next tend"

# 6. max_agents on b (1): a second agent there is refused.
if "$A" spawn dev2 -p fake --on b 2>"$TMP/err"; then fail "spawn on b should be refused past max_agents"; fi
grep -q "max_agents" "$TMP/err" || fail "refusal does not mention max_agents: $(cat "$TMP/err")"
pass "max_agents = 1 refused a second agent on b"

# 7. What leaves b in a snapshot: secret shapes redacted, a private agent's
# screen not at all. The token is synthetic (36 x's after ghp_).
fake="ghp_$(printf 'x%.0s' $(seq 1 36))"
"$B" send dev "token $fake" >/dev/null
sleep 1
snap=$("$B" ls --json --preview)
echo "$snap" | grep -q "ghp_x" && fail "snapshot carries the raw token"
echo "$snap" | grep -q "redacted" || fail "snapshot does not show the redaction: $snap"
"$B" private dev on >/dev/null
"$B" ls --json --preview | python3 -c 'import json,sys; a=[x for x in json.load(sys.stdin)["agents"] if x["name"]=="dev"][0]; sys.exit(0 if a["preview"]==[] and a["private"] else 1)' || fail "private agent still sends its screen"
pass "snapshot redacts secrets and blanks a private agent"

# 8. Federation. b listens on loopback (a token is required there); a's
# [[peer]] b switches to the listener. The shell hop is broken on purpose
# (hubcodes points at the "down" wrapper), so only the listener can answer.
export HUBCODES_FED_TOKEN="test-$$-$RANDOM"
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
python3 - "$TMP/b/config/hubcodes/hubcodes.toml" "$PORT" <<'PY'
import sys; p, port = sys.argv[1], sys.argv[2]; s = open(p).read()
s = s.replace('[sharing]\nname = "b"\n', '[sharing]\nname = "b"\nlisten = true\nlisten_bind = "127.0.0.1"\nlisten_port = %s\n' % port)
open(p, "w").write(s)
PY
"$B" daemon listen 2>"$TMP/listen.log" & LPID=$!
for _ in $(seq 1 50); do nc -z 127.0.0.1 "$PORT" 2>/dev/null && break; sleep 0.1; done
nc -z 127.0.0.1 "$PORT" || fail "b's federation listener did not start: $(cat "$TMP/listen.log")"
sed -i '' "s|hubcodes = \"$TMP/b/bin/hubcodes\"|hubcodes = \"$TMP/b/bin/hubcodes-down\"\ntransport = \"http\"\nport = $PORT|" "$TMP/a/config/hubcodes/hubcodes.toml"
"$A" peers | grep -q "^b " || fail "a cannot read b through the listener: $("$A" peers 2>&1)"
"$A" send b/dev "over federation" --direct >/dev/null || fail "send over federation failed"
sleep 1
tmux capture-pane -p -t "=$SB:dev" | grep -q "over federation" || fail "b/dev did not get the federated message"
refused=$(curl -s -w ' %{http_code}' -X POST "http://127.0.0.1:$PORT/fed/v1/run" -H "Authorization: Bearer $HUBCODES_FED_TOKEN" -H "X-Hubcodes-Peer: a" -d "{\"session\":\"$SB\",\"args\":[\"kill\",\"dev\"]}")
echo "$refused" | grep -q "may not .kill.* 403$" || fail "kill from a should be refused by b's allow list: $refused"
tmux list-windows -t "=$SB" -F '#{window_name}' | grep -qx dev || fail "b/dev was killed"
"$A" send b/dev "no line yet" | grep -q "checks with" && fail "b/dev has no line to a's lead but was routed"
"$A" connect lead b/dev >/dev/null
out=$("$A" send b/dev "routed please")
echo "$out" | grep -q "sent to b/dev; it checks with lead" || fail "a person's message to b/dev did not ask it to check with a's lead: $out"
pass "a reaches b through its listener; b's allow list still applies"
curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/fed/v1/run" -H "X-Hubcodes-Peer: a" -d '{"session":"x","args":["--version"]}' | grep -q 401 || fail "listener answered without the token"
curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/fed/v1/run" -H "Authorization: Bearer $HUBCODES_FED_TOKEN" -H "X-Hubcodes-Peer: a" -d '{"session":"x","args":["daemon","stop"]}' | grep -q 403 || fail "listener ran a verb outside the list"
pass "listener refuses a missing token and verbs outside the list"
# Watch (long poll): answers at once without an etag, then holds until b's
# agent list changes, and returns well before the 25 s hold.
W="http://127.0.0.1:$PORT/fed/v1/watch?session=$SB"
H=(-H "Authorization: Bearer $HUBCODES_FED_TOKEN" -H "X-Hubcodes-Peer: a")
"$B" private dev off >/dev/null
tag=$(curl -s "${H[@]}" "$W&etag=" | python3 -c 'import json,sys; print(json.load(sys.stdin)["etag"])')
[ -n "$tag" ] || fail "watch gave no etag"
(sleep 2; "$B" send dev "pushed change" >/dev/null) &
t0=$(date +%s)
# An answer may also come from dev turning still (5 s quiet); follow the etag
# until the change shows, as a client does.
for _ in 1 2 3; do
  got=$(curl -s "${H[@]}" "$W&etag=$tag")
  new=$(echo "$got" | python3 -c 'import json,sys; print(json.load(sys.stdin)["etag"])')
  [ "$new" != "$tag" ] || fail "watch held past a change or returned the same etag"
  tag=$new
  echo "$got" | grep -q "pushed change" && break
done
dt=$(( $(date +%s) - t0 ))
echo "$got" | grep -q "pushed change" || fail "watch answers do not carry the change"
[ "$dt" -lt 15 ] || fail "watch took ${dt}s; it should answer on the change"
pass "watch answers on a change (${dt}s), not on a timer"
kill "$LPID"; wait "$LPID" 2>/dev/null || true; LPID=
sed -i '' "s|hubcodes = \"$TMP/b/bin/hubcodes-down\"|hubcodes = \"$TMP/b/bin/hubcodes\"|; s|transport = \"http\"|transport = \"auto\"|" "$TMP/a/config/hubcodes/hubcodes.toml"
"$A" send b/dev "over ssh again" --direct >/dev/null || fail "auto did not fall back once the listener was gone"
sleep 1
tmux capture-pane -p -t "=$SB:dev" | grep -q "over ssh again" || fail "fallback message did not reach b/dev"
pass "auto falls back to the shell hop when the listener is gone"

# 9. Someone else's device. The listener marks such a caller with
# HUBCODES_FED_FOREIGN=1 (its whois login differs from this Mac's); here it
# is set by hand, as the listener would.
out=$(HUBCODES_FED_CALLER=a HUBCODES_FED_FOREIGN=1 "$B" send dev "foreign hello")
echo "$out" | grep -q "waiting for approval" || fail "a foreign send was not parked: $out"
sleep 1
tmux capture-pane -p -t "=$SB:dev" | grep -q "foreign hello" && fail "a foreign message reached dev before approval"
if HUBCODES_FED_CALLER=a HUBCODES_FED_FOREIGN=1 "$B" spawn intruder -p fake 2>"$TMP/err"; then fail "a foreign spawn should be refused"; fi
grep -q "view only" "$TMP/err" || fail "foreign refusal does not say view only: $(cat "$TMP/err")"
pid=$("$B" pending --json | python3 -c 'import json,sys; print([p for p in json.load(sys.stdin) if p["by"]=="a/human:'"$USER"'" or p["by"].startswith("a/")][0]["id"])')
"$B" pending approve "$pid" | grep -q "approved" || fail "pending approve failed"
sleep 1
tmux capture-pane -p -t "=$SB:dev" | grep -q "foreign hello" || fail "approved message did not reach dev"
pass "someone else's device: message waits for approval, spawn refused"

# 10. Rooms. The room lives on a (lead and b/dev); b posts to it from there.
"$A" room create design lead b/dev >/dev/null
HUBCODES_AGENT=dev "$B" room post a:design "schema ready" | grep -q "posted to design on a" || fail "b's post did not reach the room on a"
sleep 1
tmux capture-pane -p -t "=$SA:lead" | grep -q "\[room design\] b/dev: schema ready" || fail "lead did not hear b/dev in the room"
HUBCODES_AGENT=lead "$A" room post design "thanks" >/dev/null || fail "lead could not post"
sleep 1
tmux capture-pane -p -t "=$SB:dev" | grep -q "\[room design\] lead: thanks" || fail "b/dev did not hear lead in the room"
transcript=$("$A" room show design)
echo "$transcript" | grep -q "b/dev: schema ready" || fail "the transcript on a misses b's post: $transcript"
"$B" room list | grep -q design && fail "the room should live on a only"
if HUBCODES_AGENT=ghost "$A" room post design "hi" 2>/dev/null; then fail "a non-member posted"; fi
if HUBCODES_FED_CALLER=b "$A" room create evil 2>/dev/null; then fail "another device created a room on a"; fi
sed -i '' "s|hubcodes = \"$TMP/a/bin/hubcodes\"|hubcodes = \"$TMP/a/bin/hubcodes-down\"|" "$TMP/b/config/hubcodes/hubcodes.toml"
HUBCODES_AGENT=dev "$B" room post a:design "while you were away" | grep -q "queued" || fail "post was not queued while a is down"
sed -i '' "s|hubcodes = \"$TMP/a/bin/hubcodes-down\"|hubcodes = \"$TMP/a/bin/hubcodes\"|" "$TMP/b/config/hubcodes/hubcodes.toml"
"$B" watch --once >/dev/null
sleep 1
tmux capture-pane -p -t "=$SA:lead" | grep -q "while you were away" || fail "queued room post did not arrive once a was back"
pass "rooms: home on a, posts both ways, transcript on a only, queued while away"

# 11. --direct travels with a person's message to another device: b's own
# lead is skipped too, while a plain message on b is noted for b's lead.
"$B" spawn blead -p fake -r orchestrator >/dev/null
"$B" connect blead dev >/dev/null
"$A" send b/dev "straight past both leads" --direct >/dev/null || fail "direct send to b/dev failed"
sleep 1
tmux capture-pane -p -J -S -40 -t "=$SB:dev" | grep -A4 "straight past both leads" | grep -q "blead's team" && fail "--direct was lost on the way: b noted the message for blead"
"$B" send dev "plain on b" >/dev/null
sleep 1
tmux capture-pane -p -J -S -40 -t "=$SB:dev" | grep -A4 "plain on b" | grep -q "blead's team" || fail "a plain person's message on b was not noted for blead"
pass "--direct reaches the other device; its lead is skipped there too"

echo "all peer checks passed"
