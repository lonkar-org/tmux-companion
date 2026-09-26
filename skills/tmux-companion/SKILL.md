---
name: tmux-companion
description: Drive tmux through tmux-companion instead of raw tmux - project sessions and layouts, snapshots and resurrect, shutdown and restart, the restore.program table, finding and starting agent panes, and working alongside a human in the same tmux server. Trigger on tmux layout, project session, restoring panes after a reboot, claude --continue, sessions save/list/resurrect, running several tasks in parallel in tmux, or locating which pane an agent is in.
---

# tmux-companion

You are a guest in a human's tmux server. Companion owns sessions, layouts and
restore. Your harness owns one-shot commands: git, tests, builds. Do not replace
the harness with `send-keys`, and do not start a second tmux server.

Everything below is the shape of the tool, not its flag list. The binary is the
reference for flags:

```sh
tmux-companion --help
tmux-companion sessions resurrect --help
man tmux-companion
```

If a subcommand named here is missing from the binary in front of you, say so and
stop. Do not reach for `tmux new-session` as a substitute.

## Preflight

```sh
command -v tmux-companion || echo "not installed"
tmux-companion doctor          # binary, daemon, socket, config, state dir, tmux version
tmux-companion config path     # which file is in force
```

`doctor` starts nothing and replaces nothing, and it is the first thing to paste
into a bug report. Any client auto-starts the daemon when the socket is absent,
so `server` is not something you type.

State lives in `$XDG_STATE_HOME/tmux-companion`, or `~/.local/state/tmux-companion`:
`projects/` holds one file per project, percent-encoded from the path, and
`sessions/` holds one `<stamp>.toml` per snapshot with a `<stamp>.panes` directory
beside it.

## Do not

- `tmux new-session`, `tmux kill-server`, `tmux kill-session` unless the human
  asked for that state to go away
- Nest tmux inside tmux
- Type `project save` or `sessions save` in a pane that should be captured. That
  pane is running `tmux-companion` at the moment it looks, so that is what gets
  recorded. Save from a binding, from another pane, or `--exclude` that session
- Restore by replaying arbitrary saved command lines yourself. Unknown commands
  stay unrun, on purpose
- Treat `pane_id` (`%12`) as stable across a server restart. The key is session
  name plus window index plus pane index
- Create a session named `0`
- Run `sessions resurrect` against a live server without `--merge` or `--only`
  and a human asking for it
- Confuse `tmux-companion shutdown`, which stops the daemon only, with
  `sessions shutdown`, which saves and then stops the tmux server
- Open `run`, `keys`, `cheatsheet`, `start`, `new-window`, `project` with no
  argument or `theme pick` from an agent loop. They are pickers, they draw a
  full-screen UI and they wait for a human keypress. Most of them take `--print`,
  which is your version; `new-window` takes nothing, so from a script use
  `tmux new-window -c DIR`
- `send-keys` into a pane the human is looking at, or `switch-client` and
  `select-window` to drag their focus somewhere. Your own pane is the harness

## Two stores

|  | `project` | `sessions` |
| --- | --- | --- |
| Question | what does this project look like | what was I doing at 09:23 |
| Key | directory path | timestamp |
| Copies | one, overwritten | `keep` generations plus `last` |
| Edited by hand | yes | no |
| Scope | one session | every session on the server |
| Path | `projects/<encoded path>.toml` | `sessions/<stamp>.toml` |

Restore reads the snapshot first and falls back per session to the catalog.
`project forget` deletes a catalog entry and leaves every snapshot alone.

A session like `y`, one window, a bare shell, no project directory, lives only in
`sessions`. Do not try to key it as a project.

## Where you are

```sh
tmux-companion project show           # which layout this project gets, and which file decided
tmux-companion project --print        # live sessions, then known directories
tmux-companion sessions list --json
tmux-companion sessions show --json   # the newest snapshot, pane by pane
tmux display-message -p '#S #{session_path} #{pane_current_path} #{pane_current_command}'
tmux list-panes -a -F '#S:#I.#P #{pane_current_path} #{pane_current_command} #{pane_title}'
tmux capture-pane -p -t SESSION:WIN.PANE
```

`#{pane_start_command}` on a mac is often `reattach-to-user-namespace -l $SHELL`.
That is the wrapper, not the program. Grade a match against it as `Shell`, not
`Exact`, and read the running program from `#{pane_current_command}` or the
process tree.

If `TMUX` is unset you are outside the server:

```sh
tmux-companion start            # the picker, then attach
tmux-companion start --last     # back to the session used most recently
tmux-companion start ~/path     # straight to a project
```

Inside tmux, `tmux-companion project [DIR]` switches to that project or builds it
from the saved layout, and a saved layout wins over `[[layout]]` in the config.

## Working in parallel with a human

The unit of parallel work is a window, not a session. One session is one project,
one window is one task inside it, and panes inside that window are the editor, the
agent and a shell watching something.

1. Find the project session first: `project --print`, or `list-panes -a` matched on
   `#{pane_current_path}`. `panes --print` lists every pane as
   `session:window.pane`, program, state, directory and pane id, one per line,
   and `panes --agents --print` only the panes running a program in
   `[agents] programs`; that is how you find where the other agents are and
   whether one is `busy` or `waiting 3m`. Work there. A second session for a
   repo that already has one splits the human's attention and breaks
   `project show`.
2. One agent per repository unless the human asked for more. Two agents in one
   directory is the case where `claude --continue` picks the wrong conversation.
3. A task that is genuinely separate gets its own window. Name it after the task,
   `tmux rename-window -t SESSION:IDX rebuild-cache`, so the human reading the
   status bar knows which window is yours without switching to it.
4. Name the pane too when a window holds more than one: `tmux select-pane -t ... -T 'claude: cache'`.
5. Long jobs belong in a pane the human is not watching. `[notify]` announces a
   command that ran past `threshold_secs`, 30 by default, and finished in a pane
   nobody was looking at, through tmux's own `display-message` unless a `command`
   is configured. It is `enabled = false` out of the box, so check
   `config dump | grep -A12 '\[notify\]'` before telling the human a build will
   announce itself, and remember `ignore` already holds `claude`, `codex`,
   `gemini`, the editors and the pagers, because finishing a two-hour agent is not
   news.
6. `[project] preview_window` names the window the project picker previews for a
   live session. It is empty by default, which previews whichever window the
   session is on. A human running the editor-beside-an-agent layout sets it to
   `ai`, and that window is how they check on you without switching, so keep
   the agent in the window the layout gave it.
7. An agent renames its own window to its version string, `2.1.278`, within
   seconds. `hold_name = true` on a `[[layout.window]]` pins the name. If the
   human's layout does not have it, say so rather than renaming the window on every
   pass.
8. Themes carry the project, not the task: `theme apply SESSION` paints a session
   from the project map. Leave it alone unless asked.

## Starting or resuming an agent

```sh
tmux list-panes -a -F '#S:#I.#P #{pane_current_path} #{pane_current_command}' |
  grep -E ' (claude|codex|gemini|cursor-agent|aider|opencode)$'
```

That list is the one `sessions` itself counts for its restore headline, so it is
the same definition of "an agent is already here".

1. Already running in this repo: use that pane. Do not start a second copy.
2. Otherwise split the project window, or `tmux-companion new-window` when the task
   deserves its own. The `new-window` query starts on the pane's own directory, so
   for a human the key and then enter is "another window here"; from a script,
   prefer plain `tmux new-window -c DIR`, since the picker wants a terminal.
3. Launch the agent. `claude --continue` picks the most recent conversation in that
   directory. `claude --resume <uuid>` is for the conversation the human named.
4. After a resurrect, a pane may come back as a shell with its old command sitting
   in the snapshot unrun. Re-run only what the restore table claims.

## sessions save

Captures the whole server into `sessions/<stamp>.toml`, plus a `.panes` directory
holding what was on each screen. It writes to a temp file and renames, and the
`last` pointer moves only after the rename lands. It is all or nothing: a line
tmux cannot answer for leaves the previous generation untouched.

- `--skip-pane-history` records the metadata only, the commands, directories and
  layout. 1 ms for a whole server against 9.3 ms per pane for the screens
- `--exclude a,b` adds to `[sessions] exclude` rather than replacing it

The daemon's own timer is `[sessions] autosave`, which is `off`, `interval` or
`cron`, and it is `off` out of the box. `interval_secs` defaults to 900 with a
floor of 10, and a number under the floor is an error rather than something
rounded up quietly. Generations are counted, not timed, so `interval_secs = 10`
with the default `keep = 20` holds about 200 seconds of history. If the human asks
for a fast loop, say that before setting it, and point at `keep_days`.

```sh
tmux-companion sessions autosave --status   # last snapshot, the timer, clean or crash
tmux-companion sessions autosave --once     # take one now
```

`--once` is a nudge to the daemon's timer, not a second loop. The older top-level
`autosave --once` and `--status` are the same family under the old name and go
away after a release.

## sessions resurrect

```
refuse if a server already has sessions
start a server, create a bootstrap session
read the newest snapshot, or STAMP
per session: the snapshot first, the catalog for what the snapshot lacks
build sessions, windows, panes, layouts, directories, zoom
replay pane history
classify panes: matched, guessed, unknown, denied
show the summary and count down, unless --yes or --dry-run
run the approved commands
kill the bootstrap session last
attach, unless --detach or there is no tty
```

Commands run after the geometry exists, never before, and killing the bootstrap
session early takes the server down with it.

Against a live server the default is to refuse:

```
4 sessions already running: mysetup, lekhani, y, tmux-companion
nothing restored. --merge adds the missing ones, --only picks some
```

- `--merge` adds sessions whose names are not live, touches nothing running, and
  attaches nothing
- `--only a,b` and `--exclude a,b`
- `--dry-run` prints the exact tmux commands and runs none of them. This is the
  flag to reach for before anything else
- `--yes` runs everything the table claimed without asking
- `--detach` leaves the server running rather than attaching

Exit codes for an unattended run: `0` restored, `2` bad arguments, `3` refused
because sessions are live, `4` nothing to restore, `1` everything else.

The summary opens only when the restore does not know something: a pane no
`[[restore.program]]` row claims, one a row refused, or a command read off a
running process with its arguments already gone. A restore where everything is
known goes straight through, however many panes it holds. `[sessions] confirm_secs`
is the countdown, 5 by default, any key stops the clock, and zero is `--yes` made
permanent. A pane nobody approved still opens in the right directory at a prompt.

The snapshot header records clean shutdown against crash, the time, the tmux
version, the companion version and the hostname. A directory that no longer exists
on this machine opens at `$HOME` and has to be mentioned, not swallowed. Paths are
not rewritten across machines.

With no companion snapshot at all, resurrect reads tmux-resurrect's own
tab-separated save. Nobody needs to keep `@resurrect-processes` after that.

## What a restore is allowed to run

Default deny. `match` is a regular expression against the whole saved command
line, not the process name, and `command` is what runs, with `{command}` and
`{cwd}` filled in. `run = false` refuses a program outright. A command no row
claims is captured, shown, and left at a prompt.

```toml
[[restore.program]]
match = "^claude( |$)"
command = "{command}"

[[restore.program]]
match = "^(codex|gemini|cursor-agent|aider|opencode)( |$)"
command = "{command}"
```

Those are two of the seven rows that ship; the other five bring back vim and
nvim as a bare `nvim`, and replay lazygit, tig and gitui, htop, top, btop and
watch, tail, less and journalctl, and ssh as they were saved. `{command}` in
the agent rows is the point: an agent keeps which conversation it is in inside
its own arguments, so replaying them verbatim is what brings the conversation
back, and a bare `claude` stays bare. `claude --continue` in that row is the other choice, and it trades one
failure for another. It survives a stale snapshot, and it picks the wrong
conversation when two agents were running in one repository. Say which one the
human's config has before promising either behaviour.

nvim buffers are not tmux's job. `nvim` is the restore command; `:mksession` and
session plugins are the human's.

## Shutdown and restart

`--exclude` on save and resurrect means "do not record this" and "do not restore
this". On `sessions shutdown` it means the session is not saved **and** stopping
the server takes it anyway, so it does not come back. The command prints that
before it acts:

```
not saving: y (excluded) -- it will not come back
saving: icf-c_com, lekhani, lonkar_org, mysetup, tmux-companion, yogesh_lonkar_org
```

`sessions restart` is save, stop the server, bounce the daemon, start the server,
restore, attach. The daemon bounce is the default because `config.toml` is read
once at daemon start; `--keep-daemon` opts out. `sessions shutdown --daemon-too`
stops the daemon as well, which is the opposite default on purpose.

Both refuse to run from inside tmux and there is no flag for it: stopping the
server would take the pane they were typed into, and nothing after that would run.
Both take `--dry-run`.

`tmux-companion shutdown` and `tmux-companion restart` never touch the tmux
server. `restart` is how a config change takes effect.

## Prompt marks

`tmux-companion shell-init [SHELL]` prints the OSC 133 hooks for zsh, bash or
fish. With marks in place, wait for the last prompt before sending keys. Without
them, a short sleep, not a poll loop.

## Secrets

Pane history is the text that was on the screen, so it holds whatever got printed:
a token echoed, a `.env` catted. The snapshot directory is `0700` and every file
in it `0600`. `[sessions] pane_history = false` keeps it off the disk entirely,
and `pane_history_lines` caps what is taken.

Restored history is text pasted above a fresh prompt. It is not shell scrollback
and nothing in it is searchable history.

## Leaving a note a human will see

- `tmux rename-window`, `tmux select-pane -T 'claude: auth'`
- A file in the repo, in a sibling pane. Scrollback does not survive a restore in
  any form worth relying on

Grouped sessions and linked windows are out of scope.

## Check yourself

Stayed in the existing project session, or in the snapshot session being restored.
At most one agent pane per repository. Named the window after the task. Did not
create session `0`, did not kill a live server to resurrect over it, and did not
open a picker without `--print`.
