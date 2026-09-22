#!/bin/bash
#
# Starts the playground: two sessions, and you land in the one you can break.
#
#   instructions   the guided tour, waiting on the first step
#   playground     a shell in ~/projects/orchard-api
#
# Usage: the image's ENTRYPOINT. Any arguments are run instead of attaching,
# so `docker run --rm -it <image> zsh` gives a plain shell with everything
# already set up, and `docker run --rm <image> tmux-companion doctor` works
# without a terminal at all.
set -euo pipefail

if [ "$#" -gt 0 ]; then
  exec "$@"
fi

if [ ! -t 0 ]; then
  echo "The playground needs a terminal. Run it with -it:" >&2
  echo "  docker run --rm -it <image>" >&2
  exit 1
fi

CONF="$HOME/.config/tmux/tmux.conf"

# Running this inside tmux means every prefix key is caught by the outer
# server before the playground ever sees it, and the tour reads as broken
# rather than as nested. Say so once, up front, rather than letting somebody
# work it out from keys that do nothing.
if [ -n "${TMUX:-}" ]; then
  cat <<'NESTED'

  You are already inside tmux, so Ctrl-b goes to that server, not to this one.

  Either run this in a terminal outside tmux, which is what the tour assumes,
  or press the prefix twice to reach the playground: Ctrl-b Ctrl-b ? instead
  of Ctrl-b ?.

NESTED
  printf '  Press Enter to carry on nested, or Ctrl-c to come back outside. '
  read -r _ || true
fi

# The tour first and detached, so it is already waiting on step one by the
# time anyone goes looking for it.
tmux -f "$CONF" new-session -d -s instructions -n tour -c "$HOME" \
  '/opt/playground/tour.sh'

tmux new-session -d -s playground -n shell -c "$HOME/projects/orchard-api" \
  '/opt/playground/tour.sh welcome'

# The tour reads this to know whether a step has been done yet.
tmux set -g @playground-step "starting"

# A session that outlives its only window would leave the container running
# with nothing in it.
tmux set -g destroy-unattached off

# Attached to the tour, not to the playground. Landing in the playground put
# somebody in a shell with no idea what any of the keys were, and the only
# thing telling them was a line on a status bar they had not been told to read
# yet.
#
# In a loop, because `prefix d` is the single most important thing tmux does
# and the basics track asks somebody to press it. `exec tmux attach` would
# make detaching end the container, which teaches the opposite of the lesson:
# the whole point is that the server is still there when the client is gone.
while true; do
  tmux attach -t instructions
  # Nothing left to attach to: every session was closed, so there is nothing
  # to come back to either.
  tmux has-session -t instructions 2>/dev/null || break

  cat <<'DETACHED'

  You detached. tmux is still running, with everything in it exactly as you
  left it: the shells, the editors, the half-finished commands.

  This is what survives an ssh drop, a closed laptop and a terminal you quit
  by accident. Nothing was saved, because nothing stopped.

DETACHED
  printf '  [Enter] to attach again, or type exit to leave the container: '
  IFS= read -r answer || break
  case "$answer" in
    exit | quit | q) break ;;
  esac
done
