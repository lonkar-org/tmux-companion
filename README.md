# tmux-companion

One binary that draws your tmux status bar and runs the pickers behind your
keybindings, out of a daemon that's already warm.

![the status bar](./screenshot-tmux-status-bar.png)

## Why I wrote it

My status bar spawned five processes every second to draw one line of text. I
had built it that way over years, a script at a time, and never added the
numbers up. When I finally measured it, the bar was costing **15.4% of one
core**, all day, on a laptop running on battery.

It costs **1.8%** now.

The reason is not that Rust is fast. It's that tmux gates `#()` to
`status-interval` per attached client, so what a bar costs is set by how many
commands it spawns and not by what they compute. A fork and exec is 12.4 ms of
CPU. Computing the whole right-hand side takes 2.6 ms. Five calls a second was
me paying the postage five times to send one letter.

So there's one `#()` call now. Everything behind it lives in a daemon that
keeps its caches warm, and every other thing my tmux used to shell out for
talks to that same daemon over a unix socket: the key-binding search, the
project switcher, the theme picker, the command runner.

## What you get

| | |
| --- | --- |
| `status-right` | git, bandwidth and battery in one call |
| `keys` | fuzzy search every binding, press enter to run it |
| `cheatsheet` | the bindings you wrote, four boxes, most-used first |
| `project` | one session per project, sessions and zoxide in one list |
| `run` | pick from shell history, run it in a pane that slides out |
| `theme pick` | 76 themes with a swatch each, applied on the spot |
| `open` | open the URL or `file:line:col` under your cursor |
| `sh-jobs` | what's suspended under this pane, with your icons |
| `doctor` | everything a bug report needs, in one screen |

Full list with every flag: [docs/reference/cli.md](docs/reference/cli.md).

## Quick start

```sh
cargo build --release
sudo install -m 755 target/release/tmux-companion /usr/local/bin/

# it starts its own daemon the first time you ask it anything
tmux-companion gst .
tmux-companion doctor
```

Then one line in `tmux.conf`:

```tmux
set -g status-right "#(tmux-companion status-right --branch-max-len 40 #{pane_current_path})"
```

The annotated version, with the reasoning for every line, is in
[docs/tmux.conf.example](./docs/tmux.conf.example). Install the binary before
you apply the config, or the right-hand side goes blank till you do.

## Usage

<!-- @Yogesh(video): usage recording goes here. Suggested run: prefix+? for keys,
     M-s for project, prefix+e for run, prefix+C-t for the theme picker. Drop it
     in as a GIF or an mp4 link above this line and delete the comment. -->

Every picker is a `display-popup -E` away. `keys` is the one I'd bind first:
tmux has notes on its bindings and no way to search them, so the popup reads
your `-N` strings and runs whatever you pick.

![the left of the bar](./screenshot-tmux-status-bar-left.png)
![the middle](./screenshot-tmux-status-bar-middle.png)
![the right](./screenshot-tmux-status-bar-right.png)

## Configuration

<!-- @Yogesh(video): configuration recording goes here. Suggested run: no config
     at all, then `config dump`, then a glyph preset change, then trimming
     [git] parts. Drop it in above this line and delete the comment. -->

There's no config file till you write one, and the defaults are what the binary
did before the file existed. When you do want one:

```sh
tmux-companion config dump > ~/.config/tmux-companion/config.toml
tmux-companion config check
```

If your bar is a row of boxes, you don't have a Nerd Font and this is the line:

```toml
[glyphs]
preset = "ascii"
```

And if you've got four hundred untracked build artifacts, a count of them is
not information:

```toml
[git]
parts = ["branch", "state", "staged", "modified"]
```

Every setting with its default is in
[docs/config.example.toml](docs/config.example.toml), and the reasoning is in
[docs/reference/configuration.md](docs/reference/configuration.md).

## What it costs

Measured on one machine, and the numbers are in
[BENCHMARKS.md](./BENCHMARKS.md) with the method beside them.

| | |
| --- | --- |
| the recommended bar | 18.43 ms/s, 1.8% of a core |
| the five-call bar it replaced | 153.77 ms/s, 15.4% of a core |
| one fork and exec | 12.4 ms, or 14.6 ms as tmux runs it |
| the whole right side, computed | 2.6 ms |
| `keys`, warm, against `fzf --filter` | 8.3 ms against 18.8 ms |
| a cold `git status` after a restart | 51 ms, once |

That last one is the daemon's whole bargain: state lives in memory and dies
with the process, and you pay 51 ms for it once.

## What it doesn't do

- It's macOS and Linux. Not Windows, and not planned: the whole thing is a unix
  socket and a `SIGWINCH`.
- The pickers need tmux 3.2 for `display-popup -E`. The bar itself is happy on
  3.0.
- It won't restore your sessions. It saves them on a timer, and restoring stays
  on a key you press, cause an automatic restore would resurrect a stale layout
  over a session you've already started working in.
- It's not a theme pack. It applies themes and doesn't compete with catppuccin
  or rose-pine.

## Documentation

| | |
| --- | --- |
| [docs/reference/cli.md](docs/reference/cli.md) | every subcommand and flag |
| [docs/reference/configuration.md](docs/reference/configuration.md) | the config file, and what it changes |
| [docs/config.example.toml](docs/config.example.toml) | every setting with its default |
| [docs/tmux.conf.example](docs/tmux.conf.example) | the bar I actually run |
| [docs/tmux.conf.full.example](docs/tmux.conf.full.example) | every feature on, with what each costs |
| [docs/reference/requirements.md](docs/reference/requirements.md) | Rust, tmux, fonts, platforms |
| [DESIGN.md](DESIGN.md) | how the daemon and the protocol work |
| [BENCHMARKS.md](BENCHMARKS.md) | what the bar costs, and how that was measured |
| [CONTRIBUTING.md](CONTRIBUTING.md) | build, test, lint, and what a patch needs |
| [CHANGELOG.md](CHANGELOG.md) | what changed |

## Building

Rust 1.85 or newer, which is the first release with edition 2024. No system
libraries.

```sh
cargo build --release      # target/release/tmux-companion, about 4 MB
cargo test                 # 498 unit tests and 22 integration tests

# on a machine somebody is using, keep off every core
nice -n 15 cargo build --release -j 4
```

Patches welcome, including the ones that tell me I got something wrong.
[CONTRIBUTING.md](CONTRIBUTING.md) has the three commands CI runs.

MIT.
