#!/usr/bin/env bash
# Print one version's section of CHANGELOG.md, for the release notes.
#
# usage: scripts/changelog-section.sh [--details] VERSION [CHANGELOG]
#          --details   each `### Added`, `### Changed`, `### Fixed` and any
#                      other `###` part as a collapsed <details> block, the
#                      prelude left open: what the release notes print
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

details=0
if [ "${1:-}" = --details ]; then
  details=1
  shift
fi
if [ $# -lt 1 ] || [ $# -gt 2 ]; then
  sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//' >&2
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
if [ "$details" = 0 ]; then
  printf '%s\n' "$section"
  exit 0
fi
# GitHub renders Markdown inside <details> only with a blank line after the
# summary and before the closing tag.
# A part already ends on a blank line before the next heading, and the blank
# under a heading is dropped since the summary line brings its own.
printf '%s\n' "$section" | awk '
  /^### / {
    if (open) print "</details>\n"
    print "<details>\n<summary>" substr($0, 5) "</summary>\n"
    open = 1
    under = 1
    next
  }
  under && /^$/ { under = 0; next }
  { under = 0; print }
  END { if (open) print "\n</details>" }
'

