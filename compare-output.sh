#!/usr/bin/env bash
# Compare the rendered output of two tmux-companion binaries, byte for byte.
#
#   ./compare-output.sh OLD_BINARY NEW_BINARY [REPO_PATH]
#
# Every port commit claims the bar did not change. This is what checks that
# claim against the previous build instead of against a unit test that both
# builds happen to satisfy.
#
# Each binary gets its own server on its own socket under TMPDIR, so this never
# touches the socket a live status bar is using, and each server is killed by
# pid rather than by pkill -f, which would match a real daemon on a similar path.
#
# Written after the same loop was attempted inline in zsh, where `set -- $pair`
# does not word-split and every invocation silently ran with the whole line as
# argv[1].
set -euo pipefail

if [ $# -lt 2 ]; then
  sed -n '2,8p' "$0" >&2
  exit 2
fi

old=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
new=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
repo=${3:-$PWD}
work=$(mktemp -d "${TMPDIR:-/tmp}/tmux-companion-compare.XXXXXX")
trap 'rm -rf "$work"' EXIT

run_all() {
  local bin=$1 sock=$2 out=$3 pid
  TMUX_COMPANION_SOCK="$sock" "$bin" server &
  pid=$!
  # The client auto-starts a server, so the wait here is only to make sure the
  # one under test is the one that answers.
  sleep 0.4
  {
    TMUX_COMPANION_SOCK="$sock" "$bin" gst "$repo"
    printf '\n---\n'
    TMUX_COMPANION_SOCK="$sock" "$bin" gst --style fill --branch-max-len 40 --branch-icon "$repo"
    printf '\n---\n'
    TMUX_COMPANION_SOCK="$sock" "$bin" gst --style outline --no-cap --ttl 0 "$repo"
    printf '\n---\n'
    TMUX_COMPANION_SOCK="$sock" "$bin" status-right --branch-max-len 40 "$repo"
    printf '\n---\n'
    TMUX_COMPANION_SOCK="$sock" "$bin" window -c -i 3 -n editor -w "$repo" -p nvim -P 2 -A 1
    printf '\n---\n'
    TMUX_COMPANION_SOCK="$sock" "$bin" window -i 7 -n shell -w "$HOME" -P 1 -A 0
    printf '\n---\n'
    TMUX_COMPANION_SOCK="$sock" "$bin" clients 2 1
    printf '\n---\n'
    TMUX_COMPANION_SOCK="$sock" "$bin" preview
  } >"$out" 2>&1
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

run_all "$old" "$work/old.sock" "$work/old.txt"
run_all "$new" "$work/new.sock" "$work/new.txt"

# `net` and `battery` are left out on purpose: a rate and a charge level differ
# between two runs seconds apart for reasons that have nothing to do with the
# change under test.
if diff -u "$work/old.txt" "$work/new.txt"; then
  echo "identical: $(wc -c <"$work/new.txt" | tr -d ' ') bytes across $(grep -c -- '---' "$work/new.txt") comparisons"
else
  echo "DIFFERS" >&2
  exit 1
fi
