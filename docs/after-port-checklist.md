# After-port checklist

Status for the work in `after-the-port.md`. One line per item, updated in the
commit that does it. `blocked` carries the reason on the same line.

## Project layouts

| # | Item | Status |
| --- | --- | --- |
| 1 | panes in a layout window | done — `[[layout.window.pane]]`, presets and raw layout strings, `session_commands` is pure and pinned against the pre-panes sequence |
| 2 | `project save`, `forget`, `show`, per-project layout files | todo |
| 3 | `project close`: capture then exit | todo |
| 4 | `.tmux-companion.toml` in the project root, with a trust list | held — the only item here with a security surface, and the least asked for |

## If I only pick five

| # | Item | Status |
| --- | --- | --- |
| 5 | `shell-init`, the OSC 133 prompt marks | todo |
| 6 | git autofetch | todo |
| 7 | tmux.conf autoreload | todo |
| 8 | notify when a long command finishes | todo |
| 9 | the job table driving window names | todo |
