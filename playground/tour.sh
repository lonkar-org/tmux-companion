#!/bin/bash
#
# The guided tour. Runs in the `instructions` session; you do the steps in the
# `playground` session and come back here to press Enter.
#
# Usage:
#   /opt/playground/tour.sh            the tour
#   /opt/playground/tour.sh welcome    the banner the playground shell opens with
#
# Every step sets @playground-step, which the second status line draws, so the
# instruction is still in front of you after you have switched away from it.
set -uo pipefail

BOLD=$'\033[1m'; DIM=$'\033[2m'; OFF=$'\033[0m'
KEY=$'\033[1;38;5;222m'; OK=$'\033[38;5;114m'; WARN=$'\033[38;5;208m'
RULE=$'\033[38;5;240m'

# The four glyphs written as escapes rather than as literal bytes, so the
# codepoints stay the ones src/tmux/icons.rs uses whatever an editor does to
# this file. branch, battery, ssh, and the separator between segments.
GLYPHS=$(printf '     \U000f062c branch    \uf240 battery    \U000f08c0 ssh    \ue0bc separator')

TOTAL=16
n=0
skipped=0

rule()  { printf '%s%s%s\n' "$RULE" "────────────────────────────────────────────────────────────────────────" "$OFF"; }
keys()  { printf '\n    %s%s%s\n' "$KEY" "$*" "$OFF"; }
hint()  { tmux set -g @playground-step "$*" 2>/dev/null || true; }

begin() {              # begin <icon> <title>
  n=$(( n + 1 ))
  clear
  printf '%s%s  %s%s %s(step %d of %d)%s\n' "$BOLD" "$1" "$2" "$OFF" "$DIM" "$n" "$TOTAL" "$OFF"
  rule
  printf '\n'
}

# wait_for [verify-command] [what-is-missing]
#
# Enter moves on. With a verify command, Enter only moves on once the command
# succeeds, so a step that was not actually done says so instead of scrolling
# past. `s` skips it anyway and `q` leaves the tour.
wait_for() {
  local verify="${1:-}" missing="${2:-}"
  while true; do
    printf '\n%s[Enter] when done    [s] skip    [q] quit the tour%s ' "$DIM" "$OFF"
    IFS= read -r answer || { answer=q; printf '\n'; }
    case "$answer" in
      q|Q) printf '\n%sThe tour is over. The shell is still here: run `tour` to start again.%s\n' "$DIM" "$OFF"
           exec zsh ;;
      s|S) skipped=$(( skipped + 1 )); return ;;
      "")  if [ -z "$verify" ] || eval "$verify" >/dev/null 2>&1; then
             return
           fi
           printf '\n%sNot yet: %s%s\n' "$WARN" "$missing" "$OFF" ;;
      *)   printf '\n%sEnter, s or q.%s\n' "$DIM" "$OFF" ;;
    esac
  done
}

windows_now() { tmux list-windows -a 2>/dev/null | wc -l | tr -d ' '; }

# ── The banner the playground shell opens with ──────────────────────────────
if [ "${1:-}" = "welcome" ]; then
  clear
  cat <<EOF
${BOLD}tmux-companion playground${OFF}

You are in the ${BOLD}playground${OFF} session, in ~/projects/orchard-api.
Break whatever you like. The container is thrown away when you leave it.

The guided tour is waiting in the ${BOLD}instructions${OFF} session:

    ${KEY}prefix then i${OFF}       go to the tour        ${DIM}(prefix is Ctrl-b)${OFF}
    ${KEY}prefix then s${OFF}       tmux's own session list
    ${KEY}Alt-s${OFF}               tmux-companion's project picker

${DIM}Leave the container with: exit, in every session. Or Ctrl-d twice.${OFF}
EOF
  exec zsh
fi

# ── 1 ───────────────────────────────────────────────────────────────────────
begin "⌨️ " "Two keyboard things, or nothing below works"
cat <<EOF
${BOLD}If you are running this inside tmux${OFF}

Ctrl-b goes to the tmux you were already in, not to this one, so every step
below will look broken. Either run the container in a terminal outside tmux,
which is what the rest of this assumes, or press the prefix twice:

    ${KEY}Ctrl-b Ctrl-b ?${OFF}   instead of   ${KEY}Ctrl-b ?${OFF}

${BOLD}If you are on macOS${OFF}

Alt-s and Alt-a are two of the bindings here, and Terminal.app and iTerm2
send an accented character for Option rather than Meta until you tell them
otherwise:

    Terminal.app   Settings, Profiles, Keyboard, "Use Option as Meta key"
    iTerm2         Settings, Profiles, Keys, Left Option key: Esc+
    Ghostty        macos-option-as-alt = true

You can skip that for now. The playground also binds both to the prefix:
${KEY}prefix P${OFF} for the project picker and ${KEY}prefix A${OFF} for the toggle.
EOF
hint "step 1: outside tmux, and Option sending Meta on macOS"
wait_for

# ── 2 ───────────────────────────────────────────────────────────────────────
begin "🔤" "Fonts, before anything else"
cat <<EOF
The status bar is drawn with Nerd Font glyphs. Fonts are rendered by the
terminal on your machine, not by anything in this container, so this is the
one thing the image cannot do for you.

Here are four of them:

$GLYPHS

If those are boxes or question marks, install a Nerd Font and set it as your
terminal's font:

    https://www.nerdfonts.com/font-downloads
    https://github.com/lonkar-org/firacode-nfc-tweaked

The second is the one this was built against. Everything else in the tour
works without the font; it just reads worse.
EOF
hint "step 2: can you see the glyphs above, or boxes?"
wait_for

# ── 3 ───────────────────────────────────────────────────────────────────────
begin "📊" "What the bar is telling you"
cat <<EOF
Look at the bottom line, left to right:

    ${BOLD}session name${OFF}   coloured by the theme this session has
    ${BOLD}clock${OFF}
    ${BOLD}windows${OFF}        drawn by tmux-companion, with an icon per index
    ${BOLD}git${OFF}            branch, and what is dirty, for the pane's directory
    ${BOLD}network${OFF}        bytes per second, in and out
    ${BOLD}battery${OFF}

Two of those will stay empty in here, and it is worth knowing why rather
than wondering: a container has no battery, and the network counters sit
below the threshold the segment draws at unless something is transferring.
On a laptop both are there.

The whole right side is ${BOLD}one${OFF} subprocess per redraw. That is the point of
the tool: tmux spawns a shell for every ${DIM}#()${OFF} on the bar, which is 14.6 ms of
CPU each, so five segments in five calls cost more than computing all five.
EOF
hint "step 3: read the status bar, bottom of the screen"
wait_for

# ── 4 ───────────────────────────────────────────────────────────────────────
begin "❓" "Every binding, searchable"
cat <<EOF
tmux already knows every key you have bound and the note attached to it.
This reads them back and lets you type at them.

Try searching for ${BOLD}pane${OFF}, or ${BOLD}copy${OFF}. Escape closes it.
EOF
keys "prefix  ?"
hint "step 4: press prefix then ? to search every key binding"
wait_for

# ── 5 ───────────────────────────────────────────────────────────────────────
begin "🃏" "The cheat sheet"
cat <<EOF
The same bindings, grouped into four boxes and sorted by how often you have
actually pressed them. The order changes as you use it.
EOF
keys "prefix  Ctrl-c"
hint "step 5: prefix then Ctrl-c for the cheat sheet"
wait_for

# ── 6 ───────────────────────────────────────────────────────────────────────
begin "🚀" "One session per project"
cat <<EOF
The project picker lists live sessions first, then every directory zoxide
knows, in one list. Picking a live one switches to it; picking a directory
creates the session, with the windows your layout asks for.

There are five projects in ~/projects. ${BOLD}Open sparrow-cli.${OFF}

It opens with two windows because ~/.config/tmux-companion/config.toml says
so: an editor, and a window split into a log graph and a status.
EOF
keys "Alt-s        then type: sparrow        (or prefix P)"
hint "step 6: Alt-s, then open the sparrow-cli project"
wait_for 'tmux has-session -t sparrow-cli' "there is no sparrow-cli session yet"

# ── 7 ───────────────────────────────────────────────────────────────────────
begin "🔁" "Switching between them"
cat <<EOF
Press it again. sparrow-cli and playground are both live now, so they are at
the top of the list, with their directories underneath.

This is the whole navigation model: one key, one list, no window manager.
EOF
keys "Alt-s        then pick playground     (or prefix P)"
hint "step 7: Alt-s again, switch back to playground"
wait_for

# ── 8 ───────────────────────────────────────────────────────────────────────
begin "🪟" "A new window, here or anywhere"
before=$(windows_now)
cat <<EOF
tmux's own prefix-c opens a window in the current pane's directory and gives
you no say in it. This starts with that directory already typed, so Enter is
the same thing, and anything else you type is a directory to open instead:
the frecency list, or a full path typed out.

Open one in ${BOLD}~/projects/lantern-docs${OFF} and watch the git segment change: that
repo has a detached HEAD, so the bar names the commit rather than a branch.
EOF
keys "prefix  c      then type: lantern"
hint "step 8: prefix then c, open a window in lantern-docs"
wait_for "[ \"\$(windows_now)\" -gt $before ]" "no new window yet"

# ── 9 ───────────────────────────────────────────────────────────────────────
begin "⚡" "Run something beside what you are doing"
cat <<EOF
A command from your shell history, in a pane that slides in next to the one
you are in, and asks before it closes so you can read what it said.

The history in here has ten commands in it. Try ${BOLD}git log${OFF}.
EOF
keys "prefix  e"
hint "step 9: prefix then e, run something from history"
wait_for

# ── 10 ──────────────────────────────────────────────────────────────────────
begin "🎨" "A colour per project"
cat <<EOF
Six themes ship, each with a lighter and a darker sibling that
${DIM}tmux-companion theme gen${OFF} computed. The text colour on each one is chosen to
clear WCAG AA against its background, which is 4.5:1, so a theme cannot be
picked that you then cannot read.

${BOLD}Pick a different theme in each session.${OFF} Switch between them and watch the
session block and the pane border change with the session. A theme is a
property of the session, not of the server, which is what lets one project
be blue while another is green.
EOF
keys "prefix  Ctrl-t      in playground, then again in sparrow-cli"
hint "step 10: prefix then Ctrl-t, pick a theme in each session"
wait_for

# ── 11 ──────────────────────────────────────────────────────────────────────
begin "🧰" "Toggle the tools away"
cat <<EOF
One key to go to the tool window this session keeps, and the same key to go
back where you were. In here the layout's tool window is ${BOLD}git${OFF}; on a real
machine mine holds an agent.

Try it in sparrow-cli, which has both windows.
EOF
keys "Alt-a                                 (or prefix A)"
hint "step 11: Alt-a to toggle the tool window, Alt-a to come back"
wait_for

# ── 12 ──────────────────────────────────────────────────────────────────────
begin "📐" "The layout a project comes back with"
cat <<EOF
Arrange sparrow-cli however you like: split a pane, open a window, move
things around. Then save it, and it is what that project opens with from now
on, config or no config.

    ${KEY}prefix  S${OFF}        save this session's windows and panes for this project
    ${KEY}prefix  X${OFF}        close the project, capturing the layout on the way out
    ${KEY}Alt-s${OFF}            open it again

Closing captures before anything is asked to quit, so a clean exit and a
save are the same keystroke.
EOF
hint "step 12: rearrange sparrow-cli, prefix S to save, prefix X to close, Alt-s to reopen"
wait_for 'ls "$XDG_STATE_HOME"/tmux-companion/projects/*.toml' "nothing saved yet: prefix S in the sparrow-cli session"

# ── 13 ──────────────────────────────────────────────────────────────────────
begin "🔍" "Copy mode, and jumping by prompt"
cat <<EOF
tmux has had next-prompt and previous-prompt since 3.3 and they do nothing
until the shell says where a prompt begins. One line in .zshrc does that, and
this image already has it:

    ${DIM}eval "\$(tmux-companion shell-init zsh)"${OFF}

Run a few commands first so there is something to jump between, then:

    ${KEY}prefix  [${OFF}        into copy mode
    ${KEY}Ctrl-p / Ctrl-n${OFF}  to the previous and next prompt
    ${KEY}v${OFF} then ${KEY}y${OFF}       select, and yank to the system clipboard
    ${KEY}q${OFF}              out
EOF
hint "step 13: prefix [ then Ctrl-p and Ctrl-n to jump between prompts"
wait_for

# ── 14 ──────────────────────────────────────────────────────────────────────
begin "🔗" "Open what is under the cursor"
cat <<EOF
In the playground session:

    ${DIM}cat ~/projects/orchard-api/build.log${OFF}

That log has a compiler error in it with a path, a line and a column, and a
URL underneath. Go into copy mode, select either one, and press ${KEY}o${OFF}.

A file opens in the editor at that line. A URL opens in a browser, which
this container does not have, so it will tell you so rather than pretending.
EOF
keys "prefix  [      select the path      o"
hint "step 14: cat the build.log, select src/main.rs:2:22 in copy mode, press o"
wait_for

# ── 15 ──────────────────────────────────────────────────────────────────────
begin "🌳" "Five repositories, five different bars"
cat <<EOF
Each project is in a different state on purpose. Walk through them in the
playground session and watch only the git segment:

    ${DIM}cd ~/projects/orchard-api${OFF}     clean: a branch and nothing else
    ${DIM}cd ~/projects/orchard-web${OFF}     three modified, one untracked
    ${DIM}cd ~/projects/sparrow-cli${OFF}     staged and modified at once, and a branch
                                  name long enough to be truncated
    ${DIM}cd ~/projects/lantern-docs${OFF}    detached HEAD, untracked file
    ${DIM}cd ~/projects/anvil-infra${OFF}     two commits ahead of its upstream

A git status costs about 51 ms cold. It is cached for five seconds, per
directory, which is why walking through these is instant the second time.
EOF
hint "step 15: cd through the five projects, watch the git segment"
wait_for

# ── 16 ──────────────────────────────────────────────────────────────────────
begin "🏁" "That is the tour"
cat <<EOF
${OK}Done.${OFF}$( [ "$skipped" -gt 0 ] && printf ' %s(%d skipped)%s' "$DIM" "$skipped" "$OFF" )

What is in this image, if you want to copy it out:

    ${DIM}~/.config/tmux/companion.conf${OFF}        docs/tmux.conf.full.example, unchanged
    ${DIM}~/.config/tmux/playground.conf${OFF}       the tour's scaffolding, not part of the tool
    ${DIM}~/.config/tmux-companion/config.toml${OFF} the layout the projects open with
    ${DIM}/opt/playground/config.example.toml${OFF}  every option, annotated

Read next:

    ${DIM}tmux-companion --help${OFF}
    ${DIM}tmux-companion doctor${OFF}                what it can see on this machine
    https://github.com/lonkar-org/tmux-companion    install, configure, themes

Nothing you did in here touched your machine. Run ${BOLD}tour${OFF} to go round again.
EOF
hint "the tour is done: run \`tour\` to go round again"
printf '\n%s[Enter] for a shell%s ' "$DIM" "$OFF"
IFS= read -r _ || true
exec zsh
