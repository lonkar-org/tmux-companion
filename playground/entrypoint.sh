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
# Which step the tour last announced, so a tour that has to be rebuilt comes
# back where it was rather than at the beginning.
last_step() {
  local step
  step=$(tmux show -gv @playground-step 2>/dev/null || true)
  # The option is a sentence like "step 12: in sparrow-cli: ...".
  step=${step#step }
  step=${step%%:*}
  case $step in
    ''|*[!0-9]*) printf '1' ;;
    *) printf '%s' "$step" ;;
  esac
}

# Attach, and decide what to do each time the client comes back.
#
# Three ways out of this loop, and all three are reachable on purpose:
#
#   - typing `exit` at either prompt
#   - closing every session, so there is nothing left to attach to
#   - EOF on the prompt, for a terminal that has gone away
#
# The tour session being gone is deliberately *not* one of them, and it is
# deliberately not an automatic rebuild either. Step 12 asks for `prefix
# Shift-X` while somebody is reading it inside the tour session, so closing the
# tour by accident is the likeliest mistake in the playground -- and it used to
# end the container, because this loop attached by name and broke when the name
# was gone. Rebuilding it silently is the opposite mistake: `quit_tour` ends
# with `exec zsh`, so typing `exit` in the tour is a normal way to leave, and a
# loop that rebuilds would trap somebody in a playground they cannot get out
# of. So it asks.
while true; do
  if tmux has-session -t instructions 2>/dev/null; then
    # `|| true`, because `set -e` is on and attach exits non-zero when the
    # server goes away under it -- which is exactly the case this loop exists
    # to handle. Without it the script died right here and the container
    # stopped with status 1, before any of the logic below got a say.
    tmux attach -t instructions || true
  fi

  # Nothing at all left: no session to come back to, so there is no question
  # worth asking.
  tmux list-sessions >/dev/null 2>&1 || break

  if tmux has-session -t instructions 2>/dev/null; then
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
    continue
  fi

  # The tour session has gone. Either `prefix Shift-X` closed it -- which is
  # what that key does, to whichever project you are in, including this one --
  # or somebody typed `exit` in it after the tour handed them a shell. From out
  # here those look identical, so this asks rather than guessing.
  step=$(last_step)
  cat <<CLOSED

  The tour session is gone. That is what prefix Shift-X does: it closes
  whichever project you are in, and the tour is one of them.

  Everything else is still running.

    [Enter]  bring the tour back at step $step
    p        attach to the playground instead
    exit     leave the container

CLOSED
  printf '  Which? '
  IFS= read -r answer || break
  case "$answer" in
    exit | quit | q) break ;;
    p | playground)
      if tmux has-session -t playground 2>/dev/null; then
        tmux attach -t playground || true
      fi
      ;;
    *)
      tmux new-session -d -s instructions -n tour -c "$HOME" \
        "/opt/playground/tour.sh $step"
      ;;
  esac
done

# Leaving the playground is not a failure. Without this the script ends on the
# status of whatever last ran -- `tmux list-sessions` failing after the server
# was closed, say -- and `docker run` returns 1 for somebody who simply
# finished the tour, which reads as a broken container in any CI that runs it.
exit 0
