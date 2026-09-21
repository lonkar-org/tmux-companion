# After the port

The port replaces `~/.config/tmux/comrades` and stops. Nothing in this file
starts before that is done, and it exists so the survey behind it does not have
to be repeated, and so the port plan can stay about the port.

Star counts and last-push dates were checked on 2026-09-22 against the GitHub
API. They are here as evidence of how many people use a thing, not as a ranking.

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
`jaclu/tmux-power-zoom`, `nickdiego/tmux-pocket-pane` and
`kristopolous/tmux-gentrify` are the current list. A two-line issue on their
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

Every plugin in this table is a shell script on a timer, because a plugin has
nowhere else to live. The daemon is already up, holding caches and a tokio
runtime, so each of these is a scheduled task rather than a program that has to
be started, do its work, and exit before the next tick wants it again.

| Feature | Plugin today | Stars | Last push | What it reuses here |
| --- | --- | --- | --- | --- |
| git autofetch | `thepante/tmux-git-autofetch` | 22 | 2026-09-12 | the `autosave` task shape, the repo cache |
| notify when a long command finishes | `rickstaa/tmux-notify` | 278 | 2026-05-18 | the `sh-jobs` process scan, plus exit code and duration |
| reload tmux.conf on save | `b0o/tmux-autoreload` | 123 | 2024-02-16 | the config watch the daemon needs for `keys` anyway |
| online status, packet loss, ping | `tmux-plugins/tmux-online-status` 185, `jaclu/tmux-packet-loss` 15 | | 2023-09, 2026-08 | one async probe into a `TtlMap` |
| time spent per session | `tmux-code-time`, `tmux-timetrap` | small | | the usage log `keys` already writes |

Autofetch is the one I would build first of these, because the ahead and behind
counts the git segment renders are wrong until somebody fetches, and a status
bar that reports a stale number confidently is worse than one that reports
nothing.

## One table, four plugins

The `[[sh-jobs.job]]` table maps a process-name regex to an icon, a label and a
colour. The same table answers three more questions:

- naming windows after what is running in them, which is
  `ofirgall/tmux-window-name` (297 stars, pushed 2026-09-20) and is a Python
  daemon, so this removes a runtime as well as a process
- icons per window, `joshmedeski/tmux-nerd-font-window-name` (226 stars,
  2026-09-21)
- which panes are running an AI agent and whether it is waiting for input,
  which is where the ecosystem is thinnest and newest: `tmux-agent-indicator`,
  `tmux-claude-status`, `tmux-scout`, `marmonitor`, `opensessions` and
  `tmux-agent-view` all appeared in `awesome-tmux` recently and none of them has
  settled

Four features, one config table, and the scan that feeds them is the scan
`sh-jobs` already pays for.

## Cheap after the matcher exists

| Feature | Plugin today | Stars | Note |
| --- | --- | --- | --- |
| fuzzy search of the scrollback | `roosta/tmux-fuzzback` | 188, 2025-05 | `capture-pane` plus `nucleo` |
| filter the pane buffer by pattern | nothing with traction | | log triage, small once capture and the matcher are there |

Hint-based copy was in this table and has been taken out. `Morantron/tmux-fingers`
has 1472 stars and was pushed in June 2026, `fcsonline/tmux-thumbs` has 1099 and
is already Rust, and between them they've been the maintainers' main project for
years. The only argument for a fifth one was that the pattern table would be
shared with `open`, which is not enough to justify competing with somebody's
signature work, so `open` will ship a config snippet that hands off to thumbs
instead of replacing it.

## Segments that cost a fork today

Each of these is its own `#()` call in the configuration that ships with it,
and inside the combined right-hand side they cost nothing extra.

| Segment | Plugin today | Stars | Source of truth |
| --- | --- | --- | --- |
| kube context and namespace | `tony-sol/tmux-kubectx` | 12 | `~/.kube/config`, watched by mtime |
| AWS profile and vault expiry | `mateimicu/tmux-aws-vault` | 2 | the vault session file |
| disk free | `tassaron/tmux-df` | 44 | `statvfs`, shown only under a threshold, same discipline as `net` |
| ssh user and host, per pane | `soyuka/tmux-current-pane-hostname` | 87 | better than the `if-shell $SSH_CONNECTION` in `status-left` today, which is per server and not per pane |
| CPU and memory of this session's processes | `sjdonado/tmux-workspace-usage` | 6 | the number you actually want, and the scan already happens |
| macOS dark and light follow | `erikw/tmux-dark-notify` | 99 | pairs with `theme apply` |
| per-session colour from the session name | `imomaliev/tmux-peacock` | 39 | a hash to a hue, and `theme gen` already computes the contrast so the text colour falls out |
| world clock | `alexanderjeurissen/tmux-world-clock` | 36, dead since 2021 | pure arithmetic |

## Twenty-line utilities almost nobody has

| Feature | Plugin today | Stars |
| --- | --- | --- |
| zoom a pane into its own window and back | `jaclu/tmux-power-zoom` | 63 |
| kill the pane's process, TERM then KILL | `tmux-plugins/tmux-cowboy` | 58, dead since 2021 |
| mute local bindings for a nested remote tmux | `MunifTanjim/tmux-suspend` | 180 |
| named side panes, toggled on demand | `nickdiego/tmux-pocket-pane` | 1 |
| move panes between windows with a cut and paste flow | `kristopolous/tmux-gentrify` | 17 |
| word and line copy on double and triple click | `aless3/tmux-click-copy` | 8 |
| list listening ports and kill from a picker | `jrmoulton/tmux-port` | small |

`tmux-pocket-pane` at one star is the one I keep looking at. The idea is good
and the adoption says nothing about the idea, since a plugin's star count
measures how many people found it, which for something published without a
screenshot in a list of four hundred is close to a measure of luck.

## A page instead of a feature

`tmux-plugins/tmux-logging` has 1258 stars and is a wrapper around `pipe-pane`.
That is the whole plugin, and it is one of the most-installed things in the
ecosystem.

So `docs/how-to/things-tmux-already-does.md`, covering `pipe-pane`,
`display-menu`, `customize-mode` on prefix+C, `allow-passthrough`,
`link-window`, `join-pane -s`, `respawn-pane -k`, `select-pane -T` and the
`%if` version guards. It costs an afternoon and it probably improves more
people's setups than any segment in the tables above.

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

The first release after the port: OSC 133 `shell-init`, git autofetch,
tmux.conf autoreload, the finish notification, and the job table driving window
names. All five are daemon-native, none of them needs new UI, and together
they're smaller than any one of the pickers in the port.

What I don't know yet is whether the notification is worth it on a machine with
one attached client, since the whole point of it is the pane you aren't
looking at, and I've never measured how often that actually happens to me.
