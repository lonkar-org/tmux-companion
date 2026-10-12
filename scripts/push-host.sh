#!/usr/bin/env bash
#
# Put a binary on a host in place of the tmux-companion there, over one ssh
# command, so a host with an older build runs this one.
#
# usage:
#   scripts/push-host.sh BINARY HOST-COMMAND...
#   scripts/push-host.sh "$(scripts/build-linux.sh)" ssh vm
#
# HOST-COMMAND is the argv that opens ssh to the host, the same one a
# [[hosts]] entry carries; the remote line is appended as its last word and
# the binary travels on stdin, so no scp and no second connection.
#
# The binary goes to /usr/local/bin with `sudo -n install` when sudo needs no
# password there, else to ~/.local/bin. A daemon already running on the host
# is moved onto it with `restart`; a host with no daemon gains none. The last
# line printed is the host's `tmux-companion --version`.
set -euo pipefail

if [ $# -lt 2 ]; then
  sed -n '3,/^set -euo/p' "$0" | sed '$d' >&2
  exit 2
fi
BIN=$1
shift
[ -f "$BIN" ] || { echo "push-host: no such binary: $BIN" >&2; exit 1; }

read -r -d '' REMOTE <<'EOF' || true
set -e
tmp=$(mktemp)
cat > "$tmp"
chmod 755 "$tmp"
if sudo -n true 2>/dev/null; then
  sudo -n install -m 755 "$tmp" /usr/local/bin/tmux-companion
  dest=/usr/local/bin/tmux-companion
else
  mkdir -p "$HOME/.local/bin"
  install -m 755 "$tmp" "$HOME/.local/bin/tmux-companion"
  dest=$HOME/.local/bin/tmux-companion
fi
rm -f "$tmp"
if [ -S "/tmp/tmux-companion-$(id -u).sock" ]; then "$dest" restart >/dev/null 2>&1 || true; fi
"$dest" --version
EOF
"$@" "$REMOTE" < "$BIN"
