#!/usr/bin/env bash
# Cut a release of the Claude Code plugin: the skill in skills/ and the two
# manifests under .claude-plugin/. Nothing here touches the binary.
#
# usage:
#   scripts/release-plugin.sh check              is the skill ahead of its last tag?
#   scripts/release-plugin.sh 0.3.0              bump, validate, commit, tag, push
#   scripts/release-plugin.sh 0.3.0 --dry-run    print every step, change nothing
#   scripts/release-plugin.sh 0.3.0 --no-push    tag locally, push nothing
#   scripts/release-plugin.sh 0.3.0 --force      skip the dirty-tree and
#                                                nothing-changed refusals
#
# The plugin and the binary keep separate version numbers on purpose. They are
# released by different people at different times: the binary when Rust changes,
# the plugin when SKILL.md does, and a plugin that claimed a new version on
# every cargo release would say "changed" twenty times while the file sat still.
# The tag namespaces are separate too -- `claude plugin tag` writes
# `tmux-companion--v0.3.0` and release.yml triggers on `v*`, which that does not
# match -- so cutting a plugin release never fires a binary build.
#
# The number is the weak half of this. What catches a skill that has gone stale
# is the e2e test `the_skill_names_no_command_that_went_away`, which reads
# --help and fails on a command the file still names. This script only makes
# sure the two manifests agree and that the tag exists to install from.
set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PLUGIN=.claude-plugin/plugin.json
MARKET=.claude-plugin/marketplace.json
NAME=tmux-companion
# What a release of this plugin is made of. A change anywhere else is a change
# to the binary and gets a `v*` tag instead.
WATCHED=(skills .claude-plugin)

VERSION=""
DRY=0
PUSH=1
FORCE=0
MODE=release

say() { printf 'release-plugin: %s\n' "$*" >&2; }
die() { printf 'release-plugin: %s\n' "$*" >&2; exit 1; }
run() { if [ "$DRY" = 1 ]; then printf '  would run: %s\n' "$*" >&2; else "$@"; fi; }

while [ $# -gt 0 ]; do
  case $1 in
    check)     MODE=check; shift ;;
    --dry-run) DRY=1; shift ;;
    --no-push) PUSH=0; shift ;;
    --force)   FORCE=1; shift ;;
    -h|--help) sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*)        die "unknown argument: $1" ;;
    *)         [ -z "$VERSION" ] || die "one version, not two: $VERSION and $1"
               VERSION=$1; shift ;;
  esac
done

[ -f "$PLUGIN" ] || die "no $PLUGIN here"

current=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$PLUGIN")
last_tag=$(git tag --list "$NAME--v*" --sort=-v:refname | head -1)

# ── what has changed since the last plugin release ───────────────────────────
#
# No tag yet means everything is new, which is the right answer for the first
# release rather than something to special-case further down.
changed() {
  if [ -z "$last_tag" ]; then
    printf 'no %s--v* tag yet\n' "$NAME"
    return 0
  fi
  git diff --stat "$last_tag..HEAD" -- "${WATCHED[@]}"
}

if [ "$MODE" = check ]; then
  out=$(changed)
  if [ -n "$out" ]; then
    say "the skill is ahead of ${last_tag:-its first release}:"
    printf '%s\n' "$out" >&2
    say "cut it with: just plugin-release <version>   (current: $current)"
    exit 1
  fi
  say "skills/ and .claude-plugin/ are unchanged since $last_tag ($current)"
  exit 0
fi

[ -n "$VERSION" ] || die "which version? try: scripts/release-plugin.sh 0.3.0"
[[ $VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] ||
  die "$VERSION is not a semantic version"
[ "$VERSION" != "$current" ] || die "$PLUGIN is already at $VERSION"

if [ "$FORCE" = 0 ] && [ "$DRY" = 0 ]; then
  [ -z "$(git status --porcelain)" ] ||
    die "the working tree is dirty. Commit or stash first, or pass --force."
  [ -n "$(changed)" ] ||
    die "nothing under ${WATCHED[*]} changed since $last_tag, so there is
  nothing to release. Pass --force if you are bumping for another reason."
fi

# ── the two manifests, which have to agree ───────────────────────────────────
#
# `claude plugin tag` refuses when plugin.json and the marketplace entry carry
# different versions, so both are written here rather than leaving the second
# one to be remembered. json.dump rather than sed: marketplace.json holds a
# second `version` under `metadata`, which is the catalog's own and not this.
say "$current -> $VERSION"
if [ "$DRY" = 1 ]; then
  printf '  would write %s and the %s entry in %s\n' "$PLUGIN" "$NAME" "$MARKET" >&2
else
  python3 - "$VERSION" "$PLUGIN" "$MARKET" "$NAME" <<'PY'
import json, sys

version, plugin_path, market_path, name = sys.argv[1:5]

with open(plugin_path) as f:
    plugin = json.load(f)
plugin["version"] = version
with open(plugin_path, "w") as f:
    json.dump(plugin, f, indent=2)
    f.write("\n")

with open(market_path) as f:
    market = json.load(f)
entries = [p for p in market.get("plugins", []) if p.get("name") == name]
if not entries:
    sys.exit(f"{market_path} has no plugin entry called {name}")
for entry in entries:
    entry["version"] = version
with open(market_path, "w") as f:
    json.dump(market, f, indent=2)
    f.write("\n")
PY
fi

# ── checks ───────────────────────────────────────────────────────────────────
if command -v claude >/dev/null 2>&1; then
  run claude plugin validate .
else
  say "claude is not on PATH, so the manifests went unvalidated"
fi

run git add "$PLUGIN" "$MARKET"
run git commit -m "Cut the skill at $VERSION"

# `claude plugin tag` is the one that checks the manifests agree before writing
# the tag. Without claude, a plain annotated tag in the same namespace installs
# the same way; it just checks nothing.
tag="$NAME--v$VERSION"
if command -v claude >/dev/null 2>&1; then
  if [ "$PUSH" = 1 ]; then
    run claude plugin tag . -m "$NAME skill %s" --push
  else
    run claude plugin tag . -m "$NAME skill %s"
  fi
else
  run git tag -a "$tag" -m "$NAME skill $VERSION"
  [ "$PUSH" = 0 ] || run git push origin "$tag"
fi

say "tagged $tag"
[ "$PUSH" = 1 ] || say "nothing pushed. git push origin $tag when you are ready."
