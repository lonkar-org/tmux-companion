#!/bin/bash
#
# Check every third-party plugin this repository names: archived, gone, or
# quiet for too long.
#
# usage:
#   scripts/check-plugin-links.sh              report on all of them
#   scripts/check-plugin-links.sh --stale-days 540
#                                              change what counts as quiet
#   scripts/check-plugin-links.sh --quiet      print only what needs attention
#
# Exits non-zero when something is archived or gone, so it can gate a release.
# Staleness alone does not fail: a plugin that works and is finished is not a
# problem, and deciding otherwise is a judgement nobody should make in a script.
#
# Needs `gh`, authenticated. Rate limits apply; there are about thirty of these.
#
# Written after an audit found `b0o/tmux-autoreload` archived and
# `jrmoulton/tmux-port` deleted, both still credited here as though alive. The
# repository's convention is to credit a plugin whose idea it borrowed and to
# tell people to install theirs, which is only fair while theirs still exists.
set -euo pipefail

STALE_DAYS=540
QUIET=0
while [ $# -gt 0 ]; do
  case "$1" in
    --stale-days) STALE_DAYS=${2:?--stale-days needs a number}; shift 2 ;;
    --quiet)      QUIET=1; shift ;;
    -h|--help)    sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *)            echo "unknown flag: $1" >&2; exit 2 ;;
  esac
done

command -v gh >/dev/null || { echo "gh is not installed" >&2; exit 1; }
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

# Every `owner/repo` this repository mentions, from prose, config and code
# comments alike. Restricted to names that look like tmux plugins or are under
# a known tmux organisation, because `owner/repo` also matches paths.
# `read -a` rather than `mapfile`: macOS ships bash 3.2, where mapfile does
# not exist, and this script is run on the machine the docs are written on.
# Gathered into a file first. The obvious `while read < <(...)` form breaks
# when the pipeline inside it holds a `#` comment, which bash 3.2 reads as
# unterminated process substitution.
list=$(mktemp)
trap 'rm -f "$list"' EXIT
grep -rhoE '(^|[^/A-Za-z0-9_.-])[A-Za-z0-9][A-Za-z0-9-]*/[A-Za-z0-9-]*tmux[A-Za-z0-9-]*' \
  --include='*.md' --include='*.rs' --include='*.toml' --include='*.example' \
  docs src README.md 2>/dev/null |
  sed -E 's/^[^A-Za-z0-9]+//' |
  grep -vE '^(com|www|github|docs|src|scripts|target|tests|demo|playground|config|feature|home|share|man|tmp|plugins|usr|var|opt|etc)/' |
  grep -vE '\.(md|rs|toml|conf|example|sh)$' |
  grep -vE '/tmux-companion' |
  sort -u > "$list"

repos=()
while IFS= read -r line; do
  [ -n "$line" ] && repos+=("$line")
done < "$list"

now=$(date +%s)
bad=0
printf '%-44s %-9s %-12s %s\n' PLUGIN STATUS "LAST PUSH" STARS
printf '%s\n' "------------------------------------------------------------------------------"

for repo in "${repos[@]}"; do
  if ! json=$(gh repo view "$repo" --json isArchived,pushedAt,stargazerCount 2>/dev/null); then
    printf '%-44s %-9s %-12s %s\n' "$repo" GONE - -
    bad=1
    continue
  fi
  archived=$(printf '%s' "$json" | sed -n 's/.*"isArchived":\([a-z]*\).*/\1/p')
  pushed=$(printf '%s' "$json" | sed -n 's/.*"pushedAt":"\([^"]*\)".*/\1/p')
  stars=$(printf '%s' "$json" | sed -n 's/.*"stargazerCount":\([0-9]*\).*/\1/p')

  # BSD date and GNU date disagree about everything except -j, which only one
  # of them has.
  if date -j -f '%Y-%m-%dT%H:%M:%SZ' "$pushed" +%s >/dev/null 2>&1; then
    when=$(date -j -f '%Y-%m-%dT%H:%M:%SZ' "$pushed" +%s)
  else
    when=$(date -d "$pushed" +%s)
  fi
  days=$(( (now - when) / 86400 ))

  if [ "$archived" = true ]; then
    status=ARCHIVED; bad=1
  elif [ "$days" -gt "$STALE_DAYS" ]; then
    status=stale
  else
    status=ok
  fi

  [ "$QUIET" = 1 ] && [ "$status" = ok ] && continue
  printf '%-44s %-9s %-12s %s\n' "$repo" "$status" "${pushed%%T*}" "$stars"
done

echo
if [ "$bad" = 1 ]; then
  echo "Something above is archived or gone. This repository credits plugins and"
  echo "tells people to install them, so a dead one needs its mention changed to"
  echo "say so -- and a successor named only if its author named one."
  exit 1
fi
echo "Nothing archived or gone."
