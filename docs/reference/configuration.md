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

The tables today are `[general]`, `[dirs.aliases]`, `[git]`, `[network]`,
`[battery]`, `[glyphs]`, `[status.right]` and `[sh_jobs]`. The port adds `[[layout]]` and the rest as each phase reaches them, and `docs/comrades-port.md` has the table
saying which phase brings which.

## tmux user options

There aren't any, with three exceptions planned for the settings that have to
differ per session: `@tmux-companion-theme`, `@tmux-companion-layout` and
`@tmux-companion-status-right`.

A general mapping of the config tree onto `@` options would cost a
`show-options` round trip per option per render, and would produce names like
`@tmux-companion-layout-window-2-command`, which is a config language built out
of hyphens by accident. Anything nested lives in the file and the option refers
to it by name.

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
window, or five, or a `tail -f` on a log and no editor anywhere. A layout with
no windows in it is a plain shell, which is what somebody who wants none of
this gets by writing nothing.

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
| `none` | nothing | — |

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
which is a smaller tool and still a working one — typing a path that matches no
row opens it either way, so nothing is unreachable.

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

## Saving the session list

`[autosave]` is the saving half of tmux-resurrect on a timer in the daemon:

```toml
[autosave]
enabled = true
interval_secs = 900
```

Restoring stays on a keybinding on purpose. An automatic restore would
resurrect a stale layout over a session you've already started working in,
which is a worse failure than losing a layout to a reboot.

It isn't tmux-continuum, which is the usual answer here, because continuum
drives its timer by appending `#{continuum_save}` to `status-right`, and
`status-right` is a single `#()` into this binary tuned down from five spawns a
second. A task in the daemon leaves that alone.

What the daemon removes is the bookkeeping. The zsh version needed a PID lock
file so `prefix+r` couldn't start a second copy, a stale-lock takeover for when
a server was killed, and a liveness check between sleeps. One daemon owns one
task, and it stops when the daemon stops.
