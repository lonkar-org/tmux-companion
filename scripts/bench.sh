#!/usr/bin/env bash
#
# Every benchmark this repository has, both arms, from inside a real tmux.
#
# usage:
#   scripts/bench.sh                    pickers and the bar, both arms
#   scripts/bench.sh pickers            keypress to first row, and CPU per press
#   scripts/bench.sh bar                what the status bar costs per second
#   scripts/bench.sh segments           per-request daemon CPU, new arm only
#   RUNS=25 SECS=90 scripts/bench.sh    longer, for a quieter number
#
# env:
#   RUNS       presses per picker per arm      (default 15)
#   SECS       seconds of bar per arm          (default 45)
#   OUT        directory for the json          (default /tmp)
#   BENCH_BIN  the binary under test           (default target/release/…)
#
# The old arm is not a reconstruction. The five-call bar comes out of the
# mysetup repository at 18e8db9^, the commit that replaced those scripts with
# this tool, and the two pickers are the zsh ones still installed under
# ~/.config/tmux/comrades. Neither is vendored here: they are somebody's
# dotfiles, and a copy in this repository would drift from the original with
# nothing to notice.
#
# So this runs on the author's machine and nowhere else. Anyone else gets the
# new arm on its own, which is still the useful half: what it costs today.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
WHAT=${1:-all}
RUNS=${RUNS:-15}
SECS=${SECS:-45}
OUT=${OUT:-/tmp}
BIN=${BENCH_BIN:-$REPO/target/release/tmux-companion}

have() { command -v "$1" >/dev/null 2>&1; }

[ -x "$BIN" ] || { echo "no binary at $BIN; run \`just build\` first" >&2; exit 1; }
for c in tmux python3 zsh; do
  have "$c" || { echo "$c is not installed" >&2; exit 1; }
done

# The old arm's dependencies, reported once and together rather than as
# whichever one fails first halfway through a five minute run.
arms=both
missing=
have fzf     || missing="$missing fzf"
have zoxide  || missing="$missing zoxide"
[ -d "$HOME/.config/tmux/comrades" ] || missing="$missing ~/.config/tmux/comrades"
[ -d "$HOME/git-repos/mysetup" ]     || missing="$missing ~/git-repos/mysetup"
[ -x "$HOME/go/bin/yrl" ]            || missing="$missing yrl"
if [ -n "$missing" ]; then
  echo "the before arm needs:$missing" >&2
  echo "measuring the new arm only" >&2
  arms=new
fi

echo "binary   $BIN"
echo "arms     $arms"
echo "machine  $(uname -sr), $(sysctl -n hw.ncpu 2>/dev/null || nproc) cores, $(date +%F)"

if [ "$WHAT" = all ] || [ "$WHAT" = pickers ]; then
  echo
  echo "== pickers: keypress to first row, and CPU per press =="
  BENCH_BIN="$BIN" python3 "$HERE/bench-keys.py" --arm "$arms" --runs "$RUNS" \
    --json "$OUT/bench-pickers.json"
fi

if [ "$WHAT" = all ] || [ "$WHAT" = bar ]; then
  echo
  echo "== the status bar, per second, one attached client =="
  BENCH_BIN="$BIN" python3 "$HERE/bench-bar.py" --arm "$arms" --secs "$SECS" \
    --json "$OUT/bench-bar.json"
fi

if [ "$WHAT" = all ] || [ "$WHAT" = segments ]; then
  echo
  echo "== daemon CPU per request, new arm only =="
  python3 "$HERE/bench-cpu.py" "$BIN" "today"
fi
