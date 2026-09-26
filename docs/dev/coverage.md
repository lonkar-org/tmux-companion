> Planning record from the port, kept for history; the current behaviour is in the reference docs.

# What it covers, and what it does not

Checked on 2026-09-22 against `~/.config/tmux/tmux.conf` and the 14 scripts in
`~/.config/tmux/comrades`, by reading each binding and running its replacement.

The port claimed to be complete before this page existed. It wasn't, and the
holes were found one at a time by somebody using it rather than by the
checklist, which is the argument for writing the comparison down instead of
trusting a tick.

## Script by script

| Script | Command | State |
| --- | --- | --- |
| `keys.zsh` | `keys` | done |
| `cheatsheet.zsh` | `cheatsheet` | done |
| `project-session.zsh` | `project` | done |
| `project-preview.zsh` | the picker's preview | restored after being dropped |
| `zoxide-window.zsh` | `new-window` | restored after being dropped |
| `run-command-pane.zsh` | `run` | done |
| `open-from-text.zsh` | `open` | partial, see below |
| `choose-tmux-theme.zsh` | `theme pick`, `theme apply` | done |
| `preview-tmux-theme.zsh` | the theme picker's swatches | deliberately smaller |
| `reapply-themes.zsh` | `theme apply` | partial, see below |
| `close-project.zsh` | `project close` | done |
| `toggle-tool.zsh` | `toggle` | done |
| `session-autosave.zsh` | `autosave` | done, as a daemon task |
| `short-path.zsh` | `project::short_path` | done, as a library |

## What a binding still has no answer for

- **`open -l`**, scanning the copy-mode cursor line rather than the word under
  the cursor or the selection. Two of the bindings use it.
- **`open -i`** was on this list, and has since shipped as `open -i`, or
  `--choose`, which asks which `[[open.application]]` row opens the thing
  instead of deciding.
- **Repainting every live session** was on this list too. `theme apply --all`
  now walks every session, which is what `prefix + r` needs: sourcing
  `tmux.conf` cannot repaint a session that already exists, because the colours
  are handed out by the `session-created` hook and that does not fire again.

## Deliberately smaller

- The theme picker draws a swatch and the settings beside each theme where
  `preview-tmux-theme.zsh` drew a mock status bar. Enough to choose by, and not
  the same thing.
- `probe keys` reports what crossterm decided a keypress was, not the raw
  bytes, because by the time the code runs the parse has happened.

## What it does that no script did

`doctor`, `config`, `shell-init`, `clipboard`, `zen`, the combined
`status-right`, and the four background tasks. None of these replace anything;
they exist because a resident process can do them cheaply.

## What now checks it

`tests/e2e.rs` drives a real tmux on its own socket and asserts behaviour
rather than values, because every one of the holes above passed all 639 unit
tests. A command that was never wired to a subcommand, a footer promising a key
nobody implemented, a config line that blanked the bar, a dialog that answered
itself: none of them is a wrong value, so none of them was catchable by asking
a function what it returned.

It runs the `#()` calls out of the shipped example config, checks every
subcommand the config names actually exists, and holds a `run` pane open long
enough to prove the dialog is waiting. Both of the bugs it was written for were
put back to confirm it fails on them, since a regression test that does not
fail on the regression is furniture.

## How the comparison was made

Reading the config for every `bind` that calls a script, then running the
replacement and comparing what came back. What it cannot check is the thing
that actually bit: a binding whose script has no command at all reads as
"ported" in a checklist and as nothing at all under the key. The only test for
that is somebody pressing the key.
