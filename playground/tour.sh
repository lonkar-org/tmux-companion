#!/bin/bash
#
# The guided tour. Runs in the `instructions` session; you do the steps in the
# `playground` session and come back here to press Enter.
#
# Usage:
#   /opt/playground/tour.sh            the tour
#   /opt/playground/tour.sh welcome    the banner the playground shell opens with
#   /opt/playground/tour.sh 12         resume at a step, after the tour was closed
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

# What the heading and the prompt call where you are. The main tour sets it
# per step; the basics track sets its own, so `1.3` is visibly a detour rather
# than a step of the sixteen.
LABEL=""

begin() {              # begin <icon> <title>
  clear
  printf '%s%s  %s%s %s(%s)%s\n' "$BOLD" "$1" "$2" "$OFF" "$DIM" "$LABEL" "$OFF"
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
  : "${EXTRA_KEYS:=}"
  while true; do
    # The step number rides on the prompt as well as on the heading: a long
    # step scrolls its own heading off a 24-line pane, and then nothing on
    # screen says which step you are on.
    printf '\n%s%s   [Enter] done   [b] back   [s] skip   [q] quit%s%s ' \
      "$DIM" "$LABEL" "$OFF" "$EXTRA_KEYS"
    # One key, no Enter. `read -rsn1` returns as soon as something is
    # pressed; Enter arrives as an empty string because the newline is the
    # delimiter it stopped on, which is the case below that moves on.
    IFS= read -rsn1 answer || { answer=q; printf '\n'; }
    printf '\n\n' 
    case "$answer" in
      q|Q) return 3 ;;
      b|B) return 2 ;;
      t|T) [ -n "$EXTRA_KEYS" ] && return 4 ;;
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

# Did a window ever get made since this step began?
#
# Counting windows now and comparing is wrong the moment somebody closes the
# window they just opened: the count goes back to where it started and the
# step says "no new window yet" forever, with no way past it. This latches on
# the first time it is true.
MADE_A_WINDOW=""
made_a_window() {
  [ -n "$MADE_A_WINDOW" ] && return 0
  if [ "$(windows_now)" -gt "${1:-0}" ]; then
    MADE_A_WINDOW=yes
    return 0
  fi
  return 1
}

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

# ── tmux basics, the optional detour ───────────────────────────────────────
#
# Offered at step 1 and skipped by default, because somebody who already uses
# tmux does not want to be taught it. Nine short screens: four that explain
# the model with a diagram, four to press the keys, one to close.
#
# No checks anywhere in here. A person finding out what a pane is should not
# also be failing an assertion.

basics_01() {
LABEL="tmux basics 1.1 of 1.9"
begin "🧩" "What tmux actually is"
cat <<EOF
Not a terminal. Not a shell. tmux is a ${BOLD}server${OFF} that owns your programs,
and a ${BOLD}client${OFF} that draws them.

    your terminal                the tmux server
   ┌─────────────────┐          ┌────────────────────────┐
   │  $ tmux attach  │ ───────▶ │  nvim                  │
   │                 │ ◀─────── │  a build, still going  │
   │  draws, types   │  keys    │  three shells          │
   └─────────────────┘  pixels  └────────────────────────┘
       ${DIM}a client${OFF}                 ${DIM}started once, outlives every client${OFF}

The programs belong to the server. Your terminal is a window onto them, and
windows can be closed and opened again without the programs noticing.

That one sentence is the whole reason tmux exists. Everything else in here is
a consequence of it.
EOF
wait_for
}

basics_02() {
LABEL="tmux basics 1.2 of 1.9"
begin "🗝" "The prefix, and why there is one"
cat <<EOF
tmux and the program you are running both want your keystrokes. tmux takes
one key for itself and passes on everything else.

    ${KEY}Ctrl-b${OFF}          "the next key is for tmux"
    ${KEY}Ctrl-b  c${OFF}       tmux reads the c
    ${DIM}c${OFF}               your shell reads the c

That key is called the ${BOLD}prefix${OFF}. Ctrl-b is the default; a lot of people move
it to Ctrl-a or Ctrl-Space, and this playground leaves it at Ctrl-b so that
what you read elsewhere matches what you press here.

If you ever need to send a real Ctrl-b to the program underneath, press it
twice. That is also how you reach tmux when it is running inside tmux.
EOF
wait_for
}

basics_03() {
LABEL="tmux basics 1.3 of 1.9"
begin "🗂" "Sessions, windows, panes"
cat <<EOF
Three levels, and they nest. Learning which is which is most of learning
tmux.

  ${BOLD}session${OFF}  "orchard-api"          one project. Detach and attach this.
    │
    ├─ ${BOLD}window${OFF} 1  "edit"          one screenful. Like a tab.
    │    ┌───────────┬──────────┐
    │    │  ${BOLD}pane${OFF}     │  ${BOLD}pane${OFF}    │    a pane is one shell, one
    │    │  nvim     │  shell   │    program, one rectangle
    │    └───────────┴──────────┘
    │
    └─ ${BOLD}window${OFF} 2  "git"
         ┌─────────────────────────┐
         │  git log --graph        │
         └─────────────────────────┘

A ${BOLD}pane${OFF} is a rectangle with one program in it.
A ${BOLD}window${OFF} is a full screen, split into panes.
A ${BOLD}session${OFF} is a set of windows, and the thing you attach to and detach from.

The status bar at the bottom lists the windows of the session you are in.
Look at it now: this session has one window called ${BOLD}tour${OFF}.
EOF
wait_for
}

basics_04() {
LABEL="tmux basics 1.4 of 1.9"
begin "🔌" "Detach, the part that matters"
cat <<EOF
Closing a terminal kills what was running in it. Detaching does not.

  ${BOLD}while you are attached${OFF}              ${BOLD}after prefix d${OFF}
  ┌─────────────────────────┐           ┌─────────────────────────┐
  │  client: your terminal  │           │  ${DIM}(no client at all)${OFF}     │
  └────────────┬────────────┘           └─────────────────────────┘
  ┌────────────┴────────────┐           ┌─────────────────────────┐
  │  server: nvim, a build  │ ${DIM}the same${OFF}  │  server: nvim, a build  │
  └─────────────────────────┘           └─────────────────────────┘

    ${KEY}prefix d${OFF}        detach: leave everything running
    ${DIM}tmux attach${OFF}     come back to it

This is what an ssh connection dropping looks like from the server's side:
the client went away. Nothing else happened. You ssh back in, run
${DIM}tmux attach${OFF}, and the build you started two hours ago is still scrolling.

You will press it in a moment. It is safe in here.
EOF
wait_for
}

basics_05() {
LABEL="tmux basics 1.5 of 1.9"
begin "✂️ " "Your turn: split a pane"
cat <<EOF
Nothing is checked from here on. Press things and see what happens.

    ${KEY}prefix  %${OFF}       split this pane left and right
    ${KEY}prefix  "${OFF}       split it top and bottom
    ${KEY}prefix  o${OFF}       go to the next pane
    ${KEY}prefix  ← → ↑ ↓${OFF}   go to the pane in that direction
    ${KEY}prefix  x${OFF}       close any pane, after asking
    ${KEY}prefix  z${OFF}       make this pane full screen, and again to put it back

The tour is running in one of these panes, so splitting will squash it. That
is fine: ${KEY}prefix z${OFF} on this pane makes it readable again, and ${KEY}prefix x${OFF}
closes the one you made.

${WARN}If you close the tour's own pane${OFF} the tour stops, because it was a program
running in that pane and you just ended it. Nothing else is lost. Type
${KEY}tour${OFF} in any shell to start it again from the beginning, and ${KEY}s${OFF} skips
forward quickly.

${DIM}Those chords are tmux's own, not this tool's. They work in any tmux.${OFF}
EOF
wait_for
}

basics_06() {
LABEL="tmux basics 1.6 of 1.9"
begin "🗃" "Your turn: windows"
cat <<EOF
A window is a whole screen, and the status bar at the bottom lists them.

    ${KEY}prefix  c${OFF}       make a new window
    ${KEY}prefix  n${OFF}       next window
    ${KEY}prefix  p${OFF}       previous window
    ${KEY}prefix  1${OFF}       window 1, and 2 for window 2, and so on
    ${KEY}prefix  ,${OFF}       rename this window
    ${KEY}prefix  &${OFF}       close this window, after asking

Make one, watch the status bar grow a second entry, then come back with
${KEY}prefix 1${OFF}.

${DIM}In this playground prefix c is bound to something better, which asks where${OFF}
${DIM}to open it. Step 8 of the main tour is about that. Everything else here is${OFF}
${DIM}stock tmux.${OFF}
EOF
wait_for
}

basics_07() {
LABEL="tmux basics 1.7 of 1.9"
begin "🔌" "Your turn: detach and come back"
cat <<EOF
The one to actually feel.

    ${KEY}prefix  d${OFF}       detach

You will land back at a plain shell with a line explaining what just
happened, and pressing Enter there attaches you again. Everything in here,
including this tour and where you are in it, will be exactly as you left it.

${DIM}Outside a container you would type${OFF} tmux attach ${DIM}to come back, or${OFF}
${DIM}tmux attach -t <name> ${DIM}when there is more than one session.${OFF}
EOF
wait_for
}

basics_08() {
LABEL="tmux basics 1.8 of 1.9"
begin "\U0001f4dc" "Scrolling back, copying, and opening"
cat <<EOF
The mouse wheel works because this playground turns the mouse on. The
keyboard way is worth knowing, because it is also how you copy.

    ${KEY}prefix  [${OFF}       into copy mode: the pane freezes and you read its history
    ${KEY}k${OFF} ${KEY}j${OFF} ${KEY}h${OFF} ${KEY}l${OFF}         move, because this config sets ${DIM}mode-keys vi${OFF}
    ${KEY}w${OFF} ${KEY}b${OFF}            forward and back a word
    ${KEY}/${OFF}              search backwards, then ${KEY}n${OFF} for the next hit
    ${KEY}v${OFF}              start selecting, ${KEY}y${OFF} yanks it, ${KEY}q${OFF} leaves

The program underneath keeps running while you read; you are looking at a
scrollback, not pausing anything.

One more key, and this one is this tool rather than tmux:

    ${KEY}o${OFF}              open whatever is selected

A ${DIM}path:line:column${OFF} opens your editor there, which step 14 does with a
compiler error. A URL opens a browser, and this container has none, so it
says so instead:

    ${DIM}no xdg-open on this machine, so this was not opened: https://...${OFF}

That message is the whole thing working except the last step.
EOF
wait_for
}

basics_09() {
LABEL="tmux basics 1.9 of 1.9"
begin "\U0001f393" "That is tmux"
cat <<EOF
Four ideas and about a dozen keys:

    a ${BOLD}server${OFF} owns your programs, a ${BOLD}client${OFF} draws them
    ${BOLD}prefix${OFF} then a key talks to tmux instead of to your shell
    ${BOLD}session \u203a window \u203a pane${OFF}, nesting in that order
    ${BOLD}detach${OFF} leaves it all running

Where to go next:

    ${KEY}man tmux${OFF}                   the real thing, and it is good
    ${KEY}prefix ?${OFF}                   every binding, unsorted, from tmux itself
    ${DIM}https://learntmux.dev${OFF}       42 tasks against a real tmux in a browser
    ${DIM}https://tmuxai.dev${OFF}          a written walkthrough

${KEY}man tmux${OFF} works in here, and so does ${KEY}man tmux-companion${OFF}. Try either in
any shell; ${KEY}q${OFF} leaves the pager.

The next step of the main tour is a searchable version of ${KEY}prefix ?${OFF}, which
is where this tool starts.
EOF
wait_for
}

# The detour's own loop, the same shape as the main one.
# run_basics <first-step-index>
#
# Returns 0 when it ran off the end forwards, 2 when somebody pressed `b` on
# the first screen and wants out of the detour altogether, and 3 on quit.
#
# The start index is what makes going backwards into the detour work: coming
# back from step 2 should land on 1.9, the screen just left, not on 1.1.
run_basics() {
  local steps=(basics_01 basics_02 basics_03 basics_04 basics_05 basics_06 \
               basics_07 basics_08 basics_09)
  local j=${1:-0}
  while [ "$j" -lt "${#steps[@]}" ]; do
    "${steps[$j]}"
    case "$?" in
      2) if [ "$j" -eq 0 ]; then
           # Back off the top of the detour, which is step 1.
           return 2
         fi
         j=$(( j - 1 )) ;;
      3) return 3 ;;
      *) j=$(( j + 1 )) ;;
    esac
  done
  return 0
}

# The last screen of the detour, which is where `b` from step 2 lands.
BASICS_LAST=8

quit_tour() {
  printf '\n%sThe tour is over. Run `tour` to start it again.%s\n' "$DIM" "$OFF"
  exec zsh
}

# enter_basics <first-step-index>
#
# Runs the detour and turns its answer into a position in the main tour. Sets
# `i` when the answer is "put me back at step 1", and leaves it alone when the
# detour simply finished.
# Returns 0 when the detour finished forwards and 2 when somebody backed out
# of the top of it. The caller decides where that leaves them, because doing
# it in here and then falling into the caller's own `i=$(( i + 1 ))` is how
# backing out of 1.1 landed on step 2.
enter_basics() {
  run_basics "$1"
  local answer=$?
  case "$answer" in
    3) quit_tour ;;
    # Step 1 gets to decide again: somebody who backs out and then presses
    # Enter has come to step 2 from step 1, and `b` there should take them
    # back to step 1 rather than into a detour they just left.
    2) took_basics=0
       return 2 ;;
  esac
  return 0
}

# ── 1 ──────────────────────────────────────────────────────────────────
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

${BOLD}New to tmux?${OFF}

The rest of this assumes you know what a pane is and what the prefix does.
There are nine short screens that explain it, four of them diagrams and four
of them keys to press, and then you come back here and carry on.

    ${KEY}t${OFF}        tmux basics first
    ${KEY}Enter${OFF}    skip it, I use tmux
EOF
hint "step 1: press t for tmux basics, or Enter to carry on"
EXTRA_KEYS="   ${KEY}[t] tmux basics${OFF}${DIM}"
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
MADE_A_WINDOW=''
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
wait_for "made_a_window $before" "no new window yet"
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
151 themes, from six colours: ${DIM}theme init${OFF} writes the six, and
${DIM}theme gen --shades${OFF} adds every colour in tmux's 6x6x6 cube whose text clears
WCAG AAA, 145 more, each named after the bundled colour it sits nearest to so
they group in the picker. AAA and not AA because the worst colour in the cube
is 4.60:1, so an AA filter keeps all 216 and filters nothing. If 151 is more
than you want, ${DIM}--shades a4${OFF} gives 105, ${DIM}a5${OFF} gives 75 and ${DIM}a6${OFF} gives the
original eighteen.
${DIM}theme add --bg colour99${OFF} still makes one
from any colour tmux takes -- ${DIM}theme list-colours${OFF} prints all 256 with a
swatch. The text colour on each one is chosen to
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

${WARN}Press these in the sparrow-cli session, not here.${OFF} ${BOLD}Shift-X${OFF} closes
whichever project you are in, and right now that is this one -- the tour. It
closes just as willingly as any other session, so pressing it on this screen
closes the thing you are reading. ${DIM}Alt-s${OFF} first, pick ${BOLD}sparrow-cli${OFF}, then press it.

If you do close the tour, nothing is lost: the playground brings it back at
this step and you carry on.
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

${WARN}This only works where a shell has been printing prompts${OFF}, so do it in the
${BOLD}playground${OFF} session and not here: this pane is running the tour, not zsh, and
there are no prompts in it to jump between.

Run two or three commands there first, then:

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

That log has a compiler error in it with a path, a line and a column:

    ${DIM}  --> src/main.rs:2:22${OFF}

In the playground session:

    ${KEY}prefix  [${OFF}       into copy mode
    ${KEY}k${OFF}              up to the line holding ${BOLD}src/main.rs:2:22${OFF}
    ${KEY}v${OFF}              start selecting, then ${KEY}l${OFF} or ${KEY}w${OFF} to the end of it
    ${KEY}o${OFF}              open it

nvim opens in a pane beside you, on line 2, column 22 -- the character the
compiler was pointing at. Close it with ${DIM}:q${OFF}.

The URL two lines below works the same way and cannot finish in here, for
the reason 1.8 gave: no browser to hand it to.
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

    ${KEY}man tmux${OFF}                     tmux itself, and it is worth the hour
    ${KEY}man tmux-companion${OFF}           every command, every config section
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

# Where to start. `tour.sh 12` resumes at step 12, which is how the playground
# puts the tour back after somebody closes it: step 12 asks for `prefix Shift-X`
# and the person reading the step is sitting in the tour session while they read
# it, so closing the tour is the likeliest mistake in the whole playground and
# used to end the container.
i=0
if [ -n "${1:-}" ] && [ "$1" -eq "$1" ] 2>/dev/null; then
  i=$(( $1 - 1 ))
  [ "$i" -lt 0 ] && i=0
  [ "$i" -ge ${#STEPS[@]} ] && i=$(( ${#STEPS[@]} - 1 ))
fi

# Whether the detour was taken, which decides what `b` on step 2 means: back
# to the screen before it, which is 1.9 for somebody who went through the
# basics and step 1 for somebody who skipped them.
took_basics=0

while [ "$i" -lt "$TOTAL" ]; do
  n=$(( i + 1 ))
  LABEL="step $n of $TOTAL"
  "${STEPS[$i]}"
  answer=$?
  # Offered by one step only, and cleared straight away so the key does
  # nothing on the other fifteen.
  EXTRA_KEYS=""
  case "$answer" in
    2) if [ "$i" -eq 1 ] && [ "$took_basics" -eq 1 ]; then
         # Back into the detour at its last screen, and back out of its first
         # one lands on step 1.
         enter_basics "$BASICS_LAST" || i=0
       elif [ "$i" -gt 0 ]; then
         i=$(( i - 1 ))
       fi ;;
    3) quit_tour ;;
    4) took_basics=1
       if enter_basics 0; then
         i=$(( i + 1 ))
       else
         i=0
       fi ;;
    *) i=$(( i + 1 )) ;;
  esac
done

exec zsh
