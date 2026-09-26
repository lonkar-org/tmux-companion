# The first hour

From nothing installed to a bar you can see and a picker you've pressed. It
assumes tmux is already on the machine and that you've got a Nerd Font in the
terminal, or don't mind a row of boxes until you set `[glyphs] preset =
"ascii"` in the config.

## Install the binary

```sh
curl -fsSL https://raw.githubusercontent.com/lonkar-org/tmux-companion/main/scripts/install.sh | bash
```

It picks the archive for this machine, checks it against the release's
`checksums.txt`, and puts the binary in `/usr/local/bin` when that's writable
and `~/.local/bin` otherwise. Homebrew, tpm and building from source are in
[how-to/install.md](../how-to/install.md) if you'd rather any of those.

The binary goes in first and the tmux config second, because the bar calls
`tmux-companion status-right`, and a config that names a binary which isn't
there yet leaves the right-hand side blank and nothing says why.

## Ask it what it can see

```sh
tmux-companion doctor
```

That's the report a bug needs, and on a fresh install it's the quickest way to
find out whether the binary can see tmux, which config it's going to read and
which glyph preset it's drawing with.

## Copy the starter config

[`docs/tmux.conf.starter.example`](../tmux.conf.starter.example) is the file to
copy first: the bar and the eight bindings worth having on day one, nothing
else. Take it whole into `~/.config/tmux/tmux.conf`, or lift the lines you want
into what you already have, then reload:

```sh
tmux source-file ~/.config/tmux/tmux.conf
```

Two lines in it aren't optional. `status-style` is the background the segments
draw against, and without it tmux's default green shows through everywhere
they don't reach, and `status-right` is one `#()` for the whole right side,
git status, bandwidth and battery computed together in the daemon, so don't
split it into three, since tmux spawns a process per `#()` per second per
attached client and that spawn is the whole cost of the bar.

If the right side is still blank after the reload, the binary isn't on the
`PATH` tmux started with. `doctor` again from inside tmux says which one it
found.

## Press M-s

`Alt-s` opens the project picker: live sessions first, then every directory
zoxide knows, or whichever jumper `[project] dirs_source` names, and with none
installed it lists sessions and opens whatever path you type. Picking a
directory builds a session from `[[layout]]` in the config, and with no layout
written you get a plain shell in that directory, which is the shipped default.

`Alt-a` moves to the next window in the session, wrapping at the end. Both sit
in the root table, so they fire without the prefix and take those two keys away
from every program in the pane; drop the `-n` on either binding to put it
behind the prefix instead.

From a shell that isn't in tmux yet, `tmux-companion start` opens the same
picker and attaches to what you choose. It's worth an alias, because it's the
thing you type instead of `tmux`.

## What keys and cheatsheet need from you

`prefix ?` searches every binding and `prefix C-c` lays them out as a cheat
sheet, and both show only the bindings that carry a note:

```tmux
bind -N "custom: window new one here, or at any directory" c \
  display-popup -E -w 65% -h 65% "tmux-companion new-window"
```

The word after `custom:` picks the box on the cheat sheet, one of `pane`,
`window`, `session`, `project`, `go`, `copy`, `open`, `search`, `config` and
`help`, and the rest of the note is what the row says. A binding without the
note still works as a key and is invisible to both, so when your own bindings
don't show up, that's the reason, and the fix is a `-N` on each one.

## Later

`prefix c` opens a window here or at any directory, `prefix e` runs a command
from shell history in a pane beside this one, `prefix z` clears everything but
the pane you're in, and `prefix X` closes the project cleanly, capturing its
layout on the way out.

`tmux-companion theme init` writes the colour themes, and the starter file has
the `prefix C-t` binding and the `session-created` hook commented out under
"Later", ready to uncomment once there are themes on disk. `[sessions]
autosave` in `config.toml` snapshots every session on a timer and
`tmux-companion sessions resurrect` brings them back after a reboot.
[`docs/tmux.conf.full.example`](../tmux.conf.full.example) has the keys for
those and everything else, with what each one costs. After an upgrade, run
`tmux-companion restart`, since the daemon is one long-lived process and a new
binary on disk changes nothing until it's restarted.
