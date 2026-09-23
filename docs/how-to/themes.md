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

## Making one

One colour is enough:

```sh
tmux-companion theme list-colours          # every value tmux takes, painted
tmux-companion theme add --bg colour61 --name "Indigo"
tmux-companion theme gen --apply           # adds the border
```

`--bg` takes anything tmux does: a name, `colour0` to `colour255`, or
`#rrggbb`. The text colour is computed from it, and `--fg` overrides that when
you want to choose.

A pair under WCAG AA is refused rather than written, with the ratio in the
message. `--force` writes it anyway, since it's your terminal.

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

`@theme-color-main-1` is the only line that has to be yours, which is why
`theme add` needs one colour and not two. `theme gen
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
clear AA with room, and a test fails if an edit ever drops one under 5:1.

`theme gen --shades` is the rest of the palette: every colour in tmux's 6x6x6
cube whose text clears **WCAG AAA**, which is 7:1. That is 145 more on top of
the six, so 151 in all, each named after the bundled colour it sits nearest to
in cube space — `ember-04`, `pine-11` — so the warm ones group together when
you scroll the picker.

151 is more than some people want to scroll, so the flag takes a rung:

| | floor | themes |
| --- | --- | --- |
| `--shades aa` | 4.5:1 | 216 |
| `--shades aaa` | 7:1 | 151 |
| `--shades a4` | 9.5:1 | 105 |
| `--shades a5` | 12:1 | 75 |
| `--shades a6` | — | 18 |

`--shades` on its own is `aaa`. WCAG names the first two rungs and stops; the
rest carry on at its own spacing, 2.5 per step, so the ladder is one rule and
not four opinions. `a6` is not a floor at all — it is the bundled six with a
lighter and a darker sibling of each, which is what `--shades` did before it
swept the cube, and the only rung whose colours a person chose.

AAA and not AA, and that is not strictness for its own sake. `readable_on`
picks the better of black and white, and the worst colour in the whole cube
scores 4.60:1 on that basis, so filtering by AA keeps all 216 — a threshold
that reads like a filter and removes nothing. AAA is a real standard rather
than a number chosen to reach a pleasant count, and it lands at 148 of the 216
before the bundled six are taken out.

`theme gen` without `--apply` reports what it would change and touches nothing,
which is the way to see whether a palette you wrote yourself clears the bar:

```sh
tmux-companion theme gen
```
