#!/usr/bin/env bash
#
# The pickers and the daemon's idle cost under a capped CPU, isolated from the
# live tmux server and the live daemon.
#
# usage:
#   scripts/bench-capped.sh                  loads 0, 16 and 32, alternated twice
#                                            (on Linux 0, 1x and 2x the core count)
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
#   LINK       1 adds two fake hosts and measures the link: the idle cost
#              with one up, and the hello's time         (default off)
#   REMOTE     N gives switch sessions N rows from a fake host, and times a
#              pick of one to the host's first byte in the mirror pane, and
#              the host's agent asking to the earcon here (default 0)
#   EARCON_RUNS  asks timed with REMOTE                  (default 10)
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
# On Linux (a Raspberry Pi 5 is the machine it was written for) the same run
# reads nproc, /proc/loadavg and the device-tree or cpuinfo model in place of
# sysctl, and records throttling with `vcgencmd get_throttled`, `vcgencmd
# measure_clock arm` and thermal_zone0 in place of `pmset -g therm`; what
# vcgencmd is missing from is left out of the line rather than guessed. Over a
# bare `ssh host cmd` there is no TERM and often no UTF-8 locale, and tmux
# then refuses to attach or draws every glyph as `_`, so on Linux both get a
# default when missing. macOS output is what it was.
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
OS=$(uname -s)
if [ $# -gt 0 ]; then
  LOADS=("$@")
elif [ "$OS" = Linux ]; then
  CORES=$(nproc)
  LOADS=(0 "$CORES" $((CORES * 2)))
else
  LOADS=(0 16 32)
fi
if [ "$OS" = Linux ]; then
  if [ -z "${TERM:-}" ] || [ "$TERM" = dumb ]; then export TERM=xterm-256color; fi
  case "$(locale charmap 2>/dev/null || true)" in
    UTF-8) ;;
    *) export LC_ALL=C.UTF-8 ;;
  esac
fi

[ -x "$BIN" ] || { echo "no binary at $BIN; run \`just build\` first" >&2; exit 1; }
for c in tmux python3 zoxide cc git; do
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
  if [ "$OS" = Linux ]; then
    therm_linux
    return
  fi
  pmset -g therm 2>/dev/null | awk '/CPU_Speed_Limit|CPU_Scheduler_Limit/ {print $1 "=" $3}' | tr '\n' ' '
}

# throttled=0x0 arm=2400MHz temp=48.2C, each part only where the machine has
# it. get_throttled's bits: 0x1 under-voltage, 0x2 arm frequency capped, 0x4
# throttled, 0x8 soft temperature limit; the same four shifted by 16 mean it
# happened since boot.
therm_linux() {
  local out="" v hz z
  if command -v vcgencmd >/dev/null; then
    v=$(vcgencmd get_throttled 2>/dev/null | sed -n 's/^throttled=//p') || true
    [ -n "$v" ] && out+="throttled=$v "
    hz=$(vcgencmd measure_clock arm 2>/dev/null | sed -n 's/^frequency([0-9]*)=//p') || true
    [ -n "$hz" ] && out+="arm=$((hz / 1000000))MHz "
  fi
  if [ -z "${hz:-}" ] && [ -r /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq ]; then
    out+="cpu0=$(($(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq) / 1000))MHz "
  fi
  z=/sys/class/thermal/thermal_zone0/temp
  if [ -r "$z" ]; then
    out+="temp=$(awk '{printf "%.1fC", $1 / 1000}' "$z") "
  fi
  printf '%s' "$out"
}

loadavg() {
  if [ "$OS" = Linux ]; then
    awk '{print "{ " $1, $2, $3 " }"}' /proc/loadavg
  else
    sysctl -n vm.loadavg
  fi
}

machine() {
  if [ "$OS" != Linux ]; then
    echo "$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m), $(sysctl -n hw.ncpu) cpus, $(uname -sr)"
    return
  fi
  local model="" cpu=""
  [ -r /proc/device-tree/model ] && model=$(tr -d '\0' </proc/device-tree/model)
  cpu=$(lscpu 2>/dev/null | sed -n 's/^Model name: *//p' | head -1)
  [ -n "$cpu" ] || cpu=$(sed -n 's/^model name[[:space:]]*: *//p' /proc/cpuinfo | head -1)
  echo "${model:+$model, }${cpu:-$(uname -m)}, $(nproc) cpus, $(uname -sr)"
}

power() {
  if [ "$OS" != Linux ]; then
    echo "lowpowermode=$(pmset -g | awk '/lowpowermode/ {print $2}') $(pmset -g batt | head -1 | sed "s/.*'\(.*\)'.*/\1/")"
    return
  fi
  local gov=/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor
  echo "governor=$(cat "$gov" 2>/dev/null || echo '?') $(therm_linux)"
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
  echo "machine  $(machine)"
  echo "tmux     $(tmux -V)"
  echo "power    $(power)"
  echo "config   $CONFIG   runs $RUNS   rounds $ROUNDS   loads ${LOADS[*]}"
  echo "date     $(date '+%F %T %z')"
} | tee "$OUT/summary.txt"

for ((round = 1; round <= ROUNDS; round++)); do
  for load in "${LOADS[@]}"; do
    name="${LABEL:+$LABEL-}load$load-r$round"
    make_tmp
    echo | tee -a "$OUT/summary.txt"
    echo "== $name   before: $(therm)  loadavg $(loadavg)" | tee -a "$OUT/summary.txt"
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
