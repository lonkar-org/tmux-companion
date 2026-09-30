#!/bin/bash
# Render every non-status-bar surface in a throwaway, seeded tmux server and
# save each frame with escapes (.ansi) and without (.txt).
# Usage: scripts/capture-surfaces.sh OUT_DIR [BIN [SURFACE ...]]
#   SURFACE is a subcommand line, e.g. "panes --agents"; none means all.
# Seeding borrows demo/record-demo.sh: shipped full tmux.conf (for -N notes),
# theme init, zsh history, a compiled fake `claude` that reports `agent asked`.
# Sockets sit directly in /tmp: a unix socket path holds 103 bytes.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:?out dir}"; BIN="${2:-$REPO/target/release/tmux-companion}"
SOCK="tcs$$"; SB="$(mktemp -d /tmp/tcs.XXXX)"
mkdir -p "$OUT" "$SB"/{config/tmux-companion,state/tmux-companion,zoxide,bin,libexec}
export HOME="$SB" XDG_CONFIG_HOME="$SB/config" XDG_STATE_HOME="$SB/state" _ZO_DATA_DIR="$SB/zoxide"
export TMUX_COMPANION_SOCK="/tmp/tcs$$.sock" TERM=xterm-256color PATH="$SB/bin:$PATH"
ln -sf "$BIN" "$SB/bin/tmux-companion"
cleanup() { tmux -L "$SOCK" kill-server 2>/dev/null || true; "$BIN" shutdown >/dev/null 2>&1 || true; rm -rf "$SB" "$TMUX_COMPANION_SOCK"; }
trap cleanup EXIT
g() { git -C "$1" -c user.email=a@b -c user.name=a -c commit.gpgsign=false "${@:2}"; }
for r in acme-api docs-site billing-worker; do
  d="$SB/code/$r"; mkdir -p "$d/src"; git init -q "$d"; echo 'fn main(){}' > "$d/src/main.rs"
  g "$d" add -A; g "$d" commit -qm first; g "$d" checkout -qb feat/rate-limit; echo '// wip' >> "$d/src/main.rs"; echo s > "$d/notes.txt"
  command -v zoxide >/dev/null && zoxide add "$d" || true
done
TH="$SB/config/tmux/themes"; "$BIN" theme init --themes "$TH" >/dev/null 2>&1 || true
printf 'acme-api\tember\ndocs-site\tpine\nbilling-worker\tslate\n' > "$TH/_project-map.tsv"
HF="$SB/zsh_history"
for c in 'cargo test --all-targets' 'cargo clippy --all-targets -- -D warnings' 'git log --oneline -20' \
  'docker compose up -d postgres' 'kubectl get pods -n staging' 'rg --hidden "TODO" src/' 'cargo build --release'; do
  printf ': 1758500000:0;%s\n' "$c"; done > "$HF"
NOW=$(date +%s); for k in z z z e e g g; do printf 'prefix\t%s\t@%s\n' "$k" "$NOW"; done > "$SB/state/tmux-companion/keys-usage.tsv"
cat > "$SB/config/tmux-companion/config.toml" <<TOML
[usage]
enabled = true
[journal]
enabled = true
[project]
dirs_source = "zoxide"
[run]
history_file = "$HF"
TOML
cat > "$SB/libexec/claude.sh" <<'AGENT'
#!/usr/bin/env bash
tmux-companion agent busy >/dev/null 2>&1 || true
printf '\n  * claude\n\n  > add rate limiting to the login route\n\n  Limit every route, or only /login?\n\n'
tmux-companion agent asked >/dev/null 2>&1 || true
read -r _
AGENT
printf '#include <unistd.h>\n#include <sys/wait.h>\nint main(void){pid_t p=fork();if(p==0){execl("/bin/bash","bash","%s",(char*)0);_exit(127);}int s;waitpid(p,&s,0);return 0;}\n' "$SB/libexec/claude.sh" > "$SB/libexec/claude.c"
cc -o "$SB/bin/claude" "$SB/libexec/claude.c"
CONF="$SB/tmux.conf"; { cat "$REPO/docs/tmux.conf.full.example"; echo 'set -g default-command "/bin/zsh -f"'; } > "$CONF"
T() { tmux -L "$SOCK" "$@"; }
T -f "$CONF" new-session -d -s acme-api -x 140 -y 40 -c "$SB/code/acme-api"
T new-session -d -s docs-site -c "$SB/code/docs-site"; T new-session -d -s billing-worker -c "$SB/code/billing-worker"
T new-window -d -t billing-worker: -n agent -c "$SB/code/billing-worker" "$SB/bin/claude"
T send-keys -t acme-api 'python3 -m http.server 8765 >/dev/null 2>&1 &' Enter 'git status -s' Enter
T send-keys -t docs-site 'for i in $(seq 1 30); do echo "build line $i ok"; done; sleep 12' Enter
export TMUX="/tmp/tmux-$(id -u)/$SOCK,0,0"
sleep 3; tmux-companion sessions save >/dev/null 2>&1 || true
tmux-companion theme apply --all >/dev/null 2>&1 || true
shot() { # name, args...
  local name="$1"; shift
  # A merge only has something to ask about once a session is missing, so
  # the agent's session goes just before the resurrect screen is drawn.
  case "$name" in sessions_resurrect*) T kill-session -t billing-worker 2>/dev/null || true; sleep 1 ;; esac
  T new-window -d -t acme-api: -n "s_$name" -c "$SB/code/acme-api" "tmux-companion $* ; echo '[exit '\$?']'; sleep 600"
  sleep "${WAIT:-2}"
  # -N keeps trailing cells: without it a band or a background that runs
  # past the text is cut where the text ends, which is not what is on screen.
  T capture-pane -e -N -p -t "acme-api:s_$name" > "$OUT/$name.ansi"
  T capture-pane -p -t "acme-api:s_$name" > "$OUT/$name.txt"
  T kill-window -t "acme-api:s_$name"
}
if [ $# -gt 2 ]; then shift 2; for s in "$@"; do shot "${s// /_}" $s; done; exit 0; fi
for s in panes 'panes --agents' project new-window run ports search keys 'keys --all' cheatsheet brief inbox journal \
  'theme pick' 'sessions idle' 'sessions list' 'sessions resurrect --merge' setup doctor health start 'config show' 'quiet'; do
  shot "${s// /_}" $s
done
echo "saved to $OUT"
