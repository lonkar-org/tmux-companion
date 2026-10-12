#!/usr/bin/env bash
#
# The link to a real host, from a daemon and a tmux server of its own here,
# so nothing touches the tmux or the daemon you're using.
#
# usage:
#   scripts/host-check.sh NAME HOST-COMMAND...
#   scripts/host-check.sh vm ssh vm
#   KEEP=1 scripts/host-check.sh vm ssh -p 2222 user@127.0.0.1
#
# Builds nothing: it runs target/release/tmux-companion, so `just build` first,
# and the host needs a build with `bridge` on its PATH (`just push-host`).
# Prints `hosts` before and after `hosts connect`, then stops the daemon and
# the tmux server and removes the directory, unless KEEP=1, which prints the
# environment to reuse them.
#
# The daemon's HOME is a temporary one, so a host command that reads
# ~/.ssh through HOME (a wrapper script, or ssh's own default identity) finds
# nothing: pass `-i`, `-o UserKnownHostsFile=` and `-o IdentitiesOnly=yes`
# with real paths in the host command.
set -euo pipefail

if [ $# -lt 2 ]; then
  sed -n '3,/^set -euo/p' "$0" | sed '$d' >&2
  exit 2
fi
NAME=$1
shift
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BIN=$REPO/target/release/tmux-companion
[ -x "$BIN" ] || { echo "host-check: no $BIN; run just build" >&2; exit 1; }

ROOT=$(mktemp -d /tmp/tc-host.XXXXXX)
mkdir -p "$ROOT/home" "$ROOT/config/tmux-companion" "$ROOT/state" "$ROOT/data" "$ROOT/cache"
SOCK=$ROOT/tmux/tmux-$(id -u)/default
mkdir -p -m 700 "$(dirname "$SOCK")"
{
  printf '[[hosts]]\nname = "%s"\ncommand = [' "$NAME"
  sep=""
  for word in "$@"; do
    printf '%s"%s"' "$sep" "${word//\"/\\\"}"
    sep=", "
  done
  printf ']\n'
} >"$ROOT/config/tmux-companion/config.toml"

unset TMUX TMUX_PANE
export HOME=$ROOT/home TMUX_TMPDIR=$ROOT/tmux
export XDG_CONFIG_HOME=$ROOT/config XDG_STATE_HOME=$ROOT/state XDG_DATA_HOME=$ROOT/data XDG_CACHE_HOME=$ROOT/cache
export TMUX_COMPANION_SOCK=$ROOT/tc.sock

cleanup() {
  "$BIN" shutdown >/dev/null 2>&1 || true
  tmux -S "$SOCK" kill-server 2>/dev/null || true
  rm -rf "$ROOT"
}
if [ "${KEEP:-0}" != 1 ]; then trap cleanup EXIT; fi

tmux -S "$SOCK" -f /dev/null new-session -d -s here
echo "== hosts, before"
"$BIN" hosts
echo "== hosts connect $NAME"
start=$(date +%s)
"$BIN" hosts connect "$NAME" || true
for _ in $(seq 1 30); do
  if "$BIN" hosts | grep -q "link up"; then break; fi
  sleep 1
done
echo "(waited $(($(date +%s) - start))s)"
echo "== hosts, after"
"$BIN" hosts
echo "== doctor, hosts line"
"$BIN" doctor 2>&1 | grep -i host || true
if [ "${KEEP:-0}" = 1 ]; then
  echo "kept: export HOME=$HOME TMUX_TMPDIR=$TMUX_TMPDIR XDG_CONFIG_HOME=$XDG_CONFIG_HOME XDG_STATE_HOME=$XDG_STATE_HOME TMUX_COMPANION_SOCK=$TMUX_COMPANION_SOCK"
fi
