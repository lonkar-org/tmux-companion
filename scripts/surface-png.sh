#!/bin/bash
# Render the .ansi frames scripts/capture-surfaces.sh wrote into PNGs, so a
# change to the pickers' colours can be looked at rather than guessed at.
# Usage: scripts/surface-png.sh DIR [WIDTH HEIGHT]
# Each DIR/NAME.ansi becomes DIR/NAME.png. Needs agg (asciinema's gif
# renderer) and a Nerd Font in ~/Library/Fonts; sips turns agg's GIF into PNG.
# A capture is wrapped as a one-frame asciinema cast because agg only reads
# casts; the lines need \r\n or every row starts where the last one ended.
set -euo pipefail
DIR="${1:?dir}"; W="${2:-140}"; H="${3:-40}"
FONT_DIR="${FONT_DIR:-$HOME/Library/Fonts}"; FONT="${FONT:-FiraCode Nerd Font}"
for f in "$DIR"/*.ansi; do
  name="${f%.ansi}"
  python3 - "$f" "$W" "$H" > "$name.cast" <<'PY'
import json, sys
data = open(sys.argv[1], encoding="utf-8", errors="replace").read().replace("\n", "\r\n")
print(json.dumps({"version": 2, "width": int(sys.argv[2]), "height": int(sys.argv[3])}))
print(json.dumps([0.0, "o", "\x1b[H\x1b[2J" + data.rstrip("\r\n")]))
print(json.dumps([0.5, "o", ""]))
PY
  agg --font-dir "$FONT_DIR" --font-family "$FONT" --font-size 14 --theme monokai --last-frame-duration 0.1 "$name.cast" "$name.gif" >/dev/null 2>&1
  sips -s format png "$name.gif" --out "$name.png" >/dev/null
  rm -f "$name.cast" "$name.gif"
done
echo "pngs in $DIR"
