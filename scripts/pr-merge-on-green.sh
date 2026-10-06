#!/usr/bin/env bash
#
# Squash-merge pull requests one at a time, each only once every check on its
# current head has passed.
#
# usage:
#   scripts/pr-merge-on-green.sh 3 5 6          after asking for rebases
#   FRESH=0 scripts/pr-merge-on-green.sh 7      judge the head it has now
#   POLL=60 TIMEOUT=7200 scripts/pr-merge-on-green.sh 5
#
# With FRESH=1, the default, a pull request is not judged until its head
# commit has moved from the one it had when this started: the use is right
# after `@dependabot rebase`, and the checks still on the old head are the
# stale red ones the rebase is for. A pull request that conflicts with main,
# which merging the one before it can cause (two Cargo.lock bumps), gets
# `@dependabot rebase` once more and is waited on again. One failed check
# stops this with the check's name and leaves that pull request open; nothing
# red is merged.
#
# Written 2026-10-06 for the three Dependabot pull requests that went red
# against the tmux CI used to install, rather than waiting on each by hand.
set -euo pipefail

POLL="${POLL:-30}"
TIMEOUT="${TIMEOUT:-5400}"
FRESH="${FRESH:-1}"
[ $# -gt 0 ] || { echo "usage: scripts/pr-merge-on-green.sh PR..." >&2; exit 2; }

view() { gh pr view "$1" --json state,headRefOid,mergeStateStatus,statusCheckRollup; }

# Every head as it is now, before any is waited on: a pull request rebased
# while the one before it was still being merged has already moved, and
# reading its head only when its turn came would wait for a move that was
# over. (The first run sat on #6 for exactly that.)
# An indexed array in the order given, not an associative one: macOS's own
# bash is 3.2 and has none.
STARTED=()
for pr in "$@"; do STARTED+=("$(view "$pr" | jq -r .headRefOid)"); done

i=0
for pr in "$@"; do
  start="${STARTED[$i]}"
  i=$((i + 1))
  need_new="$FRESH"
  deadline=$(( $(date +%s) + TIMEOUT ))
  asked_at=""
  echo "#$pr: waiting (head ${start:0:7})"
  while :; do
    [ "$(date +%s)" -lt "$deadline" ] || { echo "#$pr: timed out" >&2; exit 1; }
    v=$(view "$pr")
    state=$(jq -r .state <<<"$v")
    head=$(jq -r .headRefOid <<<"$v")
    merge=$(jq -r .mergeStateStatus <<<"$v")
    if [ "$state" = "MERGED" ]; then echo "#$pr: already merged"; break; fi
    if [ "$state" != "OPEN" ]; then echo "#$pr: $state, skipped" >&2; break; fi
    if [ "$need_new" = "1" ] && [ "$head" = "$start" ]; then sleep "$POLL"; continue; fi
    if [ "$merge" = "DIRTY" ]; then
      if [ "$asked_at" != "$head" ]; then
        gh pr comment "$pr" --body "@dependabot rebase" >/dev/null
        asked_at="$head"
        echo "#$pr: conflicts with main, asked for a rebase"
      fi
      start="$head"; need_new=1
      sleep "$POLL"; continue
    fi
    # Every check run and status on this head: pending, failed or passed.
    pending=$(jq '[.statusCheckRollup[] | select((.status // "COMPLETED") != "COMPLETED" or (.state // "") == "PENDING")] | length' <<<"$v")
    failed=$(jq -r '[.statusCheckRollup[] | select((.conclusion // .state) as $c | $c == "FAILURE" or $c == "ERROR" or $c == "CANCELLED" or $c == "TIMED_OUT")] | map(.name // .context) | join(", ")' <<<"$v")
    total=$(jq '.statusCheckRollup | length' <<<"$v")
    if [ -n "$failed" ]; then
      echo "#$pr: red on ${head:0:7}: $failed; left open" >&2
      exit 1
    fi
    if [ "$total" -eq 0 ] || [ "$pending" -gt 0 ] || [ "$merge" = "UNKNOWN" ]; then
      sleep "$POLL"; continue
    fi
    echo "#$pr: $total checks green on ${head:0:7}, merging"
    gh pr merge "$pr" --squash --match-head-commit "$head"
    break
  done
done
