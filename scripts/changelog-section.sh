#!/usr/bin/env bash
# Print one version's section of CHANGELOG.md, for the release notes.
#
# usage: scripts/changelog-section.sh VERSION [CHANGELOG]
#          VERSION     0.8.0 or v0.8.0
#          CHANGELOG   the file to read, CHANGELOG.md by default
#
# Prints what sits under `## VERSION - date`, prelude and Added, Changed and
# Fixed, up to the next `## ` heading, with the blank lines at either end
# trimmed. Exits 1 with a line on stderr when the file has no section for the
# version, or the section is empty, which is how the release gate stops a tag
# whose changelog was never cut from `## Unreleased`.
#
# Written on 2026-10-07 because every GitHub release up to 0.8.0 went out
# with install lines, checksums and GitHub's generated PR list, which only
# ever held Dependabot's PRs, and nothing a person had written about what
# changed.
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
  sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
fi
version="${1#v}"
file="${2:-CHANGELOG.md}"

section=$(awk -v v="$version" '
  /^## / {
    if (inside) exit
    # `## 0.8.0 - 2026-10-06`, or a bare `## 0.8.0`.
    if ($2 == v && (NF == 2 || $3 == "-")) { inside = 1; next }
  }
  inside { print }
' "$file" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')

if [ -z "${section//[[:space:]]/}" ]; then
  echo "changelog-section: $file has no section for $version; cut it from ## Unreleased before tagging" >&2
  exit 1
fi
printf '%s\n' "$section"
