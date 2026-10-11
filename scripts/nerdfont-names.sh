#!/usr/bin/env bash
# Regenerate src/presets/nf-md-names.tsv from one Nerd Fonts release.
#
# usage: scripts/nerdfont-names.sh [VERSION SHA256]
#          VERSION  a Nerd Fonts release tag without the v, default below
#          SHA256   sha256 of that release's glyphnames.json
#
# The git segment picks a glyph for a branch prefix it has no entry for by
# looking the prefix word up among the Material Design glyph names (nf-md-*),
# so `parked/` finds nf-md-parking. That list is embedded in the binary, and
# this is the one way it is made: fetch glyphnames.json at a pinned tag, check
# its hash, keep only the md set, and write it sorted by name for the binary
# search in src/branch_icons.rs.
#
# Only md, because it is one family drawn to one grid, it is the largest set
# (6880 names in 3.4.0) and it sits in its own plane (U+F0001..U+F1AF0), so no
# pick can land on a codepoint another family's glyph shares.
#
# glyphnames.json sits at the root of the Nerd Fonts repository, outside every
# folder with its own licence, so it is under the repository's MIT licence;
# the header written below carries that notice. The names come from Material
# Design Icons (Pictogrammers), Apache 2.0. Only names and codepoints are
# copied, no glyph shapes.
#
# A new version can renumber or drop a name. The picks the daemon has already
# made are kept in its state directory by codepoint, so regenerating this file
# changes nothing a running bar draws for a prefix it has seen.
set -euo pipefail

VERSION="${1:-3.4.0}"
SHA256="${2:-e2d10d23f5bff0bd6f0676e9b01d9789fcdc656de7b498a2955c27716ea4439c}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/src/presets/nf-md-names.tsv"
URL="https://raw.githubusercontent.com/ryanoasis/nerd-fonts/v$VERSION/glyphnames.json"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl -sSfL -o "$work/glyphnames.json" "$URL"
got="$(shasum -a 256 "$work/glyphnames.json" | cut -d' ' -f1)"
if [[ "$got" != "$SHA256" ]]; then
  echo "glyphnames.json for v$VERSION hashes to $got, expected $SHA256" >&2
  exit 1
fi

{
  cat <<EOF
# Nerd Fonts v$VERSION glyphnames.json, the md (Material Design) set only:
# one glyph name without its nf-md- prefix, a tab, its codepoint in hex.
# Sorted by name in byte order; src/branch_icons.rs binary-searches it.
# Written by scripts/nerdfont-names.sh; regenerate rather than edit.
#
# Source: $URL
# Names from Material Design Icons (Pictogrammers), Apache License 2.0.
#
# The MIT License (MIT)
#
# Copyright (c) 2014 Ryan L McIntyre
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.
EOF
  jq -r 'to_entries[] | select(.key | startswith("md-")) | "\(.key[3:])\t\(.value.code)"' \
    "$work/glyphnames.json" | LC_ALL=C sort
} >"$OUT"

echo "wrote $(grep -vc '^#' "$OUT") names to $OUT"
