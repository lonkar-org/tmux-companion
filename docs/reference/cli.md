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
| `gst [PATH] [PANE_PID]` | Git status on its own. Same flags, plus `--no-cap` |
| `battery` | Percentage and icon |
| `net` | Bandwidth since the previous call |
| `clients SESSION_ATTACHED WINDOW_ACTIVE_CLIENTS` | How many other clients are attached |
| `sh-jobs PANE_PID` | Jobs stopped or running under a pane, per `[sh_jobs]` |
| `window -i INDEX [flags]` | One window's status. Driven by tmux format strings: `-c` current, `-n` name, `-w` path, `-p` process, `-s` start path, `-f` flags, `-P` pane count, `-A` pane index |
| `preview` | Sample segments in every style, locally, with no daemon |

`vim-bg PANE_PID` still works and is `sh-jobs` under its old name. It prints a
line saying so, and goes away after one release.

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

`--all` opens with no query, `--query` sets a different one, `--refresh`
rebuilds from tmux rather than using what the daemon holds, and `--print` lists
the rows instead of opening the picker.

The picker runs in this process rather than in the daemon, because a daemon has
no terminal.

## Themes

| Command | Does |
| --- | --- |
| `theme gen` | Report which themes need a different text colour or a more visible border |
| `theme gen --apply` | Write `@theme-color-on-main` and `@theme-color-border` into each theme file |
| `theme gen --shades` | Also mint a lighter and a darker sibling of each cube colour |

`--themes DIR` says where the files are, and `--background '#rrggbb'` gives the
terminal background to measure borders against. Without it, ghostty is asked
and the xterm default stands in when ghostty isn't there.

Reporting is the default and writing takes a flag, because a command that
rewrites 76 files on a bare invocation is one people run once by accident.

## Diagnostics

| Command | Does |
| --- | --- |
| `doctor` | The binary and its build, the daemon and its build, the socket with its mode and owner, the config in use, the glyph preset, the state directory, the tmux version and the platform |

Ask for `doctor` output on any bug report. It reads without starting or
replacing anything.
