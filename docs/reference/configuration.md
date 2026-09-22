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

The tables today are `[general]`, `[dirs.aliases]`, `[git]`, `[network]`,
`[battery]`, `[glyphs]` and `[status.right]`. The port adds `[[layout]]` and the rest as each phase reaches them, and `docs/comrades-port.md` has the table
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

## Glyphs, if your bar is a row of boxes

The default set is Nerd Fonts v3, whose codepoints sit in the private use area,
so a font without that patch draws boxes and a new reader can't tell whether
the install worked. One line fixes it:

```toml
[glyphs]
preset = "ascii"
```

`nerd-font-v3` and `ascii` ship today. A preset is a table of names to strings
in its own file, so adding `powerline` or `unicode` later is a data change with
no Rust in it, which also makes a preset about the easiest first patch anybody
could send.

Individual glyphs override the preset, by the constant name in
`src/tmux/icons.rs`:

```toml
[glyphs.icons]
STAGED = "*"
ARROW_RIGHT = ""
```

One icon missing from your font is a reason to replace that icon rather than to
drop to a whole preset below it.

How it works is worth knowing for one reason: the substitution is applied once
to a finished segment rather than threaded through the 263 places a glyph is
used, so a preset can only replace glyphs the default set already contains. It
can't add a glyph somewhere there wasn't one.

## Choosing what the git segment shows

`[git] parts` is an ordered list, and a part left out of it isn't drawn:

```toml
[git]
parts = ["branch", "state", "staged", "modified"]
```

Four hundred untracked build artifacts aren't information, and if you never
push then `ahead` and `behind` are two counts you'll never read. The names are
what a reader sees rather than what the code calls things, so it's `conflicts`
rather than `unmerged`.

The full vocabulary is in [`docs/config.example.toml`](../config.example.toml),
and an unknown name is a config error rather than a part that silently does
nothing.

Order is honoured between groups: branch info (`ahead`, `behind`,
`conflicts`), then the work-tree counts, then `staged`, then `stash`. Inside a
group it's fixed, because a group is a single colour run and reordering its
counters would move escape sequences rather than glyphs. `parts = []` renders
an almost empty segment, which is a legitimate thing to ask for and not a
crash.

## Building the right-hand side

`[[status.right.segments]]` is the list of segments and what goes in front of
each one:

```toml
[[status.right.segments]]
name = "git"
separator_before = ""

[[status.right.segments]]
name = "battery"
separator_before = " | "
```

That drops the bandwidth segment and puts a plain pipe before the battery.
`separator_before = ""` means nothing at all between two segments, which is a
preference nobody could express while the literals lived in `tmux.conf`.

`{NAME}` in a separator expands to the glyph of that name in
`src/tmux/icons.rs`, so the file stays readable in an editor with no patched
font, and a name that doesn't exist is left as you wrote it rather than
dropped, since a separator rendering `{ARROW_RIGH}` is a typo you can see.

A separator is drawn whether or not the segment after it rendered anything,
because that's what the `tmux.conf` literals did: a non-repo pane on a quiet
network still drew the wedge in front of an absent battery. And
`trailing_space` exists because tmux draws the right side flush to the terminal
edge, so without it the last glyph sits against the border.
