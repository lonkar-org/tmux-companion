#!/usr/bin/env bash
#
# Does `shell-init` actually give tmux prompts it can jump between?
#
# usage:
#   scripts/check-prompt-marks.sh                 every shell it knows
#   scripts/check-prompt-marks.sh zsh bash        only these
#   BIN=target/debug/tmux-companion scripts/check-prompt-marks.sh
#
# For each shell: start a private tmux server, run that shell in a pane, install
# the hook the way its own comment tells a person to, run three commands, then
# press `previous-prompt` twice and report where the cursor landed.
#
# It exists because the hook installing cleanly says nothing about whether it
# works. `shell-init zsh` defined its functions, `add-zsh-hook` registered them,
# every mark was printed -- and the cursor never moved, because zsh's PROMPT_SP
# and PROMPT_CR redraw the prompt line after precmd has run and wipe the mark
# tmux recorded on it. Nothing short of driving tmux catches that, and it had
# been shipped and demonstrated for months.
#
# A jump is counted as working when the cursor moves to a different row than the
# one before it. Landing on a prompt is not enough: a cursor that has not moved
# is also sitting on a prompt.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BIN=${BIN:-$HERE/target/release/tmux-companion}
[ -x "$BIN" ] || { echo "no binary at $BIN; cargo build --release" >&2; exit 2; }

SHELLS=("$@")
[ ${#SHELLS[@]} -gt 0 ] || SHELLS=(zsh bash fish)

# The line that installs the hook, as each shell's own comment spells it.
install_line() {
  case $1 in
    fish) printf '%s shell-init fish | source\n' "$BIN" ;;
    *)    printf 'eval "$(%s shell-init %s)"\n' "$BIN" "$1" ;;
  esac
}

fails=0
for shell in "${SHELLS[@]}"; do
  if ! command -v "$shell" >/dev/null 2>&1; then
    printf '  %-5s skipped, not installed\n' "$shell"
    continue
  fi

  sock="tcpm$$$RANDOM"
  # -f /dev/null so the user's own tmux.conf cannot decide the answer.
  tmux -L "$sock" -f /dev/null new-session -d -s t -x 80 -y 20 -c /tmp "$shell" 2>/dev/null
  sleep 1

  tmux -L "$sock" send-keys -t t "$(install_line "$shell")" Enter
  sleep 1
  # Clear first: the install line and its output are scrollback nobody is
  # testing, and a short pane makes a failure obvious when this is read by eye.
  tmux -L "$sock" send-keys -t t "clear" Enter
  sleep 0.6
  for c in "echo one" "echo two" "echo three"; do
    tmux -L "$sock" send-keys -t t "$c" Enter
    sleep 0.5
  done
  sleep 0.5

  tmux -L "$sock" copy-mode -t t
  rows=()
  for _ in 1 2; do
    tmux -L "$sock" send-keys -X -t t previous-prompt
    sleep 0.3
    rows+=("$(tmux -L "$sock" display-message -p -t t '#{copy_cursor_y}')")
  done
  start=$(tmux -L "$sock" display-message -p -t t '#{copy_cursor_y}')
  line=$(tmux -L "$sock" display-message -p -t t '#{copy_cursor_line}')
  tmux -L "$sock" kill-server 2>/dev/null || true

  if [ "${rows[0]}" != "${rows[1]}" ]; then
    printf '  %-5s ok      jumps moved the cursor: row %s then %s, on %s\n' \
      "$shell" "${rows[0]}" "${rows[1]}" "[$line]"
  else
    printf '  %-5s BROKEN  both jumps landed on row %s, so nothing moved\n' \
      "$shell" "${rows[0]}"
    fails=$(( fails + 1 ))
  fi
  : "$start"
done

[ "$fails" -eq 0 ] || { echo "prompt marks are broken in $fails shell(s)" >&2; exit 1; }
echo "prompt marks work in every shell checked"
