#!/bin/bash
#
# Checks the playground image is actually a playground: the projects are in the
# states the tour claims, the bar draws something for each of them, the theme
# hook painted both sessions, and a layout still builds its windows.
#
# Usage:
#   scripts/playground.sh smoke        from the host, in a throwaway container
#   /opt/playground/smoke.sh           inside a running playground
#
# It asserts behaviour rather than output: the pickers are terminal programs in
# a popup and cannot be driven from here, so what is checked is the state they
# leave behind. `project <dir>` is the non-interactive half of the picker and
# stands in for it.
set -uo pipefail

pass=0; fail=0
ok()   { pass=$(( pass + 1 )); printf '  ok    %s\n' "$1"; }
bad()  { fail=$(( fail + 1 )); printf '  FAIL  %s\n    %s\n' "$1" "${2:-}"; }
check() {                        # check <name> <command...>
  local name="$1"; shift
  local out; out=$("$@" 2>&1)
  if [ -n "$out" ]; then ok "$name"; else bad "$name" "empty output from: $*"; fi
}

printf '\n== projects\n'
for p in orchard-api orchard-web sparrow-cli lantern-docs anvil-infra; do
  if [ -d "$HOME/projects/$p/.git" ]; then ok "$p is a repository"
  else bad "$p is a repository" "no .git under $HOME/projects/$p"; fi
done

printf '\n== the git segment says something different for each\n'
declare -A seen=()
for p in orchard-api orchard-web sparrow-cli lantern-docs anvil-infra; do
  out=$(tmux-companion gst "$HOME/projects/$p" 2>&1)
  if [ -z "$out" ]; then bad "$p renders" "gst printed nothing"; continue; fi
  key=$(printf '%s' "$out" | tr -d '[:space:]')
  if [ -n "${seen[$key]:-}" ]; then
    bad "$p differs from ${seen[$key]}" "both render the same bar"
  else
    seen[$key]="$p"; ok "$p renders, and differently"
  fi
done

printf '\n== the rest of the bar\n'
check "status-right"  tmux-companion status-right --branch-max-len 40 "$HOME/projects/orchard-web"
check "doctor"        tmux-companion doctor

printf '\n== the picker has rows to show\n'
n=$(zoxide query -l 2>/dev/null | wc -l | tr -d ' ')
[ "$n" -ge 5 ] && ok "zoxide knows $n directories" || bad "zoxide knows five directories" "it knows $n"
n=$(grep -c ';' "$HOME/.zsh_history" 2>/dev/null || echo 0)
[ "$n" -ge 5 ] && ok "$n commands in history" || bad "history has commands" "found $n"

printf '\n== themes\n'
for f in _apply.tmux _reset.tmux ember.tmux pine-dark.tmux; do
  [ -f "$HOME/.config/tmux/themes/$f" ] && ok "themes/$f" || bad "themes/$f" "not written"
done
if grep -q 'set -g ' "$HOME/.config/tmux/themes/_apply.tmux"; then
  bad "themes are session-scoped" "_apply.tmux uses set -g, so one session paints them all"
else
  ok "themes are session-scoped"
fi

if [ -n "${TMUX:-}" ] || tmux has-session -t playground 2>/dev/null; then
  printf '\n== the running playground\n'
  for s in playground instructions; do
    v=$(tmux show -t "$s" -v @theme-session-name-bg 2>&1)
    case "$v" in
      colour*) ok "$s was painted by the session-created hook" ;;
      *)       bad "$s was painted by the session-created hook" "@theme-session-name-bg is $v" ;;
    esac
  done

  # The non-interactive half of the project picker, which is what Alt-s runs
  # once something has been chosen.
  tmux-companion project "$HOME/projects/lantern-docs" >/dev/null 2>&1
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    windows=$(tmux list-windows -t lantern-docs -F '#{window_name}' 2>/dev/null)
    [ "$(printf '%s' "$windows" | grep -c .)" -ge 2 ] && break
    sleep 0.5
  done
  if [ "$(printf '%s\n' "$windows" | grep -c .)" -ge 2 ]; then
    ok "a project opens with the layout's windows ($(printf '%s' "$windows" | tr '\n' ' '))"
  else
    bad "a project opens with the layout's windows" "got: ${windows:-nothing}"
  fi
  tmux kill-session -t lantern-docs 2>/dev/null
fi

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
