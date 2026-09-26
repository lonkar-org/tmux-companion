# Design: a pocket pane per session

Status: proposed, 2026-09-26. Queue item 8. Nothing here is built.

## The problem

A shell you reach for beside the editor, look at, and put away, without losing
it. `run` slides a pane out for one command and takes it down through a dialog
when the command exits; `zen` hides everything but the pane you're in. Neither
keeps a pane around between two presses of a key, so the scratch shell in a
project session is either a window that stays open or a split you make and
kill by hand each time.

`docs/dev/after-the-port.md` names `nickdiego/tmux-pocket-pane` as the plugin
that does this and the one I keep looking at.

## Today

What tmux gives us: `split-window` with the stepped `resize-pane` that `run`
already uses for the slide (`run::slide_steps`, `[run] slide_steps`),
`break-pane -d` to move a pane into a window of its own without switching to
it, `join-pane -s` to bring it back beside the current pane, and `select-pane
-T` to name it. All of it is in tmux 3.4, which is what CI runs.

## Options

1. **Park the pane in a hidden window, bring it back with `join-pane`.** One
   window named `_pocket` per session holds the parked panes; `pocket` joins
   the named one beside the current pane at `[run] width_percent`, sliding
   the way `run` does, and a second press breaks it back into `_pocket`. The
   process and its scrollback survive. This is what the plugin does.

2. **A popup over a detached session.** `display-popup -E` attaching a nested
   tmux to a session called `pocket/<session>`. Nothing in the layout moves,
   the shell survives closing the popup, and the popup can be any size. The
   cost is a tmux inside a tmux: its own status line has to be turned off in
   that session, the prefix inside it is the outer one's, and every snapshot
   would carry the `pocket/*` sessions unless `[sessions] exclude` learns
   about them.

3. **Kill and recreate.** A `split-window` on the first press and `kill-pane`
   on the second. Simple, and it loses the scrollback, which is most of the
   reason to want the pane back.

## Recommendation

Option 1, as `tmux-companion pocket [NAME]`, one window `_pocket` per
session, `NAME` defaulting to `shell`. The slide code is already written and
the pane stays a pane, which is what every other command here knows how to
count, capture and close. `_pocket` has to be known in a few places:
`project save` skips that window when it captures a layout, `project close`
sends its panes the same `exit` as any other, and the window picker and
`panes` show its panes with the window name so nothing is hidden by accident.

## What would change my mind

If joining and breaking a pane makes the editor beside it repaint badly, since
each step of the slide is a SIGWINCH to the neighbour, I'd take option 2 and
pay for the nested tmux. I haven't tried it with `nvim` under a 5-step slide,
which is the case that matters.

## Left open

More than one pocket per session at a time, what `zen` does to a pocket pane
that's open, and whether a pocket should come back after `sessions resurrect`
as a parked pane or as nothing.
