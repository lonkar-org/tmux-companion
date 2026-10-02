#!/bin/bash
#
# Build and run the tmux-companion playground container.
#
# Usage:
#   scripts/playground.sh                 build if needed, then run
#   scripts/playground.sh build           build only
#   scripts/playground.sh run             run only, fails if not built
#   scripts/playground.sh shell           a plain shell in the image, no tour
#   scripts/playground.sh a11y [on]       the accessibility walkthrough instead
#                                         of the tour; `on` with the settings
#   scripts/playground.sh smoke           check the image is what the tour claims
#   scripts/playground.sh rebuild         build from scratch, no cache
#   scripts/playground.sh tourpass        drive the tour with keys, hardened and
#                                         plain, before a release (not for CI)
#
# Env:
#   IMAGE   image tag to build and run   (default tmux-companion:playground)
#   PLAIN   1 to run and smoke without the hardening flags below, the way
#           `docker run --rm -it <image>` does
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

# The container runs with nothing it does not use, and every flag here was
# checked against smoke.sh and a pass through the tour's commands:
#
#   --cap-drop=ALL, no-new-privileges   play is uid 1000 and never needs a
#                                       capability or a setuid binary
#   --read-only, and two tmpfs          the image cannot be written; /tmp holds
#                                       the tmux and daemon sockets, /home/play
#                                       the copy the entrypoint makes of the
#                                       seeded home, owned by play's uid 1000.
#                                       Both noexec, Docker's default for a
#                                       tmpfs, which nothing in here minds
#   --network=none                      nothing in the tour leaves the machine:
#                                       anvil-infra's remote is a bare repo in
#                                       the home, and [online] is off
#   --pids-limit, --memory              a fork bomb or a runaway build in the
#                                       playground stops at the container. A
#                                       tour pass peaked at 63 tasks and 28 MB;
#                                       the pids headroom is for the daemon's
#                                       tokio runtime, one thread per core
#
# PLAIN=1 runs without any of it, which is the one-line command the README
# gives. playground.yml smokes the image both ways.
#
# SC2054 reads the commas in the tmpfs options as a mistyped array.
# shellcheck disable=SC2054
HARDEN=(
  --cap-drop=ALL
  --security-opt=no-new-privileges
  --read-only
  --tmpfs /tmp:rw,nosuid,nodev,size=64m
  --tmpfs /home/play:rw,nosuid,nodev,uid=1000,gid=1000,mode=0750,size=256m
  --network=none
  --pids-limit=512
  --memory=512m
)
# `${FLAGS[@]+...}` rather than "${FLAGS[@]}", because macOS's bash 3.2 calls
# an empty array unbound under set -u.
if [ "${PLAIN:-0}" = 1 ]; then FLAGS=(); else FLAGS=("${HARDEN[@]}"); fi

# --rm because it is a playground, and -it because tmux needs a terminal.
run() { docker run --rm -it ${FLAGS[@]+"${FLAGS[@]}"} "$IMAGE" "$@"; }

case "${1:-default}" in
  build)   build ;;
  rebuild) build --no-cache ;;
  run)     have_image || { echo "no image $IMAGE: run '$0 build' first" >&2; exit 1; }
           run ;;
  shell)   have_image || build
           run zsh ;;
  a11y)    have_image || build
           shift
           run /opt/playground/a11y.sh "$@" ;;
  smoke)   have_image || build
           # The checks run inside a tmux session rather than as the
           # container's own process: `project` opens a session and attaches
           # to it, which with a terminal on the container would leave the
           # smoke run sitting in nvim forever.
           docker run --rm ${FLAGS[@]+"${FLAGS[@]}"} "$IMAGE" bash -c '
             tmux -f ~/.config/tmux/tmux.conf new-session -d -s instructions
             tmux new-session -d -s playground -c ~/projects/orchard-api
             sleep 1
             tmux new-session -d -s smoke \
               "/opt/playground/smoke.sh >/tmp/smoke.log 2>&1; echo \$? >/tmp/smoke.rc"
             for _ in $(seq 1 60); do [ -f /tmp/smoke.rc ] && break; sleep 1; done
             cat /tmp/smoke.log
             exit "$(cat /tmp/smoke.rc 2>/dev/null || echo 1)"' ;;
  tourpass)
           have_image || build
           # Both ways every time, whatever PLAIN says: the point is that the
           # flags break nothing the plain run does. playground/tourpass.sh
           # says what it checks and why it's not a CI job.
           status=0
           for mode in hardened plain; do
             if [ "$mode" = plain ]; then flags=(); else flags=("${HARDEN[@]}"); fi
             echo "== $mode"
             docker run --rm -i ${flags[@]+"${flags[@]}"} "$IMAGE" bash -s \
               < "$ROOT/playground/tourpass.sh" || status=1
           done
           exit "$status" ;;
  default) have_image || build
           run ;;
  *)       sed -n '2,25p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
           exit 1 ;;
esac
