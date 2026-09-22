# Try it without installing anything

A container with tmux, the binary, the config with everything turned on, five
fake projects and a guided tour that walks through the bindings one at a time.
Nothing is mounted from your machine and nothing is published off it, so the
whole thing goes away with the container.

```sh
docker run --rm -it ghcr.io/lonkar-org/tmux-companion:playground
```

From a clone, `scripts/playground.sh` builds the image and runs it. The first
build compiles the crate inside the image and takes a few minutes; the rest
reuse the cargo layer.

```sh
scripts/playground.sh            # build if needed, then run
scripts/playground.sh shell      # a shell in the image, no tour
scripts/playground.sh smoke      # check the image is what the tour claims
```

## Run it outside tmux

If you start the container from inside a tmux session, `Ctrl-b` goes to that
server and the playground never sees it, so every binding in the tour looks
broken. Use a terminal that is not already in tmux.

Nested anyway, press the prefix twice: `Ctrl-b Ctrl-b ?` reaches the inner
tmux. The entrypoint says so and waits before attaching when it sees `$TMUX`.

## Option as Meta, on macOS

Two of the bindings are `Alt-s` and `Alt-a`. Terminal.app and iTerm2 send an
accented character for Option until they are told otherwise:

| | |
| --- | --- |
| Terminal.app | Settings, Profiles, Keyboard, "Use Option as Meta key" |
| iTerm2 | Settings, Profiles, Keys, Left Option key: `Esc+` |
| Ghostty | `macos-option-as-alt = true` |
| Alacritty, kitty, WezTerm | already send it |

The playground binds the same two to the prefix as well, so `prefix P` opens
the project picker and `prefix A` toggles, and nothing has to be configured
before the tour works. Those two are playground scaffolding; the shipped
config uses the Alt keys.

## What you land in

Two tmux sessions. You start in `playground`, a shell in
`~/projects/orchard-api`, and the tour waits in `instructions`.

```
prefix then i     the tour            (prefix is Ctrl-b)
Alt-s             the project picker
```

The tour is sixteen steps. Each one says what to press, sets the step on a
second status line so it is still in front of you after you have switched
sessions, and waits for Enter. Some steps check that the thing actually
happened and say so when it did not; `s` skips one and `q` leaves the tour for
a shell. `tour` starts it again.

## Fonts

This is the one thing the image cannot do for you. Glyphs are rendered by the
terminal on your machine, so a Nerd Font has to be installed and selected
there, not in the container. The first step of the tour prints four of them so
you can see whether yours works.

- [Nerd Fonts](https://www.nerdfonts.com/font-downloads), any of them
- [firacode-nfc-tweaked](https://github.com/lonkar-org/firacode-nfc-tweaked),
  the one this was built against

Everything works without the font. It reads worse.

## What is not there

A container has no battery, so that segment stays empty. The network counters
sit below the threshold the bandwidth segment draws at unless something is
transferring, so that one is usually empty too. Both are there on a laptop, and
`tmux-companion doctor` inside the container says what it can and cannot see.

The window segment is off, the same as in the bar the author runs: drawing
windows through `tmux-companion window` costs one process spawn per window per
redraw, and tmux redraws on pane output as well as on the timer.

## The five projects

Each one is in a different state, so the git segment has something different to
say in each:

| | |
| --- | --- |
| `orchard-api` | clean |
| `orchard-web` | three modified, one untracked |
| `sparrow-cli` | staged and modified at once, on a branch long enough to be truncated |
| `lantern-docs` | detached HEAD, one untracked file |
| `anvil-infra` | two commits ahead of its upstream |

`orchard-api/build.log` holds a compiler error with a path, a line and a
column in it, for the copy-mode `o` binding to open.

## Taking the config with you

Everything the playground runs is a file in the image:

| | |
| --- | --- |
| `~/.config/tmux/companion.conf` | `docs/tmux.conf.full.example`, unchanged |
| `~/.config/tmux/playground.conf` | the tour's scaffolding, not part of the tool |
| `~/.config/tmux-companion/config.toml` | the layout the projects open with |
| `/opt/playground/config.example.toml` | every option, annotated |

`docker cp <container>:/home/play/.config/tmux/companion.conf .` copies one
out, or read them in the container and copy what you want.

## After the playground

[Install](install.md) it properly, then [themes](themes.md).
