#!/usr/bin/env bash
# Exercise `project save`, `show` and `forget` against a real tmux, without
# going anywhere near the tmux the developer is sitting in.
#
# usage: ./smoke-project-layout.sh [binary]
#          binary   defaults to ./target/release/tmux-companion
#
# Everything runs on its own tmux server (-L tcloop) with XDG_STATE_HOME
# pointed at a temporary directory, so the saved-layout files this writes
# cannot collide with the real ones. The commands run from *inside* a tcloop
# session on purpose: the binary calls bare `tmux`, which follows $TMUX, and
# $TMUX is the only thing that makes it talk to this server rather than the
# default one.
#
# `-f /dev/null` is load-bearing. A separate socket does not mean a separate
# configuration: without it the server reads ~/.tmux.conf, and the first run of
# this script targeted pane .0 on a server whose pane-base-index was 1 and got
# "can't find pane: 0".
set -euo pipefail

BIN=${1:-./target/release/tmux-companion}
BIN=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")
SOCKET=tcloop

STATE=$(mktemp -d)
PROJECT=$(mktemp -d)
OUT=$(mktemp -d)
cleanup() {
  tmux -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$STATE" "$PROJECT" "$OUT"
}
trap cleanup EXIT

export XDG_STATE_HOME="$STATE"

# Run a command in a pane and wait for it to finish, because send-keys returns
# the moment the keys are queued and the output file would not be there yet.
in_pane() {
  local target=$1 label=$2 cmd=$3
  tmux -L "$SOCKET" send-keys -t "$target" \
    "XDG_STATE_HOME=$STATE $cmd > $OUT/$label 2>&1; touch $OUT/$label.done" C-m
  for _ in $(seq 1 150); do
    [ -f "$OUT/$label.done" ] && return 0
    sleep 0.1
  done
  echo "timed out waiting for: $cmd" >&2
  return 1
}

# save, show and forget all act on the session they run in.
in_session() { in_pane smoke:work "$1" "$2"; }

echo "== building a session with two windows and three panes =="
tmux -L "$SOCKET" -f /dev/null new-session -d -s smoke -c "$PROJECT" -n work
tmux -L "$SOCKET" split-window -d -t smoke:work -c "$PROJECT"
tmux -L "$SOCKET" split-window -d -t smoke:work -c /tmp
tmux -L "$SOCKET" new-window -d -t smoke: -c "$PROJECT" -n notes
tmux -L "$SOCKET" set-option -t smoke @tmux-companion-project "$PROJECT"
tmux -L "$SOCKET" list-panes -s -t smoke -F '#{window_name}.#{pane_index} #{pane_current_path}'

echo
echo "== project save =="
in_session save "$BIN project save"
cat "$OUT/save"

echo
echo "== the file it wrote =="
find "$STATE" -name '*.toml' -exec cat {} \;

echo
echo "== project show =="
in_session show "$BIN project show"
cat "$OUT/show"

echo
echo "== project forget, then show again =="
in_session forget "$BIN project forget"
cat "$OUT/forget"
in_session show2 "$BIN project show"
cat "$OUT/show2"

echo
echo "== close-project captures on the way out =="
# From a second session, not from inside `smoke`. close-project tells every
# shell in the target session to exit, and a pane running close-project on
# itself would be told to exit while it was still working.
tmux -L "$SOCKET" new-session -d -s driver -c "$PROJECT"
in_pane driver "close" "$BIN close-project smoke"
cat "$OUT/close"
for _ in $(seq 1 100); do
  tmux -L "$SOCKET" has-session -t smoke 2>/dev/null || break
  sleep 0.1
done
if tmux -L "$SOCKET" has-session -t smoke 2>/dev/null; then
  echo "session did not close" >&2
  tmux -L "$SOCKET" list-panes -s -t smoke -F '#{pane_id} #{pane_current_command}' >&2
  exit 1
fi
echo "session closed, and it left this behind:"
find "$STATE" -name '*.toml' -exec cat {} \;

echo
echo "OK"
