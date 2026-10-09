#!/usr/bin/env bash
#
# Two binaries through bench-capped, alternated: a git revision against the
# working tree's release build.
#
# usage:
#   scripts/bench-ab.sh <rev>                  load 0, two rounds, every picker
#   scripts/bench-ab.sh <rev> 0 32             these loads
#   ONLY=keys RUNS=4 ROUNDS=3 scripts/bench-ab.sh HEAD~1
#
# env:
#   RUNS, ONLY, CONFIG   passed to bench-capped.sh   (CONFIG default mine)
#   ROUNDS               before/after pairs per load (default 2)
#   OUT                  where each run's json goes  (default target/bench/ab-<rev>-<stamp>)
#
# The revision is built in a throwaway git worktree into target/ab/<sha>, so
# the working tree is not touched and a second run against the same revision
# reuses the build. The arms alternate, before then after, ROUNDS times per
# load, because a machine under load heats up and whichever arm ran second
# would otherwise carry the thermal limit.
#
# Written after the shared pane list was measured by hand this way: the
# worktree build, the alternation and the cleanup are three things a hand run
# gets wrong in a different way each time.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
[ $# -ge 1 ] || { sed -n '3,13p' "$0" >&2; exit 2; }
REV=$1
shift
if [ $# -gt 0 ]; then LOADS=("$@"); else LOADS=(0); fi
ROUNDS=${ROUNDS:-2}
export CONFIG=${CONFIG:-mine}
SHA=$(git -C "$REPO" rev-parse --short "$REV")
OUT=${OUT:-$REPO/target/bench/ab-$SHA-$(date +%Y%m%d-%H%M%S)}
BEFORE=$REPO/target/ab/$SHA/release/tmux-companion
AFTER=$REPO/target/release/tmux-companion
mkdir -p "$OUT"

if [ ! -x "$BEFORE" ]; then
  tree=$(mktemp -d /tmp/tcab.XXXXXX)
  trap 'git -C "$REPO" worktree remove --force "$tree" 2>/dev/null || true; rm -rf "$tree"' EXIT
  git -C "$REPO" worktree add -q --detach "$tree" "$SHA"
  (cd "$tree" && nice -n 15 cargo build --release -j 4 --target-dir "$REPO/target/ab/$SHA")
fi
(cd "$REPO" && nice -n 15 cargo build --release -j 4)

for load in "${LOADS[@]}"; do
  for ((r = 1; r <= ROUNDS; r++)); do
    for arm in before after; do
      bin=$BEFORE
      [ "$arm" = after ] && bin=$AFTER
      echo "== load $load round $r: $arm ($([ "$arm" = before ] && echo "$SHA" || echo working tree))"
      BENCH_BIN="$bin" ROUNDS=1 OUT="$OUT/$arm-load$load-r$r" \
        "$HERE/bench-capped.sh" "$load" | grep -E '^  ' || true
    done
  done
done
echo
echo "wrote $OUT"
