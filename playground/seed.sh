#!/bin/bash
#
# Build-time setup for the playground image. Runs once, as the `play` user,
# and leaves a home directory the tour can walk through:
#
#   ~/projects/*        five git repositories, each in a different state, so
#                       the git segment has something different to say in each
#   ~/.zsh_history      commands for the run picker to offer
#   ~/.config/tmux      the full example config plus the tour's second bar line
#   ~/.config/tmux-companion/config.toml
#                       a layout whose commands exist in this image
#   the zoxide database seeded with the five projects, because the project
#   picker lists what zoxide knows and a directory it has never seen cannot
#   be picked.
#
# Usage: /opt/playground/seed.sh    (called from the Dockerfile, not by hand)
set -euo pipefail

PROJECTS="$HOME/projects"
REMOTES="$HOME/.remotes"

git config --global user.name  "Playground"
git config --global user.email "play@example.invalid"
git config --global init.defaultBranch main
git config --global advice.detachedHead false

mkdir -p "$PROJECTS" "$REMOTES"

# ── Five projects, five different things for the bar to show ────────────────

new_repo() {           # new_repo <name>
  local dir="$PROJECTS/$1"
  mkdir -p "$dir"
  git -C "$dir" init -q
  printf '# %s\n\nA fake project. Nothing here talks to a network.\n' "$1" > "$dir/README.md"
  git -C "$dir" add -A
  git -C "$dir" commit -qm "First commit"
}

# 1. Clean. The bar shows a branch and nothing else, which is the state most
#    of a working day is actually in.
new_repo orchard-api
mkdir -p "$PROJECTS/orchard-api/src"
cat > "$PROJECTS/orchard-api/src/main.rs" <<'EOF'
fn main() {
    println!("orchard-api");
}
EOF
git -C "$PROJECTS/orchard-api" add -A
git -C "$PROJECTS/orchard-api" commit -qm "A main that prints its own name"

# 2. Dirty: three modified and one untracked.
new_repo orchard-web
for f in index.html style.css app.js; do
  echo "/* $f */" > "$PROJECTS/orchard-web/$f"
done
git -C "$PROJECTS/orchard-web" add -A
git -C "$PROJECTS/orchard-web" commit -qm "The three files every site starts with"
for f in index.html style.css app.js; do
  echo "/* edited, uncommitted */" >> "$PROJECTS/orchard-web/$f"
done
echo "notes to self" > "$PROJECTS/orchard-web/TODO.txt"

# 3. Staged and modified at once, on a branch long enough to be truncated:
#    the bar keeps the first 20 characters and the last 10.
new_repo sparrow-cli
git -C "$PROJECTS/sparrow-cli" checkout -qb feat/really-long-branch-name-for-truncation
echo "staged change" > "$PROJECTS/sparrow-cli/cli.py"
git -C "$PROJECTS/sparrow-cli" add cli.py
echo "and then modified again" >> "$PROJECTS/sparrow-cli/cli.py"

# 4. Untracked only, and the HEAD is detached, which the bar names rather than
#    showing a branch that is not there.
new_repo lantern-docs
echo "draft" > "$PROJECTS/lantern-docs/draft.md"
git -C "$PROJECTS/lantern-docs" checkout -q --detach HEAD

# 5. Ahead of its upstream by two commits. A bare repository stands in for the
#    remote so this needs no network.
git init -q --bare "$REMOTES/anvil-infra.git"
git clone -q "$REMOTES/anvil-infra.git" "$PROJECTS/anvil-infra"
cd "$PROJECTS/anvil-infra"
echo 'resource "null_resource" "example" {}' > main.tf
git add -A && git commit -qm "First commit"
git push -q origin main
git branch -q --set-upstream-to=origin/main main
echo 'variable "region" {}' > variables.tf
git add -A && git commit -qm "A variable nothing reads yet"
echo '# outputs come later' > outputs.tf
git add -A && git commit -qm "A comment where outputs will go"
cd "$HOME"

# A build log with a real-looking error in it, for the copy-mode `o` binding:
# select the path and tmux-companion opens it in the editor at that line.
cat > "$PROJECTS/orchard-api/build.log" <<'EOF'
   Compiling orchard-api v0.1.0
error[E0425]: cannot find value `porrt` in this scope
  --> src/main.rs:2:22
   |
 2 |     println!("{}", porrt);
   |                    ^^^^^ help: a local variable with a similar name exists: `port`

Docs for this error: https://doc.rust-lang.org/error_codes/E0425.html
EOF

# ── The directories the project picker offers ───────────────────────────────
#
# zoxide lists what it has seen. Adding each one twice gives them a frecency
# score above zero, which is what puts them in the list at all.
for d in "$PROJECTS"/*/; do
  zoxide add "$d" || true
  zoxide add "$d" || true
done

# ── History for the run picker ──────────────────────────────────────────────
#
# zsh's extended format: a colon, the timestamp, a zero, a semicolon, the
# command. The run picker parses exactly this.
{
  ts=$(( $(date +%s) - 4000 ))
  while read -r cmd; do
    printf ': %d:0;%s\n' "$ts" "$cmd"
    ts=$(( ts + 60 ))
  done <<'EOF'
git status --short
git log --oneline -10
cargo build --release
cargo test -j 4
nvim src/main.rs
grep -rn TODO .
git diff --stat
tmux-companion doctor
git push origin main
nvim README.md
EOF
} > "$HOME/.zsh_history"
chmod 600 "$HOME/.zsh_history"

# ── The shell ───────────────────────────────────────────────────────────────
cat > "$HOME/.zshrc" <<'EOF'
HISTFILE=$HOME/.zsh_history
HISTSIZE=2000
SAVEHIST=2000
setopt EXTENDED_HISTORY INC_APPEND_HISTORY SHARE_HISTORY

# A short prompt, so the terminal is mostly the thing being demonstrated.
PROMPT='%F{244}%1~%f %F{250}❯%f '

eval "$(zoxide init zsh)"

# Prompt marks. tmux has had next-prompt and previous-prompt since 3.3 and
# they do nothing until the shell says where a prompt begins; this is the one
# line that tells it.
eval "$(tmux-companion shell-init zsh)"

alias tour='/opt/playground/tour.sh'
EOF

# ── neovim ──────────────────────────────────────────────────────────────────
mkdir -p "$XDG_CONFIG_HOME/nvim"
cat > "$XDG_CONFIG_HOME/nvim/init.lua" <<'EOF'
vim.opt.number = true
vim.opt.termguicolors = true
vim.opt.swapfile = false
vim.cmd.colorscheme("habamax")
EOF

# ── tmux ────────────────────────────────────────────────────────────────────
mkdir -p "$XDG_CONFIG_HOME/tmux"
cp /opt/playground/tmux.conf.full.example "$XDG_CONFIG_HOME/tmux/companion.conf"
cp /opt/playground/tmux.playground.conf   "$XDG_CONFIG_HOME/tmux/playground.conf"
cat > "$XDG_CONFIG_HOME/tmux/tmux.conf" <<'EOF'
# The playground's tmux config is the shipped example with everything turned
# on, plus one file of its own for the tour's second status line. Read
# companion.conf: it is docs/tmux.conf.full.example unchanged, and every block
# in it carries what it costs.
source-file ~/.config/tmux/companion.conf
source-file ~/.config/tmux/playground.conf
EOF

# ── tmux-companion ──────────────────────────────────────────────────────────
mkdir -p "$XDG_CONFIG_HOME/tmux-companion"
cat > "$XDG_CONFIG_HOME/tmux-companion/config.toml" <<'EOF'
# The playground's config. docs/config.example.toml is the annotated one and
# it is at /opt/playground/config.example.toml in this image.

[project]
layout = "default"

# Every project opens with the layout below, except one. sparrow-cli matches
# this override and opens `editor` and `ai` instead, so the tour has both
# halves on screen: what a project gets by default, and what one asks for when
# the default is not what that project is for.
[[project.override]]
match = "~/projects/sparrow-cli"
use_layout = "agent"

[[layout]]
name = "agent"

[[layout.window]]
name = "editor"
command = "nvim README.md"
hold_name = true

[[layout.window]]
name = "ai"
command = "echo \"run or continue with your favourite ai tool\""
# Without this the window is renamed after whatever is running in it, and an
# agent renames itself to its own version string within seconds of starting.
hold_name = true

[[layout]]
name = "default"

[[layout.window]]
name = "edit"
command = "nvim README.md"
hold_name = true

[[layout.window]]
name = "git"
layout = "main-horizontal"
main_size = "60%"

[[layout.window.pane]]
command = "git log --oneline --graph --all"
focus = true

[[layout.window.pane]]
command = "git status"
EOF

# ── Themes ──────────────────────────────────────────────────────────────────
#
# Exactly what a new user runs. --background is given explicitly because there
# is no terminal at build time to ask, and colour233 is what the bar draws
# against.
tmux-companion theme init
tmux-companion theme gen --apply --shades --background '#121212'

echo "seeded: $(ls "$PROJECTS" | tr '\n' ' ')"
