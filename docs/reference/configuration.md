# Configuration

There is no config file until you write one, and you may never need to: every
setting has a default, and the defaults are what the binary did before the file
existed. A test holds that line rather than a promise, by parsing
`docs/config.example.toml` and asserting it equals the built-in defaults.

## Where the file goes

First one found wins:

1. `--config <path>`
2. `$TMUX_COMPANION_CONFIG`
3. `$XDG_CONFIG_HOME/tmux-companion/config.toml`, which is
   `~/.config/tmux-companion/config.toml` when that variable isn't set
4. `~/tmux-companion.toml`

```sh
tmux-companion config path    # which file is being read
tmux-companion config check   # parse it, say what is wrong, exit nonzero
tmux-companion config dump    # every setting with its default
```

`config dump` writes a valid config file, so `tmux-companion config dump >
~/.config/tmux-companion/config.toml` is a reasonable way to start.

## When it doesn't parse

The daemon refuses to start. That's the deliberate half; the other half is that
a refusal is only defensible if you find out why within seconds, so the error
goes three places:

- the whole thing to stderr, and to
  `$XDG_STATE_HOME/tmux-companion/last-error`
- one line into the status bar, from whichever client tried and failed to start
  a server, naming the command that explains it
- the whole thing again, with the line, the column and the key you probably
  meant, from `tmux-companion config check`

The bar gets one line because a status bar is about 150 columns wide and a
parse error doesn't fit in them.

An unrecognised key is an error rather than something ignored. A silently
dropped setting is the same bug as a typo'd argument reading back as `None`,
and it costs somebody an evening.

## The settings

Every key, its default and what it does is in
[`docs/config.example.toml`](../config.example.toml), which is the file to copy
from. A test asserts that every key `config dump` produces appears there, so a
setting can't exist without being written down.

The tables today are `[general]`, `[dirs.aliases]`, `[git]`, `[network]` and
`[battery]`. The port adds `[glyphs]`, `[status.right]`, `[[layout]]` and the
rest as each phase reaches them, and `docs/comrades-port.md` has the table
saying which phase brings which.

## tmux user options

There aren't any, with three exceptions planned for the settings that have to
differ per session: `@tmux-companion-theme`, `@tmux-companion-layout` and
`@tmux-companion-status-right`.

A general mapping of the config tree onto `@` options would cost a
`show-options` round trip per option per render, and would produce names like
`@tmux-companion-layout-window-2-command`, which is a config language built out
of hyphens by accident. Anything nested lives in the file and the option refers
to it by name.
