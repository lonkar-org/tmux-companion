<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://media.lonkar.org/tmux-companion/banner-dark-2026-10-07.png">
    <img alt="tmux-companion" src="https://media.lonkar.org/tmux-companion/banner-light-2026-10-07.png" width="1280">
  </picture>
</h1>

[![CI](https://github.com/lonkar-org/tmux-companion/actions/workflows/ci.yml/badge.svg)](https://github.com/lonkar-org/tmux-companion/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/tmux-companion?logo=rust&logoColor=white)](https://crates.io/crates/tmux-companion)
[![Docs](https://img.shields.io/badge/docs-with--love.lonkar.org-8250df?logo=astro&logoColor=white)](https://with-love.lonkar.org/tmux-companion/)
[![docs.rs](https://img.shields.io/docsrs/tmux-companion?logo=docsdotrs&label=docs.rs)](https://docs.rs/tmux-companion)
[![Homebrew](https://img.shields.io/badge/brew-lonkar--org%2Ftap-fbb040?logo=homebrew&logoColor=white)](https://github.com/lonkar-org/homebrew-tap)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![macOS and Linux](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey)](docs/reference/requirements.md)
[![Playground](https://img.shields.io/docker/image-size/lonkarorg/tmux-companion/playground?logo=docker&logoColor=white&label=playground)](https://hub.docker.com/r/lonkarorg/tmux-companion)
[![Release](https://img.shields.io/github/v/release/lonkar-org/tmux-companion?logo=github&logoColor=white&color=success)](https://github.com/lonkar-org/tmux-companion/releases/latest)
[![Rust 1.95+](https://img.shields.io/badge/rust-1.95%2B-dea584?logo=rust&logoColor=white)](rust-toolchain.toml)
[![tmux 3.2+](https://img.shields.io/badge/tmux-3.2%2B-1BB91F?logo=tmux&logoColor=white)](docs/reference/requirements.md)

A companion binary for your tmux: agents list/brief/journal, project sessions and saved layouts, chunk clock, statusbar segments, served by a warm daemon instead of a script per keypress.
If you run multi agents in terminal and use tmux, this makes it easy to be on top of them.

<p align="center">
  <a href="https://media.lonkar.org/tmux-companion/panes-2026-10-07.jpg" target="_blank" rel="noopener">
    <img src="https://media.lonkar.org/tmux-companion/panes-2026-10-07.jpg" width="48%"
         alt="The panes picker: every Claude Code pane on the server in one list, each marked asked, waiting or done with how long ago and its last line"></a>
  <a href="https://media.lonkar.org/tmux-companion/brief-2026-10-07.jpg" target="_blank" rel="noopener">
    <img src="https://media.lonkar.org/tmux-companion/brief-2026-10-07.jpg" width="48%"
         alt="The brief: the four agents waiting on you, numbered to jump to, then health, the session and agent count, and how long you have been sitting"></a>
</p>

<p align="center">What each agent is doing at a glance | What's brief status across sessions</p>

<p align="center">
  <a href="https://asciinema.org/a/1267082">
    <img src="https://media.lonkar.org/tmux-companion/usage-2026-09-30-popups.gif"
         alt="tmux-companion: the project picker, a new window and run">
  </a>
</p>

<p align="center">
  <a href="https://asciinema.org/a/1267082">https://asciinema.org/a/1267082</a>
</p>

## What you get

|                                     |                                                                                                           |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `inbox`, `brief`, `journal`         | the agents waiting on you and what each asked, a nudge when one waits too long, and what happened today   |
| [`SKILL.md`](docs/how-to/agents.md) | the skill that teaches a coding agent to share your tmux server                                           |
| `sessions`                          | every session saved on a timer, and `sessions resurrect` brings them back after a reboot                  |
| `project`                           | one session per project, sessions and your directory jumper in one list                                   |
| `project save`                      | capture this session's panes as the layout it reopens with                                                |
| `chunk`                             | how long this sitting has been in focus, painted on the status line, and a notice when the budget is over |
| `panes`, `search`, `ports`          | jump to any pane, any line of any scrollback, or whatever's listening on that port                        |
| `promote`, `note`, `quiet`          | a pane into a session of its own, a note on a pane, an hour with nothing nagging                          |
| `status-right`                      | git, bandwidth and battery in one call                                                                    |
| `keys`                              | fuzzy search every binding, press enter to run it                                                         |
| `cheatsheet`                        | the bindings you keep looking up, until you've learned them                                               |
| `run`                               | pick from shell history, run it in a pane that slides out                                                 |
| `theme pick`                        | your themes with a swatch each, applied on the spot; `theme init` writes six to start                     |

Every flag: [docs/reference/cli.md](docs/reference/cli.md).

## Try it first

```sh
docker run --rm -it lonkarorg/tmux-companion:playground
```

tmux, the binary, five fake projects and a guided tour through the bindings, in
a container that goes away when you leave it. Nothing is mounted from your
machine. [docs/how-to/playground.md](docs/how-to/playground.md).

## Install

**All five paths**, with the flags and how to remove it again, are in
[docs/how-to/install.md](docs/how-to/install.md).

### Quick

```sh
curl -fsSL https://raw.githubusercontent.com/lonkar-org/tmux-companion/main/scripts/install.sh | bash
tmux-companion doctor
```

That downloads the binary for your machine, checks it against the checksums the
release published, and puts it on PATH. Nothing to compile. Read the script
first if you'd rather not pipe it, which is a fair thing to want.

Then the way in, from a shell that is not in tmux yet:

```sh
tmux-companion setup
tmux-companion start
```

That opens the project picker, live sessions first and then every directory
your jumper knows, and attaches to what you choose. `tmux` on its own leaves you in
a session called `0` with one bare shell, which is the thing this replaces.
`start --last` goes back to whatever you were in without asking, and
`start ~/src/thing` skips the picker.

Then [docs/tutorial/first-hour.md](docs/tutorial/first-hour.md) takes it from
there, one step at a time.

## Configuration

There's no config file till you write one, and the defaults are what the binary
did before the file existed.

```sh
tmux-companion config init      # a fifteen-line starter, refuses to overwrite
tmux-companion config check
```

Every setting with its default is in
[docs/config.example.toml](docs/config.example.toml), and the reasoning is in
[docs/reference/configuration.md](docs/reference/configuration.md).

## What it doesn't do

- macOS and Linux. Not Windows, and not planned: the whole thing is a unix
  socket and a `SIGWINCH`. WSL2 may work but is untested, see
  [docs/how-to/install.md](docs/how-to/install.md).
- The pickers need tmux 3.2 for `display-popup -E`. The bar is happy on 3.0.
- It won't restore your sessions on its own. It saves them on a timer, and
  restoring stays on a key you press, cause an automatic restore would
  resurrect a stale layout over a session you'd already started working in.

## Why

My tmux config shelled out for everything. The bar spawned five processes a
second. Every binding that needed to think ran a zsh script that started a
shell, read some config, called `fzf`, and exited. I'd built it that way over
years, a script at a time, and never added it up.
That's how this repository started, I wrote a post about it too: [ten years of tmux](https://yogesh.lonkar.org/posts/ten-years-of-tmux/).

Then multi agent workspace became difficult to context-switch and focus,
this time instead of writing scripts add feature to companion for handling multiple agents.

## Documentation

<img src="https://with-love.lonkar.org/favicon.svg" height="18" alt="With love"/> [https://with-love.lonkar.org/tmux-companion/](https://with-love.lonkar.org/tmux-companion/) for easy search and navigation.

|              |                                                                                    |                                                                                            |
| ------------ | ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| tutorial     | [docs/tutorial/first-hour.md](docs/tutorial/first-hour.md)                         | from nothing installed to a bar you can see and a picker you've pressed                    |
| how-to       | [docs/how-to/playground.md](docs/how-to/playground.md)                             | a container to try it in, and the tour inside it                                           |
| how-to       | [docs/how-to/install.md](docs/how-to/install.md)                                   | the five ways in, and how to remove it                                                     |
| how-to       | [docs/how-to/themes.md](docs/how-to/themes.md)                                     | where themes live, what one is, and the contrast they clear                                |
| how-to       | [docs/how-to/agents.md](docs/how-to/agents.md)                                     | the skill that teaches a coding agent to share your tmux server                            |
| how-to       | [docs/how-to/which-key.md](docs/how-to/which-key.md)                               | list of key bindings that you could setup and what it could do for you                     |
| how-to       | [docs/how-to/things-tmux-already-does.md](docs/how-to/things-tmux-already-does.md) | logging, menus, moving panes, one config across versions: tmux does these without a plugin |
| reference    | [docs/reference/cli.md](docs/reference/cli.md)                                     | every subcommand and flag                                                                  |
| reference    | [docs/reference/configuration.md](docs/reference/configuration.md)                 | the config file, and what it changes                                                       |
| reference    | [docs/reference/requirements.md](docs/reference/requirements.md)                   | tmux, fonts, platforms, Rust                                                               |
| example      | [docs/tmux.conf.starter.example](docs/tmux.conf.starter.example)                   | the bar and the eight bindings worth having on day one                                     |
| example      | [docs/tmux.conf.example](docs/tmux.conf.example)                                   | the bar I actually run                                                                     |
| example      | [docs/tmux.conf.full.example](docs/tmux.conf.full.example)                         | every feature on, with what each costs                                                     |
| example      | [docs/config.example.toml](docs/config.example.toml)                               | every setting with its default                                                             |
| explanation  | [docs/explanation/cheatsheet.md](docs/explanation/cheatsheet.md)                   | why the cheat sheet forgets what you've learned                                            |
| contributing | [CONTRIBUTING.md](CONTRIBUTING.md)                                                 | build, test, lint, and what a patch needs                                                  |
| contributing | [CHANGELOG.md](CHANGELOG.md)                                                       | what changed                                                                               |

### Contributing

The code and features are stable enough that I have not done lot of fixes,
I do use it everyday so changes I make could be very opinionated.
The whole idea for this was to have tailored DX.
Now that the code is public the DX could be improved, generalised.

Patches welcome, including the ones that tell me I got something wrong.
[CONTRIBUTING.md](CONTRIBUTING.md) has the three commands CI runs.

MIT.
