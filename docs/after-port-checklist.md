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
| 7 | tmux.conf autoreload | todo |
| 8 | notify when a long command finishes | todo |
| 9 | the job table driving window names | todo |

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
