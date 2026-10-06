#!/usr/bin/env bash
#
# Run the integration suites again and again on one pinned tmux, with more
# test threads than cores, and count the failures per test.
#
# usage:
#   scripts/stress-e2e.sh                 5 runs on tmux 3.7c, 2 threads a core
#   scripts/stress-e2e.sh 20 3.4          20 runs on tmux 3.4
#   THREADS=32 scripts/stress-e2e.sh 10   a heavier crowd
#
# A flake is a test that holds on a laptop and loses a race on a runner with
# three or four busy cores. Rerunning it here at the laptop's own pace proves
# nothing: on 2026-10-06 a pocket test that typed into a shell 500 ms after
# it opened failed on macOS CI and never here. Twice as many test threads as
# cores makes every test share the machine with the others, which is the
# runner's condition, and the per-test count at the end names a one-in-ten
# flake that a pass/fail per run would hide. `just repro-ci` is the same idea
# inside an Ubuntu container; this one runs natively, on any pinned tmux.
set -euo pipefail

RUNS="${1:-5}"
VERSION="${2:-3.7c}"
CORES=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
THREADS="${THREADS:-$((CORES * 2))}"
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$HERE/.."

bin=$("$HERE/tmux-build.sh" "$VERSION")
export PATH="$bin:$PATH"
[ "$(tmux -V)" = "tmux $VERSION" ] || { echo "PATH gives $(tmux -V), not $VERSION" >&2; exit 1; }

LOGS=$(mktemp -d)
trap './scripts/reap-daemons.sh --force --sweep-files --quiet || true' EXIT INT TERM
SUITES=(--test e2e --test socket_round_trip --test config_file)
nice -n 15 cargo test -j 4 --no-run "${SUITES[@]}" >/dev/null 2>&1

failed_runs=0
for i in $(seq 1 "$RUNS"); do
  if RUST_TEST_THREADS="$THREADS" nice -n 15 cargo test -q --no-fail-fast "${SUITES[@]}" \
    >"$LOGS/run-$i.log" 2>&1; then
    echo "run $i/$RUNS: ok"
  else
    failed_runs=$((failed_runs + 1))
    echo "run $i/$RUNS: FAILED ($(grep -c ' \.\.\. FAILED' "$LOGS/run-$i.log" || true) tests)"
  fi
done

echo
echo "tmux $VERSION, $RUNS runs, $THREADS test threads on $CORES cores: $failed_runs failed"
if [ "$failed_runs" -gt 0 ]; then
  echo "failures per test:"
  cat "$LOGS"/run-*.log | grep -oE '^test [a-z_0-9:]+ \.\.\. FAILED' \
    | awk '{print $2}' | sort | uniq -c | sort -rn
  first=$(grep -l ' \.\.\. FAILED' "$LOGS"/run-*.log | head -1)
  echo
  echo "the first failure in full, from $first:"
  sed -n '/^failures:$/,/^test result/p' "$first" | head -60
  exit 1
fi
rm -rf "$LOGS"
