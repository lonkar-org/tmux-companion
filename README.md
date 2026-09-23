# tmux-companion

[![CI](https://github.com/lonkar-org/tmux-companion/actions/workflows/ci.yml/badge.svg)](https://github.com/lonkar-org/tmux-companion/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/lonkar-org/tmux-companion?logo=github&logoColor=white&color=success)](https://github.com/lonkar-org/tmux-companion/releases/latest)
[![Homebrew](https://img.shields.io/badge/brew-lonkar--org%2Ftap-fbb040?logo=homebrew&logoColor=white)](https://github.com/lonkar-org/homebrew-tap)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-dea584?logo=rust&logoColor=white)](rust-toolchain.toml)
[![macOS and Linux](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey)](docs/reference/requirements.md)
[![tmux 3.2+](https://img.shields.io/badge/tmux-3.2%2B-1BB91F?logo=tmux&logoColor=white)](docs/reference/requirements.md)
[![Demo](https://img.shields.io/badge/demo-asciinema-d40000?logo=asciinema&logoColor=white)](https://asciinema.org/a/1266213)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

One binary behind your whole tmux config. It draws the status bar, runs the
pickers behind your keybindings, and builds your project sessions, out of a
daemon that is already warm.

<p align="center">
  <a href="https://asciinema.org/a/1266213">
    <img src="https://media.lonkar.org/tmux-companion/usage.gif"
         alt="tmux-companion: the project picker, a new window, run and zen">
  </a>
</p>

<p align="center">
  <a href="https://asciinema.org/a/1266213">https://asciinema.org/a/1266213</a>
</p>

Twenty four seconds of it: the project picker, a window opened somewhere else,
a command pulled out of shell history, and zen. The chords are in the right
hand column and every one of them was really pressed, so those are the real
popups. The full recording behind that link runs four minutes and covers the
rest, and it is worth watching there rather than here because you can pause it
and it has a marker on every chapter.

## Try it first

```sh
docker run --rm -it lonkarorg/tmux-companion:playground
```

tmux, the binary, five fake projects and a guided tour through the bindings, in
a container that goes away when you leave it. Nothing is mounted from your
machine. [docs/how-to/playground.md](docs/how-to/playground.md).

## What it costs

Measured from inside tmux on one machine, against the zsh this replaced, with
the method beside the numbers in [BENCHMARKS.md](./BENCHMARKS.md).

| | |
| --- | --- |
| the bar | 29.71 ms/s, 3.0% of a core |
| the zsh bar it replaced | 344.01 ms/s, 34.4% of a core |
| a picker, keypress to first row | 86 ms, and the zsh took 82 |
| a picker, CPU per press | 33.3 ms, against 66.7 for the zsh |
| the whole right side, computed | 1.43 ms |
| one fork and exec, as tmux runs it | 13.07 ms |

The picker rows are the honest part. Opening one is not faster: what a person
waits through is `display-popup` at 21 ms, a process starting and a terminal
painting, and none of that got cheaper. What halved is what it costs to do.

## Why

My tmux config shelled out for everything. The bar spawned five processes a
second. Every binding that needed to think ran a zsh script that started a
shell, read some config, called `fzf`, and exited. I'd built it that way over
years, a script at a time, and never added it up.

So there's one process now. It holds its caches, answers over a unix socket, and
everything my tmux used to shell out for talks to it instead.

I wrote a post about it too: [ten years of tmux](https://yogesh.lonkar.org/posts/ten-years-of-tmux/).

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
