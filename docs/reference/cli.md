# Command reference

Every subcommand, and what it takes. `tmux-companion --help` prints the same
list, and `tmux-companion <command> --help` the flags.

## The daemon

| Command | Does |
| --- | --- |
| `server` | Bind the socket and serve until killed. Started automatically by any client that finds nothing listening, so you rarely type it. |

## Status segments

Each one prints tmux markup on stdout and exits.

| Command | Arguments |
| --- | --- |
| `status-right [PATH]` | The whole right-hand side in one call: git, bandwidth and battery, computed concurrently. `--style`, `--branch-max-len`, `--branch-icon`, `--force`, `--ttl` |
| `gst [PATH] [PANE_PID]` | Git status on its own. Same flags, plus `--no-cap`, `--no-daemon` and `--no-tmux` |
| `battery` | Percentage and icon |
| `net` | Bandwidth since the previous call. `--no-daemon`, `--no-tmux` |
| `clients SESSION_ATTACHED WINDOW_ACTIVE_CLIENTS` | How many other clients are attached |
| `sh-jobs PANE_PID` | Jobs stopped or running under a pane, per `[sh_jobs]` |
| `window -i INDEX [flags]` | One window's status. Driven by tmux format strings: `-c` current, `-n` name, `-w` path, `-p` process, `-s` start path, `-f` flags, `-P` pane count, `-A` pane index |
| `preview` | Sample segments in every style, locally, with no daemon |

`vim-bg PANE_PID` still works and is `sh-jobs` under its old name. It prints a
line saying so, and goes away after one release.

### Using `gst` and `net` outside tmux

Both print a string and neither needs tmux to be running, so they work in a
shell prompt, in a bar that takes a command, or in a script. `--no-tmux` writes
ANSI escapes instead of tmux's `#[fg=...]` markup and resets the terminal at the
end, and a segment that drew nothing prints nothing at all, not even a newline.

```sh
PS1='$(tmux-companion gst --no-daemon --no-tmux) $ '
```

`--no-daemon` computes the answer in that one process and exits, with no socket
opened and no server started. What it costs is the cache: every call pays for a
cold `git status`, which was 51 ms against a large tree, where the daemon
answers a warm one in well under a millisecond. That's the right trade for a
prompt you press enter on and the wrong one for a bar redrawing once a second,
so inside tmux leave both flags off.

`net` is a rate and needs two counter readings, so with no daemon holding the
first one it goes in `$XDG_STATE_HOME/tmux-companion/net-sample`. The first call
after a reboot records the reading and draws nothing, which is exactly what the
daemon does on its own first call.

The two flags are independent. `--no-tmux` on its own still asks the daemon and
is the cheap way to put a segment in a prompt on a machine where tmux is running
anyway, and `--no-daemon` on its own prints tmux markup for a `#()` in a config
on a machine where you would rather not have a resident process.

## Configuration

| Command | Does |
| --- | --- |
| `config path` | Print which config file is being read |
| `config check [PATH]` | Parse it, report what's wrong, exit nonzero if it is |
| `config dump` | Print every setting with its default, as a config file |

## Pickers

| Command | Does |
| --- | --- |
| `keys` | Searchable key bindings. Enter runs the binding, ctrl-a widens past the opening query to tmux's own, esc cancels |
| `cheatsheet` | The bindings you wrote, in four boxes, most-used first. Any key closes it, and `--plain` prints and exits |
| `start [DIR]` | The way in from a shell that is not in tmux yet: the project picker, then attach. `--last` goes back to the session used most recently without asking; `DIR` skips the picker. Inside tmux it switches rather than attaching, so it is the same thing as `project` |
| `project [DIR]` | Switch to a project, or build its session from `[[layout]]`. With no argument it lists live sessions newest first, then the directories `[project] dirs_source` knows — zoxide by default, or `z`, `cdr`, `ghq`, a `dirs_command`, or none at all; `--print` lists and exits |
| `project save` | Capture this session's windows and panes as this project's layout. All or nothing: a line tmux cannot answer for leaves the saved layout untouched. `--no-commands` keeps the shape and leaves every pane a shell |
| `project forget` | Delete this project's saved layout, so the config decides again |
| `project show` | Which layout this project gets, which file decided, and the windows it opens |

`--all` opens with no query, `--query` sets a different one, `--refresh`
rebuilds from tmux rather than using what the daemon holds, and `--print` lists
the rows instead of opening the picker.

A saved layout wins over `[[layout]]`, because somebody pressed a key to make
it and the config is what they had before they did. `project show` is the way to
find out which one is in force without opening a session to see.

Run `project save` from a binding rather than by typing it into a pane. Typed,
the pane it runs in is running `tmux-companion` at the moment it looks, so that
is the command it records for that pane.

A directory whose name is `save`, `forget` or `show` has to be written as a
path, `project ./save`, because a bare one reads as the subcommand.

The picker runs in this process rather than in the daemon, because a daemon has
no terminal.

## Sessions

| Command | Does |
| --- | --- |
| `run` | Pick a command from history and run it in a pane beside this one. Enter runs the pick, alt-enter runs exactly what you typed, `--print` lists and exits |
| `toggle [SESSION] [WINDOW]` | Move to the next window in this session's layout, falling back to tmux's last-window when the current window isn't in one |
| `autosave --once` | Save the session list now |
| `autosave --status` | Say when the last save happened |

The autosave loop itself runs in the daemon, so there's nothing to start and
nothing to keep from starting twice.

## Themes

| Command | Does |
| --- | --- |
| `theme pick` | Choose a theme and apply it. `-r SESSION` remembers it for a session instead, `-t TARGET` applies it somewhere specific, `--print` lists and exits |
| `theme apply SESSION` | Apply the theme that session should have, from the project map or the namespace rules. This is what the session-created hook calls |
| `theme init` | Write six starter colours and the two files that apply them, into the themes directory this machine's tmux actually reads. Overwrites nothing |
| `theme add --bg C` | Write a theme from one colour. `--fg` chooses the text colour instead of computing it, and a pair under AA is refused unless `--force` |
| `theme list-colours` | Every colour tmux takes, painted, with its hex and the contrast its text colour clears |
| `theme gen` | Report which themes need a different text colour or a more visible border |
| `theme gen --apply` | Write `@theme-color-on-main` and `@theme-color-border` into each theme file |
| `theme gen --shades [LEVEL]` | Also write a theme for every cube colour whose text clears a contrast floor, named after the bundled one each sits nearest to. `aa` 216, `aaa` 151 (the default), `a4` 105, `a5` 75, `a6` the bundled six and their siblings, 18 |

`--themes DIR` says where the files are, and `--background '#rrggbb'` gives the
terminal background to measure borders against. Without it, ghostty is asked
and the xterm default stands in when ghostty isn't there.

Reporting is the default and writing takes a flag, because a command that
rewrites 76 files on a bare invocation is one people run once by accident.

## The rest

| Command | Does |
| --- | --- |
| `open [TEXT…]` | Open a URL or a `file:line:col` found in text. `--cursor-x` picks whatever is under that column, which is how the copy-mode binding needs nothing selected; `--pane` says which pane it is for; `-s` scans the tmux selection, `-n` prints what it would open |
| `new-window` | Pick a directory and open a window there, from the same source as `project`. The query starts on the pane's own directory, so the key then enter is "another window here"; any path can be typed in full, listed or not |
| `shell-init [SHELL]` | Print the shell code that emits the OSC 133 prompt marks, for zsh, bash or fish. Defaults to `$SHELL` |
| `close-project [SESSION]` | Capture the layout, then let every window exit on its own rather than killing the session. `--discard` quits editors with `:qa!`, `--no-save` closes without capturing |
| `clipboard` | Copy to the system clipboard, picking the command for the platform |
| `zen [--pane ID]` | Clear everything but this pane: a zoom when there are other panes, the status bar when there are not. `--pane` says which, and the binding passes it. `zoom` is the old name, kept one release |
| `probe keys` | Show what the terminal sends for a key |
| `probe cells` | Ask how many cells the terminal advances for a string |

## Diagnostics

| Command | Does |
| --- | --- |
| `doctor` | The binary and its build, the daemon and its build, the socket with its mode and owner, the config in use, the glyph preset, the state directory, the tmux version and the platform |

Ask for `doctor` output on any bug report. It reads without starting or
replacing anything.
