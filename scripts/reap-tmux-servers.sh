#!/usr/bin/env bash
#
# Usage: scripts/reap-tmux-servers.sh [-f|--force] [-a|--all] [--min-age SECONDS]
#                                     [--sweep-files] [-q|--quiet]
#
#   -f, --force        actually stop servers and delete files.  Without it this
#                      only reports.
#   -a, --all          ignore --min-age: stop every `tmux -L` server, including
#                      one a test started this second.
#   --min-age SECONDS  how old a `tmux -L` server has to be before it counts as
#                      abandoned.  Default 1800.
#   --sweep-files      also delete socket files in tmux's directory that no
#                      server answers on.
#   -q, --quiet        print nothing when there is nothing to reap.
#
# Stop the tmux servers tests, spikes and demos started on sockets of their
# own, and only those. The companion to reap-daemons.sh, which does the same
# for tmux-companion daemons.
#
# Why this exists: the e2e suite, the key-routing spikes and capture-surfaces.sh
# each start tmux with `-L <name>`, and a run that is interrupted, or a spike
# that is never torn down, leaves its server running with nothing attached. On
# 2026-10-07 there were seventeen of them, five days old, two running `stty
# raw; exec dd` forever, and 5361 socket files in /tmp/tmux-501 from runs long
# finished.
#
# What it never touches: the server on the `default` socket and the one $TMUX
# names, which between them are the tmux somebody is working in. A server is
# picked out by `-L` on its command line, and stopped with `tmux -L NAME
# kill-server`, never with a signal, so tmux closes its own panes. A socket
# file is deleted only when `list-sessions` on it gets no answer, because a
# file with no server behind it and a live server look the same to `ls`.
set -euo pipefail

force=0
all=0
quiet=0
sweep=0
min_age=1800

while [ $# -gt 0 ]; do
  case "$1" in
    -f | --force) force=1 ;;
    -a | --all) all=1 ;;
    -q | --quiet) quiet=1 ;;
    --sweep-files) sweep=1 ;;
    --min-age)
      shift
      min_age=${1:?--min-age needs a number of seconds}
      ;;
    -h | --help)
      sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "reap-tmux-servers: unknown argument: $1" >&2
      exit 2
      ;;
  esac
  shift
done

say() { [ "$quiet" = 1 ] || echo "$@"; }

# ps's etime, [[dd-]hh:]mm:ss, in seconds. macOS ps has no etimes.
seconds() {
  local t=$1 d=0 h=0 m=0 s=0
  case "$t" in *-*) d=${t%%-*}; t=${t#*-} ;; esac
  IFS=: read -r -a p <<<"$t"
  case ${#p[@]} in
    3) h=${p[0]}; m=${p[1]}; s=${p[2]} ;;
    2) m=${p[0]}; s=${p[1]} ;;
    1) s=${p[0]} ;;
  esac
  echo $((10#$d * 86400 + 10#$h * 3600 + 10#$m * 60 + 10#$s))
}

dir="${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"
# The socket this shell's tmux is on, when it is one with a name of its own.
mine=""
if [ -n "${TMUX:-}" ]; then
  mine=$(basename "${TMUX%%,*}")
fi
unset TMUX

# Each name once, with the age of its oldest process: an inner test server
# shows up twice, once as the server and once as the client the outer one runs.
declare -a names=() ages=()
while read -r etime name; do
  [ -n "$name" ] || continue
  age=$(seconds "$etime")
  found=0
  for i in "${!names[@]}"; do
    if [ "${names[$i]}" = "$name" ]; then
      found=1
      [ "$age" -gt "${ages[$i]}" ] && ages[$i]=$age
    fi
  done
  [ "$found" = 1 ] || { names+=("$name"); ages+=("$age"); }
done < <(ps -axo etime=,command= | awk '$2 == "tmux" && $3 == "-L" { print $1, $4 }')

stopped=0
young=0
for i in "${!names[@]}"; do
  name=${names[$i]}
  age=${ages[$i]}
  case "$name" in default | "$mine") continue ;; esac
  if [ "$all" = 0 ] && [ "$age" -lt "$min_age" ]; then
    young=$((young + 1))
    continue
  fi
  if [ "$force" = 1 ]; then
    if tmux -L "$name" kill-server 2>/dev/null; then
      say "stopped tmux -L $name (${age}s old)"
    else
      say "gone already: tmux -L $name"
    fi
  else
    say "would stop tmux -L $name (${age}s old)"
  fi
  stopped=$((stopped + 1))
done
[ "$young" = 0 ] || say "left $young younger than ${min_age}s; --all takes them too"

swept=0
if [ "$sweep" = 1 ] && [ -d "$dir" ]; then
  [ "$force" = 1 ] && sleep 1
  for s in "$dir"/*; do
    [ -S "$s" ] || continue
    name=$(basename "$s")
    case "$name" in default | "$mine") continue ;; esac
    if ! tmux -S "$s" list-sessions >/dev/null 2>&1; then
      [ "$force" = 1 ] && rm -f "$s"
      swept=$((swept + 1))
    fi
  done
  if [ "$force" = 1 ]; then
    [ "$swept" = 0 ] || say "deleted $swept socket files nothing answers on, in $dir"
  else
    [ "$swept" = 0 ] || say "would delete $swept socket files nothing answers on, in $dir"
  fi
fi

if [ "$stopped" = 0 ] && [ "$swept" = 0 ]; then
  say "nothing to reap"
elif [ "$force" = 0 ]; then
  say "run again with --force to do it"
fi
