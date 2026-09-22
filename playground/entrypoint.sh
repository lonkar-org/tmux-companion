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

# The tour first and detached, so it is already waiting on step one by the
# time anyone goes looking for it.
tmux -f "$CONF" new-session -d -s instructions -n tour -c "$HOME" \
  '/opt/playground/tour.sh'

tmux new-session -d -s playground -n shell -c "$HOME/projects/orchard-api" \
  '/opt/playground/tour.sh welcome'

# A session that outlives its only window would leave the container running
# with nothing in it.
tmux set -g destroy-unattached off

exec tmux attach -t playground
