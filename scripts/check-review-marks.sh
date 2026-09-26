#!/usr/bin/env bash
# Fail when a review marker is still in the tree.
#
# usage: scripts/check-review-marks.sh [--quiet]
#          --quiet   print nothing when the tree is clean
#
# A question left for the owner is written as an at-sign, the name Yogesh or
# claude, and a subject in parentheses, in whatever comment syntax the file
# already uses. They are working notes: each one is a hole somebody has to fill
# before the thing ships, so a tree that still holds one is not a tree to
# release from. This prints every one as file:line:text and exits 1 when there
# is any. `just ci` and the release gate both run it.
#
# rg runs with --hidden on purpose. Without it rg skips every dotted path, so
# in a dotfiles repository it searches almost nothing and reports a clean tree
# while a marker sits in home/.config/tmux/tmux.conf. --hidden also pulls .git
# into the search, which is why that directory is excluded by name; target/ is
# excluded because a build copies source files into it.
#
# Where rg is not installed, a fresh CI runner say, it falls back to grep -r,
# which has no notion of a hidden path and so needs no --hidden, only the same
# two exclusions. Both tools exit 1 for "nothing matched", which here is the
# good outcome, and 2 for a real error, which still stops the script.
set -euo pipefail

QUIET=0
case "${1:-}" in
  --quiet)   QUIET=1 ;;
  -h|--help) sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  "")        ;;
  *)         echo "unknown argument: $1" >&2; exit 2 ;;
esac

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

pattern='@(Yogesh|claude)\('
if command -v rg >/dev/null 2>&1; then
  found=$(rg --hidden -n "$pattern" --glob '!target' --glob '!.git' . || [ $? -eq 1 ])
else
  found=$(grep -rnIE "$pattern" --exclude-dir=target --exclude-dir=.git . || [ $? -eq 1 ])
fi
found=$(printf '%s\n' "$found" | sed 's|^\./||' | sed '/^$/d')

if [ -n "$found" ]; then
  printf '%s\n' "$found"
  n=$(printf '%s\n' "$found" | wc -l | tr -d ' ')
  echo
  echo "$n review marker(s) still open. Each is a question for the owner; answer"
  echo "it and take the line out before this ships."
  exit 1
fi
[ "$QUIET" = 1 ] || echo "No review markers."
