# Configuration

There is no config file until you write one, and you may never need to: every
setting has a default, and the defaults are what the binary did before the file
existed. A test holds that line rather than a promise, by parsing
`docs/config.example.toml` and asserting it equals the built-in defaults.

## Where the file goes

First one found wins:

1. `--config <path>`
2. `$TMUX_COMPANION_CONFIG`
3. `$XDG_CONFIG_HOME/tmux-companion/config.toml`, which is
   `~/.config/tmux-companion/config.toml` when that variable isn't set
4. `~/tmux-companion.toml`

```sh
tmux-companion config path    # which file is being read
tmux-companion config check   # parse it, say what is wrong, exit nonzero
tmux-companion config dump    # every setting with its default
```

`config dump` writes a valid config file, so `tmux-companion config dump >
~/.config/tmux-companion/config.toml` is a reasonable way to start.

## When it doesn't parse

The daemon refuses to start. That's the deliberate half; the other half is that
a refusal is only defensible if you find out why within seconds, so the error
goes three places:

- the whole thing to stderr, and to
  `$XDG_STATE_HOME/tmux-companion/last-error`
- one line into the status bar, from whichever client tried and failed to start
  a server, naming the command that explains it
- the whole thing again, with the line, the column and the key you probably
  meant, from `tmux-companion config check`

The bar gets one line because a status bar is about 150 columns wide and a
parse error doesn't fit in them.

An unrecognised key is an error rather than something ignored. A silently
dropped setting is the same bug as a typo'd argument reading back as `None`,
and it costs somebody an evening.

## The settings

Every key, its default and what it does is in
[`docs/config.example.toml`](../config.example.toml), which is the file to copy
from. A test asserts that every key `config dump` produces appears there, so a
setting can't exist without being written down.

The top-level tables, in the order the example file has them, and where each
one is explained on this page:

| Table | Decides | Section |
| --- | --- | --- |
| `[general]` | where the daemon's log goes | [The daemon's log](#the-daemons-log) |
| `[dirs.aliases]` | a label for a long path in the window segment | [Labels for long paths](#labels-for-long-paths) |
| `[git]`, `[[git.branch_types]]`, `[git.autofetch]` | what the git segment shows, the glyph per branch prefix, fetching | [Choosing what the git segment shows](#choosing-what-the-git-segment-shows) |
| `[network]` | the bandwidth segment's threshold and colours | [Bandwidth and battery](#bandwidth-and-battery) |
| `[battery]` | how long a battery reading stays fresh | [Bandwidth and battery](#bandwidth-and-battery) |
| `[glyphs]`, `[glyphs.icons]` | the icon preset and single-icon overrides | [Glyphs](#glyphs-if-your-bar-is-a-row-of-boxes) |
| `[status.right]`, `[[status.right.segments]]` | which segments the right side draws, and the separators | [Building the right-hand side](#building-the-right-hand-side) |
| `[sh_jobs]`, `[[sh_jobs.job]]` | the icon per process under a pane, and its window name | [Jobs under a pane](#jobs-under-a-pane) |
| `[usage]` | whether picked bindings are recorded | [The usage log](#the-usage-log) |
| `[project]`, `[[project.override]]` | the directory source, the visit command, which layout | [Where the directory list comes from](#where-the-directory-list-comes-from) |
| `[[layout]]`, `[[layout.window]]`, `[[layout.window.pane]]` | the windows a new project session starts with | [What a new project session starts with](#what-a-new-project-session-starts-with) |
| `[notify]` | announcing a long command that finished out of sight | [Saying a long command finished](#saying-a-long-command-finished) |
| `[agents]` | which programs are coding agents, and when one counts as waiting | [Which programs are agents](#which-programs-are-agents) |
| `[window_names]` | naming a window after what runs in it | [Naming windows after what is running](#naming-windows-after-what-is-running) |
| `[autoreload]` | sourcing tmux's config when it changes | [Reloading tmux's config when it changes](#reloading-tmuxs-config-when-it-changes) |
| `[autosave]` | deprecated; the older, narrower `[sessions]` | [Saving every session](#saving-every-session) |
| `[sessions]` | snapshots of the whole server, kept in generations | [Saving every session](#saving-every-session) |
| `[[restore.program]]` | what a restore is allowed to run in a pane | [What a restore may run](#what-a-restore-may-run) |
| `[run]` | where `run` reads history from, and the pane it opens | [The pane a command runs in](#the-pane-a-command-runs-in) |
| `[clipboard]` | the command a copy is piped into | [Copying to the clipboard](#copying-to-the-clipboard) |
| `[theme]`, `[theme.namespace]` | the theme a session gets before anybody picked one | [The theme a session gets](#the-theme-a-session-gets) |
| `[bar]` | the background the segments draw against | [The bar's own background](#the-bars-own-background) |
| `[picker]`, `[picker.<name>]` | how every picker is laid out, and one picker's exceptions | [How the pickers are drawn](#how-the-pickers-are-drawn) |
| `[open]`, `[[open.application]]` | what `open` does with a file, and what `open --choose` offers | [What open does with a file](#what-open-does-with-a-file) |

There are no tmux user options. A mapping of this tree onto `@` options would
cost a `show-options` round trip per option per render, and would produce
names like `@tmux-companion-layout-window-2-command`, which is a config
language built out of hyphens by accident. Anything nested lives in the file,
and the `@theme-*` options a theme file sets when `theme apply` sources it are
tmux's own, per session, not a second copy of this tree.

## The daemon's log

```toml
[general]
log = "~/.local/state/tmux-companion/daemon.log"
```

That's the default, spelled out: `daemon.log` in the state directory, which is
`~/.local/state/tmux-companion` unless `XDG_STATE_HOME` says otherwise. The
daemon writes one line when it starts and one per failure, so "autosave
failed" and a config that wouldn't parse end up here, `tmux-companion doctor`
prints the last line, and past 1 MiB it's rotated to `daemon.log.1`. A `~`
in the path is expanded, as in every path in the file.

## Labels for long paths

```toml
[dirs.aliases]
"/Users/you/work/some-very-long-project" = "proj"
```

The window segment abbreviates a path, and this replaces the abbreviation with
a name of your choosing. It took over from `~/.yrl/lib/dir-aliases`, which was
a path on one laptop, and that file is still read when the table is empty so an
upgrade drops nothing.

## Bandwidth and battery

```toml
[network]
threshold_bps = 20480
download_colour = "#5cae36"
upload_colour = "#0262a8"
unit_colour = "colour237"

[battery]
ttl_secs = 30.0
```

Below `threshold_bps`, 20 KiB/s by default, the bandwidth segment draws
nothing, since a bar that reacts to every background poll is noise. For a
while this setting was read from the config and then ignored, because the
segment used a constant of the same value, so changing it did nothing at all;
it's honoured now. The two colours are the blocks each rate is drawn on, with
the number written in the bar's own background colour on top, and
`unit_colour` is the `KiB/s` after it, drawn dimmer so the figure reads first.
The default there is a dark grey chosen for a dark bar, and on a light one it's
very nearly invisible.

`ttl_secs` is how long a battery reading stays fresh. Reading it is expensive
and the number doesn't move fast enough to matter.

## The usage log

```toml
[usage]
enabled = true
path = "~/.local/state/tmux-companion/keys-usage.tsv"
```

Which bindings get picked, so the cheat sheet can order each box by it and the
keys you reach for float to the top of their group. It records what you press,
which is your business and not the tool's, so turning it off is one line and
nothing else changes. `path` unset means `$XDG_STATE_HOME/tmux-companion/`.

## Glyphs, if your bar is a row of boxes

The default set is Nerd Fonts v3, whose codepoints sit in the private use area,
so a font without that patch draws boxes and a new reader can't tell whether
the install worked. One line fixes it:

```toml
[glyphs]
preset = "ascii"
```

`nerd-font-v3` and `ascii` ship today. A preset is a table of names to strings
in its own file, so adding `powerline` or `unicode` later is a data change with
no Rust in it, which also makes a preset about the easiest first patch anybody
could send.

Individual glyphs override the preset, by the constant name in
`src/tmux/icons.rs`:

```toml
[glyphs.icons]
STAGED = "*"
ARROW_RIGHT = ""
```

One icon missing from your font is a reason to replace that icon rather than to
drop to a whole preset below it.

How it works is worth knowing for one reason: the substitution is applied once
to a finished segment rather than threaded through the 263 places a glyph is
used, so a preset can only replace glyphs the default set already contains. It
can't add a glyph somewhere there wasn't one.

## Choosing what the git segment shows

`[git] parts` is an ordered list, and a part left out of it isn't drawn:

```toml
[git]
parts = ["branch", "state", "staged", "modified"]
```

Four hundred untracked build artifacts aren't information, and if you never
push then `ahead` and `behind` are two counts you'll never read. The names are
what a reader sees rather than what the code calls things, so it's `conflicts`
rather than `unmerged`.

The full vocabulary is in [`docs/config.example.toml`](../config.example.toml),
and an unknown name is a config error rather than a part that silently does
nothing.

Order is honoured between groups: branch info (`ahead`, `behind`,
`conflicts`), then the work-tree counts, then `staged`, then `stash`. Inside a
group it's fixed, because a group is a single colour run and reordering its
counters would move escape sequences rather than glyphs. `parts = []` renders
an almost empty segment, which is a legitimate thing to ask for and not a
crash.

## The glyph in front of the branch name

The first thing in the git segment is a glyph chosen from how the branch name
starts, so `feat/picker-preview` gets the feature icon and drops the `feat/`,
and a name nothing claims keeps the plain branch glyph, which is why `main`,
`master`, `dev` and `stable` were never special cases.

Six groups ship, covering `feat/`, `feature/`, `features/`, `fix/`, `fixes/`,
`bugfix/`, `bugfixes/`, `hotfix/`, `chore/`, `chores/`, `release/`,
`releases/`, `tag/` and `tags/`, which is a reasonable guess at how most people
name branches and wrong the moment you name one `posts/` or `parked/`, so the
whole table is `[[git.branch_types]]`:

```toml
[[git.branch_types]]
icon = "{TAG}"
prefixes = ["post/", "posts/"]

[[git.branch_types]]
icon = "P "
prefixes = ["parked/"]
```

Entries are tried in the order written and the first prefix that claims the
name wins, the match ignores case, and the prefix is cut off the name the bar
draws. They're plain prefixes rather than regular expressions: `posts/` is what
you'd type anyway, and an unanchored pattern hitting the middle of a branch
name is a bug you'd find on the status bar rather than in a test.

`icon` takes `{NAME}` for any glyph in
[`src/tmux/icons.rs`](../../src/tmux/icons.rs), the same spelling
`separator_before` uses, so the file is readable in an editor with no patched
font. Anything else is drawn as written, so an emoji, a couple of letters, or a
codepoint your own font has all work, and the trailing space is yours to
include or leave out.

Your list replaces the six rather than adding to them, so run
`tmux-companion config dump`, copy the `[[git.branch_types]]` blocks you want
to keep, and add yours. Replacing rather than merging is what lets you delete
`tag/` if a branch of yours starts with it and you'd rather it didn't get the
tag glyph.

## Building the right-hand side

`[[status.right.segments]]` is the list of segments and what goes in front of
each one:

```toml
[[status.right.segments]]
name = "git"
separator_before = ""

[[status.right.segments]]
name = "battery"
separator_before = " | "
```

That drops the bandwidth segment and puts a plain pipe before the battery.
`separator_before = ""` means nothing at all between two segments, which is a
preference nobody could express while the literals lived in `tmux.conf`.

`{NAME}` in a separator expands to the glyph of that name in
`src/tmux/icons.rs`, so the file stays readable in an editor with no patched
font, and a name that doesn't exist is left as you wrote it rather than
dropped, since a separator rendering `{ARROW_RIGH}` is a typo you can see.

A separator is drawn whether or not the segment after it rendered anything,
because that's what the `tmux.conf` literals did: a non-repo pane on a quiet
network still drew the wedge in front of an absent battery. And
`trailing_space` exists because tmux draws the right side flush to the terminal
edge, so without it the last glyph sits against the border.

## Jobs under a pane

`tmux-companion sh-jobs <pane_pid>` says what's stopped or running under a
pane. It was `vim-bg`, which asked one question with one answer compiled in, so
anybody suspending `vim` or `claude` or a `cargo watch` got nothing at all.

```toml
[[sh_jobs.job]]
match = "^claude$"
icon = "󰚩 "
color = "#d97757"
```

`match` is a regular expression against the process name, so `^vim$` doesn't
match `nvim`, and the first entry that matches wins. An unparseable pattern
costs that row its icon and nothing else, because the pattern came from a file
and one bad row shouldn't take down a daemon. `states = "any"` counts
background jobs as well as stopped ones, and `max` bounds what a busy pane can
put on the bar.

The old name still works for one release and prints a line saying so.

Cost is why this isn't on the status bar by default: finding the children of
one pid means enumerating the whole process table, which measured 16.25 ms of
server CPU per call. Bind it to a key, or put it on the bar knowing what it
costs.

### Fetching in the background

The ahead and behind counts are only as fresh as your last fetch, and a bar
saying "up to date" because nothing has fetched in a week is worse than a bar
saying nothing, because the first one gets believed.

```toml
[git.autofetch]
enabled = true
interval_secs = 600
```

It's off by default and that's deliberate rather than shy: this is the only
part of tmux-companion that touches a network, and a daemon quietly reaching a
remote is not a surprise anybody should get from a status bar.

The fetch runs with `GIT_TERMINAL_PROMPT=0`, empty askpass helpers and
`ssh -oBatchMode=yes`, because a prompt on a daemon doesn't fail, it waits, and
it would wait every interval for as long as the daemon runs. `timeout_secs`
catches whatever gets past that and kills the git behind it. Repositories are
fetched one at a time, since the point is that the counts are right by the time
you look and not that they're right quickly.

Only repositories the bar has actually drawn get fetched, and only for
`remember_secs` after it last drew one, so a daemon running for a month doesn't
end up fetching everything you visited in that month. Two panes in two
subdirectories of one tree are resolved to one root and fetched once.

The plugin this comes from is
[thepante/tmux-git-autofetch](https://github.com/thepante/tmux-git-autofetch).
It's alive and it's worth installing if you aren't running this.

### Saying a long command finished

```toml
[notify]
enabled = true
threshold_secs = 30
```

The last four words of the heading are the feature. A command that finishes in
front of you needs no announcement, and firing for those is exactly the noise
that teaches people to ignore the ones that matter, so `only_when_unwatched` is
on by default.

A pane counts as out of sight if it was hidden at any point while the command
ran, not only at the end. The usual shape is starting something, switching away,
and coming back when it's already done, and a check taken at the finish would
say you'd been watching all along.

The default notifier is tmux's own `display-message`, which needs nothing
installed and behaves the same on every platform. A desktop notification is a
line of config away and deliberately isn't the default, because shelling out to
`osascript` or `notify-send` on a machine that has neither is a failure
somebody has to debug:

```toml
command = ["notify-send", "{command}", "ran for {duration}"]
```

`{command}`, `{duration}`, `{pane}` and `{message}` are substituted in every
argument.

The `ignore` list is doing real work. An editor, a pager or an agent runs for
hours, and without the list every `:q` fires a notification about a two-hour
nvim session. The default covers the editors, pagers and agents; add whatever
else you leave open.

Nothing is announced on the first scan after the daemon starts, or a restart
would announce everything running everywhere at once, and it can't know how
long any of it had already been going.

The plugin this comes from is
[rickstaa/tmux-notify](https://github.com/rickstaa/tmux-notify).

### Which programs are agents

```toml
[agents]
programs = ["claude", "codex", "gemini", "cursor-agent", "aider", "opencode"]
waiting_secs = 10
interval_secs = 2
inbox = true
nudge_after_secs = 0
nudge_command = []
```

`inbox` keeps the agents that have stopped, with the last lines of each one's
screen captured the moment it stopped, so `tmux-companion inbox` shows the
question an agent asked even on a window you haven't looked at; one
`capture-pane` per stop. `nudge_after_secs` says it out loud, through
`nudge_command` or tmux's own `display-message`, once an agent has waited that
long; zero, the default, never does, since the bar already counts them.

One list, read by everything that asks whether a pane is an agent:
`tmux-companion panes --agents`, the `agents` segment on the bar, and the
headline `sessions resurrect` shows, which counts them. It used to be compiled
into that headline, so an agent the code hadn't heard of was invisible to all
three. `programs` is matched against `pane_current_command`.

`waiting_secs` is how long an agent has to draw nothing before it counts as
waiting on you, which is the number the bar colours and the picker sorts
first. tmux keeps no activity time per pane, only per window, so it's measured
on the window the agent is in: a shell you're typing into beside it keeps it
reading as busy. Ten seconds is long enough that a model thinking between two
tool calls isn't called idle, and short enough that a question left on the
screen is noticed before you wonder why nothing's happening.

`interval_secs` is how often the daemon re-reads the pane list for the bar's
segment. One `tmux list-panes` each, shared by every attached client, and none
at all unless a `[[status.right.segments]]` block names `agents`:

```toml
[[status.right.segments]]
name = "agents"
separator_before = " "
```

That draws `4 agents · 1 waiting`, with the second half coloured, and nothing
when no agent is running, so the separator goes with it.

### Naming windows after what is running

```toml
[window_names]
enabled = true

[[sh_jobs.job]]
match = "nvim"
icon = "..."
window_name = "edit"
```

One table answering two questions. `[[sh_jobs.job]]` already maps a
process-name pattern to an icon for the bar, and a row with a `window_name` on
it also says what a window holding that process should be called. A row without
one says nothing about names, which is what every row written before the field
existed says.

It won't take a name away from you. A window with `automatic-rename` off was
pinned deliberately, by `hold_name` in a layout or by hand, and this leaves it
alone unless it was the thing that pinned it, which it knows from a
`@tmux-companion-named` option it sets on its own work. When a window stops
matching, the name is handed back and tmux goes back to renaming it.

Off by default, because renaming somebody's windows is visible.

The plugins this comes from are
[ofirgall/tmux-window-name](https://github.com/ofirgall/tmux-window-name),
which is a Python daemon, and
[joshmedeski/tmux-nerd-font-window-name](https://github.com/joshmedeski/tmux-nerd-font-window-name)
for the icon half.

### Reloading tmux's config when it changes

```toml
[autoreload]
enabled = true
```

The daemon already stats your tmux config, because the rows behind `keys` and
`cheatsheet` get rebuilt when the file is newer than they are, so noticing the
same change and running `source-file` costs nothing it wasn't doing.

Off by default, since reloading somebody's tmux config without being asked is a
thing that happens to their running sessions. The first pass after the daemon
starts never reloads, or a daemon started right after an edit would source the
file at startup, which is a surprise and a loop when the config is what starts
the daemon.

A `source-file` that fails puts tmux's first error line in a `display-message`
rather than the daemon's stderr, which nobody reads. A config with a syntax
error in it is exactly when you need telling.

The plugin this comes from is
[b0o/tmux-autoreload](https://github.com/b0o/tmux-autoreload), which watches
with `entr` or `inotifywait` where this compares a modification time. It was
archived in February 2024 and its author named no successor, so that link is
history rather than a recommendation: this section is the replacement.

## What a new project session starts with

`[[layout]]` is the windows, and `[project] layout` picks which one:

```toml
[[layout]]
name = "default"

  [[layout.window]]
  name = "edit"
  command = "nvim"

  [[layout.window]]
  name = "ai"
  command = "claude"
```

Those two names are what one laptop runs. Yours might be vim and codex, or one
window, or five, or a `tail -f` on a log and no editor anywhere. With no
`[[layout]]` written a project opens as one plain shell, which is the shipped
default: nothing assumes an editor or an agent is installed until you name one.

`[[project.override]]` sends projects under a path to a different layout, first
match wins:

```toml
[[project.override]]
match = "~/work/*"
use_layout = "work"
```

A layout name nothing defines costs the windows rather than the session: you
get a shell and no error, because a typo shouldn't stop you opening a project.

`hold_name` keeps a window's name against the running program, and it's on by
default for a reason. Without it an editor window follows whatever is running,
and an agent window renames itself to its own version string, which is how
windows end up called `2.1.278`.

### Panes

`command` on a window is the shorthand for a window holding one pane. A
`[[layout.window.pane]]` table takes over when there's more than one, and the
panes are created in the order you list them:

```toml
  [[layout.window]]
  name = "work"
  layout = "main-vertical"
  main_size = "60%"

    [[layout.window.pane]]
    command = "nvim"
    focus = true

    [[layout.window.pane]]
    command = "claude"

    [[layout.window.pane]]
    cwd = "~/src"
```

`layout` is one of tmux's own five names, `even-horizontal`, `even-vertical`,
`main-horizontal`, `main-vertical` or `tiled`, and panes with nothing set get
tiled, because the shape repeated splitting leaves behind is an accident of the
order the splits ran in rather than a layout anybody chose. `main_size` sets
`main-pane-width` under `main-vertical` and `main-pane-height` under
`main-horizontal`, and the other three read neither, so it's ignored there
rather than silently setting an option that does nothing. A percentage needs
tmux 3.4; before that it has to be a cell count.

`focus` is the pane selected when the window opens, the first pane when nothing
sets it, and the first one that sets it when several do, since a layout with two
focused panes is a typo and not a question worth refusing to start over. `cwd`
takes a `~`, and a pane that leaves it out starts in the project directory.

`layout` also takes a raw tmux layout string, which is what
`tmux list-windows -F '#{window_layout}'` prints:

```toml
  layout = "bb62,272x67,0,0{136x67,0,0,1,135x67,137,0,2}"
```

Nothing here parses that string, it goes straight to `select-layout`, which is
what lets you arrange a window by hand with the bindings you already have and
paste the result instead of learning a layout language. The cell sizes in it are
absolute and tmux rescales them proportionally, so a layout captured on a wide
display comes back cramped on a laptop and a preset name travels better between
screens.

The commands are sent after the geometry is settled, which is deliberate: a
full-screen program started before the splits draws itself at the pre-split size
and then repaints, and that looks broken on every single session start.

## Where the directory list comes from

The project picker lists live sessions and then directories, and zoxide is the
default source rather than a requirement. `[project] dirs_source` picks another:

| Name | What it reads | Order |
| --- | --- | --- |
| `zoxide` | `zoxide query -l` | frecency, zoxide's own |
| `z` | the `~/.z` database, honouring `$_Z_DATA` | rank, highest first |
| `cdr` | zsh's `~/.chpwd-recent-dirs`, honouring `$ZDOTDIR` | most recent first |
| `ghq` | `ghq list -p` | ghq's own |
| `none` | nothing | none |

`z` and `cdr` are read as files rather than run as commands, because both are
shell functions. Spawning `zsh -ic 'z -l'` to reach one sources a whole
interactive rc for a list of paths, and prints whatever that rc prints into the
middle of it; the data file is the part that is stable.

That is also why the list stops at four names. Everything else that jumps
directories is either a shell function with its own file format, or a binary
that prints paths, and the second kind needs no support here:

```toml
[project]
dirs_command = ["fd", "-td", "-d2", ".", "/Users/you/src"]
```

`dirs_command` is a list of words and it wins over `dirs_source`. Anything
wanting a pipe, a glob or a filter goes through a shell:

```toml
dirs_command = ["sh", "-c", "ls -d ~/src/*/"]
```

autojump, fasd, jump and anything else that can list directories go here as
well. Which flag each one wants is its own manual's business and not repeated
here, because they do not agree on one and a wrong flag copied out of this page
would look like the setting being broken.

That last one is the answer for a machine with no jumper installed at all,
which is a real and reasonable way to work: a code directory and a glob over
it gives a project picker without anything to install.

A source that is not installed, or a database never written, is the empty list
and not an error. The picker falls back to live sessions and whatever you type,
which is a smaller tool and still a working one, since typing a path that
matches no row opens it either way, so nothing is unreachable.

### Recording a visit

Picking a project or opening a window tells the source it was visited, so the
place you just went floats up the list next time. Only zoxide has a command for
this by default:

```toml
[project]
visit_command = ["myjumper", "add"]
```

`z` and `cdr` are written by your shell on every `cd` and want no help from
here, and `ghq` lists clones rather than visits. The directory is appended to
whatever `visit_command` names.

`[project] zoxide = false` is the old spelling of `dirs_source = "none"`. It
still works, and it still wins over anything else in the section, so a config
written before this had more than one source keeps behaving as it did. It goes
away in the next release, and `tmux-companion config check` names it until then:

```
~/.config/tmux-companion/config.toml: ok
  `[project] zoxide = false` is deprecated; use `dirs_source = "none"`
```

## Saving every session

`[sessions]` is snapshots of the whole tmux server, kept in generations, so a
reboot doesn't cost the sessions you had open:

```toml
[sessions]
autosave = "interval"
interval_secs = 900
keep = 20
```

This is the saving half. Restoring stays a command you run, `tmux-companion
sessions resurrect`, because an automatic restore drops a stale layout over a
session you've already started working in, which is a worse failure than
losing a layout to a reboot.

`autosave` is `off` by default, where a snapshot is whatever you ask for by
hand, `interval` takes one every `interval_secs`, and `cron` follows the
five-field schedule in `cron`, `"0 * * * *"` unless you change it, for people
who want it on the hour. The floor for `interval_secs` is 10, since below that
the writes start overlapping the capture on a busy machine, and a number under
it is an error rather than something quietly rounded up. An earlier design had
a fourth mode that wrote on every change the daemon noticed, and it's gone,
because `interval_secs = 10` buys the same thing with the cost written down
instead of hidden behind a word.

What it costs, measured on one laptop with 7 sessions and 14 panes, with the
40-pane column extrapolated from that rate rather than measured:

| `interval_secs` | 14 panes | 40 panes | what `keep = 20` spans |
| --- | --- | --- | --- |
| 900, the default | 0.015% | 0.04% | 5 hours |
| 60 | 0.2% | 0.6% | 20 minutes |
| 10 | 1.3% | 3.7% | 200 seconds |
| 10, no history | 0.01% | 0.03% | 200 seconds |

That last column is the one nobody expects. Generations are counted, not
timed, so a ten-second interval with the default `keep` holds under four
minutes of history and a crash you notice after lunch has already rolled off
the end of it. A short interval wants a large `keep`, or `keep_days`, which
also keeps anything younger than that many days however many files that is,
and is how you keep a month of hourly snapshots without setting `keep` to 700.
Pruning runs after a successful write, oldest first, and never touches the one
the `last` pointer names.

`pane_history` is the expensive half, at 9.3 ms per pane against 1 ms for the
metadata of a whole server, and it's what makes a short interval costly.
Turning it off leaves the half that carries what each pane was running and
where, which is the half worth having often. It's also the half that holds
secrets: pane history is the text that was on your screen, the token you
echoed and the `.env` you catted included, so the directory is created 0700
and every file in it 0600, and this is the switch that keeps it off the disk
entirely. `pane_history_lines`, 2000 by default, is how much of each pane.

`exclude` is sessions never captured, by name. On a shutdown that means the
session isn't saved and doesn't come back, since stopping the server takes
every session with it either way, and the command says so before it acts.
`confirm_secs`, 5 by default, is how long the restore summary counts down
before going ahead; it only opens when the restore doesn't know something,
any key stops the clock, and zero is `--yes` made permanent.

`[autosave]` is the older, narrower version of this and is deprecated: it
shelled out to tmux-resurrect's save script on a timer and kept one file,
where `[sessions]` keeps generations of its own and knows what each pane was
running. It's off since `[sessions]` arrived, a config that still asks for it
keeps it for now, and `tmux-companion config check` names it:

```
  `[autosave]` is deprecated; `[sessions] autosave` keeps generations of its own and records what each pane was running
```

## What a restore may run

Default deny. Restoring means executing commands your own machine recorded
weeks ago, and "re-run anything I saw" is one bad afternoon away from
restoring a `curl | sh` that was in a pane six weeks back, so a command no row
claims is captured, shown, and left to you at a prompt in the right directory.

```toml
[[restore.program]]
match = "^claude( |$)"
command = "{command}"

[[restore.program]]
match = "^n?vim( |$)"
command = "nvim"
```

`match` is a regular expression against the whole saved command, not the
process name, because the process name of an agent is its version string,
`2.1.281` rather than `claude`, and everything worth matching on is in the
arguments. `command` is what runs, with `{command}` the saved command verbatim
and `{cwd}` the pane's directory, and `run = false` is how you say "never
bring this back" without leaving it to fall through to the unknown pile and be
asked about every time.

The rows that ship, in order: the agents, `claude` and then `codex`, `gemini`,
`cursor-agent`, `aider` and `opencode`, replay their arguments verbatim,
because an agent keeps which conversation it's in inside those arguments and
a bare `claude` stays bare. `vim` and `nvim` come back as a bare `nvim`, since
the saved arguments are a file list from an hour ago and reopening buffers is
the editor's job. `lazygit`, `tig` and `gitui`, then `htop`, `top`, `btop` and
`watch`, then `tail`, `less` and `journalctl`, and `ssh` each replay as saved.
Writing the table replaces those rows rather than adding to them, so run
`tmux-companion config dump`, copy what you want to keep, and add yours.

## The pane a command runs in

```toml
[run]
history = "auto"
width_percent = 33
slide_steps = 5
slide_ms = 150
shell = ""
```

`history` is where `run` reads the commands from: `auto`, `zsh`, `bash`,
`fish` or `atuin`. `auto` reads whichever shell `$SHELL` names, and it used to
be `zsh`, which meant a bash user's picker read a `~/.zsh_history` that wasn't
there and came up empty with nothing said, the one failure shape that looks
like the feature having nothing to offer. atuin is worth naming because anybody
using it has no shell history worth reading; it keeps its own database and
answers through its own command. `history_file` is for a history that isn't
where the shell usually puts it.

The pane is `width_percent` of the window, and it slides out rather than
appearing: tmux has no animation primitive, so this is a stepped `resize-pane`
eased to read as a slide. Every step sends `SIGWINCH` to the neighbouring pane,
whose shell repaints its prompt, so more steps is smoother here and flickerier
next door, and zero opens the pane at its full width at once. `shell` empty
means `$SHELL`; it used to be `zsh`, so on a machine without zsh, which is most
Linux boxes, the pane slid out and the command never ran.

## Copying to the clipboard

```toml
[clipboard]
copy = "xclip -selection clipboard"
```

Unset picks one for the platform: `pbcopy` on macOS, then `wl-copy`, then
`xclip`. This was two `if-shell` branches on `uname` in `tmux.conf`, and one
binary picking the right command is one less thing the Linux branch has to
special-case.

## The theme a session gets

```toml
[theme]
default = "ink"

[theme.namespace]
w = "slate"
a = "plum"
```

`default` is the theme a session gets before anybody has picked one.
`docs/tmux.conf.full.example` sets a `session-created` hook that runs
`theme apply` for every new session, and the starter file ships the same
line commented out, so with the hook in place this decides the colour of a
session nothing else claims; a session a project map already claims keeps its
own. `theme init` writes six, `ember`, `pine`, `slate`, `plum`, `sand` and
`ink`, each with a lighter and a darker sibling once `theme gen --shades` has
run, and you can name one of those or one of your own. `by-name` gives every
unclaimed session one of the six chosen from its name, the same one every time
and on every machine, so sessions are told apart at a glance with nothing
picked; the project picker shows a directory in the colour its session will
get, and a theme picked by hand still wins. Empty leaves unclaimed
sessions unpainted, which is also what happens when the theme named here isn't
on disk: nothing is sourced and nothing is said, because this runs once per
session created and a message here would land on the terminal at the moment
somebody opens a session.

`[theme.namespace]` is a theme per session-name prefix, where the prefix is
everything before the first `/`, so `w/api` and `w/web` both match `w`. Skip
it unless your session names already carry a namespace.

## The bar's own background

```toml
[bar]
background = "color233"
current_window_background = "color236"
```

Every segment ends in a powerline cap, and a cap is two colours: the segment's,
and whatever is behind it. That second one used to be a compiled-in
`colour233`, which is one person's `tmux.conf` and nobody else's, so a bar set
to anything else got wedges and outline backgrounds in a colour that appears
nowhere on screen. Set it to whatever `status-style` says; `colour233`,
`#121212`, `black` and `default` all work, and `default` leaves the terminal's
own background showing through, which is what a transparent bar wants.
`tmux-companion doctor` reads the live `status-style` and says when the two
have drifted apart. `current_window_background` sits behind the current window
in the window list, and only the `window` segment draws it.

## How the pickers are drawn

Everything under `[picker]` is the answer for every picker at once, and every
key in the example file is commented out because unset isn't the same as set
to the value that happens to be the default: unset lets each picker use its
own answer, and writing one here takes that away from all of them.

```toml
[picker]
preview = "right"
preview_percent = 55
border = "rounded"
label_position = "bottom-right"
list_from = "top"
```

`preview` is where the preview pane goes, `right`, `left`, `bottom`, `top` or
`none`, and left out each picker uses its own: the key search puts three lines
underneath, the project list puts a screen of what that session is doing on
the right, the theme list a card beside a narrow column of names, and the run
history has none because the command is the row. `preview_percent` is its
share of the popup, clamped to 20-80, and `ctrl-p` inside a picker cycles it
through 30, 50, 70 and off, since there's no drag-resize to reach for. How big
the popup itself is belongs in `tmux.conf`, on the `display-popup -w` and `-h`
of the binding that opens it.

The rest is the shape inside that: `border` (`none` when `display-popup -B`
already drew one, or two borders nest), `label_position` and `label_offset`,
`hint_position`, `prompt_position`, `list_from` (`bottom` is fzf's default and
puts the best match nearest the query, `top` is what everything else on a
screen does), `counter`, `rules`, `marker`, `column_order` (`[1, 0]` turns the
key search round, a position left out is a column not drawn), `min_list_width`
(the fewest columns a list keeps before the preview moves underneath, 24 by
default, zero to split whatever it's given the way fzf does), `preview_border`,
`preview_label_position` and `preview_label_offset`. The comments in
[`docs/config.example.toml`](../config.example.toml) say what each one does
and what fzf or skim called it.

`[picker.keys]`, `[picker.project]`, `[picker.window]`, `[picker.theme]`,
`[picker.run]`, `[picker.open]` and `[picker.panes]` hold one picker's exceptions, and a key left out of one takes
whatever `[picker]` says. `label`, `hint` and `preview_label` exist only there,
because they're the words one picker says rather than a shape they share:
`label` is what it calls itself on its border, `hint` the line naming the
keys, and `preview_label` what it calls its preview.

## What open does with a file

```toml
[open]
split = "right"
size_percent = 0
editor = "nvim '+call cursor({line},{column})' {path}"
```

When `open` finds a file rather than a URL, `split` says which side of the pane
the editor opens on. Right by default, because a file and the log you found it
in read better side by side, and bottom is better on a narrow terminal, where
two columns of sixty are two columns nobody can read. `size_percent` zero lets
tmux halve it.

`editor` is how the editor is told to jump to a line and column, with `{path}`,
`{line}` and `{column}` replaced, and the path arrives already quoted for the
shell, so don't quote `{path}` yourself. The quotes around the `+call` matter:
tmux runs a split's command through `sh`, and `nvim +call cursor(2,22) file` is
a shell syntax error, so the pane opened, complained to nobody, and closed
again, and opening a file at a line had never worked until they were added.
The example file has the helix and emacs spellings.

`[[open.application]]` is what `open --choose`, or `open -i`, offers: a `name`,
a `command` with `{url}` or `{path}` in it, and `pane = true` for something
that wants a tmux pane beside the one you're in rather than being launched and
left alone. An editor wants a pane and a browser doesn't, and the wrong answer
is either a browser holding a pane open forever or an editor with nowhere to
draw. Empty means no chooser, and `--choose` then opens what it would have
opened anyway rather than showing a picker with nothing in it.
