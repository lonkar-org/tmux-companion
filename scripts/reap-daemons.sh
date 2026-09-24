#!/usr/bin/env bash
#
# Usage: scripts/reap-daemons.sh [-f|--force] [-a|--all] [--min-age SECONDS]
#                                [--sweep-files] [-q|--quiet]
#
#   -f, --force        actually kill.  Without it this only reports.
#   -a, --all          ignore --min-age: reap every daemon that is not the live
#                      one, including a test run happening this second.
#   --min-age SECONDS  how old a daemon on somebody else's socket has to be
#                      before it counts as abandoned.  Default 1800.
#   --sweep-files      also delete socket files that nothing is accepting on.
#   -q, --quiet        print nothing when there is nothing to reap.  For cron.
#
# Kill the tmux-companion daemons nothing will ever reach again, and only those.
#
# Why this exists: every test, benchmark and demo harness here points
# TMUX_COMPANION_SOCK at a socket of its own, and until the daemon grew a
# watchdog (src/server/mod.rs, watch_socket) nothing made one exit when its
# harness went away.  They accumulate one per run and nothing reports them.
#
# It classifies by the socket a process holds, never by its command line.
# `pkill -f 'tmux-companion server'` is the obvious version of this script and
# it is wrong twice: it takes the live daemon down with the orphans, and
# tests/socket_round_trip.rs records what happened when the suite did that to
# itself -- innocent tests failing with `SIGTERM before listening`, on CI only,
# because the windows overlap there.
#
# Two things learned the hard way, and why this is not three lines.  A
# leftover socket FILE does not mean a live daemon: an interrupted `cargo test`
# leaves /tmp/tce2e*.sock files with nothing behind most of them, so the file
# has to be probed rather than stat'ed.  And a daemon that IS accepting on its
# own socket can still be abandoned, because the test binary that started it
# died an hour ago, which is what --min-age is for.
set -euo pipefail

force=0
all=0
quiet=0
sweep=0
min_age=1800
while [[ $# -gt 0 ]]; do
  case "$1" in
    -f|--force)     force=1 ;;
    -a|--all)       all=1 ;;
    -q|--quiet)     quiet=1 ;;
    --sweep-files)  sweep=1 ;;
    --min-age)      shift; min_age="${1:?--min-age needs a number of seconds}" ;;
    -h|--help)      sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "reap-daemons: unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

# Both are load-bearing and neither is optional.  Without lsof no process has
# a socket, and without python3 no socket is accepting: either way every daemon
# on the machine falls into the reap bucket, including the live one.  A tool
# that deletes the right things only when its dependencies are installed is a
# tool that deletes the wrong things on the machine where they are not.
for tool in lsof python3; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "reap-daemons: needs $tool to tell a live daemon from an orphan; refusing to guess." >&2
    exit 3
  }
done

uid=$(id -u)
live_sock="${TMUX_COMPANION_SOCK:-/tmp/tmux-companion-${uid}.sock}"

# The socket files the harnesses in this repository create, so --sweep-files
# cleans up after them and touches nothing else under /tmp.  tce2e comes from
# tests/e2e.rs, tcr from the recording driver, tc-* from scripts/bench.sh.
sweep_globs=(/tmp/tce2e*.sock /tmp/tcr*.sock /tmp/tc-*.sock)

# Is anything accepting on this socket?  `nc -U -z` answers 0 either way on
# macOS, which is worse than no answer at all, so this asks the kernel.
accepting() {
  python3 -c 'import socket,sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(1)
sys.exit(0 if s.connect_ex(sys.argv[1]) == 0 else 1)' "$1" 2>/dev/null
}

# ps gives [[dd-]hh:]mm:ss and no macOS ps has etimes.  10# on every field
# because 08 and 09 are not octal numbers.
etime_seconds() {
  local e="$1" d=0 h=0 a b c
  [[ "$e" == *-* ]] && { d=${e%%-*}; e=${e#*-}; }
  IFS=: read -r a b c <<<"$e"
  if [[ -n "${c:-}" ]]; then h=$a; else c=$b; b=$a; a=0; h=0; fi
  echo $(( 10#$d * 86400 + 10#${h:-0} * 3600 + 10#${b:-0} * 60 + 10#${c:-0} ))
}

# The first named unix socket a process holds.  A daemon has exactly one: its
# listener.  The unnamed rows lsof also prints are whoever is talking to it.
held_socket() {
  lsof -p "$1" -a -U -F n 2>/dev/null | sed -n 's|^n\(/.*\)$|\1|p' | head -1
}

reap=()
spared=()
live=()

# `pgrep -f` loose enough to catch a copy of the binary renamed into a
# scratchpad, then a second look at the command so a client mid-request is not
# mistaken for a daemon.
while read -r pid; do
  [[ -z "$pid" ]] && continue
  [[ "$pid" == "$$" || "$pid" == "$PPID" ]] && continue
  cmd=$(ps -o command= -p "$pid" 2>/dev/null || true)
  [[ "$cmd" == *" server"* ]] || continue

  age=$(etime_seconds "$(ps -o etime= -p "$pid" 2>/dev/null | tr -d ' ')")
  sock=$(held_socket "$pid")

  if [[ "$sock" == "$live_sock" ]]; then
    live+=("$pid|$sock|the live daemon")
  elif [[ -z "$sock" ]]; then
    reap+=("$pid|<none>|holds no named socket")
  elif [[ ! -S "$sock" ]]; then
    reap+=("$pid|$sock|its socket file is gone")
  elif ! accepting "$sock"; then
    reap+=("$pid|$sock|not accepting on its own socket")
  elif [[ $all -eq 1 || $age -ge $min_age ]]; then
    reap+=("$pid|$sock|abandoned, up ${age}s")
  else
    spared+=("$pid|$sock|up ${age}s, younger than ${min_age}s")
  fi
done < <(pgrep -u "$uid" -f 'tmux-companion' || true)

# Two processes claiming the live socket is the bind race itself: one holds an
# unlinked inode and lsof still reports the name it bound under, so the path
# cannot separate them.  Refusing is the honest answer -- `tmux-companion
# __shutdown` and one client call sort it out.
if [[ ${#live[@]} -gt 1 ]]; then
  echo "reap-daemons: ${#live[@]} processes claim $live_sock; refusing to guess which one answers." >&2
  printf '  %s\n' "${live[@]}" >&2
  exit 1
fi

# The sockets nothing is accepting on.  Scanned twice when there is killing to
# do, and this is the second thing that has to be in the right order: a file is
# only stale once its daemon has gone, so a list built before the kill misses
# every file belonging to something on the reap list.  The first pass is for
# the report; the kill runs; the second pass is what actually gets deleted.
stale_files() {
  local f
  for f in "${sweep_globs[@]}"; do
    [[ -S "$f" ]] || continue
    [[ "$f" == "$live_sock" ]] && continue
    accepting "$f" || echo "$f"
  done
}

stale=()
if [[ $sweep -eq 1 ]]; then
  while read -r f; do [[ -n "$f" ]] && stale+=("$f"); done < <(stale_files)
fi

if [[ ${#reap[@]} -eq 0 && ${#stale[@]} -eq 0 ]]; then
  [[ $quiet -eq 1 ]] || echo "reap-daemons: nothing to reap (${#live[@]} live, ${#spared[@]} spared)."
  exit 0
fi

if [[ ${#reap[@]} -gt 0 ]]; then
  printf '%-8s %-40s %s\n' PID SOCKET WHY
  for t in "${reap[@]}"; do
    IFS='|' read -r pid sock why <<<"$t"
    printf '%-8s %-40s %s\n' "$pid" "$sock" "$why"
  done
fi
[[ ${#stale[@]} -gt 0 ]] && echo "${#stale[@]} socket files with nothing behind them."

if [[ $force -eq 0 ]]; then
  echo
  echo "${#reap[@]} daemons to reap. Re-run with --force to kill them."
  [[ ${#spared[@]} -gt 0 ]] && \
    echo "${#spared[@]} spared as too young; --all or --min-age includes them."
  exit 0
fi

if [[ ${#reap[@]} -gt 0 ]]; then
  pids=()
  for t in "${reap[@]}"; do IFS='|' read -r pid _ _ <<<"$t"; pids+=("$pid"); done

  kill "${pids[@]}" 2>/dev/null || true
  for _ in $(seq 1 20); do
    left=()
    for pid in "${pids[@]}"; do kill -0 "$pid" 2>/dev/null && left+=("$pid"); done
    [[ ${#left[@]} -eq 0 ]] && break
    sleep 0.1
  done
  left=()
  for pid in "${pids[@]}"; do kill -0 "$pid" 2>/dev/null && left+=("$pid"); done
  if [[ ${#left[@]} -gt 0 ]]; then
    echo "reap-daemons: ${#left[@]} ignored SIGTERM; sending SIGKILL."
    kill -9 "${left[@]}" 2>/dev/null || true
  fi
  echo "reap-daemons: reaped ${#pids[@]} daemons."
fi

if [[ $sweep -eq 1 ]]; then
  gone=0
  while read -r f; do
    [[ -n "$f" ]] || continue
    rm -f "$f" && gone=$((gone + 1))
  done < <(stale_files)
  echo "reap-daemons: removed $gone socket files."
fi

echo "reap-daemons: ${#live[@]} live daemon left running, ${#spared[@]} spared."
