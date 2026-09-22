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

skipped=0

rule()  { printf '%s%s%s\n' "$RULE" "────────────────────────────────────────────────────────────────────────" "$OFF"; }
keys()  { printf '\n    %s%s%s\n' "$KEY" "$*" "$OFF"; }
hint()  { tmux set -g @playground-step "$*" 2>/dev/null || true; }

begin() {              # begin <icon> <title>
  clear
  printf '%s%s  %s%s %s(step %d of %d)%s\n' "$BOLD" "$1" "$2" "$OFF" "$DIM" "$n" "$TOTAL" "$OFF"
  rule
  printf '\n'
}

# wait_for [verify-command] [what-is-missing]
#
# Enter moves on. With a verify command, Enter only moves on once the command
# succeeds, so a step that was not actually done says so instead of scrolling
# past. `b` goes back one, `s` skips this one and `q` leaves the tour.
#
# The answer comes back as an exit status rather than as output, because the
# caller is a loop over step numbers and going back means handing it one.
wait_for() {
  local verify="${1:-}" missing="${2:-}"
  while true; do
    # The step number rides on the prompt as well as on the heading: a long
    # step scrolls its own heading off a 24-line pane, and then nothing on
    # screen says which step you are on.
    printf '\n%sstep %d/%d   [Enter] done   [b] back   [s] skip   [q] quit%s ' \
      "$DIM" "$n" "$TOTAL" "$OFF"
    IFS= read -r answer || { answer=q; printf '\n'; }
    case "$answer" in
      q|Q) return 3 ;;
      b|B) return 2 ;;
      s|S) skipped=$(( skipped + 1 )); return 0 ;;
      "")  if [ -z "$verify" ] || eval "$verify" >/dev/null 2>&1; then
             return 0
           fi
           printf '\n%sNot yet: %s%s\n' "$WARN" "$missing" "$OFF" ;;
      *)   printf '\n%sEnter, b, s or q.%s\n' "$DIM" "$OFF" ;;
    esac
  done
}

# Every step that sends somebody to another session ends with this, so nobody
# is left over there wondering how to get back.
back_here() {
  printf '\n%sBack here when you are done: %sprefix then i%s\n' "$DIM" "$KEY" "$OFF"
}

windows_now() { tmux list-windows -a 2>/dev/null | wc -l | tr -d ' '; }
panes_now()   { tmux list-panes -a 2>/dev/null | wc -l | tr -d ' '; }

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

# ── 1 ──────────────────────────────────────────────────────────────────────
step_01() {
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
}

# ── 2 ──────────────────────────────────────────────────────────────────────
step_02() {
begin "🔤" "Fonts, before anything else"
cat <<EOF
The status bar is drawn with Nerd Font glyphs. Fonts are rendered by the
terminal on your machine, not by anything in this container, so this is the
one thing the image cannot do for you.

Here are four of them:

$GLYPHS

Those four are a sample, not what is on your bar right now. This session's
bar has the session name, the clock and the window list on it; the git,
battery and network segments only draw when they have something to say, and
the next step is about when that is.

If those are boxes or question marks, install a Nerd Font and set it as your
terminal's font:

    https://www.nerdfonts.com/font-downloads
    https://github.com/lonkar-org/firacode-nfc-tweaked

The second is the one this was built against. Everything else in the tour
works without the font; it just reads worse.
EOF
hint "step 2: can you see the glyphs above, or boxes?"
wait_for
}

# ── 3 ──────────────────────────────────────────────────────────────────────
step_03() {
begin "📊" "What the bar is telling you"
cat <<EOF
Look at the bottom line of ${BOLD}this${OFF} session, left to right:

    ${BOLD}session name${OFF}   coloured by the theme this session has
    ${BOLD}clock${OFF}
    ${BOLD}windows${OFF}        the window list

That is all there is here, and the reason is the point of the whole right
side: every segment draws only when it has something to say.

    ${BOLD}git${OFF}       needs a repository. This session's directory is not one.
              Switch to the ${BOLD}playground${OFF} session, which sits in
              ~/projects/orchard-api, and the branch appears.
    ${BOLD}network${OFF}   needs traffic above 20 KiB/s. An idle container has none,
              so this one stays empty until something transfers.
    ${BOLD}battery${OFF}   needs a battery. A container has no access to one, so
              this segment will not appear in here at all. On a laptop it
              does.

None of those three is broken in here. They are three segments with nothing
to report, which is what an empty segment means everywhere else too.

The whole right side is ${BOLD}one${OFF} subprocess per redraw. That is the point of
the tool: tmux spawns a shell for every ${DIM}#()${OFF} on the bar, which is 14.6 ms of
CPU each, so five segments in five calls cost more than computing all five.
EOF
hint "step 3: the bar here has no git, network or battery, and step 3 says why"
wait_for
}

# ── 4 ──────────────────────────────────────────────────────────────────────
step_04() {
begin "❓" "Every binding, searchable"
cat <<EOF
tmux already knows every key you have bound and the note attached to it.
This reads them back and lets you type at them.

Try searching for ${BOLD}pane${OFF}, or ${BOLD}copy${OFF}. Escape closes it.
EOF
keys "prefix  ?"
hint "step 4: press prefix then ? to search every key binding"
wait_for
}

# ── 5 ──────────────────────────────────────────────────────────────────────
step_05() {
begin "🃏" "The cheat sheet"
cat <<EOF
The same bindings, grouped into four boxes and sorted by how often you have
actually pressed them. The order changes as you use it.

The bindings written in tmux.conf come first in each box. Where a box would
otherwise be nearly empty -- this config binds one pane key of its own --
tmux's own bindings fill it out, which is why Panes shows the two splits you
did not write.
EOF
keys "prefix  Ctrl-c"
hint "step 5: prefix then Ctrl-c for the cheat sheet"
wait_for
}

# ── 6 ──────────────────────────────────────────────────────────────────────
step_06() {
begin "🚀" "One session per project"
cat <<EOF
The project picker lists live sessions first, then every directory zoxide
knows, in one list. Picking a live one switches to it; picking a directory
creates the session, with the windows your layout asks for.

There are five projects in ~/projects. ${BOLD}Open sparrow-cli.${OFF}

It opens with two windows because ~/.config/tmux-companion/config.toml says
so: an editor, and a window split into a log graph and a status.

It does not ask you for a colour. A new session takes the theme in
${DIM}[theme] default${OFF}, and you pick a different one per session in step 10; the
picker remembers it against the project, so it comes back the same way next
time.
EOF
keys "Alt-s        then type: sparrow        (or prefix P)"
back_here
hint "step 6: Alt-s, open sparrow-cli, then prefix i to come back"
wait_for 'tmux has-session -t sparrow-cli' "there is no sparrow-cli session yet"
}

# ── 7 ──────────────────────────────────────────────────────────────────────
step_07() {
begin "🔁" "Switching between them"
cat <<EOF
Press it again. sparrow-cli and playground are both live now, so they are at
the top of the list, with their directories underneath.

This is the whole navigation model: one key, one list, no window manager.
EOF
keys "Alt-s        then pick playground     (or prefix P)"
back_here
hint "step 7: Alt-s again, switch to playground, then prefix i to come back"
wait_for
}

# ── 8 ──────────────────────────────────────────────────────────────────────
step_08() {
begin "🪟" "A new window, here or anywhere"
before=$(windows_now)
cat <<EOF
tmux's own prefix-c opens a window in the current pane's directory and gives
you no say in it. This starts with that directory already typed, so Enter is
the same thing, and anything else you type is a directory to open instead:
the frecency list, or a full path typed out.

It opens with the pane's own directory already typed, and on the list as
${BOLD}here${OFF}, so there are three things you can do with it:

    ${KEY}Enter${OFF}          a window in the same directory, which is what tmux's
                   own prefix-c does and what you want most of the time
    ${KEY}Ctrl-u${OFF}         clear it, and pick from the directories zoxide knows
    ${DIM}type a path${OFF}    absolute like ${DIM}/home/play/projects/anvil-infra${OFF}, or
                   relative like ${DIM}~/projects/anvil-infra${OFF}, whether or not
                   zoxide has ever seen it

Open one in ${BOLD}~/projects/lantern-docs${OFF} and watch the git segment change: that
repo has a detached HEAD, so the bar names the commit rather than a branch.

The new window is in whichever session you pressed it in, so ${KEY}prefix i${OFF}
brings you back here, or close the window with ${DIM}exit${OFF}.
EOF
keys "prefix  c      then type: lantern"
hint "step 8: prefix then c, open a window in lantern-docs"
wait_for "[ \"\$(windows_now)\" -gt $before ]" "no new window yet"
}

# ── 9 ──────────────────────────────────────────────────────────────────────
step_09() {
# Counted before the step rather than after, so the check is "the pane you
# opened is gone again" rather than a number baked in when this was written.
local before
before=$(panes_now)
begin "⚡" "Run something beside what you are doing"
cat <<EOF
A command from your shell history, in a pane that slides in next to the one
you are in, and asks before it closes so you can read what it said.

The history in here has ten commands in it. Try ${BOLD}git log${OFF}.

The command runs in that pane, and when it finishes the pane does not
disappear: it asks what to do with what you are looking at.

    ${KEY}c${OFF}   close the pane
    ${KEY}v${OFF}   leave it open to read, with nothing running in it
    ${KEY}R${OFF}   run the same command again

It opens on ${BOLD}close${OFF} when the command succeeded and on ${BOLD}restart${OFF} when it
failed, because those are the two things anybody does next.

Press ${KEY}c${OFF} to close it before coming back.
EOF
keys "prefix  e"
back_here
hint "step 9: prefix e, run something, then c to close the pane"
wait_for "[ \"\$(panes_now)\" -le $before ]" "the run pane is still open: press c in it"
}

# ── 10 ─────────────────────────────────────────────────────────────────────
step_10() {
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
back_here
hint "step 10: prefix Ctrl-t, pick a theme in two sessions, then prefix i"
wait_for
}

# ── 11 ─────────────────────────────────────────────────────────────────────
step_11() {
begin "🧰" "Toggle the tools away"
cat <<EOF
One key to go to the tool window this session keeps, and the same key to go
back where you were. In here the layout's tool window is ${BOLD}git${OFF}; on a real
machine mine holds an agent.

Try it in sparrow-cli, which has both windows.
EOF
keys "Alt-a                                 (or prefix A)"
back_here
hint "step 11: Alt-a there and back, then prefix i to come back here"
wait_for
}

# ── 12 ─────────────────────────────────────────────────────────────────────
step_12() {
begin "📐" "The layout a project comes back with"
cat <<EOF
Arrange sparrow-cli however you like: split a pane, open a window, move
things around. Then save it, and it is what that project opens with from now
on, config or no config.

    ${KEY}prefix  Shift-S${OFF}  save this session's windows and panes for this project
    ${KEY}prefix  Shift-X${OFF}  close the project, capturing the layout on the way out
    ${KEY}Alt-s${OFF}            open it again

Capital letters, so those are prefix and then ${BOLD}Shift-S${OFF} and ${BOLD}Shift-X${OFF}. Lower
case s and x are tmux's own session list and pane kill.

${WARN}One warning before you press Shift-X:${OFF} closing the last session on a tmux
server ends the server, and in this container that puts you back at a plain
shell with no tmux at all. Close ${BOLD}sparrow-cli${OFF}, not this one, and there will
still be two sessions left. If you do end up outside, ${DIM}exit${OFF} leaves the
container and you can start it again.
EOF
back_here
hint "step 12: in sparrow-cli: prefix Shift-S saves, prefix Shift-X closes, Alt-s reopens"
wait_for 'ls "$XDG_STATE_HOME"/tmux-companion/projects/*.toml' "nothing saved yet: prefix S in the sparrow-cli session"
}

# ── 13 ─────────────────────────────────────────────────────────────────────
step_13() {
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
}

# ── 14 ─────────────────────────────────────────────────────────────────────
step_14() {
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
}

# ── 15 ─────────────────────────────────────────────────────────────────────
step_15() {
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
}

# ── 16 ─────────────────────────────────────────────────────────────────────
step_16() {
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
}

# ── The driver ───────────────────────────────────────────────────────────
#
# A loop over the steps rather than a straight run, so `b` can hand back the
# one before this. Each step is a function with no state of its own, so going
# back into one redraws it from scratch.
STEPS=(step_01 step_02 step_03 step_04 step_05 step_06 step_07 step_08 step_09 step_10 step_11 step_12 step_13 step_14 step_15 step_16)
TOTAL=${#STEPS[@]}

i=0
while [ "$i" -lt "$TOTAL" ]; do
  n=$(( i + 1 ))
  "${STEPS[$i]}"
  case "$?" in
    2) [ "$i" -gt 0 ] && i=$(( i - 1 )) ;;
    3) printf '\n%sThe tour is over. Run `tour` to start it again.%s\n' "$DIM" "$OFF"
       exec zsh ;;
    *) i=$(( i + 1 )) ;;
  esac
done

exec zsh
