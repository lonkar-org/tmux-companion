#!/bin/bash
#
# Build and run the tmux-companion playground container.
#
# Usage:
#   scripts/playground.sh                 build if needed, then run
#   scripts/playground.sh build           build only
#   scripts/playground.sh run             run only, fails if not built
#   scripts/playground.sh shell           a plain shell in the image, no tour
#   scripts/playground.sh smoke           check the image is what the tour claims
#   scripts/playground.sh rebuild         build from scratch, no cache
#
# Env:
#   IMAGE   image tag to build and run   (default tmux-companion:playground)
#
# The build compiles the crate inside the image, so the first one takes a few
# minutes and later ones reuse the cargo registry layer. Nothing is mounted
# from the host and no ports are published: the container is disposable and
# anything done inside it dies with it.
set -euo pipefail

IMAGE="${IMAGE:-tmux-companion:playground}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

build() {
  docker build "$@" -f "$ROOT/playground/Dockerfile" -t "$IMAGE" "$ROOT"
}

have_image() { docker image inspect "$IMAGE" >/dev/null 2>&1; }

# --rm because it is a playground, and -it because tmux needs a terminal.
run() { docker run --rm -it "$IMAGE" "$@"; }

case "${1:-default}" in
  build)   build ;;
  rebuild) build --no-cache ;;
  run)     have_image || { echo "no image $IMAGE: run '$0 build' first" >&2; exit 1; }
           run ;;
  shell)   have_image || build
           run zsh ;;
  smoke)   have_image || build
           # The checks run inside a tmux session rather than as the
           # container's own process: `project` opens a session and attaches
           # to it, which with a terminal on the container would leave the
           # smoke run sitting in nvim forever.
           docker run --rm "$IMAGE" bash -c '
             tmux -f ~/.config/tmux/tmux.conf new-session -d -s instructions
             tmux new-session -d -s playground -c ~/projects/orchard-api
             sleep 1
             tmux new-session -d -s smoke \
               "/opt/playground/smoke.sh >/tmp/smoke.log 2>&1; echo \$? >/tmp/smoke.rc"
             for _ in $(seq 1 60); do [ -f /tmp/smoke.rc ] && break; sleep 1; done
             cat /tmp/smoke.log
             exit "$(cat /tmp/smoke.rc 2>/dev/null || echo 1)"' ;;
  default) have_image || build
           run ;;
  *)       sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
           exit 1 ;;
esac
