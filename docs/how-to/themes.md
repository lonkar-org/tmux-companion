# Themes

tmux-companion is not a theme pack. It ships the machinery, six colours to
start from, and the contrast maths; the palette is yours.

## Getting some

```sh
tmux-companion theme init
tmux-companion theme gen --apply --shades
```

The first writes six base colours and the two files that apply them. The second
turns those six into eighteen, by minting a lighter and a darker sibling of
each, and computes a text colour and a border for every one of them.

Nothing is overwritten. Run `theme init` beside themes you already wrote and it
writes only what's missing.

## Where they go

Beside whichever tmux config this machine actually uses, since tmux looks for
`$XDG_CONFIG_HOME/tmux/tmux.conf` before `~/.tmux.conf`:

| If you have | Themes land in |
| --- | --- |
| `~/.config/tmux/tmux.conf` | `~/.config/tmux/themes` |
| `~/.tmux.conf` | `~/.tmux/themes` |
| neither | `~/.config/tmux/themes`, which is what tmux 3.1 and later prefer |

`--themes DIR` overrides it, and the generated files carry the resolved path in
their `source-file` lines rather than a guess, so a theme written on one layout
keeps working on the other.

## What a theme is

```tmux
source-file "~/.config/tmux/themes/_reset.tmux"

set @theme-name         "Ember"
set @theme-color-main-1 colour208
set @theme-color-on-main  colour16

source-file "~/.config/tmux/themes/_apply.tmux"
```

`@theme-color-main-1` is the only line that has to be yours. `theme gen
--apply` fills in `@theme-color-on-main`, the text colour that reads on that
block, and `@theme-color-border`, a colour with the same hue that stays visible
against your terminal's background.

`_reset.tmux` puts the themed options back to neutral first, so a theme that
leaves one out doesn't inherit it from whichever theme ran before.

`_apply.tmux` decides what a theme actually changes: the session name block,
the active pane border, the current window, tmux's messages and copy mode. It
ships commented, one block per thing, because that file is the opinionated part
and somebody who wants less should be able to delete lines and see what they
lost.

## Contrast

Text on a theme's own block clears **WCAG 2.1 AA**, which is 4.5:1 for body
text. Borders clear 3:1, the AA threshold for a user interface component,
measured against your real terminal background rather than an assumed one:
`theme gen` reads it from `ghostty +show-config`, and `--background '#rrggbb'`
supplies it anywhere else.

The six bundled colours were chosen so that the base and both of its shades
clear AA with room. The weakest of the eighteen is 5.53:1, and a test fails if
an edit ever drops one under 5:1, since `readable_on` picks the better of black
and white and the worst case for a cube colour lands at about 4.5:1 exactly.
Passing on paper and reading badly is the thing worth guarding against.

`theme gen` without `--apply` reports what it would change and touches nothing,
which is the way to see whether a palette you wrote yourself clears the bar:

```sh
tmux-companion theme gen
```
