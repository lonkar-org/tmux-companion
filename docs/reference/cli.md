# Command reference

Every subcommand, and what it takes. `tmux-companion --help` prints the same
list, and `tmux-companion <command> --help` the flags.

## The daemon

| Command | Does |
| --- | --- |
| `server` | Bind the socket and serve until killed. Started automatically by any client that finds nothing listening, so you rarely type it. |
| `shutdown` | Stop the daemon, and only that. tmux and its sessions are not touched; the next client starts a new daemon. `sessions shutdown` is the one that stops tmux |
| `restart` | Stop the daemon and start a fresh one. This is how a change to `config.toml` takes effect, since the file is read once at startup and held for the daemon's whole life. tmux is not touched; `sessions restart` is the one that restarts it |

A client that finds a daemon from an older build replaces it and says so; one
that finds a newer build leaves it alone and says so once. A development build
run beside the installed one wants its own socket, in `TMUX_COMPANION_SOCK`,
rather than the two taking turns replacing each other's daemon.

## Status segments

Each one prints tmux markup on stdout and exits.

| Command | Arguments |
| --- | --- |
| `status-right [PATH]` | The whole right-hand side in one call: git, bandwidth and battery, computed concurrently. `--style` is `fill`, `outline` or `outline-bright` and anything else is refused with that list; `--branch-max-len N` middle-ellipsizes a branch name longer than N, and without the flag `[git] branch_max_len` decides, 20 out of the box; `--branch-icon`, `--force`, `--ttl SECS` |
| `gst [PATH] [PANE_PID]` | Git status on its own. Same flags, plus `--no-cap`, `--no-daemon` and `--no-tmux` |

A daemon error exits the command 1 with the error on stderr. tmux ignores the
exit status of a `#()`, so the bar sees nothing different; a prompt or a script
can tell a failed segment from an empty one.
| `battery` | Percentage and icon |
| `net` | Bandwidth since the previous call. `--no-daemon`, `--no-tmux` |
| `clients SESSION_ATTACHED WINDOW_ACTIVE_CLIENTS` | How many other clients are attached |
| `sh-jobs PANE_PID` | Jobs stopped or running under a pane, per `[sh_jobs]` |
| `window -i INDEX [flags]` | One window's status. Driven by tmux format strings: `-c` current, `-n` name, `-w` path, `-p` process, `-s` start path, `-f` flags, `-P` pane count, `-A` pane index |
| `preview` | Sample segments in every style, locally, with no daemon |

`vim-bg PANE_PID` still works and is `sh-jobs` under its old name, and
`close-project` still works and is `project close`. Both print a line saying so,
and go away after one release.

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
| `config init` | Write a short starter config where `config path` would read it, creating the directory, and print the path. It sets the glyph preset, the directory source and the snapshot timer, and carries the editor-beside-an-agent layout as a comment to uncomment. A file already there is refused unless `--force` |

Every other client-side command reads the config too, and one that does not
parse is used as the defaults with one line on stderr saying so, once per
process, pointing at `config check`. The daemon is stricter and refuses to
start on it.

## Pickers

| Command | Does |
| --- | --- |
| `keys` | Searchable key bindings. Enter runs the binding, ctrl-a widens past the opening query to tmux's own, esc cancels. `--all` opens with no query, `--query TEXT` sets a different one from the default `custom: `, `--refresh` rebuilds from tmux rather than using what the daemon holds, and `--print` lists the rows instead of opening the picker |
| `cheatsheet` | The bindings you wrote, in four boxes, most-used first. Any key closes it, and `--print` prints and exits (`--plain` is the old spelling and still works) |
| `start [DIR]` | The way in from a shell that is not in tmux yet: the project picker, then attach. `--last` goes back to the session used most recently without asking; `DIR` skips the picker. Inside tmux it switches rather than attaching, so it is the same thing as `project`. `--hook` is for the `client-attached` hook in tmux.conf: it opens the picker only when the session is one tmux named itself, all digits with one window, one pane and a shell in it |
| `project [DIR]` | Switch to a project, or build its session from `[[layout]]`. With no argument it lists live sessions newest first, then the directories `[project] dirs_source` knows — zoxide by default, or `z`, `cdr`, `ghq`, a `dirs_command`, or none at all; `--print` lists and exits, opens nothing, and cannot be combined with a directory |
| `project save` | Capture this session's windows and panes as this project's layout. All or nothing: a line tmux cannot answer for leaves the saved layout untouched. `--no-commands` keeps the shape and leaves every pane a shell |
| `project forget [DIR]` | Delete this project's saved layout, so the config decides again. The project is the session this runs in, or `DIR` |
| `project show [DIR]` | Which layout this project gets, which file decided, and the windows it opens. A saved file that is there and not used is named with the reason |
| `project close [SESSION]` | Close this project by letting every window exit, capturing the layout on the way out. An editor (nvim, vim, vi, hx) is asked to quit first and the close stops with it on screen when it will not; `--discard` quits editors with `:qa!`, `--no-save` leaves the saved layout alone. A session that is not there is an error. `close-project` is the old name and works for one release |

`keys` and `cheatsheet` list the bindings whose `-N` note starts with
`custom: `, which is what the shipped configs write. When no binding carries
the note, both print one hint on stderr pointing at
`docs/tmux.conf.starter.example` and exit 0 rather than drawing nothing.

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
| `run [--pane ID]` | Pick a command from history and run it in a pane beside this one. Enter runs the pick, alt-enter runs exactly what you typed, `--print` lists and exits. `--pane` says which pane it belongs beside, for a caller that knows it. The binding passes nothing and the attached client's session decides, because the picker is a popup: a popup is not a client, an untargeted split lands in whichever session the server touched last, and tmux does not expand `#{pane_id}` in a `display-popup` command anyway |
| `toggle [SESSION]` | Move to the next window in this session by index, wrapping at the end. `--last` flips to the window the session was on before instead, tmux's own `last-window`, which is what a toggle means once there are more than two; a trailing `WINDOW` is accepted and ignored |
| `autosave` | Deprecated: the `[autosave]` script timer, which `[sessions] autosave` replaces. `--once` runs the script now, `--status` says when it last ran, and with no flag it reports like `--status` |
| `sessions save` | Capture every session on the server as a new generation. All or nothing, like `project save`. `--skip-pane-history` leaves out what was on each pane's screen; `--exclude a,b` adds to `[sessions] exclude` |
| `sessions resurrect [STAMP]` | Rebuild a server from a generation, newest by default, falling back to tmux-resurrect's own newest save when there is no generation of ours. Refuses a server that already holds sessions; `--merge` adds only what is missing. `--dry-run` prints the exact tmux commands, `--only`/`--exclude` pick, `--yes` runs everything the table claimed without asking, `--detach` leaves the server running. A snapshot from a newer build is read with one stderr line saying the restore may miss what that build knew. Exit codes 0/2/3/4/1 |
| `sessions autosave` | The snapshot timer the daemon runs. `--once` takes one now, the same one the timer takes; `--status`, or no flag at all, says when the last snapshot was, what the timer is set to, and whether the last daemon stopped cleanly |
| `sessions shutdown` | Save every session, then stop the tmux server. `--exclude a,b` is not "leave these alone" — the server takes every session with it either way, so an excluded one does not come back, and the command says which before it acts. The daemon keeps running unless `--daemon-too`. `--dry-run` says what it would do |
| `sessions restart` | The same, then bring the server back with what it had. Stops the daemon by default so `config.toml` is reread; `--keep-daemon` turns that off. With no server running it says to use `sessions resurrect` |
| `sessions list` | Every generation, newest first, with what each holds and whether it was taken at shutdown or while running. `--json` for a script |
| `sessions show [STAMP]` | What one generation holds, down to each pane's directory and command, defaulting to the newest. `--json` prints the snapshot itself |

The autosave loop itself runs in the daemon, so there's nothing to start and
nothing to keep from starting twice.

`sessions shutdown` and `sessions restart` refuse to run from inside tmux, and
there is no flag for it: stopping the server would take the pane they were typed
into, and nothing after that would run. Refusals, and "no tmux server running",
go to stderr; stdout carries only what the command did.

`project` and `sessions` answer different questions and neither replaces the
other. A project's layout is a catalogue entry: one file, overwritten when you
press the key, edited by hand, kept for good. A snapshot is a moment: one file
per capture, kept in generations, dropped oldest first. A session with no
project to be keyed on, a scratch session you named yourself, exists only in
the second.

When a restore does not know what a pane should run and somebody is watching, it
shows the list and counts down before going ahead — never on a count of panes,
only on what it is unsure of. `[sessions] confirm_secs` is the countdown, zero
never draws it, and any key stops the clock. A pane left unapproved still opens
in the right directory, at a prompt.

What a restore is allowed to run lives in `[[restore.program]]`, and it is
default deny: `match` is a regular expression against the whole saved command,
`command` is what runs with `{command}` and `{cwd}` filled in, and `run = false`
refuses a program outright. A command no row claims is shown and left alone,
because restoring is executing what your own machine recorded weeks ago.

Pane history is a directory of one file per pane beside the snapshot rather than
an archive, because this crate has no tar or gzip dependency and fourteen panes
come to 50 KB uncompressed against 16.7 KB gzipped. Twenty generations is a
megabyte either way. Both the snapshot and the history are written `0600` under
a `0700` directory, since the history is the text that was on your screen.

## Themes

| Command | Does |
| --- | --- |
| `theme pick` | Choose a theme and apply it to the session an attached client is on. `-r SESSION` remembers it for a session by name instead, one that need not exist yet; `-t TARGET` applies it somewhere specific, `--print` lists and exits |
| `theme apply SESSION` | Apply the theme that session should have, from the project map or the namespace rules. This is what the session-created hook calls. `-t TARGET` applies it somewhere other than the session |
| `theme apply --all` | Repaint every session with its own theme, which is what a `tmux.conf` reload needs: sourcing the file resets the global options a theme sets |
| `theme init` | Write six starter colours and the two files that apply them, into the themes directory this machine's tmux actually reads. Overwrites nothing |
| `theme add --bg C` | Write a theme from one colour. `--fg` chooses the text colour instead of computing it, `--name` names the file instead of the colour naming it, and a pair under AA is refused unless `--force` |
| `theme list-colours` | Every colour tmux takes, painted, with its hex and the contrast its text colour clears. `--print` is one name per line with no swatch, for piping (`--plain` is the old spelling and still works) |
| `theme gen` | Report which themes need a different text colour or a more visible border |
| `theme gen --apply` | Write `@theme-color-on-main` and `@theme-color-border` into each theme file |
| `theme gen --shades [LEVEL]` | Also write a theme for every cube colour whose text clears a contrast floor, named after the bundled one each sits nearest to. `aa` 216, `aaa` 151 (the default), `a4` 105, `a5` 75, `a6` the bundled six and their siblings, 18 |

`--themes DIR` says where the files are, and `--background '#rrggbb'` gives the
terminal background to measure borders against. Without it, `ghostty
+show-config` is asked and the xterm default of colour232 stands in when ghostty
isn't there.

Reporting is the default and writing takes a flag, because a command that
rewrites 76 files on a bare invocation is one people run once by accident.

## The rest

| Command | Does |
| --- | --- |
| `open [TEXT…]` | Open a URL or a `file:line:col` found in text. `--cursor-x` picks whatever is under that column, which is how the copy-mode binding needs nothing selected; `--pane` says which pane it is for; `-s` scans the tmux selection, `-d DIR` resolves a relative path against DIR rather than the pane's directory, `-i` (`--choose`) asks which `[[open.application]]` opens it, `-n` (`--dry-run`) prints what it would open |
| `new-window` | Pick a directory and open a window there, from the same source as `project`. The query starts on the pane's own directory, so the key then enter is "another window here"; any path can be typed in full, listed or not. Both the directory it starts on and the session the window lands in come from the attached client, not from tmux's current session, which inside a popup is whichever one the server touched last |
| `shell-init [SHELL]` | Print the shell code that emits the OSC 133 prompt marks, for zsh, bash or fish. Defaults to `$SHELL` |
| `clipboard` | Copy to the system clipboard, picking the command for the platform. `--stdin` reads standard input rather than the tmux buffer |
| `zen [--pane ID]` | Clear everything but this pane: a zoom when there are other panes, the status bar when there are not. `--pane` says which, and the binding passes it. `zoom` is the old name, kept one release |

## Diagnostics

| Command | Does |
| --- | --- |
| `doctor` | The binary and its build, the daemon and its build, the socket with its mode and owner, the config in use, the glyph preset, the state directory, the daemon log's last line, both autosave timers, the tmux version and the platform |
| `probe keys` | Show what the terminal sends for a key. `-n COUNT` stops after that many |
| `probe cells [STRING…]` | Ask how many cells the terminal advances for a string, or for a built-in set |

Ask for `doctor` output on any bug report. It reads without starting or
replacing anything. A daemon from another build, one that does not answer, and
a config file edited after the daemon started each end their line with `run
tmux-companion restart`, because that is the answer to all three.
