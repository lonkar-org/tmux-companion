# tmux-companion playground

A throwaway tmux with everything turned on: the binary, the full config, five
fake git projects and a sixteen step tour that walks you through the bindings
one at a time.

```sh
docker run --rm -it lonkarorg/tmux-companion:playground
```

27 MB, amd64 and arm64. Nothing is mounted from your machine and nothing is
published off it, so the whole thing goes away with the container.

**Run it from a terminal that isn't already in tmux.** Start it from inside a
tmux session and `Ctrl-b` goes to that server instead, so every binding in the
tour looks broken. If you're nested anyway, press the prefix twice: `Ctrl-b
Ctrl-b ?` reaches the inner one.

## What you land in

Two sessions. You start in `playground`, a shell in `~/projects/orchard-api`,
and the tour waits in `instructions`.

| | |
| --- | --- |
| `prefix` then `i` | the tour (prefix is `Ctrl-b`) |
| `Alt-s`, or `prefix P` | the project picker |
| `prefix` then `?` | search every binding, enter runs it |

Step one offers a detour: press `t` for nine screens on tmux itself, what a
server and a client are, how sessions, windows and panes nest, and what
detaching actually does. Each tour step says what to press, puts itself on a
second status line so it's still in front of you after you've switched
sessions, and waits for Enter. `s` skips a step, `q` leaves for a shell, and
`tour` starts it again.

On macOS, Terminal.app and iTerm2 send an accented character for Option until
you turn on "Use Option as Meta key" or set the left Option key to `Esc+`. The
playground binds the two Alt keys to the prefix as well, so the tour works
either way.

## The five projects

Each is in a different state, so the git segment has something different to say
in each one:

| | |
| --- | --- |
| `orchard-api` | clean |
| `orchard-web` | three modified, one untracked |
| `sparrow-cli` | staged and modified at once, on a branch long enough to truncate |
| `lantern-docs` | detached HEAD, one untracked file |
| `anvil-infra` | two commits ahead of its upstream |

`orchard-api/build.log` holds a compiler error with a path, a line and a column
in it, for the copy-mode `o` binding to open.

## What isn't there

A container has no battery, so that segment stays empty, and the network
counters sit below the threshold the bandwidth segment draws at unless
something is transferring, so that one usually is too. Both are there on a
laptop. `tmux-companion doctor` inside the container says what it can and
cannot see.

The one thing the image can't do for you is the font. Glyphs are drawn by the
terminal on your machine, so a [Nerd Font](https://www.nerdfonts.com/font-downloads)
has to be installed and selected there. The first step of the tour prints four
of them so you can see whether yours works. Everything runs without one, it
just reads worse.

## Tags

| | |
| --- | --- |
| `playground` | the latest build |
| `playground-v0.1.0` | pinned to a release |

## Taking the config with you

Everything the playground runs is a file in the image, so a shell in it
(`docker run --rm -it lonkarorg/tmux-companion:playground bash`) and a
`docker cp` are enough to walk away with any of it:

| | |
| --- | --- |
| `~/.config/tmux/companion.conf` | the shipped example config, unchanged |
| `~/.config/tmux/playground.conf` | the tour's scaffolding, not part of the tool |
| `~/.config/tmux-companion/config.toml` | the layout the projects open with |
| `/opt/playground/config.example.toml` | every option, annotated |

Source, docs and issues: https://github.com/lonkar-org/tmux-companion
