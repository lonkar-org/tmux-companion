#!/usr/bin/env bash
#
# The pickers and the daemon's idle cost under a capped CPU, isolated from the
# live tmux server and the live daemon.
#
# usage:
#   scripts/bench-capped.sh                  loads 0, 16 and 32, alternated twice
#   scripts/bench-capped.sh 0 16             only these loads
#   LABEL=lowpower scripts/bench-capped.sh 0 one run, named for what you changed
#   CONFIG=mine scripts/bench-capped.sh      a copy of your config and themes
#   ONLY=project,panes RUNS=25 scripts/bench-capped.sh
#
# env:
#   RUNS       presses per picker per load              (default 15)
#   ROUNDS     times the load list is repeated          (default 2)
#   CONFIG     default (none) or mine: a copy of your own tmux-companion
#              config, tmux.conf and tmux themes  (default default)
#   LABEL      prefix for each run's name               (default none)
#   ONLY       comma-separated pickers                  (default all)
#   BAR        call, auto or push: the status-right line and [bar] mode
#                                                         (default call)
#   OUT        directory for the json and the summary   (default target/bench/<stamp>)
#   BENCH_BIN  the binary under test                    (default target/release/…)
#
# Load N is N busy loops at normal priority for the length of the run. This
# machine has no efficiency cores to push work onto, so contention is the
# reproducible stand-in for a capped CPU: the tmux server, the popup and the
# daemon all compete with it the way they compete with a build. Loads are
# alternated rather than run back to back, because sustained load heats the
# machine and the thermal limit that follows drifts into whichever load ran
# last. `pmset -g therm` is recorded around every run so that drift shows.
# Low Power Mode is the other kind of cap, a lower clock rather than
# contention; it needs sudo, so it is a person's step: turn it on, run with
# LABEL=lowpower, turn it off.
#
# Isolation, which the older bench scripts did not have and which this exists
# for: HOME, every XDG directory and TMUX_TMPDIR point into a temporary
# directory, TMUX is unset, and the daemon socket is in there too. A bare
# `tmux` from the bench daemon's background loops therefore finds the private
# server and nothing else, the crash marker it writes is the private one, and
# stopping it is by its own pid, never by a pkill on the binary path.
set -euo pipefail

# tmux reads $TMUX before TMUX_TMPDIR. Run from inside a tmux pane, a
# `TMUX_TMPDIR=<private> tmux kill-server` therefore kills the server the pane
# lives in, not the private one, and that is what the first version of this
# script did to the live server it was started from. Every tmux call here
# names its socket with -S as well, so neither variable is load-bearing alone.
unset TMUX TMUX_PANE
bench_tmux() {
  local t=$1
  shift
  tmux -S "$t/tmux/tmux-$(id -u)/default" "$@"
}

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
RUNS=${RUNS:-15}
ROUNDS=${ROUNDS:-2}
CONFIG=${CONFIG:-default}
LABEL=${LABEL:-}
ONLY=${ONLY:-}
BIN=${BENCH_BIN:-$REPO/target/release/tmux-companion}
STAMP=$(date +%Y%m%d-%H%M%S)
OUT=${OUT:-$REPO/target/bench/$STAMP${LABEL:+-$LABEL}}
if [ $# -gt 0 ]; then LOADS=("$@"); else LOADS=(0 16 32); fi

[ -x "$BIN" ] || { echo "no binary at $BIN; run \`just build\` first" >&2; exit 1; }
for c in tmux python3 zoxide; do
  command -v "$c" >/dev/null || { echo "$c is not installed" >&2; exit 1; }
done
mkdir -p "$OUT"

LOOPS=()
stop_load() {
  if [ ${#LOOPS[@]} -gt 0 ]; then
    kill "${LOOPS[@]}" 2>/dev/null || true
    wait "${LOOPS[@]}" 2>/dev/null || true
  fi
  LOOPS=()
}
start_load() {
  local n=$1 i
  for ((i = 0; i < n; i++)); do
    yes >/dev/null &
    LOOPS+=($!)
  done
}

TMPS=()
cleanup() {
  stop_load
  for t in "${TMPS[@]}"; do
    bench_tmux "$t" kill-server 2>/dev/null || true
    rm -rf "$t"
  done
}
trap cleanup EXIT INT TERM

therm() {
  pmset -g therm 2>/dev/null | awk '/CPU_Speed_Limit|CPU_Scheduler_Limit/ {print $1 "=" $3}' | tr '\n' ' '
}

# Short: a unix socket path holds 104 bytes on macOS, and the per-user
# $TMPDIR under /var/folders eats most of that. Sets $t rather than printing
# it: called as $(make_tmp), the TMPS append would happen in a subshell and
# cleanup would never see the directory.
make_tmp() {
  t=$(mktemp -d /tmp/tcb.XXXXXX)
  TMPS+=("$t")
  mkdir -p "$t/home/.config/tmux" "$t/state" "$t/data" "$t/cache" "$t/bin"
  # tmux -S does not create the socket's directory, and when it cannot create
  # the socket it says so on stderr and still exits 0.
  mkdir -p -m 700 "$t/tmux/tmux-$(id -u)"
  # Whatever runs `tmux-companion` by name inside the bench (your tmux.conf's
  # hooks and bindings, the bar) runs the binary under test, not an installed one.
  ln -s "$BIN" "$t/bin/tmux-companion"
  if [ "$CONFIG" = mine ]; then
    # Copied with links followed: dotfiles are often symlinks into a repository,
    # and a symlink copied as a symlink would let the bench write through it.
    if [ -d "$HOME/.config/tmux-companion" ]; then
      cp -RL "$HOME/.config/tmux-companion" "$t/home/.config/"
    fi
    for f in tmux.conf themes; do
      if [ -e "$HOME/.config/tmux/$f" ]; then
        cp -RL "$HOME/.config/tmux/$f" "$t/home/.config/tmux/"
      fi
    done
  fi
}

{
  echo "binary   $BIN ($("$BIN" --version 2>/dev/null || echo '?'))"
  echo "commit   $(git -C "$REPO" rev-parse --short HEAD)$(git -C "$REPO" diff --quiet || echo '+dirty')"
  echo "machine  $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m), $(sysctl -n hw.ncpu) cpus, $(uname -sr)"
  echo "tmux     $(tmux -V)"
  echo "power    lowpowermode=$(pmset -g | awk '/lowpowermode/ {print $2}') $(pmset -g batt | head -1 | sed "s/.*'\(.*\)'.*/\1/")"
  echo "config   $CONFIG   runs $RUNS   rounds $ROUNDS   loads ${LOADS[*]}"
  echo "date     $(date '+%F %T %z')"
} | tee "$OUT/summary.txt"

for ((round = 1; round <= ROUNDS; round++)); do
  for load in "${LOADS[@]}"; do
    name="${LABEL:+$LABEL-}load$load-r$round"
    make_tmp
    echo | tee -a "$OUT/summary.txt"
    echo "== $name   before: $(therm)  loadavg $(sysctl -n vm.loadavg)" | tee -a "$OUT/summary.txt"
    start_load "$load"
    sleep 2
    env -u TMUX -u TMUX_PANE \
      HOME="$t/home" \
      XDG_CONFIG_HOME="$t/home/.config" \
      XDG_STATE_HOME="$t/state" \
      XDG_DATA_HOME="$t/data" \
      XDG_CACHE_HOME="$t/cache" \
      TMUX_TMPDIR="$t/tmux" \
      PATH="$t/bin:$PATH" \
      BENCH_TMP="$t" \
      BENCH_BIN="$BIN" \
      python3 "$HERE/bench-capped.py" --label "$name" --runs "$RUNS" \
        ${ONLY:+--only "$ONLY"} --json "$OUT/$name.json" | tee -a "$OUT/summary.txt"
    stop_load
    echo "   after: $(therm)" | tee -a "$OUT/summary.txt"
    bench_tmux "$t" kill-server 2>/dev/null || true
    sleep 5   # let the machine settle before the next condition
  done
done

echo
echo "wrote $OUT"
