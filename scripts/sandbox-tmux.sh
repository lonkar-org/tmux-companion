#!/usr/bin/env bash
#
# Run a command, or a shell, against a throwaway tmux server and a throwaway
# tmux-companion daemon, with nothing of the live ones in reach.
#
# usage:
#   scripts/sandbox-tmux.sh [--bin PATH] [--keep] [CMD ARGS...]
#   just sandbox                              # a shell inside the sandbox
#   just sandbox bash -c 'tmux new -d -s a; tmux-companion switch sessions --print'
#
# --bin   the tmux-companion to put first on PATH, as `tmux-companion`
#         (default: target/debug/tmux-companion, built if missing)
# --keep  leave the sandbox directory behind and print where it is
#
# Inside, these all point into one fresh directory under /tmp: HOME, every
# XDG directory, TMUX_TMPDIR (so a bare `tmux` starts and finds the
# sandbox's own server) and TMUX_COMPANION_SOCK (so the binary starts and
# talks to the sandbox's own daemon). TMUX and TMUX_PANE are unset, because
# tmux reads $TMUX before TMUX_TMPDIR and would otherwise reach the server
# this was started from. $SANDBOX names the directory, and $TC_BIN the
# binary by its full path: inside a pane, a login shell on macOS runs
# path_helper from /etc/zprofile, which puts /usr/local/bin back in front, so
# a bare `tmux-companion` typed into a sandbox pane is the installed one, not
# this build. Send `"$TC_BIN" ...` to panes.
#
# On the way out, however it ends, the sandbox's tmux server is killed and
# its daemon shut down with `shutdown`, both through the sandbox's own
# sockets, then the directory goes.
#
# What this fixed, so nobody learns it again: three hand-written repros
# leaked into the live setup. 2026-10-02 a debug picker without
# TMUX_COMPANION_SOCK replaced the live daemon. 2026-10-09 a wrapper with
# only TMUX_TMPDIR set ran `tmux kill-server` inside tmux and killed the live
# server, since $TMUX wins. 2026-10-13 a repro set HOME, XDG and TMUX_TMPDIR
# but not TMUX_COMPANION_SOCK, and the debug binary replaced the live daemon
# again; its cleanup called `stop`, which isn't a subcommand, so the
# sandbox daemons it started stayed running too. The daemon socket is
# /tmp/tmux-companion-<uid>.sock unless TMUX_COMPANION_SOCK says otherwise,
# whatever HOME is.
set -euo pipefail

usage() {
  sed -n '3,/^set -euo/p' "$0" | sed '$d' >&2
  exit 2
}

bin=""
keep=0
while [ $# -gt 0 ]; do
  case "$1" in
    --bin) [ $# -ge 2 ] || usage; bin=$2; shift 2 ;;
    --keep) keep=1; shift ;;
    -h|--help) usage ;;
    --) shift; break ;;
    *) break ;;
  esac
done

repo=$(cd "$(dirname "$0")/.." && pwd)
if [ -z "$bin" ]; then
  bin="$repo/target/debug/tmux-companion"
  [ -x "$bin" ] || (cd "$repo" && nice -n 15 cargo build -q -j 4)
fi
[ -x "$bin" ] || { echo "sandbox: no binary at $bin" >&2; exit 1; }
bin=$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")

# Short, because a unix socket path holds 103 bytes and two live under here.
SANDBOX=$(mktemp -d /tmp/tcsb.XXXXXX)
export SANDBOX
export HOME="$SANDBOX/home"
export XDG_CONFIG_HOME="$SANDBOX/config"
export XDG_STATE_HOME="$SANDBOX/state"
export XDG_CACHE_HOME="$SANDBOX/cache"
export XDG_DATA_HOME="$SANDBOX/data"
export TMUX_TMPDIR="$SANDBOX/tmux"
export TMUX_COMPANION_SOCK="$SANDBOX/d.sock"
unset TMUX TMUX_PANE
mkdir -p "$HOME" "$XDG_CONFIG_HOME/tmux-companion" "$XDG_STATE_HOME" \
  "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$TMUX_TMPDIR" "$SANDBOX/bin"
ln -s "$bin" "$SANDBOX/bin/tmux-companion"
export PATH="$SANDBOX/bin:$PATH"
export TC_BIN="$bin"

cleanup() {
  tmux kill-server >/dev/null 2>&1 || true
  "$bin" shutdown >/dev/null 2>&1 || true
  if [ "$keep" = 1 ]; then
    echo "sandbox: kept $SANDBOX" >&2
  else
    rm -rf "$SANDBOX"
  fi
}
trap cleanup EXIT

# The guard: the sockets this shell would use are the sandbox's, or nothing
# runs. A future edit that loses one of the exports above fails here rather
# than in somebody's live session.
for v in HOME XDG_CONFIG_HOME XDG_STATE_HOME TMUX_TMPDIR TMUX_COMPANION_SOCK; do
  case "${!v}" in
    "$SANDBOX"/*) ;;
    *) echo "sandbox: $v is ${!v}, not inside $SANDBOX; refusing" >&2; exit 1 ;;
  esac
done
if [ -n "${TMUX:-}" ]; then
  echo "sandbox: TMUX is still set; refusing" >&2
  exit 1
fi

if [ $# -eq 0 ]; then
  echo "sandbox: $SANDBOX (exit the shell to throw it away)" >&2
  "${SHELL:-bash}" -i
else
  "$@"
fi
