#!/usr/bin/env bash
# tpm entry point. Sourced by tmux once, when the plugin loads.
#
#   set -g @plugin 'lonkar-org/tmux-companion'
#
# All this does is make sure the binary exists. It binds no keys and sets no
# options, because what goes on your status bar and which key opens which
# picker is yours to decide, and a plugin that decided for you would be the
# thing this tool exists to avoid.
#
# docs/tmux.conf.example is the bar, docs/tmux.conf.full.example is every
# feature with what each costs.
#
# On first load it downloads the release binary for this machine, checks it
# against the published checksums, and installs it. With no matching release
# it builds from the checkout tpm already made, which needs Rust 1.85.
#
# @tmux-companion-install-prefix sets where the binary goes; the default is
# /usr/local when writable and ~/.local otherwise. Set it before the plugin
# line:
#
#   set -g @tmux-companion-install-prefix "$HOME/.local"
#
# Set @tmux-companion-auto-install to "off" to be told rather than helped.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

option() {
  local value
  value=$(tmux show-option -gqv "$1" 2>/dev/null || true)
  [ -n "$value" ] && printf '%s' "$value" || printf '%s' "$2"
}

notify() { tmux display-message "tmux-companion: $*" 2>/dev/null || true; }

command -v tmux-companion >/dev/null 2>&1 && exit 0

if [ "$(option @tmux-companion-auto-install on)" != "on" ]; then
  notify "not installed. Run $HERE/scripts/install.sh"
  exit 0
fi

PREFIX=$(option @tmux-companion-install-prefix "")
args=()
[ -n "$PREFIX" ] && args+=(--prefix "$PREFIX")

# Detached, because tpm runs this while tmux is starting and a build is
# minutes. A status bar that waits for a compiler is a tmux that looks hung.
(
  if "$HERE/scripts/install.sh" "${args[@]}" >"$HERE/.install.log" 2>&1; then
    notify "installed. Reload your config to use it."
  else
    notify "install failed, see $HERE/.install.log"
  fi
) &
