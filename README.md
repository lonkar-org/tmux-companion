# tmux-companion

_Status bar_

![screenshot-tmux-status-bar.png](./screenshot-tmux-status-bar.png)

_Left_

![screenshot-tmux-status-bar-left.png](./screenshot-tmux-status-bar-left.png)

_Middle_

![screenshot-tmux-status-bar-middle.png](./screenshot-tmux-status-bar-middle.png)

_Right_

![screenshot-tmux-status-bar-right.png](./screenshot-tmux-status-bar-right.png)

---

A single self-contained Rust binary that replaces a collection of shell scripts
and a Go tool (`yrl gst`) used to drive tmux status-line segments.

Instead of spawning six short-lived processes every second, tmux-companion runs
as a persistent background daemon. Each tmux refresh sends a lightweight JSON
request over a Unix socket and prints the result — no process startup overhead,
no re-reading config files, no repeated disk I/O.

The status bar makes **one** `#()` call. tmux gates `#()` to `status-interval`
per attached client, so what the bar costs is set by how many distinct commands
it spawns, not by what they compute: a fork/exec is 12.4 ms of CPU (14.6 ms
including the shell tmux wraps jobs in), while computing the entire right-hand
side server-side takes 2.6 ms. The whole bar costs 1.8% of one core; it used to
cost 15.4%. See [BENCHMARKS.md](./BENCHMARKS.md).

**Requires font with nerdfonts glyphs**

## Segments

| Subcommand           | Replaces                      | What it shows                                                 |
| -------------------- | ----------------------------- | ------------------------------------------------------------- |
| `status-right [PATH]`| the three calls below         | **The whole right side in one call** — git, bandwidth, battery |
| `gst [PATH]`         | `yrl gst`                     | Powerline git-status segment                                  |
| `window …`           | `window-status.zsh`           | Window index icon, abbreviated path, process dot, alert flags |
| `battery`            | `battery-life.zsh`            | Battery percentage and icon (macOS)                           |
| `net`                | `net-monitor.zsh`             | Download / upload bandwidth                                   |
| `clients <sa> <wac>` | `check-clients.zsh`           | Other tmux clients connected to the same server               |
| `vim-bg <pid>`       | `check-vim-in-background.zsh` | Suspended nvim in current pane                                |

`status-right` is what the status bar should use. The individual segment
commands all still work and are useful by hand; `clients` and `vim-bg` are no
longer on the bar at all, because between them they cost two process spawns and
a whole-process-table scan every second.

All subcommands speak to the same daemon; only `server` starts the daemon
itself, and only `preview` needs no daemon at all.

## Quick start

```sh
cargo build --release
sudo mv target/release/tmux-companion <some-dirctory-in-your-PATH>

# Manual smoke test
tmux-companion server &         # auto-started by clients, but you can start it explicitly
tmux-companion gst              # git status for current directory
tmux-companion battery
```

## tmux.conf integration

The full annotated block is in [docs/tmux.conf.example](./docs/tmux.conf.example).
The short version — one `#()` call, everything else native tmux:

```tmux
set -g  status-interval 1
set -g  status-left  "#[fg=#{@theme-session-name-fg},bg=#{@theme-session-name-bg}] #S "
if-shell '[ -n "$SSH_CONNECTION" ]' \
  'set -ga status-left "#[fg=color203,bg=color233] 󰣀 #h"'
set -ga status-left  "#[fg=color240,bg=color233] %H:%M:%S"
set -ga status-left  "#[fg=color235,bg=color233] "
set -g  status-right "#(tmux-companion status-right --branch-max-len 40 #{pane_current_path})"
```

**Install the binary before applying the config** — it references the
`status-right` subcommand, which older binaries do not have.

Note the absence of `#{pane_pid}`. Passing it made the git segment call
`has_suspended_nvim`, which enumerates every process on the machine — 18.5 ms of
the 26.0 ms the segment cost per call. The segment gives up its suspended-nvim
marker in exchange; `tmux-companion gst <path> <pid>` still honours a pid.

## The `gst` segment in detail

```sh
tmux-companion gst [PATH] [-f]
```

- `PATH` defaults to the current pane directory (`#{pane_current_path}`).
- `-f` / `--force` — bypass the cache and fetch a fresh status. Useful for a
  keybinding that refreshes on demand. The fresh result is still cached, so the
  next ordinary call is warm.
- `--ttl SECS` — how long a cached status stays fresh (default 5, `0` disables
  caching). Applies to the git segment only; bandwidth is never cached.
- Outputs an empty string for non-git directories (safe to use everywhere).

Segment anatomy (left → right):

```
 <remote-ok/fail/loading>  <branch-type-icon> <branch-name>  <ahead↑ behind↓ unmerged>  <unstaged>  <staged>  <stash>
```

Background colours change with repo state:

| State                  | Colour           |
| ---------------------- | ---------------- |
| Clean                  | Green (120)      |
| New branch             | Light grey (251) |
| Gone upstream          | Dark red (088)   |
| Dirty / ahead / behind | Orange (209)     |

## Server lifecycle

The daemon holds all of its state in memory — the git-status cache, the
is-inside-work-tree cache, the battery reading and the bandwidth previous-sample
— and that state dies with it. That is the intended lifetime: one cold
`git status` after a restart costs 51 ms, once.

The daemon starts automatically when any client subcommand is invoked and no
server is listening on the socket yet. It runs until the system restarts or
until it is killed explicitly. Because it lives inside the user's tmux session
lifetime, no init-system integration is needed.

Socket: `/tmp/tmux-companion-<uid>.sock`, or `$TMUX_COMPANION_SOCK` when set —
which is how the benchmarks run a server beside the live one without disturbing
the status bar you are looking at.

## Documentation

| | |
| --- | --- |
| [docs/reference/requirements.md](docs/reference/requirements.md) | Rust, tmux, fonts, platforms |
| [docs/tmux.conf.example](docs/tmux.conf.example) | the recommended status-bar configuration |
| [DESIGN.md](DESIGN.md) | how the daemon and the protocol work |
| [BENCHMARKS.md](BENCHMARKS.md) | what the bar costs, and how that was measured |
| [CONTRIBUTING.md](CONTRIBUTING.md) | build, test, lint, and what a patch needs |
| [docs/reference/cli.md](docs/reference/cli.md) | every subcommand and flag |
| [docs/reference/configuration.md](docs/reference/configuration.md) | the config file, and what it can change |
| [docs/config.example.toml](docs/config.example.toml) | every setting with its default |
| [docs/tmux.conf.full.example](docs/tmux.conf.full.example) | every feature on, with its measured cost |
| [CHANGELOG.md](CHANGELOG.md) | what changed |
| [docs/comrades-port.md](docs/comrades-port.md) | what was built, and why |

## Building

Requires Rust 1.85 or newer, which is the first release with edition 2024. No
system libraries needed. Full list in [docs/reference/requirements.md](docs/reference/requirements.md).

```sh
cargo build --release

# on a machine someone is using, keep off every core:
nice -n 15 cargo build --release -j 4
```

Binary: `target/release/tmux-companion` (≈ 4.0 MB).

## Tests

```sh
cargo test          # unit and integration tests, no external dependencies required
```
