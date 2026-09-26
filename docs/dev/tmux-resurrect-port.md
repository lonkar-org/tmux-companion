> Planning record from the port, kept for history; the current behaviour is in the reference docs.

# Porting tmux-resurrect into tmux-companion

A pane running an AI agent doesn't come back after a reboot, and the reason is
one line of configuration that cannot be written correctly. tmux-resurrect
restores a program only when its name appears in `@resurrect-processes`, and on
this laptop on 2026-09-25 the six agent panes reported their process name as
`2.1.281`, which is claude's version string rather than anything a person would
think to put in a list.

So the save side already works and the restore side throws that work away. That
asymmetry is what this document is about.

## What happens today

The measurements below are from one laptop, taken on 2026-09-25.

The last save tmux-resurrect wrote is 2,690 bytes across 14 panes, with a
24 KB `pane_contents.tar.gz` beside it. Seven of those panes are nvim, six are
claude, one is a bare shell. The file records the full command:

```
:claude --resume cfba62df-ffde-43e2-944b-5fc36aec3ed5
```

Everything needed to bring that conversation back is on disk and has been there
since 09:23. `@resurrect-processes` here is `vim nvim "git log"`, so on restore
the seven nvim panes come back and the six claude panes come back as empty
shells, with the resume ids sitting in a file nobody reads.

Matching happens in `_proc_matches_full_command`, which tests the saved command
against each entry in the list, either as a whole word or, with a `~` prefix, as
a substring. A person who knows that can write `"~claude"` and fix their own
machine in one line, and almost nobody knows it, because the option's named
after processes and the thing that fails is an argument.

tmux-companion already saves layouts of its own. `src/saved.rs` is 1,087 lines
and captures windows, panes, layout strings, working directories, a command per
pane, and a grade for how sure it is:

```rust
pub enum Confidence { Exact, Guessed, Shell }
```

That grade gets computed and then used for almost nothing. It's the missing
input for every decision below.

### A capture bug that this feature would inherit

The saved layout for this repository, captured 2026-09-23 11:43:23 UTC, says:

```toml
[[window]]
name = "zsh"
command = "\"reattach-to-user-namespace -l /bin/zsh\""
```

`command_for` compares `pane_start_command` against `$SHELL` and treats a
mismatch as an exact answer. This laptop sets `default-command
"reattach-to-user-namespace -l $SHELL"` in tmux.conf, which macOS users have
been copying off each other for a decade, so `pane_start_command` is that
wrapper for every pane, it never equals the shell, and so every pane on the
machine gets graded `Exact` while holding a command that opens a shell inside
the shell tmux already started. Both windows of that project would restore as
bare prompts and the file would claim to be certain about it.

Fixed in `243334a`, and the shape it landed in is worth recording because it
isn't the one this document first proposed. Unwrapping known launchers would
have been a list of program names that is wrong the first time somebody wraps
their shell in something nobody here has heard of, so the capture reads
`default-command` off the server and compares against that instead. tmux hands
the setting back bare from `show-options` and quoted inside
`pane_start_command`, so one layer of matched quotes comes off first.

The setting gets consulted twice, and the second one is the part I had not
thought through. Grading the start command as "nothing asked for" only sends
the function on to `pane_current_command`, where a server set to
`/usr/local/bin/fish` reports `fish` in every idle pane and would have come
back `Guessed("fish")`, which restores fish inside the fish that
`default-command` already started. So the running process is measured against
the setting as well. Where there is no setting at all, a start command whose
last word is the shell reads the same way, and that fallback fires only when
the setting is empty so a deliberately typed `env FOO=1 /bin/zsh` keeps its
variable.

## What tmux-resurrect does that tmux-companion does not

| | tmux-resurrect | `saved.rs` today |
| --- | --- | --- |
| Unit of state | the whole server, one snapshot | one project, keyed by directory |
| Session names | kept verbatim | derived from the directory basename |
| Pane contents | `pane_contents.tar.gz` | not captured |
| Zoom | restored | not captured |
| Active session, window, pane | restored | window and pane, inside one project |
| Grouped and linked windows | handled | no model for them |
| History | timestamped files plus a `last` pointer | one file per project, overwritten |

The gap that matters is the first row. A project layout answers "what does this
project look like", a snapshot answers "what was I doing at 09:23", and merging
them into one store loses a session like `y` on this machine, which is one
window, a bare shell, no project, and no directory worth keying on.

## Two stores, not one

`project` stays what it's been: one file per directory, overwritten on demand,
edited by hand, kept forever. `sessions` is new: a generational snapshot of the
whole server, disposable, keyed by time.

| | `project` | `sessions` |
| --- | --- | --- |
| Question | what does this project look like | what was I doing at 09:23 |
| Key | directory path | timestamp |
| Copies | one, overwritten | `keep` generations, plus `last` |
| Edited by hand | yes | no |
| Scope | one session | every session |
| Path | `state/projects/<path>.toml` | `state/sessions/<stamp>.toml` |

Restore reads the snapshot first and falls back per session to the catalog,
which is what makes a machine with no snapshot still open a project correctly
and a machine with a stale catalog still restore the session it had. `project
forget` deletes a catalog entry and touches no snapshot.

## The commands

```
tmux-companion start                         # the way in, unchanged

tmux-companion project save                  # the catalog
tmux-companion project forget
tmux-companion project show
tmux-companion project close                 # was close-project
tmux-companion project autosave

tmux-companion sessions save      [--skip-pane-history] [--exclude a,b]
tmux-companion sessions resurrect [STAMP] [--only a,b] [--exclude a,b]
                                  [--merge] [--dry-run] [--yes] [--detach]
tmux-companion sessions shutdown  [--exclude a,b] [--daemon-too]
tmux-companion sessions restart   [--exclude a,b] [--keep-daemon]
tmux-companion sessions list      [--json]
tmux-companion sessions show      [STAMP] [--json]
tmux-companion sessions autosave  [--once] [--status]

tmux-companion shutdown                      # the daemon, nothing else
tmux-companion restart                       # the daemon, to reload config.toml
```

`tmux-companion shutdown` and `restart` act on the companion daemon and leave
tmux alone. `sessions shutdown` saves and then stops the tmux server.
`sessions restart` does the whole cycle: save, stop the server, bounce the
daemon, start a server, restore, attach.

`sessions restart` bounces the daemon by default and `--keep-daemon` opts out,
which is the opposite of the flag on `shutdown`. The reason is `config.toml`:
the daemon loads it once at startup in `src/server/mod.rs:75` and holds it for
its whole life, so a restart that leaves the daemon running gives you a new
binary, a freshly sourced tmux.conf, and yesterday's configuration, which is a
failure nobody would connect to the command they ran.

`close-project` becomes a hidden alias for `project close` and stays one
release, the same treatment `vim-bg` and `zoom` got.

### What `--exclude` means on a shutdown

On `save` and `resurrect` it reads the way you expect. On `shutdown` it can't,
because stopping the server takes every session with it whether or not it was
captured, so `--exclude y` there means `y` is not saved and `y` does not come
back. That's a footgun with a plain fix: the command names what it is dropping
before it does anything.

```
not saving: y (excluded) -- it will not come back
saving: icf-c_com, lekhani, lonkar_org, mysetup, tmux-companion, yogesh_lonkar_org
```

### The shared poll that turned out not to be worth it

An earlier draft had the daemon make one pair of tmux calls and hand the result
to `[notify]`, `[window_names]` and the snapshot timer, so the timer would not
be a third poller. That was written while the third autosave mode still existed
and the timer could fire on every change the daemon noticed.

With that mode gone the timer's floor is 10 seconds and its default is 900, and
the arithmetic stops working:

| poller | interval | cost |
| --- | --- | --- |
| `[notify]` | 2s | 0.05% of one core |
| the timer at its floor | 10s | 0.01% |
| the timer at its default | 900s | 0.0001% |

The timer's own polling is five times cheaper than what `[notify]` already does,
so sharing buys a rounding error and costs real coupling between three loops
with different lifetimes and different reasons to be turned off. Each one makes
its own calls, which is what the daemon's other tasks already do.

What the sharing was also meant to fix, a project capture and a snapshot
disagreeing about a server they looked at a second apart, is not a problem worth
that either: they write different files, and `[project] autosave` is off.

## The restore flow

```
sessions resurrect
  refuse if a server is already running with sessions in it
  start a server, create a bootstrap session
  read the newest snapshot, or the one named
  per session: snapshot first, catalog for what the snapshot lacks
  build sessions, windows, panes, layouts, directories, zoom
  replay pane history unless --skip-pane-history
  classify every pane: matched, guessed, unknown, denied
  show the summary, which proceeds on its own
  run the approved commands
  kill the bootstrap session last
  attach
```

The bootstrap session is the only thing holding the server up until the real
sessions exist, so killing it early takes the server down with everything just
built in it. And commands run after
the geometry is applied, never before, which `session_commands` in
`src/project.rs` already gets right and says so in its doc comment.

Attaching at the end happens when stdin is a terminal and the command was not
run from inside tmux, so `sessions restart` in a Ghostty window attaches and the
same command in a launchd job does not. `--detach` says no and `--attach NAME`
picks the session. Which session it would have picked comes from the snapshot
header, which records the one that was attached when it was taken, and
`most_recent_session` in `src/project.rs` answers when the header has nothing.

### Refusing rather than merging

A server with live sessions in it is a person's working state. `resurrect`
prints what is live and stops:

```
4 sessions already running: mysetup, lekhani, y, tmux-companion
nothing restored. --merge adds the missing ones, --only picks some
```

A restore that half-overwrote live work is the one bug in this feature that
costs somebody a day, so the default's to do nothing.

`--merge` adds only the sessions whose names aren't already live. It never
touches a running session, it attaches nothing, and it prints what it added.

The exit codes matter here because this runs unattended: `0` restored, `2` bad
arguments, `3` refused because sessions are live, `4` nothing to restore, `1`
for everything else.

### Deciding what to run

A pane's command is classified against a table, and only matched commands run
without being asked about:

```toml
[[restore.program]]
match = "^claude"              # regex against the full saved command
command = "claude --continue"

[[restore.program]]
match = "^(codex|gemini|cursor-agent|aider)"
command = "{command}"          # replay what was saved, verbatim

[[restore.program]]
match = "^nvim"
command = "nvim"
```

`{command}` is the saved command and `{cwd}` the directory. A command no row
matches is captured, shown, and not run unless you say so, because restoring is
running recorded commands and "re-run anything I saw" is one bad afternoon away
from restoring a `curl | sh` that was in a pane six weeks ago.

This table replaces the `strategies/` directory, which is four bash scripts
doing per-program command rewriting: `irb` strips environment variables out of
its own command line, `mosh-client` rebuilds its arguments, `vim` and `nvim`
check for a `Session.vim`. Configuration does that job better than four scripts
nobody can find.

I went back and forth on whether these rows belong on `[[sh_jobs.job]]`, which
is already a per-process match table with icons and window names. They stay
separate. `sh_jobs` is about jobs you suspended and can see on the status bar,
this is about foreground programs that get re-run at restore, the two sets
barely overlap, and one table serving both would read as serving neither. If
the overlap turns out bigger than I think, merging later is easier than pulling
them apart.

### Resume by continue, not by id

`claude --resume <uuid>` is exact and brittle. The id saved 15 minutes before a
crash might not be the conversation you were in, and a conversation that was
compacted or deleted makes the flag fail outright, leaving you with a pane that
exited instead of a pane with the wrong history.

`claude --continue` takes the most recent conversation in that directory. It
survives a crash, and it picks wrong when two agents were running in one
repository, which on this laptop happens in `lonkar_org`, where windows 1 and 3
are both nvim on the same project. The default is continue, the pinned id is
opt-in per row, and the summary screen says which one it is about to use.

### The summary

```
restoring 7 sessions · 14 panes · 6 agents     from 09:23, clean shutdown
2 panes need a decision            [enter] go  [tab] choose  [q] cancel   4...
```

It proceeds on its own and waits the moment you touch it. The trigger for
showing the picker at all is what the restore does not know, never a count of
panes: everything `Exact` and matched goes straight through however many panes
there are, and one `Guessed` or unknown pane in a two-pane restore stops to ask.
A threshold on pane count would prompt on every normal restore here, where 14
panes is a quiet Tuesday, and stay silent on the one case worth reading.

A crash marker present means the picker always opens, because that's the
restore somebody wants to look at before it runs.

### Waiting for the prompt

Commands are sent with `send-keys` into a pane whose shell has to be ready to
receive them, and a shell that hasn't drawn its prompt yet drops the keys or
mangles them. The usual answer is a sleep, which is wrong on a slow morning and
wasteful every other time.

There is a better one available here since the prompt marks landed: a shell
running `tmux-companion shell-init` emits OSC 133 at every prompt, tmux records
it, and `#{pane_last_prompt}` or a copy-mode search says the shell is ready.
Where the mark is present the restore waits for it and sends immediately after,
and where it's absent it falls back to a short sleep. That's the first thing
the prompt-mark feature does for somebody who never presses `C-p` in copy mode.

## Crash and sudden restart

A clean exit runs `project close` or `sessions shutdown` and captures on the way
out. A crash runs nothing, so the only state that survives is whatever was
already written, and the whole design reduces to how stale that is.

```toml
[sessions]
autosave = "off"        # off | interval | cron
interval_secs = 900     # floor of 10
cron = "0 * * * *"
keep = 20               # generations
keep_days = 0           # also keep anything younger than this, 0 disables
pane_history = true
pane_history_lines = 2000
exclude = []
```

`interval` is a timer and `cron` is a schedule for people who want it at the top
of the hour. There was a third mode in an earlier draft of this document,
writing on every change the daemon noticed, and it's gone: somebody who wants a
loss window of seconds can set `interval_secs = 10` and read the table below to
see what they are paying for it, which is a better deal than a mode name that
hides the same arithmetic.

The floor is 10 seconds. Below that the writes start overlapping the capture on
a busy machine and there's nothing sensible for the daemon to do about it.

### What a short interval costs

Measured on this laptop on 2026-09-25 against the live server, 7 sessions, 14
panes, 14 windows. Capture is 9.3 ms of CPU per pane and the metadata is 1 ms
for the whole server, so the 40-pane column is that rate extrapolated and not
something I have run.

| | 14 panes | 40 panes |
| --- | --- | --- |
| Metadata, both tmux calls | 1 ms | ~3 ms |
| Pane history, capture | 130 ms | ~370 ms |
| Pane history on disk, gzipped | 16.7 KB | ~48 KB |

Which turns into this, per snapshot:

| `interval_secs` | 14 panes | 40 panes | what `keep = 20` spans |
| --- | --- | --- | --- |
| 900 (default) | 0.015% of a core | 0.04% | 5 hours |
| 60 | 0.2% | 0.6% | 20 minutes |
| 10 | 1.3% | 3.7% | 200 seconds |
| 10, `pane_history = false` | 0.01% | 0.03% | 200 seconds |

The last column is the consequence nobody expects. Generations are counted, not
timed, so a 10 second interval with the default `keep` holds 200 seconds of
history, and a crash you only notice after lunch has already rolled off the end
of it. Short interval, large `keep`, or both.

`pane_history = false` is what makes a short interval cheap, and metadata is the
half that carries which pane was running `claude --resume <uuid>` in which
directory, so it's the half worth having often.

Writes go to a temporary file and get renamed, so a crash during a write costs
the newest snapshot and leaves the previous ones intact. The `last` pointer
moves only after the rename succeeds.

Pruning runs after a successful write, oldest first, down to `keep`. A file
younger than `keep_days` survives that even when it's past the count, which is
how somebody keeps a month of hourly snapshots without setting `keep` to 700.
The generation `last` points at is never pruned.

The daemon writes a marker at startup and removes it on clean shutdown. Finding
it at the next startup is how `resurrect` knows the machine went down badly, and
that turns a silent replay into a sentence worth reading: from 09:23, forty
seconds before the crash.

The daemon dies in the crash along with everything else. Whatever wasn't on
disk at that moment is gone and no amount of design recovers it.

## Pane history

Same shape as the existing tarball: one archive per snapshot, named for the same
timestamp, holding one file per pane. 16.7 KB for 14 panes at 2,000 lines each,
so `keep = 20` is a third of a megabyte on this laptop, which isn't a number
worth optimising until somebody pairs a 10 second interval with a large `keep`.

Pane history is the text that was on your screen, so it contains whatever you
printed, including the token you echoed and the `.env` you catted. The snapshot
directory is created `0700` and every file in it `0600`, and
`pane_history = false` turns the capture off for anybody who would rather not
have it on disk at all.

Restored history is replayed text above a fresh prompt. It isn't scrollback you
can search in the shell, and it never was under the old implementation either.

## Identity, versions and machines

A pane is keyed by session name, window index and pane index. `pane_id` looks
like the obvious key and isn't: `%12` is handed out by the running server and
means nothing after that server exits.

Every snapshot carries a header with its own format version, the time, whether
the shutdown was clean, the tmux version, the companion version and the
hostname. The version is there because this is a store with 20 generations in
it, and a format change that can't be detected is a format change that reads
old files as garbage.

The hostname matters more than it looks. These dotfiles are synced across
machines, and a snapshot taken on one of them names directories that may not
exist on the other. A pane whose directory is missing opens at `$HOME` and says
so in the summary rather than failing the restore.

## Migrating

The old format is tab-separated and the parse is an afternoon's work, so
`sessions resurrect` reads it directly when no snapshot of its own exists. That
means nobody's history dies on the way in, including the seven sessions on this
laptop, and it gives the restore table a real corpus to be tested against
instead of fixtures I made up.

After this lands, `tmux-resurrect` and `@resurrect-processes` aren't needed.
`[autosave] script` goes away and `sessions autosave` replaces it.

## Everything reachable without a terminal

Every decision the summary screen offers has a flag: `--only`, `--exclude`,
`--yes`, `--dry-run`. `sessions list --json` prints the generations with their
provenance. `--dry-run` prints the exact tmux commands the restore would run and
exits without running them.

This is partly for scripts and CI and partly because a picker is the wrong
interface inside `tmux-companion sessions restart` running from a bare terminal
at boot. A feature that can only be driven by a human at a keyboard is a feature
that can't be tested end to end, and this one restores work, so it needs to be
testable end to end.

## What this does not do

nvim buffers aren't tmux's business. The old `nvim` strategy checks for a
`Session.vim` in the pane's directory and runs `nvim -S` if it finds one, and it
never writes that file itself, so it only helps people who already run
`:mksession` by hand. Anybody who wants their buffers back should install an
nvim session plugin, which saves and restores per directory and makes plain
`nvim` the correct restore command. Pointing at that plugin is the whole of
tmux-companion's answer here.

Grouped sessions and linked windows are out of scope for the first version. The
model for them is a genuine addition rather than a field, and I've never used
either, so I'd be designing from the documentation rather than from something
I have run.

Restoring a snapshot onto a different machine is supported only as far as the
directories existing. Nothing translates paths between machines.

## What would change my mind

If the snapshot timer measures above 1% of one core on this laptop at its
default, it is doing something other than what it was measured doing, because
the daemon's whole argument is that it costs less than the five `#()` calls it
replaced.

If the restore table needs more than about a dozen rows to cover the programs
people run, then a table's the wrong shape and the answer is closer to
recording the shell command as typed, which needs shell history correlation and
is a much larger feature.

And if importing the old format turns out to need the `strategies/` scripts to
be reimplemented one by one rather than expressed as table rows, then the table
isn't as general as this document claims and the scope has to grow before any
of it ships.

## Open items

The 40-pane column in the cost table is extrapolated from 14 panes at 9.3 ms
each. The rate probably holds, since `capture-pane` is a per-pane copy out of a
ring buffer with nothing shared between panes, but I have not run it on a
machine that big and the table says so.

The shared poll has no test for the case where all three consumers want
different intervals and one of them is off. It is the sort of thing that works
until somebody sets `[notify] enabled = false` and the snapshot timer quietly
inherits a 5 second poll it did not ask for.

Restoring a `cron` schedule string across a daylight-saving change is unhandled
and I have not decided whether it should be.
