# Why the themes needed arithmetic

tmux can't do arithmetic on a colour, so every theme carried a main colour and
guessed at the rest, which is how twenty-one themes ended up painting dark text
on a dark block and seventeen ended up with an active pane border you could not
see against the terminal background they were actually being drawn on.
`tmux-companion theme gen` computes both.

## Dark text on a dark block

`_apply.tmux` painted the session name with `@theme-color-black` on
`@theme-color-main-1`, which is fine on amber and unreadable on indigo. Indigo
is colour60, or `(95, 95, 135)`: black on it measures **3.47:1**, and white
measures **6.05:1**. Twenty-one of the 76 themes were on the wrong side of
that.

Each theme now carries `@theme-color-on-main`, computed with the WCAG 2.1
relative-luminance formula and set to whichever of black or white reads better
against its own colour.

The interesting case is violet, colour98. It measures 4.65:1 on black against
4.52:1 on white, so it keeps black by a margin thin enough that a change in the
luminance code would flip it, which is why there's a test pinning both numbers.

## Black is not black

The first version wrote the words `black` and `white`, and 48 of 76 themes came
out below 4.5:1 anyway.

tmux resolves those words to palette entries 0 and 7, and a terminal theme is
free to redefine them. Ghostty's Birds of Paradise paints entry 0 as `#573d26`,
a mid brown, and entry 7 as `#e0dbb7`, a cream. Computing against `#000000` and
then writing `black` answered a question the terminal was never asked.

The fix is `colour16` and `colour231`, the same two endpoints in the
256-colour cube, which no theme remaps.

## Borders you can't see

WCAG 2.1 SC 1.4.11 asks 3.0:1 of anything carrying meaning by colour alone, and
an active pane border is exactly that. Seventeen of the dark themes were below
it: blue-dark's colour17 measured **1.13** against the terminal background,
which is a border that isn't there.

`@theme-color-border` walks the xterm cube one step lighter at a time until it
clears 3.0:1, rather than substituting a grey that passes the threshold and
tells you nothing about which session you are looking at, so blue-dark's border
comes out at colour67 and is still recognisably blue.

The terminal background is read from `ghostty +show-config` rather than
assumed, for the same reason as the black-is-not-black case, and the report
says which source it used.

## Shades

Ten projects were sharing nine distinct colours, with azure landing on two of
them. Stepping one level along each axis of the 6x6x6 cube gives a lighter and
a darker sibling per hue, which is enough separation to tell two sessions apart
in a status bar. A channel already at an end stays there, so a shade never
wraps into somebody else's hue.

## What the port changed

Nothing in the arithmetic. The Rust and the Python agree on all 76 themes, all
21 text-colour decisions, all 17 border lifts and every shade, which was
checked by diffing the two reports rather than by reading the code twice.

What it removes is the python3 dependency, and the need to remember that the
generator lives in a different repository from the themes it writes.

## Why `-t` exists on apply

`_apply.tmux` sets window options: `mode-style`, both pane border styles and
`clock-mode-colour`. A window option lands on one window, so applying a theme
without a target paints whichever window happened to be current and leaves
every other window in the session on the global default.

That's why copy-mode selection could be readable in one window and not the next
one along.
