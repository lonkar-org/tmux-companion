#!/bin/bash
#
# Drive the playground the way a person does, from inside the container.
#
# Usage: not run by hand. `scripts/playground.sh tourpass` pipes this into the
# image's entrypoint, once with the hardening flags and once without:
#
#   docker run --rm -i [flags] <image> bash -s < playground/tourpass.sh
#
# smoke.sh checks what the image contains. It can't press a key: nothing is
# attached, so no popup ever opens. This starts an outer tmux server and runs
# the real entrypoint inside it, which gives the playground an attached client,
# and sends keys to the outer pane so they reach it exactly as typing would.
#
# Each check waits up to nine seconds for text that only that screen draws, and
# on a miss prints the screen and carries on, so one run lists every failure.
# The waits are polls, but the keys between them are paced with sleeps, so a
# loaded machine can fail a check that is fine. That's why this is a manual
# check before a release and not a CI job.
#
# Exit status: the number of failed checks, capped at 100.
set -euo pipefail

OUTER=(tmux -L outer)
fails=0

screen() { "${OUTER[@]}" capture-pane -p -t outer; }

# `grep <<<"$(screen)"` rather than `screen | grep -q`: under pipefail, grep -q
# leaving early can SIGPIPE capture-pane and fail a check that matched.
see() { # see <label> <regex>
  for _ in $(seq 1 30); do
    if grep -Eq -- "$2" <<<"$(screen)"; then
      echo "  ok    $1"
      return 0
    fi
    sleep 0.3
  done
  echo "  FAIL  $1   (wanted /$2/)"
  screen | sed 's/^/  | /'
  fails=$((fails + 1))
}

key() { "${OUTER[@]}" send-keys -t outer "$@"; sleep 0.6; }

# A command typed into the playground session, and what it printed.
typed() { # typed <command> [seconds]
  tmux send-keys -t playground:1 "clear; $1; echo RC=\$?" Enter
  sleep "${2:-1.5}"
  tmux capture-pane -p -t playground:1
}

said() { # said <label> <output> <regex>...
  local label=$1 out=$2 re line
  shift 2
  for re in "$@"; do
    if ! grep -Eq -- "$re" <<<"$out"; then
      echo "  FAIL  $label   (wanted /$re/)"
      while IFS= read -r line; do printf '  | %s\n' "$line"; done <<<"$out"
      fails=$((fails + 1))
      return 0
    fi
  done
  echo "  ok    $label"
}

"${OUTER[@]}" -f /dev/null new-session -d -s outer -x 200 -y 50 \
  "env -u TMUX /opt/playground/entrypoint.sh; echo ENTRYPOINT-EXIT-\$?; sleep 30"

echo "starting"
see "the tour opens on step 1" "step 1 of 18"
see "step 1 is the keyboard step" "Two keyboard things"
see "the second status line carries the step" "step 1: press t for tmux basics"

for _ in $(seq 1 20); do
  pgrep -f 'tmux-companion server' >/dev/null && break
  sleep 0.3
done
if pgrep -f 'tmux-companion server' >/dev/null; then
  echo "  ok    the daemon is running"
else
  echo "  FAIL  the daemon is running"
  fails=$((fails + 1))
fi

echo "commands in the playground session"
said "doctor" "$(typed 'tmux-companion doctor' 3)" 'RC=0' 'daemon +running'
said "setup --print" "$(typed 'tmux-companion setup --print | head -5' 3)" 'RC=0' 'Keys'
said "the git segment renders" \
  "$(typed 'tmux-companion gst ~/projects/orchard-web | cat -v | head -c 200; echo')" \
  'RC=0' 'M-'
said "both manual pages open" \
  "$(typed 'MANPAGER=cat man tmux-companion | head -3; MANPAGER=cat man tmux | head -3' 2)" \
  'RC=0' 'TMUX-COMPANION' 'TMUX\(1\)'
said "zoxide, git and the local remote" \
  "$(typed 'z anvil && pwd && git status -sb | head -1 && git fetch origin 2>&1 | tail -1' 2)" \
  'RC=0' 'anvil-infra' 'ahead 2'
said "sessions save, then list" \
  "$(typed 'tmux-companion sessions save && tmux-companion sessions list | head -3' 3)" \
  'RC=0' 'taken while running'
said "cheatsheet --print" "$(typed 'tmux-companion cheatsheet --print | head -2' 2)" 'RC=0'
# kill reads the terminal's foreground group, which busybox ps can't show.
said "ps has the columns kill needs" "$(typed 'ps -o pid,pgid,tpgid,comm | head -3' 2)" \
  'RC=0' 'TPGID'

echo "keys through the attached client"
key C-b ')'
key C-b ')'
key C-b '?'
see "prefix ? opens the key search" '\[ Keys \]'
key Escape
key C-b C-c
see "prefix C-c draws the cheat sheet" 'Panes, windows, sessions, projects'
key Escape
# The playground gives prefix P to the project picker, for a browser that
# swallows Alt.
key C-b P
see "prefix P opens the project picker" '\[ Project \]'
key -l "lantern"
key Enter
sleep 2
see "picking a project opens it in nvim" 'A fake project. Nothing here talks'
key Escape
key -l ":qa!"
key Enter
key C-b C-t
see "prefix C-t opens the theme picker" '\[ Theme'
key Escape
key C-b e
see "prefix e opens the run picker" '\[ Run command \]'
key Escape
# The journal can be empty this early, and then the reason is on the message
# line instead of a list.
key C-b J
see "prefix J opens the journal" '\[ Journal \]|nothing in the journal'
key Escape
key C-b i
see "prefix i goes back to the tour" 'step [0-9]+ of 18'

echo "detaching"
key C-b d
see "detaching lands on the entrypoint's prompt" 'You detached'
key Enter
see "Enter attaches again" 'step [0-9]+ of 18'

echo "what it cost"
echo "  info  processes now: $(ps -A -o pid= | wc -l)"
echo "  info  home: $(du -sh ~ | cut -f1), /tmp: $(du -sh /tmp 2>/dev/null | cut -f1)"
echo "  info  cgroup memory.peak: $(cat /sys/fs/cgroup/memory.peak 2>/dev/null || echo n/a)," \
  "pids.peak: $(cat /sys/fs/cgroup/pids.peak 2>/dev/null || echo n/a)"

echo "fails=$fails"
exit $((fails > 100 ? 100 : fails))
