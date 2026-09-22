# After-port checklist

Status for the work in `after-the-port.md`. One line per item, updated in the
commit that does it. `blocked` carries the reason on the same line.

## Project layouts

| # | Item | Status |
| --- | --- | --- |
| 1 | panes in a layout window | done — `[[layout.window.pane]]`, presets and raw layout strings, `session_commands` is pure and pinned against the pre-panes sequence |
| 2 | `project save`, `forget`, `show`, per-project layout files | done — `src/saved.rs`, files under `$XDG_STATE_HOME/tmux-companion/projects/`, smoke-project-layout.sh covers it end to end |
| 3 | capture before `close-project` exits | done — folded into `close-project`, which already owns the X binding; capture runs before anything is asked to quit |
| 4 | `.tmux-companion.toml` in the project root, with a trust list | held — the only item here with a security surface, and the least asked for |

## If I only pick five

| # | Item | Status |
| --- | --- | --- |
| 5 | `shell-init`, the OSC 133 prompt marks | done — zsh, bash and fish; the escape bytes are verified by running each snippet, the tmux-side navigation wants one manual check (see below) |
| 6 | git autofetch | done — `[git.autofetch]`, off by default, sequential with a timeout and every git prompt disabled |
| 7 | tmux.conf autoreload | done — `[autoreload]`, off by default, first pass never reloads, errors go to display-message |
| 8 | notify when a long command finishes | done — `[notify]`, off by default, display-message needs nothing installed; whether it earns its place on a one-client machine is still unmeasured |
| 9 | the job table driving window names | done — `[window_names]` plus `window_name` on a job row, off by default, never overrides a name somebody pinned |

## Wants a manual check

`shell-init`'s escape sequences are verified: each snippet is sourced by its
real shell in the test run and `od -c` shows the bytes tmux is documented to
read. What is not verified automatically is the other half, that tmux's
`previous-prompt` then jumps between them, because `send-keys -X` acts on an
attached client's copy-mode state and every attempt to attach one from a script
took the test server down with it.

Ten seconds in a real terminal settles it:

```sh
eval "$(tmux-companion shell-init zsh)"
echo one; echo two; echo three
# prefix + [ to enter copy mode, then C-p with the binding from
# docs/tmux.conf.full.example
```

The cursor should land on each prompt line going up.

## Distribution

| # | Item | Status |
| --- | --- | --- |
| 10 | four release binaries, tag-triggered, approval-gated | done — all four targets compile here, the two macOS ones link and are the right architecture; the musl pair is CI's first run to prove |
| 11 | `scripts/install.sh`, checksum-verified | done — shellcheck clean, exercised end to end through its build-from-source fallback |
| 12 | tpm plugin entry point | done — `tmux-companion.tmux`, binds nothing |
| 13 | the `release` environment with required reviewers | **needs Yogesh** — a repo settings change, see below |

### The release environment

`.github/workflows/release.yml` gates its publish job on an environment called
`release`. That environment does not exist yet, and creating it is a repository
settings change rather than something a workflow can do for itself.

Settings, Environments, New environment, name it `release`, tick Required
reviewers and add yourself. Until that exists the publish job runs without
waiting for anybody, so it is worth doing before the first tag rather than
after.

## Recordings

The README's two video slots are filled from recordings made locally. The
driver that makes them is not part of this repository.

One finding from making them does belong here, because it was a bug in what
this repository ships: `docs/tmux.conf.full.example` passed `#{pane_pid}` to
`status-right`, which takes a path and nothing else, so clap rejected it and
the whole right-hand side came up blank for anybody who copied that file. The
invariant in `CLAUDE.md` already said `status-right` must never be passed a
pane pid.

Two more worth knowing for anybody automating tmux:

- A `display-popup` pane does not appear in `list-panes -a` and `send-keys`
  cannot reach it, so a popup can be shown in an automated session but never
  driven or closed.
- A separate tmux socket is not a separate configuration. Without `-f` the
  server reads the config of whoever started it.
