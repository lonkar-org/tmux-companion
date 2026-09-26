> Planning record from the port, kept for history; the current behaviour is in the reference docs.

# After the port

The port replaces `~/.config/tmux/comrades` and stops. Nothing in this file
starts before that is done, and it exists so the survey behind it does not have
to be repeated, and so the port plan can stay about the port.

Last-push dates were checked on 2026-09-22 against the GitHub API. They say
whether a plugin is still being worked on, which is what the tiers below turn
on, and they are not a ranking, and nothing here is a judgement of anybody's
work. Star counts have been left out for the same reason, since they decide
nothing in the rules below and a column of them beside somebody's repository
would only be sizing people up.

## The rule for anything on this list

A feature earns a place when tmux-companion is a better home for it than a
plugin, which happens when:

1. it needs a resident process, and every plugin doing it today is a shell
   script polling on a timer
2. it costs a `#()` spawn per refresh as a plugin, which is 14.6 ms of CPU per
   second per attached client, and nothing inside an already-running daemon
3. it reuses a table or a matcher the port built anyway, so the marginal cost
   is small enough to be honest about
4. it removes a language runtime from the dependency list

Anything that fails all four is somebody else's plugin and should stay theirs.

## The one worth doing first

tmux has had `next-prompt` and `previous-prompt` since 3.3, and they do nothing
until the shell emits `\033]133;A\033\\` at the start of each prompt. Almost
nobody wires that up, so a feature that ships with tmux sits unused on most
machines.

`tmux-companion shell-init zsh|bash|fish` prints the hook. Once the marks are
there, the daemon can answer questions it cannot answer today: jump from prompt
to prompt, copy the last command's output without selecting it by hand, rerun
the last command, and show the exit status and duration of what ran in each
window.

tmux does not forward OSC 133 to the outer terminal, which is
[tmux#5237](https://github.com/tmux/tmux/issues/5237), still open, so a
terminal's own shell integration goes quiet inside tmux. That is upstream's
problem and not mine, but it is worth knowing before promising anybody that
their Kitty or WezTerm features will work through this.

## What the bundle is worth, and how it gets measured

Small composable plugins are the right shape for the ecosystem and a cost that
lands on the person installing them. Ten features means ten repositories in
`tpm`, ten blocks of `tmux.conf`, several runtimes somebody has to already have,
and ten `#()` calls that each pay 14.6 ms of CPU per second per attached client.
The composability is real and so is the bill, and they are paid by different
people.

That's the argument for the bundle: one install, one config file, one process,
and segments that share a daemon instead of each starting their own. It's also
a claim, and a claim with no number behind it is marketing, so every feature
that arrives here from a plugin arrives with the measurement that says whether
the bundle was worth it.

The instrument exists. `bench-cpu.py` already measures server CPU from the
daemon's own `getrusage`, spawn CPU from `RUSAGE_CHILDREN` deltas around real
fork/exec, and end to end against a real tmux on its own socket, and the
plugin-stack arm is the same harness with a different bar in it.

### The unit is one idea, measured both ways

Not a headline number for the bundle. One row per idea brought in here, with
what it costs as the plugin somebody would otherwise install and what it costs
inside the daemon, measured on the same machine on the same day:

| Column | What goes in it |
| --- | --- |
| idea | the feature, one line |
| plugin | repository and the version or commit measured |
| plugin CPU | ms/s with one attached client at `status-interval 1` |
| companion CPU | ms/s for the same feature in the combined call |
| plugin setup | repositories, runtimes and lines of `tmux.conf` it needs |
| companion setup | config keys it needs, usually one |
| measured | date, tmux version, machine |

Warm latency joins the row for anything interactive, keypress to first frame,
since a picker is judged on that and not on CPU.

Setup cost is the column nobody publishes and half the reason people give up. A
plugin that needs Python, another that needs `fzf`, a third that shells out to
`jq`, and the person is four installs deep before their bar shows a battery
icon.

The stack total is then the sum of those rows against one measurement of the
combined bar, and the gap between the two is the daemon's actual contribution
rather than a claim about it. That gap is the number worth putting in the
README, and it can only be honest if every row under it was measured one idea at
a time.

### Fairness, which is what stops this being an advertisement

- the plugin arm runs the plugin's own recommended configuration, unmodified,
  at its own default interval
- both arms run on the same machine, the same tmux version and the same thermal
  state, interleaved rather than one after the other, which is what
  `bench-cpu.py` already does for spawn CPU
- the plugin's spawns get counted, not only its wall clock
- an arm that tmux-companion loses gets published as an arm that
  tmux-companion loses, and a feature that's genuinely cheaper as a plugin does
  not get built here
- every table states the tmux version, the machine and the date it was measured

The five-call configuration measured at 153.77 ms/s against 18.43 ms/s for the
combined call, so a ten-plugin bar should land somewhere near twice the "before"
arm. That's arithmetic rather than a measurement and it goes in the document
only once somebody has run it, per idea, both ways.

### Where the numbers live

A committed `bench-plugin-stack.sh` next to `bench.sh`, taking the arm to run as
an argument so nobody rebuilds the invocation from memory, and an entry in the
task runner on the day this repository grows one.

`BENCHMARKS.md` gains a table that grows by one row per idea, and the stack
total underneath it, when there are numbers to put in them. `README.md` gains one line with the headline figure and a link to
that section, because the person deciding whether to install this reads the
README and nothing else.

The per-feature rule from the port plan extends by one word: a ported feature
records the plugin it was measured against, by name and by version, in the same
commit.

## Credit, and what we don't reimplement

Ideas aren't owned and implementations are, so the question for each row above
isn't whether the idea is somebody else's. It's whether the plugin is alive,
whether it's the author's main project, and whether this repository says where
the idea came from.

That gives three tiers, and they decide what gets built rather than describing
what already happened:

**Nothing owed.** A tmux feature nobody enabled, like the OSC 133 marks and
`pipe-pane`, or a plugin that stopped: `tmux-plugins/tmux-cowboy` last pushed in
May 2021 and `alexanderjeurissen/tmux-world-clock` in 2021. Reimplementing an
abandoned plugin is closer to a service than to taking something.

**Credit, and a link that sends people to them.** Every alive plugin whose idea
lands here gets a line in `reference/` and in the `//!` header of the module,
naming the plugin, linking it, and saying plainly that somebody not running
tmux-companion should install theirs. `rickstaa/tmux-notify`,
`ofirgall/tmux-window-name`, `joshmedeski/tmux-nerd-font-window-name`,
`thepante/tmux-git-autofetch`, `roosta/tmux-fuzzback`, `MunifTanjim/tmux-suspend`,
`jaclu/tmux-power-zoom`, `nickdiego/tmux-pocket-pane`,
`kristopolous/tmux-gentrify`, `tmux-plugins/tmux-resurrect` and
`tmux-plugins/tmux-continuum` are the current list, and the last two are on it
for what they are rather than for being busy: neither has been pushed since
August 2024. A two-line issue on their
repository before shipping costs nothing, and an author who hears about it first
usually reacts better than one who finds out from a README.

**Don't build it.** A living plugin that is the author's signature project, where
integration buys the user little. Hint-based copy is the one this rule removed.

Code is a separate question from ideas. A function copied from a plugin carries
that plugin's licence and copyright header, and goes in a `THIRD-PARTY.md` with
the file, the upstream commit and the licence text. Most of these are MIT and
the `tmux-plugins` organisation is MIT, but that is per repository and gets
checked before anything is pasted, not after.

The reason to write this down rather than leave it to taste: this repository is
public now, so whatever norm I apply to other people's work is the one I am
inviting somebody to apply to mine.

## Daemon-native

These are all shell scripts on a timer, which is the shape the plugin API leaves
an author with. The daemon is already up, holding caches and a tokio runtime, so
the same idea lands here as a scheduled task rather than as a program that has
to start, do its work, and exit before the next tick wants it again. That is a
difference in where the code lives and not in how well it was written.

| Feature | Plugin today | Last push | What it reuses here |
| --- | --- | --- | --- |
| git autofetch | `thepante/tmux-git-autofetch` | 2026-09-12 | the `autosave` task shape, the repo cache |
| notify when a long command finishes | `rickstaa/tmux-notify` | 2026-05-18 | the `sh-jobs` process scan, plus exit code and duration |
| reload tmux.conf on save | `b0o/tmux-autoreload`, archived | 2024-02-16 | the config watch the daemon needs for `keys` anyway |
| online status, packet loss, ping | `tmux-plugins/tmux-online-status`, `jaclu/tmux-packet-loss` | 2023-09, 2026-08 | one async probe into a `TtlMap` |
| time spent per session | `tmux-code-time`, `tmux-timetrap` | | the usage log `keys` already writes |

Autofetch is the one I would build first of these, because the ahead and behind
counts the git segment renders are wrong until somebody fetches, and a status
bar that reports a stale number confidently is worse than one that reports
nothing.

## One table, four questions

The `[[sh-jobs.job]]` table maps a process-name regex to an icon, a label and a
colour. The same table answers three more questions:

- naming windows after what is running in them, which
  `ofirgall/tmux-window-name` does well and does as a Python daemon, so the
  version here removes a runtime as well as a process
- icons per window, which is `joshmedeski/tmux-nerd-font-window-name`
- which panes are running an AI agent and whether it is waiting for input,
  which is where the ecosystem is thinnest and newest: `tmux-agent-indicator`,
  `tmux-claude-status`, `tmux-scout`, `marmonitor`, `opensessions` and
  `tmux-agent-view` all appeared in `awesome-tmux` recently and none of them has
  settled

Both of the first two are alive and worth installing on their own, so what this
section is about is one config table answering more than it was written for
rather than replacing either of them, and the scan that feeds it is the scan
`sh-jobs` already pays for.

## Cheap after the matcher exists

| Feature | Plugin today | Last push | Note |
| --- | --- | --- | --- |
| fuzzy search of the scrollback | `roosta/tmux-fuzzback` | 2025-05 | `capture-pane` plus `nucleo` |
| filter the pane buffer by pattern | nothing found | | log triage, small once capture and the matcher are there |

Hint-based copy was in this table and has been taken out. `Morantron/tmux-fingers`
was pushed in June 2026 and `fcsonline/tmux-thumbs` is already Rust, and both
have been their maintainers' main project for years, which puts them squarely in
the third tier. The only argument for a fourth implementation was that the
pattern table would be shared with `open`, and that is not reason enough, so
`open` will ship a config snippet that hands off to thumbs instead.

## Segments that cost a fork today

Each of these is its own `#()` call in the configuration that ships with it,
and inside the combined right-hand side they cost nothing extra.

| Segment | Plugin today | Source of truth |
| --- | --- | --- |
| kube context and namespace | `tony-sol/tmux-kubectx` | `~/.kube/config`, watched by mtime |
| AWS profile and vault expiry | `mateimicu/tmux-aws-vault` | the vault session file |
| disk free | `tassaron/tmux-df` | `statvfs`, shown only under a threshold, same discipline as `net` |
| ssh user and host, per pane | `soyuka/tmux-current-pane-hostname` | per pane, where the `if-shell $SSH_CONNECTION` in `status-left` today is per server |
| CPU and memory of this session's processes | `sjdonado/tmux-workspace-usage` | the scan already happens for `sh-jobs` |
| macOS dark and light follow | `erikw/tmux-dark-notify` | pairs with `theme apply` |
| per-session colour from the session name | `imomaliev/tmux-peacock` | a hash to a hue, and `theme gen` already computes the contrast so the text colour falls out |
| world clock | `alexanderjeurissen/tmux-world-clock`, dead since 2021 | pure arithmetic |

## Twenty-line utilities almost nobody has

| Feature | Plugin today |
| --- | --- |
| zoom a pane into its own window and back | `jaclu/tmux-power-zoom` |
| kill the pane's process, TERM then KILL | `tmux-plugins/tmux-cowboy`, dead since 2021 |
| mute local bindings for a nested remote tmux | `MunifTanjim/tmux-suspend` |
| named side panes, toggled on demand | `nickdiego/tmux-pocket-pane` |
| move panes between windows with a cut and paste flow | `kristopolous/tmux-gentrify` |
| word and line copy on double and triple click | `aless3/tmux-click-copy` |
| list listening ports and kill from a picker | gone: `jrmoulton/tmux-port` 404s and no living equivalent was found |

`tmux-pocket-pane` is the one I keep looking at. It has a single star and the
idea deserves better than that, which is the whole reason star counts are not a
column in these tables: what a number like that measures is how many people
happened to find a thing published without a screenshot in a list of four
hundred, and that is closer to luck than to quality.

## A page instead of a feature

`tmux-plugins/tmux-logging` is a wrapper around `pipe-pane`, and with 1258 stars
it is one of the most-installed things in the ecosystem. The number is worth
quoting here because it is the argument: a thin wrapper gets installed that
often when the thing it wraps is hard to find, so the gap it fills is
documentation.

So `docs/how-to/things-tmux-already-does.md`, covering `pipe-pane`,
`display-menu`, `customize-mode` on prefix+C, `allow-passthrough`,
`link-window`, `join-pane -s`, `respawn-pane -k`, `select-pane -T` and the
`%if` version guards. It costs an afternoon and it probably improves more
people's setups than any segment in the tables above.

## Project layouts

These share one shape, and they arrive in this order because each one is what
makes the next worth having. The four-reason rule lands almost
entirely on rule 3 here and it is worth being straight about that: none of this
is a status segment so it costs no `#()` spawn, nothing here is resident because
every trigger is a keypress, and there is no runtime to remove. What it has is
that `layout_for`, `session_name`, `state_dir` and the `project` picker were all
built by the port already, and these three are the config table those pieces
were shaped for.

### Panes in a layout

`[[layout.window]]` takes a `command` and gives you one pane, which is what
`project-session.zsh` did and what the port kept. A window is usually more than
one pane, so the table grows a `pane` array, and the geometry comes from the
five layout names tmux already ships:

```toml
[[layout.window]]
name = "edit"
layout = "main-vertical"   # or even-horizontal, even-vertical, main-horizontal, tiled
main_size = "60%"

[[layout.window.pane]]
command = "nvim"
focus = true

[[layout.window.pane]]
command = "claude"

[[layout.window.pane]]
cwd = "~/src"
```

`command` on the window stays as the single-pane shorthand, a `pane` array
overrides it, and `Config::default()` does not move, so the pinned
`assemble_right` tests and the byte-identity rule are untouched.

The escape hatch for anything five preset names cannot express is a raw tmux
layout string in the same field:

```toml
layout = "bb62,272x67,0,0{136x67,0,0,1,135x67,137,0,2}"
```

which is what `tmux list-windows -F '#{window_layout}'` prints, so you arrange
the panes by hand with the bindings you already have and paste the result.
tmuxinator has taken raw layout strings for years and it is the reason its own
syntax never had to grow.

What this deliberately is not is a new layout language. The survey that produced
this section looked at tmuxinator, tmuxp, smug, teamocil, zellij and tmuxomatic,
and what carries over is that everybody who tried to reference panes by position
regretted it, because tmux renumbers panes by position the moment you split one,
and that zellij moved its layouts off YAML and TOML to KDL and said why in the
release notes: TOML does not nest, and layouts are nesting.
A flat pane array under a preset name sidesteps both, covers what people
actually run, and leaves the raw string for the rest.

One implementation detail that is easy to get wrong and expensive to debug:
create every pane as a bare shell first, then apply the layout, then send the
commands. Commands first means `nvim` and `claude` draw themselves at the
pre-layout geometry and repaint, which looks broken for a second on every
session start.

### Per-project layouts, and a key that saves one

Once a window can hold panes, the layout worth starting a project with is the
one you ended up with last time, and arranging it by hand and transcribing it
into TOML is the part nobody does twice. So a binding that captures it:

```
tmux-companion project save      # capture this session, write it for this project
tmux-companion project forget    # drop it, fall back to the named layout
tmux-companion project show      # which layout would this directory get, and from where
```

Capture is two calls, and `-a` means the count does not depend on how many
sessions are open:

```
tmux list-windows -a -F '#{session_path}|#{window_index}|#{window_name}|#{window_layout}'
tmux list-panes   -a -F '#{session_path}|#{window_index}|#{pane_index}|#{pane_current_command}|#{pane_current_path}|#{pane_start_command}'
```

No new session metadata is needed for the lookup, since `#{session_path}` is the
directory the session was created in and tmux never changes it, which is the
same key `session_name` already maps to a name. A `@tmux-companion-project`
session option set at spawn is worth adding anyway as belt and braces, because
it survives a rename and it says which project a session belongs to when the
directory was resolved through zoxide rather than typed.

Resolution is three sources, most specific wins, and `save` writes to the middle
one:

```
[[layout]] + [project] layout + [[project.override]]      what exists today
  <  $XDG_STATE_HOME/tmux-companion/projects/<project>.toml
  <  .tmux-companion.toml in the project root             opt-in, see below
```

One file per project rather than one shared file, so `save` never rewrites
something a person hand-maintains, `forget` is a delete, and there is no comment
preservation problem. The real path goes in a `path` key inside the file as the
authority, so a filename collision is detectable instead of silent.

`project show` earns its place the moment there are three sources, for the same
reason `doctor` earns its place: a person whose session came up wrong needs to
know which file won before they can fix anything.

Capture is lossy and the tool should say so rather than pretend. A pane sitting
at a prompt reports `pane_current_command` as `zsh`, and writing that back gives
you a shell inside a shell on restore, so it has to be compared against `$SHELL`
and dropped. `nvim src/config.rs` reports `nvim` with the arguments gone, and
`pane_start_command` has the full line but is empty for any pane that was split
interactively. An agent that renames itself to its own version string is the
problem `hold_name` already exists for. So capture writes what it found, marks
the panes it was unsure about, and prints the path, because a template you will
re-run for months deserves one human pass before it is trusted.

**The in-repo file is the part with a security surface.** A
`.tmux-companion.toml` at a project root runs commands when somebody presses the
project key, so cloning a repository you do not control and opening it would
execute whatever that file says. This is the same problem direnv has with
`.envrc` and vim has with `set exrc`, and both landed on the same answer: the
file is inert until the user explicitly trusts that path. If it gets built it
should be off by default under `[project] read_project_file`, and a trusted path
should be recorded with the file's hash so that editing it after trusting asks
again. This is the least-demanded of the three tiers and the only one that can
hurt somebody, so it goes last or not at all.

### The layout a project comes back with

The step after saving by hand is not having to do it as a separate act. Close a
project, open it again, and the windows and the geometry are what they were,
without any of the processes coming back.

Nothing watches for this and nothing runs on a timer. Every trigger is somebody
pressing a key: `project save` when you want to keep the arrangement you just
built, and a close binding that captures before it exits.

```
bind X run-shell 'tmux-companion project close'
```

`X` is free in tmux's default table, where `x` is kill-pane and `&` is
kill-window, so the binding does not take anything away from anyone.

The order inside that one command is the whole feature, and it is why the
binding calls the binary rather than chaining tmux commands: capture, write the
file, then kill the session. A `run-shell` that killed the session first would
have nothing left to read, and two chained tmux commands leave the write racing
the kill. One process doing them in order cannot get that wrong.

Dropping the timer takes the awkward parts with it. There is no `session-closed`
hook to work around, which matters because that hook fires after the session's
windows are gone and there is nothing left to read. There is no periodic cost to
measure or to give a config key for turning off. And there is no split between a
rolling snapshot and a deliberate save, so one project has one file, `project
save` and `project close` write the same one, and the timer that would have
quietly overwritten a template you sat down and built does not exist.

The honest cost of that choice: closing a project any other way keeps the last
file you wrote and loses whatever you rearranged since. Killing the session,
closing the terminal window, the last pane exiting, a reboot. Resurrect already
covers crash recovery across the whole server and this does not try to, so the
rule is simple enough to say in one line in the docs, which is that the layout
is what it was when you last pressed one of the two keys.

Layout only is the point and it is also what keeps this safe. Nothing about
restoring a pane's shape can run anything, so what gets stored is window names,
the `window_layout` string and each pane's working directory. Whether `project
close` also records commands the way `project save` does is one flag on one
capture function rather than a second code path.

Applying it on open looks like it contradicts what the README says about
resurrect, and it does not, for a reason worth writing down before somebody
raises it. The objection to automatic restore is that it resurrects a stale
layout over a session you have already started working in. A session the project
picker just created is empty, so there is nothing to clobber, and the saved
layout is the only layout it has.

The failure mode that does need a config key is the screen it comes back on. A
`window_layout` string carries absolute cell geometry, so `select-layout`
rescales it proportionally and six panes captured on a 272x67 display land on a
120x30 laptop below the minimum pane size and come back mangled. The capture
should store the window size it was taken at, and a restore onto something much
smaller should fall back to `tiled` rather than insisting on the saved string.

This is not a session manager and it should not grow into one. There is no
session list, no switcher and no session naming scheme here, because `project`
already does all three and `sesh`, `sessionx` and `t` do them for everybody
else. The scope is the layout of a session the project picker made.

Credit where it belongs: `tmux-plugins/tmux-resurrect` and
`tmux-plugins/tmux-continuum` are unarchived, are the maintainers' signature
work,
and this repository already shells out to resurrect's save script for
`autosave`. They snapshot every session on a timer for crash recovery and
restore on demand, across the whole server, processes included. This is one
project, layout only, written when a key is pressed, applied to a session that
is one second old. Different scope and different trigger, no code shared, and
the measurement row for it names both plugins as the arm it is measured against,
with their versions, per the rule above.

## Not this

- themes. `catppuccin`, `dracula`, `rose-pine` and `tokyo-night` own that
  ground and do it better; `theme` applies a theme, it doesn't compete with one
- session managers. `sesh`, `sessionx` and `t` are crowded and good, and
  `project` already covers what I use
- `tilish`, `tilit`, `modal`. That's a window manager's worth of UX and a
  different product
- `1password` and `bitwarden` integrations. Handling somebody's credentials is
  a liability I don't want in a status bar
- spotify, weather, tickers, crypto. Unrelated to what the tool is for

## If I only pick five

Built, all five, and `docs/dev/after-port-checklist.md` carries the state of each
one. Every one of them is off until a config line turns it on, which was not
the plan when this section was written and became the plan as each one landed:
a daemon that starts fetching from a remote, renaming windows or sourcing a
config on its own is a surprise, and the argument for the bundle is that it
costs less than the plugins, not that it decides more.

What this section said when it was a plan: the first release after the port is
OSC 133 `shell-init`, git autofetch, tmux.conf autoreload, the finish
notification, and the job table driving window names. All five are daemon-native, none of them needs new UI, and together
they're smaller than any one of the pickers in the port.

What I don't know yet is whether the notification is worth it on a machine with
one attached client, since the whole point of it is the pane you aren't
looking at, and I've never measured how often that actually happens to me.

Panes in a layout sits outside that five and ahead of it, because it is the one
thing here that changes a config table the port already shipped rather than
adding a feature beside it, and because the two project-layout features under it
have nothing to capture until a window can hold more than one pane.
