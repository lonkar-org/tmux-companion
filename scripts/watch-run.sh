#!/usr/bin/env bash
#
# Wait for a GitHub Actions run to finish, then print what it did and what it
# complained about.
#
# usage:
#   scripts/watch-run.sh                    the newest run on this branch
#   scripts/watch-run.sh ci.yml             the newest run of one workflow
#   scripts/watch-run.sh release.yml        ...
#   scripts/watch-run.sh 35864695338        a run id
#   TIMEOUT=1800 scripts/watch-run.sh       give a slow matrix longer
#
# It prints a conclusion per job and then the annotations, because a run can be
# green and still be telling you something: the workflows sat on four
# `actions/*` at Node 20 for weeks while every run reported success, and the
# only place that showed up was the annotations block `gh run view` hides
# behind a flag nobody passes.
#
# A run that is waiting on an environment approval is reported as such and
# exits 0 rather than hanging until TIMEOUT: `release.yml` parks its publish
# job behind a required reviewer on purpose, so waiting for it is waiting for a
# person.
set -euo pipefail

TIMEOUT=${TIMEOUT:-1200}
INTERVAL=${INTERVAL:-15}

arg=${1:-}
if [[ $arg =~ ^[0-9]+$ ]]; then
  run=$arg
else
  # --limit 1 after the filters, so a workflow name picks that workflow's
  # newest run rather than filtering one run that may be another workflow's.
  run=$(gh run list ${arg:+--workflow="$arg"} --branch "$(git rev-parse --abbrev-ref HEAD)" \
        --limit 1 --json databaseId -q '.[0].databaseId')
  [ -n "$run" ] || { echo "no run found${arg:+ for $arg} on this branch" >&2; exit 1; }
fi

echo "watching run $run" >&2
deadline=$(( $(date +%s) + TIMEOUT ))
while :; do
  read -r status conclusion < <(gh run view "$run" --json status,conclusion \
    -q '[.status, (.conclusion // "-")] | @tsv')
  [ "$status" = completed ] && break

  # A job whose status is waiting is sitting on a deployment approval. Nothing
  # here can move it, so say so and stop rather than burning the timeout.
  waiting=$(gh run view "$run" --json jobs -q '[.jobs[] | select(.status == "waiting") | .name] | join(", ")')
  if [ -n "$waiting" ]; then
    echo "waiting on an environment approval: $waiting" >&2
    break
  fi

  [ "$(date +%s)" -lt "$deadline" ] || { echo "still $status after ${TIMEOUT}s" >&2; break; }
  sleep "$INTERVAL"
done

gh run view "$run" --json status,conclusion,displayTitle,url \
  -q '"\(.displayTitle)\n\(.status)\t\(.conclusion // "-")\n\(.url)"'
echo
gh run view "$run" --json jobs -q '.jobs[] | "\(.conclusion // .status)\t\(.name)"'
echo
gh run view "$run" --log-failed 2>/dev/null | tail -40 || true
echo "=== annotations ==="
gh run view "$run" --json jobs -q '.jobs[].databaseId' | while read -r job; do
  gh api "repos/{owner}/{repo}/check-runs/$job/annotations" \
    -q '.[] | "\(.annotation_level)\t\(.path)\t\(.message)"' 2>/dev/null || true
done | sort -u
