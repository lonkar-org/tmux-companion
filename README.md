# tmux-companion

[![CI](https://github.com/lonkar-org/tmux-companion/actions/workflows/ci.yml/badge.svg)](https://github.com/lonkar-org/tmux-companion/actions/workflows/ci.yml)

One binary behind your whole tmux config. It draws the status bar, runs the
pickers behind your keybindings, and builds your project sessions, out of a
daemon that is already warm.

## Why

My tmux config shelled out for everything. The bar spawned five processes a
second. Every binding that needed to think ran a zsh script that started a
shell, read some config, called `fzf`, and exited. I'd built it that way over
years, a script at a time, and never added it up.

The bar alone cost 15.4% of one core, all day, on a laptop running on battery.
It costs 1.8% now, and the pickers open in 8.3 ms instead of 18.8.

None of that's because Rust is fast. tmux gates `#()` to `status-interval` per
attached client, so a bar costs what it spawns and not what it computes: a fork
and exec is 12.4 ms of CPU, and computing the whole right-hand side takes 2.6.
The same arithmetic runs the other way for a picker, where the cost was a shell
starting up before anything showed on screen.

So there's one process now. It holds its caches, answers over a unix socket, and
everything my tmux used to shell out for talks to it instead.

## What you get

| | |
| --- | --- |
| `status-right` | git, bandwidth and battery in one call |
| `keys` | fuzzy search every binding, press enter to run it |
| `cheatsheet` | the bindings you wrote, four boxes, most-used first |
| `project` | one session per project, sessions and your directory jumper in one list |
| `project save` | capture this session's panes as the layout it reopens with |
| `run` | pick from shell history, run it in a pane that slides out |
| `open` | open the URL or `file:line:col` under your cursor |
| `theme pick` | your themes with a swatch each, applied on the spot; `theme init` writes six to start |
| `shell-init` | the prompt marks tmux's `next-prompt` has waited for since 3.3 |
| `sh-jobs` | what's suspended under this pane, with your icons |
| `doctor` | everything a bug report needs, in one screen |

Background tasks, all of them off till you turn them on: fetching your repositories so
ahead and behind mean something, sourcing tmux's config when it changes, naming
windows after what's running in them, and saying when a long command finished
somewhere you weren't looking.

Every flag: [docs/reference/cli.md](docs/reference/cli.md).

## Try it first

```sh
docker run --rm -it lonkarorg/tmux-companion:playground
```

tmux, the binary, five fake projects and a guided tour through the bindings, in
a container that goes away when you leave it. Nothing is mounted from your
machine. [docs/how-to/playground.md](docs/how-to/playground.md).

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/lonkar-org/tmux-companion/main/scripts/install.sh | bash
tmux-companion doctor
```

That downloads the binary for your machine, checks it against the checksums the
release published, and puts it on PATH. Nothing to compile. Read the script
first if you'd rather not pipe it, which is a fair thing to want.

With [tpm](https://github.com/tmux-plugins/tpm):

```tmux
set -g @plugin 'lonkar-org/tmux-companion'
```

It binds no keys and sets no options. Or clone it and run
`cargo build --release` yourself.

On a Mac or a Linux with Homebrew:

```sh
brew tap lonkar-org/tap
brew trust lonkar-org/tap
brew install tmux-companion
```

Homebrew 7 won't load a formula from a tap you haven't trusted, hence the middle
line. That path brings the manual with it, so `man tmux-companion` works
straight after.

All four paths, with the flags and how to remove it again, are in
[docs/how-to/install.md](docs/how-to/install.md).

Then the way in, from a shell that is not in tmux yet:

```sh
tmux-companion start
```

That opens the project picker, live sessions first and then every directory
your jumper knows, and attaches to what you choose. `tmux` on its own leaves you in
a session called `0` with one bare shell, which is the thing this replaces.
`start --last` goes back to whatever you were in without asking, and
`start ~/src/thing` skips the picker.

The jumper is zoxide by default and is not a requirement: `[project]
dirs_source` also takes `z`, `cdr`, `ghq` or `none`, `dirs_command` takes
anything else that prints paths, and with nothing installed at all you get live
sessions and whatever you type. [docs/reference/configuration.md](docs/reference/configuration.md#where-the-directory-list-comes-from)
has the table.

One line in `tmux.conf` gets you the bar:

```tmux
set -g status-right "#(tmux-companion status-right --branch-max-len 40 #{pane_current_path})"
```

The annotated version, with the reasoning for every line and what each feature
costs, is [docs/tmux.conf.full.example](docs/tmux.conf.full.example). The
smaller one I actually run is [docs/tmux.conf.example](docs/tmux.conf.example).

## Usage

[![tmux-companion: the project picker, a new window, run and zen](https://media.lonkar.org/tmux-companion/usage.gif)](https://asciinema.org/a/1266213)

Twenty four seconds of it: the project picker, a window opened somewhere else,
a command pulled out of shell history, and zen. The chords are in the right
hand column and every one of them was really pressed, so those are the real
popups. The [full recording](https://asciinema.org/a/1266213) runs
four minutes and covers the rest, and it is worth watching there rather than
here because you can pause it.

Every picker is a `display-popup -E` away. `keys` is the one I'd bind first:
tmux has notes on its bindings and no way to search them, so the popup reads
your `-N` strings and runs whatever you pick.

The recordings are made with
[firacode-nfc-tweaked](https://github.com/lonkar-org/firacode-nfc-tweaked), Fira
Code patched with Nerd Fonts. Any v3 [Nerd Font](https://www.nerdfonts.com/font-downloads)
draws the same glyphs, and `[glyphs] preset = "ascii"` covers you if you haven't
got one.

## Configuration

There's no config file till you write one, and the defaults are what the binary
did before the file existed.

```sh
tmux-companion config dump > ~/.config/tmux-companion/config.toml
tmux-companion config check
```

If your bar's a row of boxes you don't have a Nerd Font, and this is the line:

```toml
[glyphs]
preset = "ascii"
```

Every setting with its default is in
[docs/config.example.toml](docs/config.example.toml), and the reasoning is in
[docs/reference/configuration.md](docs/reference/configuration.md).

## What it costs

Measured on one machine, with the method beside the numbers in
[BENCHMARKS.md](./BENCHMARKS.md).

| | |
| --- | --- |
| the recommended bar | 18.43 ms/s, 1.8% of a core |
| the five-call bar it replaced | 153.77 ms/s, 15.4% of a core |
| one fork and exec | 12.4 ms, or 14.6 ms as tmux runs it |
| the whole right side, computed | 2.6 ms |
| `keys`, warm, against `fzf --filter` | 8.3 ms against 18.8 ms |
| a cold `git status` after a restart | 51 ms, once |

## What it doesn't do

- macOS and Linux. Not Windows, and not planned: the whole thing is a unix
  socket and a `SIGWINCH`.
- The pickers need tmux 3.2 for `display-popup -E`. The bar is happy on 3.0.
- It won't restore your sessions on its own. It saves them on a timer, and
  restoring stays on a key you press, cause an automatic restore would
  resurrect a stale layout over a session you'd already started working in.
- It's not a theme pack. `theme init` writes six colours and the two files that
  apply them, `theme gen --shades` grows that to 151, every colour in tmux's
  cube whose text clears WCAG AAA, with the contrast computed against your
  terminal. `--shades a4`, `a5` and `a6` are shorter lists if 151 is more than
  you want to scroll, down to the original eighteen. After that the palette is
  yours; it doesn't compete with catppuccin or rose-pine.

## Documentation

| | |
| --- | --- |
| [docs/reference/cli.md](docs/reference/cli.md) | every subcommand and flag |
| [docs/reference/configuration.md](docs/reference/configuration.md) | the config file, and what it changes |
| [docs/config.example.toml](docs/config.example.toml) | every setting with its default |
| [docs/tmux.conf.example](docs/tmux.conf.example) | the bar I actually run |
| [docs/tmux.conf.full.example](docs/tmux.conf.full.example) | every feature on, with what each costs |
| [docs/how-to/playground.md](docs/how-to/playground.md) | a container to try it in, and the tour inside it |
| [docs/how-to/install.md](docs/how-to/install.md) | the three ways in, and how to remove it |
| [docs/how-to/themes.md](docs/how-to/themes.md) | where themes live, what one is, and the contrast they clear |
| [docs/reference/requirements.md](docs/reference/requirements.md) | tmux, fonts, platforms, Rust |
| [DESIGN.md](DESIGN.md) | how the daemon and the protocol work |
| [BENCHMARKS.md](BENCHMARKS.md) | what it costs, and how that was measured |
| [CONTRIBUTING.md](CONTRIBUTING.md) | build, test, lint, and what a patch needs |
| [CHANGELOG.md](CHANGELOG.md) | what changed |

## Building

Rust 1.85 or newer, which is the first release with edition 2024. No system
libraries.

```sh
cargo build --release      # target/release/tmux-companion, about 4 MB
cargo test                 # 639 unit, 22 integration, 5 end-to-end against a real tmux

# on a machine somebody is using, keep off every core
nice -n 15 cargo build --release -j 4
```

Releases carry four binaries: macOS on Apple silicon and Intel, Linux on ARM and
on Intel or AMD. The Linux pair link statically against musl, so one binary runs
on any distribution.

Patches welcome, including the ones that tell me I got something wrong.
[CONTRIBUTING.md](CONTRIBUTING.md) has the three commands CI runs.

MIT.
