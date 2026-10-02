#!/bin/bash
#
# The playground's second way in: the accessibility walkthrough, for trying
# tmux-companion with a screen reader, colour filters or Sticky Keys.
#
# Usage:
#   /opt/playground/a11y.sh        the shipped defaults, what an install gets
#   /opt/playground/a11y.sh on     with the accessibility settings switched on
#
# From the host: `scripts/playground.sh a11y [on]`, or `just playground-a11y`.
#
# The tour is left out on purpose. Its second status line and its step-by-step
# session are scaffolding for sighted learning, and a screen reader would read
# them as part of the product. tmux starts on docs/tmux.conf.full.example and
# nothing else, so what gets tested is what somebody installs.
#
# Runs after entrypoint.sh has copied the seeded home in, which is why this is
# an argument to the image rather than an ENTRYPOINT of its own.
set -euo pipefail

if [ ! -t 0 ]; then
  echo "The walkthrough needs a terminal. Run it with -it." >&2
  exit 1
fi

CONF="$HOME/.config/tmux/companion.conf"
CONFIG="$HOME/.config/tmux-companion/config.toml"

# `on` adds what the walkthrough's second round tests. The seeded config has
# none of these sections, so appending them cannot make a table twice.
if [ "${1:-}" = on ]; then
  cat >> "$CONFIG" <<'EOF'

# Added by a11y.sh on: the settings the walkthrough's second round tests.
[notify]
enabled = true
threshold_secs = 10
message_ms = 0

[sessions]
confirm_wait = true
EOF
fi

tmux -f "$CONF" new-session -d -s walkthrough -n steps -c "$HOME" \
  "less /opt/playground/a11y-walkthrough.txt; exec zsh"
tmux new-session -d -s orchard-api -c "$HOME/projects/orchard-api"
tmux new-session -d -s sparrow-cli -c "$HOME/projects/sparrow-cli"
tmux set -g destroy-unattached off

# One attach, no loop: leaving the walkthrough ends the container, which is
# the simplest thing to say out loud and the easiest to do without looking.
exec tmux attach -t walkthrough
