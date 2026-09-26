> Planning record from the port, kept for history; the current behaviour is in the reference docs.

# Porting comrades into tmux-companion

`~/.config/tmux/comrades` is 1,495 lines of zsh across 13 scripts, plus 294
lines of generators and probes in `mysetup/scripts`. This is the inventory of
what moves in here, so the companion stops being a status-bar renderer and
becomes the one binary the config talks to.

Anything that is not in `comrades` today is out of scope for this document.
`docs/dev/after-the-port.md` is where the survey of what could come afterwards
lives, so that the scope of this one stays what the title says.

The port is phase 1. Phase 0 is the crate it lands on, because a foundation
that holds 2,671 lines isn't automatically one that holds twice that with a
TUI on top. Running alongside both are two tracks, one step per phase each: a
documentation track, which is what turns a personal tool into one somebody else
can send a patch to, and a configuration track, which is what stops it being
configured the way one laptop is set up.

## Phase 0: the ground the port lands on

None of this is worth starting until the crate it lands in can take twelve
new subcommands without going soft. The port roughly doubles the production
code and adds a TUI, a matcher and a usage log, so whatever's loose now gets
multiplied rather than diluted. This is what a check of the tests and the code
actually found.

### What is already right

Worth saying first, because it decides how much of phase 0 is repair and how
much is addition. There are 255 unit tests against 2,671 lines of production
code and 2,862 lines of test, they pass, and the whole suite runs in 0.02
seconds with no subprocess and no hardware read. The pure functions really are
extracted: `advance` takes its clock, every `TtlMap` method takes the `Instant`,
and `assemble_right` is pinned byte for byte. `DESIGN.md` carries a module map
and `CLAUDE.md` carries the invariants. Only two clippy lints fire across the
whole crate. The problems below are structural and they are few, which is the
good case.

### What fails today

`cargo clippy --all-targets -- -D warnings` fails on two lints, a
`collapsible_if` at `segments/git.rs:159` and an inherent `to_string` at
`tmux/format.rs:158` that should be a `Display` implementation. The 37 `cargo
fmt --check` diffs across 12 files this paragraph used to report were already
gone when phase 0 step 1 ran, fixed somewhere in the status-bar rework, so the
formatting half of that step turned out to be done already. Both are one-line fixes and neither's interesting. What is
interesting is that they're there at all, which is the direct consequence of
there being no `.github` directory, and so nothing that runs either check.
A contributor's first pull request currently fails on formatting they had no way
to know about.

### The test gaps

Four files hold 488 lines with no test at all: `main.rs` at 283, `client.rs` and
`server/mod.rs` at 74 each, and `tmux/icons.rs` at 57. That's 18% of the
production code, and it isn't an arbitrary 18%. It's the argument plumbing,
the socket client, the accept loop and the singleton guarantee, which is to say
every part the port touches on its way in and no part that a segment test
reaches.

There's also no `tests/` directory and no `src/lib.rs`, so an integration test
can't be written even if somebody wanted one. The crate is a binary and nothing
outside it can link to it. The consequence is that a client and a server have
never been started together in a test, and the one path every port subcommand
depends on is the one path covered by eyeballing it.

`preview.rs` is the third gap, 223 lines of production code against 33 lines of
test, and it's the closest thing in the crate to the TUI work the pickers need.

Both `README.md` and `DESIGN.md` state the test count as a literal 255, which is
right today and won't be on the first commit of the port.

### Every command is defined three times

This is the finding that matters most, because it's the one the port
multiplies. Adding a command means writing it in three places:

1. a `Cmd` variant with its clap flags in `main.rs`
2. a hand-built `serde_json::json!` block in `run()`, also in `main.rs`
3. a `match` arm in `dispatch`, plus hand-written `req.args["key"]` reads

The two ends agree on nothing but string literals. `req.args["pane_pid"]` on a
key that was never sent, or was sent under a different spelling, returns
`Option::None` rather than an error, so a typo doesn't fail, it silently
changes behaviour. `run()` is 136 lines today for ten commands and grows by
roughly 15 per command, which puts it near 320 lines once the port lands.

The crate already contains the fix, applied once. `window` sends a typed struct
and the handler reads it with `serde_json::from_value::<WindowArgs>`, which
turns the whole class of mismatch into a parse error carrying a field name.
`gst`, `status-right`, `clients` and `vim-bg` still do it by hand. Phase 0 is
finishing what `window` started: one args struct per command, serialised on the
way out and deserialised on the way in, so the flags, the wire and the handler
share one definition.

### Modularization

`segments/` is the only extension point and it's named for what it renders, a
piece of the status bar. Six of the ported subcommands are not segments: they
are interactive, they own the terminal for as long as the user is looking at
them, and they exit with an action rather than a string. Putting `keys` in
`segments/` because it's the only folder available is how a module map stops
describing anything. The port needs a second axis alongside it, and the split's
already obvious from the protocol: a segment answers with bytes for the bar, a
command answers by doing something to tmux.

Adding `src/lib.rs` is the other half, and it is cheap. Move the modules behind
a library, leave `main.rs` as the thin binary, and integration tests become
possible in the same commit.

### Documentation

124 public items carried 19 `///` comments, which was 15%, and two files out of
eighteen had a `//!` module header. The comments that existed were unusually
good, several of them record the bug that caused them, so the gap was coverage
rather than quality. Phase 0 step 6 closed it: `missing_docs` found 197 items,
which is more than the 105 that count implied because struct fields and
enum variants count too, and the lint plus a `cargo doc` job with
`RUSTDOCFLAGS: -D warnings` is what stops it reopening.

`CLAUDE.md` has a seven-step "Adding a new segment" and it's accurate. There's
no equivalent for adding a command, which is what twelve of the next commits
will each be doing.

For a public repository there is no `LICENSE` and no `CONTRIBUTING.md`, and
nothing pins the toolchain, so the formatting a contributor gets depends on the
rustc they happen to have.

### The phase 0 list

0. transfer the repository to the `lonkar-org` organisation, before any of
   the following writes the URL down
1. `cargo fmt`, fix the two clippy lints, add a `rust-toolchain.toml`
2. a GitHub Actions workflow running `fmt --check`, `clippy -D warnings` and
   `test` on Linux, so the previous step cannot come undone
3. `src/lib.rs`, `main.rs` reduced to argument parsing, `tests/` created
4. one typed args struct per command, replacing the `json!` blocks and the
   `req.args["…"]` reads, with `window` as the template
5. an integration test that starts a server, runs a client against the real
   socket, and asserts on the round trip, plus tests for the singleton
6. `//!` headers on every module and `///` on every public item, with
   `#![warn(missing_docs)]` turned on so it stays that way
7. `LICENSE`, `CONTRIBUTING.md`, an "Adding a command" section in `CLAUDE.md`,
   and the hardcoded test count replaced with a sentence that does not rot
8. the config loader and everything under it, per the configuration section
   below: the precedence chain, `config path`, `config check`, `config dump`,
   the `[dirs]`, `[colors]`, `[glyphs]`, `[git] parts`, `[[layout]]` and
   `[status.right]` tables,
   `docs/config.example.toml` and `docs/tmux.conf.full.example`
9. `vim-bg` renamed to `sh-jobs` with a config-driven job table and a hidden
   alias for one release, done here because step 4 is rewriting its args anyway
10. a version field on the wire, a socket created 0600 and checked before use,
   and a `doctor` subcommand, per the open questions near the end

Step 0 is first because it's free now and annoying later. `lonkar-org` is the
namespace paired with the blog, `firacode-nfc-tweaked` and `blog-comments` are
already there, and the business sites in it are moving to the `icf-c`
organisation. Fewer than five clones exist, most likely from bots, so there's no
install line in anybody's config to break. Steps 1, 2 and 7 each write the
repository URL into something durable, the crates.io `repository` field, a CI
badge, the clone line in `CONTRIBUTING.md`, and doing the transfer afterwards
means editing all of them twice while a badge quietly points at a redirect. One
thing to remember afterwards: never recreate `yogeshlonkar/tmux-companion`,
because GitHub's redirect survives the move and dies the moment that name is
taken again.

Steps 3 and 4 are the ones with real work in them. The rest is an afternoon, and
all of it's cheaper now than after twelve subcommands have copied the current
shape.

## Configuration, since the repository is public

Everything the bar renders is a constant. 23 colour constants in
`tmux/format.rs`, 37 Nerd Font codepoints in `tmux/icons.rs`, a 20-colour
animation cycle and a 10-icon battery ramp with its four thresholds in
`segments/window.rs` and `segments/battery.rs`, a 20 KiB/s bandwidth floor, a
five second git TTL, a branch cut at 20 characters with a 10 character tail,
and the order of the three segments inside `assemble_right` with the tmux
literals between them. Two of those are past opinionated and into personal:
`load_dir_aliases` reads `~/.yrl/lib/dir-aliases`, and `DIR_LOGOS` in
`segments/window.rs` maps `~/g/mysetup`, `~/g/` and `~/b/` to icons, so the
directory layout of one laptop is compiled into a binary that a stranger is
being invited to `cargo install`.

The port gets a config file and it lands in phase 0 with the rest of the
foundation, since every phase after it adds settings, and fitting a config
layer under twelve subcommands afterwards costs more than building it under
two.

### The file

TOML, read with `toml` and a `serde` derive, which is one dependency and the
format anybody working in Rust already has open in `Cargo.toml`.

Resolution order, first hit wins:

1. `--config <path>`
2. `$TMUX_COMPANION_CONFIG`
3. `$XDG_CONFIG_HOME/tmux-companion/config.toml`, which is
   `~/.config/tmux-companion/config.toml` on a machine that doesn't set the
   variable
4. `~/tmux-companion.toml`
5. the built-in defaults, when there is no file at all

Two paths rather than one because the XDG directory is where a Linux user will
look and `~/tmux-companion.toml` is where somebody who wants it in front of
them will put it. Both carry the `.toml` extension, so an editor highlights the
file and `config check` can name the format in an error without guessing.

`tmux-companion config path` prints the one it picked, because the commonest
config question is which file is being read.

A key the parser doesn't recognise is an error naming the key and the line, via
`deny_unknown_fields`. A silently ignored key is the same bug class as
`req.args["pane_pid"]` on a name that was never sent, and phase 0 is already
removing that one from the wire.

The daemon refuses to start on a config it can't parse. Refusing is only
defensible if the person finds out why within seconds, so the error travels
three ways and each one is a different amount of room:

1. the daemon writes the full error to stderr and to
   `$XDG_STATE_HOME/tmux-companion/last-error`, then exits non-zero
2. the client that tried to start it reads that file and renders one line into
   the status bar, which is the only surface guaranteed to be in front of
   somebody: `config: unknown key 'colour' at line 12 — run tmux-companion
   config check`
3. `tmux-companion config check` prints the whole thing: the file it read, the
   line and column, the offending line with a caret under it, and, for an
   unknown key, the closest key that does exist

The bar line is deliberately short and deliberately names the command that
explains it, since a status bar has about 150 columns and a parse error doesn't
fit in them. `config check` exits non-zero, so it also works in a pre-commit
hook or in CI.

`toml` carries line and column on its errors and `serde` names the field, so
none of this needs a hand-written parser. The did-you-mean comes from an edit
distance over the known key names, which is the one piece with any code in it.

### Reload

The daemon parses the file once at start and holds an `Arc<Config>` in
`ServerState`. `tmux-companion reload` re-reads it, and the daemon compares the
config's mtime on the tick it will already be running to watch `tmux.conf` for
`keys`, so a colour change survives `prefix+r` without a `pkill`.

Resolution stays off the hot path. Segments take a `&Config`, and the
`status-right` composition is turned into a render plan at load rather than
looked up per segment per second, since `DESIGN.md` opens by saying the daemon
exists to stop repeated reads of config files and a config file read per render
would be a poor joke.

### tmux user options, and why not

The tmux-native idiom is `set -g @companion-git-ttl 5`, and that is how most
tmux plugins take their settings. It costs a `show-options` round trip per
option per render unless the daemon caches them, and then there are two sources
of truth with a cache in between, which is the invalidation bug this port is
already deleting from `keys.zsh`. The file is read once by a daemon that is
running anyway.

What survives is a short whitelist, and only for settings that have to differ
per session, which is what a file can't express at all:

| Option | Does |
| --- | --- |
| `@tmux-companion-theme` | the theme for this session, overriding `[theme]` |
| `@tmux-companion-layout` | the `[[layout]]` a new window in this session starts from |
| `@tmux-companion-status-right` | the segment list for this session's bar |

Three options, flat, read once per session and cached, and not a second copy of
the config tree. A general mapping would give you `@tmux-companion-git-parts-3`
and `@tmux-companion-layout-window-2-command`, which is a config language built
out of hyphens by accident, and it stops working the moment a value needs to be
a list. Anything nested lives in the file and gets a name there, so a session
says `@tmux-companion-layout work` and `work` is a `[[layout]]` in the file
with as much structure inside it as it likes.

### What moves into the config, and when

Same rule as the documentation track: a phase isn't finished until the
constants it touched are settings and its row here is done.

| Phase | Settings |
| --- | --- |
| 0 | the loader, the precedence chain, `config path`, `config check`, `config dump` |
| 0 | `[dirs]` aliases and logos, which is what kills `~/.yrl/lib/dir-aliases` and `~/g/` |
| 0 | `[colors]`, and `[glyphs]` with a `preset` key and per-glyph overrides; `nerd-font-v3` and `ascii` ship here, `powerline`, `unicode` and `none` land later as data files |
| 0 | `[git]`, `[network]`, `[battery]`, `[window]`: TTLs, thresholds, the branch lengths, and file defaults for the flags that already exist on the CLI |
| 0 | `[status.right]`, the segment list, the separators between them and the end cap, all of which accept an empty string |
| 0 | `[git] parts`, an ordered list of what the git segment renders, so nobody is given counts they didn't ask for |
| 1 | `[theme]` directory and palette source for `theme gen` |
| 2 | `[keys]`, `[cheatsheet]`, and `[usage]` with its path and an off switch |
| 3 | `[project]` sources, so zoxide is one option rather than the assumption, plus the project map and `[[layout]]`, the windows a new session starts with |
| 4 | `[autosave]` interval and enable, `[toggle]` window names, which come from the layout rather than from `edit` and `ai` being hardcoded |
| 5 | `[run]` history source (zsh, bash, fish, atuin), pane side and width, animation, default dialog button, the shell commands run under |
| 6 | `[open]` opener command, `[clipboard]` copy command for pbcopy, xclip or wl-copy |

### Glyphs, since most people don't have a Nerd Font

`git.rs` renders 37 codepoints from a patched font, and to somebody without one
the segment is a row of boxes. `README.md` states a Nerd Font requirement
without naming which font or which version, so that reader can't even tell
whether their install worked.

Five presets, chosen by what a font is guaranteed to carry rather than by taste:

| Preset | What it assumes | For |
| --- | --- | --- |
| `nerd-font-v3` | the full Nerd Fonts v3 private-use range | today's bar, unchanged |
| `powerline` | the powerline range alone, U+E0A0 to U+E0B3 | the many terminals patched years ago and never re-patched |
| `unicode` | box drawing and geometric shapes any modern font has | a stock font with no patching at all |
| `ascii` | 7-bit | ssh into a box whose font nobody controls |
| `none` | nothing | text only |

A preset is a starting point and not a cage: `[glyphs.icons]` overrides one
glyph at a time, so somebody who has exactly one icon missing fixes that icon
rather than dropping to a whole preset below.

Phase 0 builds the whole mechanism and ships two of the five, `nerd-font-v3`
and `ascii`. The mechanism is the part with code in it and a preset is a table
of names to strings, so each one lives in its own TOML file pulled in with
`include_str!`, and `powerline`, `unicode` and `none` arrive later as data with
no Rust attached. That also makes a preset the easiest first contribution a
stranger can make: a file, a test that renders it, no logic.

Separators get the same treatment and the same escape hatch. The segment
separator, the end cap and the powerline wedge each accept an empty string,
because "I want nothing between these two" is a real preference and today it
isn't expressible at all.

### The git segment is a list, not a fixed shape

Everything `GitStatus` knows gets rendered today, in one order, whether or not
the reader wanted it. Somebody who works on a branch with 400 untracked build
artifacts doesn't want a count of them, somebody who never pushes doesn't want
ahead and behind, and somebody who only wants a branch name should be able to
say so:

```toml
[git]
parts = ["branch", "sync", "staged", "modified", "untracked", "stash"]
counts = true          # false renders presence, not numbers
hide_when_clean = false
```

The list decides both which parts appear and in what order, an omitted part
isn't rendered, and an unknown part name is the config error the loader already
knows how to raise. Available parts come straight from the struct: `branch`,
`type` for the feature and bugfix and chore glyphs, `ahead`, `behind`, `sync`
for the remote state, `staged`, `modified`, `untracked`, `deleted`, `renamed`,
`copied`, `conflicts`, `stash`.

### Layouts, because `edit` and `ai` are my two windows

`M-s` starts a session with an editor window and an AI window, and both of those
are mine. Somebody else runs `vim` or `nano` or `helix`, or `codex` or `gemini`
or `cursor` or nothing at all, and somebody else again wants a window running
`tail -f` on a log and no editor anywhere:

```toml
[[layout]]
name = "default"

  [[layout.window]]
  name = "editor"
  command = "nvim"

  [[layout.window]]
  name = "ai"
  command = "claude"

[[layout.override]]
match = "~/work/*"
use = "work"
```

`project` starts a session from the layout, `toggle` cycles the windows the
layout named instead of the two names compiled into `toggle-tool.zsh` today, and
a layout with no windows in it is a plain shell, which is what somebody who
wants none of this gets by writing nothing.

Two levels is all tmux has, a window and its panes, so panes are a list inside
the window entry with an optional split direction, and that is where the nesting
stops.

### Why TOML and not YAML

The session layout above is the one part of this config that would read better
in YAML, and every tool in that space uses it: `tmuxinator`, `teamocil`, `smug`
and `tmuxp` are all YAML. So the question is real and the answer is still TOML.

The deciding fact is maintenance rather than syntax. `serde_yaml` was archived
by its author on 2024-03-25 and its last release is `0.9.34+deprecated`. The
forks that inherited it, `serde_yaml_ng` at 0.10.0 from May 2024 and
`serde_norway` at 0.9.42 from December 2024, have both sat still for about two
years. `toml` is at 1.1.6, was updated on 2026-09-10, and has 925 million
downloads. Those figures were checked on 2026-09-22. Picking YAML means picking
an unmaintained parser for a program somebody installs on their machine.

After that the smaller reasons agree with it. YAML 1.1 coerces `no`, `off`,
`yes` and `on` into booleans, which bites a config whose values are short
strings like `no` for a glyph and `on` for a separator, and quoting fixes it
right up until somebody forgets. YAML is whitespace-significant, so a block
copied out of a README with the wrong indentation or a tab fails, while TOML
doesn't care. And somebody installing a Rust program already has `Cargo.toml`
open, with `starship`, `alacritty` and `helix` configured the same way.

Multi-format loaders exist, `config` and `figment` both do it, and taking one
would mean two sets of error messages, two example files and a question in every
issue about which file won. One format, one `config.example.toml`, one parse
error.

If somebody turns up with a layout deep enough that `[[layout.window]]` genuinely
hurts, layouts can move to their own file and that file can have its own loader.
That's a change to one table rather than to the format everything else is in.

### The test that keeps my bar the same

`Config::default()` has to render byte for byte what the binary renders today,
and the `assemble_right` tests that are already pinned become the proof of it.
Two more: every field has a default, so an empty file is valid, and
`config dump` writes the commented `docs/config.example.toml` that CI then
diffs against the committed copy, which is the same trick the CLI reference
uses to stop `--help` drifting away from the docs.

### `vim-bg` becomes `sh-jobs`

The segment asks one question with one answer baked in: is there a suspended
`nvim` under this pane. `has_suspended_nvim` matches a process name containing
`nvim` and the render string in `segments/vim_bg.rs` spells out `nvim` in three
colours. Somebody who suspends `vim`, or `claude`, or a `cargo watch`, gets
nothing, and the name of the subcommand tells them the tool wasn't written
for them.

So it gets renamed to `sh-jobs` and takes its matches from the config:

```toml
[[sh-jobs.job]]
match = "nvim"
icon  = "󰕷"
label = "nvim"
color = "#539035"

[[sh-jobs.job]]
match = "^claude$"
icon  = "󰚩"
color = "#d97757"
```

First match in file order wins, `states` decides whether a background job
counts or only a stopped one, and `max` bounds how many icons a busy pane can
put on the bar. `vim-bg` stays as a hidden alias for one release with a
deprecation line in `CHANGELOG.md`, since it is in my `tmux.conf` and possibly
in somebody else's by the time the rename lands.

The cost doesn't change with the rename and it's the reason the segment is
off the bar: 16.25 ms of server CPU per call, because `sysinfo::System`
enumerates the whole process table to find children of one pid. A refresh
scoped to the pane's children instead of everything might cut most of that, and
I haven't measured it, so the phase that does the rename measures it and
either earns the segment a place on the bar or writes down what it costs to put
it there.

### The two tmux.conf examples

`docs/tmux.conf.example` is the recommended bar and stays what it is: one
`#()` call, 18.43 ms/s of CPU, which is 1.8% of one core. It gets there by
leaving three subcommands off the bar, so it documents the cheap configuration
and not the tool.

The port adds `docs/tmux.conf.full.example`, which turns everything on: the
`window` segment driving `window-status-format`, `clients`, `sh-jobs`, `gst`
with `#{pane_pid}`, and every picker binding from the comrades port. Each block
carries the measurement next to it rather than a warning in the abstract:

| Feature | Cost | Where it is measured |
| --- | --- | --- |
| each extra `#()` on the bar | 14.6 ms of CPU per second per attached client | `BENCHMARKS.md`, fork/exec |
| `sh-jobs` on the bar | 16.25 ms of server CPU per call, plus its spawn | process-table scan |
| `clients` on the bar | 4.93 ms per call, plus its spawn | `tmux list-clients` |
| `#{pane_pid}` on `status-right` | 18.5 ms per gst call | the suspended-job scan again |
| `window` in `window-status-format` | one spawn per window per redraw | eight windows is eight spawns |
| everything on, five calls | 153.77 ms/s, 15.4% of one core | the "before" column |

The full example is the honest one: somebody who wants the suspended-job marker
on their bar can have it, and they can see the 15.4% before they choose it
rather than after their fans come on.

### `docs/config.example.toml`

Nobody has to create a config file, so the reference copy has to carry every
key with its default and a comment saying what it does, and it is generated by
`tmux-companion config dump` rather than typed, with CI diffing the generated
output against the committed file. A hand-maintained example config goes stale
in one phase, and a stale one's worse than none, because a reader copies it.

`reference/configuration.md` is the prose version, and it moves from phase 3 to
phase 0 for the same reason the loader does.

## The documentation track

Phase 0 fixes the crate. This fixes the thing around the crate, and it runs as
one step per phase rather than as a block at the end, because documentation
written at the end is written from memory and reads like it.

The shape is [Diátaxis](https://diataxis.fr): four kinds of document, each
answering a different question, each failing when it tries to answer two.
Tutorials teach a beginner by having them do something that works. How-to
guides solve one problem for somebody who already knows what they want.
Reference describes the machinery and nothing else. Explanation is the
discussion that doesn't fit in any of the other three, which here is most of
`DESIGN.md` and all of `BENCHMARKS.md`.

### Where the current docs sit

| File | Diátaxis mode | State |
| --- | --- | --- |
| `README.md` | all four at once | screenshots, a pitch, a quick start, a tmux.conf how-to, `gst` reference and server-lifecycle explanation in 155 lines |
| `DESIGN.md` | explanation, with reference inside it | strong; the module map and the protocol are reference wearing an explanation's clothes |
| `BENCHMARKS.md` | explanation | strong, and the best writing in the repo |
| `CLAUDE.md` | how-to | two good guides, in a file humans don't open |
| `docs/tmux.conf.example` | reference | good, and the only thing already in the right mode |

The tutorial quadrant is empty. Nothing takes a stranger from a clone to a
status bar they can see, which is the one document that decides whether anybody
else ever runs this.

The reference quadrant is scattered. Ten subcommands carry roughly thirty flags
and no page lists them; the JSON wire format is described in prose in
`DESIGN.md` and defined in `proto.rs`; the two environment variables that exist,
`TMUX_COMPANION_SOCK` and `HOME`, are documented nowhere.

### Requirements, which are currently wrong

`README.md` line 140 says "Requires Rust 1.82+ (edition 2024)". Edition 2024
needs 1.85, so the stated minimum can't build the crate. `Cargo.toml` has no
`rust-version` field, so nothing checks it either, and cargo reports a type
error in the edition rather than a clear message about the toolchain.

`Cargo.toml` is also missing `description`, `license`, `repository`, `readme`,
`keywords` and `categories`, which is the whole of the crates.io metadata and
the reason the crate can't be published as it stands.

Nothing states a minimum tmux version. That's survivable today and isn't after
the port, because `display-popup -E` is tmux 3.2 and every picker depends on it.
The nerd-font requirement is stated but not named, so a reader can't tell which
font or which version supplies the glyphs.

### Target tree

```
README.md            front door: what it is, the screenshots, one link per mode
CONTRIBUTING.md      how to build, test, lint and open a pull request
CHANGELOG.md         keep-a-changelog, one entry per phase
LICENSE
docs/
  config.example.toml       every key, its default and a comment; generated
  tmux.conf.example         the cheap bar, one `#()` call
  tmux.conf.full.example    every feature on, each with its measured cost
  tutorial/
    getting-started.md      clone to a visible status bar, no prior knowledge
  how-to/
    integrate-tmux-conf.md
    add-a-segment.md        moved out of CLAUDE.md
    add-a-command.md        written while doing it, not before
    change-icons.md         moved out of CLAUDE.md
    run-the-benchmarks.md
  reference/
    cli.md                  every subcommand and flag
    protocol.md             the JSON request and response
    configuration.md        environment, tmux user options, state files
    modules.md              the module map, moved out of DESIGN.md
    requirements.md         Rust, tmux, fonts, platforms
  explanation/
    architecture.md         DESIGN.md minus the reference parts
    performance.md          BENCHMARKS.md
    after-the-port.md       the survey of what could come next, moved from docs/
```

`CLAUDE.md` stays where it is and shrinks to the invariants, which is the part
that's genuinely for an agent rather than a person.

### One step per phase

The rule is that a phase isn't finished until its row is done. The work lands
next to the code that motivated it, which is the only time anybody knows what to
write.

| Phase | Documentation step |
| --- | --- |
| 0 | `LICENSE`, `CONTRIBUTING.md`, the `docs/` skeleton, `reference/requirements.md` with a verified minimum, `reference/configuration.md` and the generated `docs/config.example.toml`, `docs/tmux.conf.full.example` with a cost against every feature, `rust-version` and the rest of the crates.io metadata in `Cargo.toml`, `README.md` cut down to a front door |
| 1 `theme gen` | `reference/cli.md` started with the first subcommand, and the contrast arithmetic written up in `explanation/` while it's fresh |
| 2 `keys`, `cheatsheet` | `how-to/add-a-command.md`, written from having just done it twice, and `tutorial/getting-started.md` extended to the first picker |
| 3 `project` | the theme map and the project map file formats appended to `reference/configuration.md`, which by then already exists |
| 4 `autosave`, `toggle` | `explanation/architecture.md` updated for daemon-owned background tasks, since that's a change in what the daemon is |
| 5 `theme`, `run` | a themes tutorial, because choosing a theme is the one thing in here a new user will want on day one |
| 6 the rest | `reference/protocol.md` finalised, `reference/modules.md` regenerated, `CHANGELOG.md` closed for the port |

### What makes it enforceable

Documentation that isn't checked goes stale at the speed of the code, so three
of these get a CI job rather than a good intention:

- `cargo doc --no-deps -D warnings` with `#![warn(missing_docs)]`, so a new
  public item can't land undocumented
- a link checker over `docs/`, so a moved file fails the build rather than
  rotting into a 404
- a test that diffs `tmux-companion --help` and each subcommand's `--help`
  against `reference/cli.md`, so the CLI reference can't drift from the CLI
- a test that diffs `tmux-companion config dump` against
  `docs/config.example.toml`, so a new setting can't land without appearing in
  the file people copy from
- `rg --hidden '@(Yogesh|claude)\('` over the tree, so an open question in a
  doc can't be merged as if it were answered

The rest of the gold-standard list is small and mechanical: a code of conduct, a
pull request template naming the three commands a contributor should run before
pushing, an issue template that asks for the tmux and binary versions, a release
workflow that attaches built binaries to a tag, and `cargo deny` in CI once
there's a licence to enforce.

## What already exists, and why that matters

The port is cheaper than the line count suggests, because most of the hard
parts are built:

- a daemon holding state in memory, with clients that connect over a Unix
  socket and exchange one JSON line (`src/server/`, `src/client.rs`,
  `src/proto.rs`)
- a TTL cache whose freshness is decided by the reader (`src/cache.rs`, 223
  lines)
- a tmux format and colour layer (`src/tmux/format.rs`, 386 lines)
- process and tty inspection, including `has_suspended_nvim`
  (`src/segments/vim_bg.rs`, which phase 0 generalises into `sh-jobs`)
- a preview path that renders a segment to ANSI for eyeballing
  (`src/preview.rs`)

The daemon is the part that changes the arithmetic. `keys.zsh` currently keeps
its rows in `keys-cache.tsv` and rebuilds them when `tmux.conf` is newer, which
is a file, an mtime comparison and an invalidation bug waiting to happen. A
daemon that already holds caches can hold this one, watch the config itself,
and answer a client in well under a millisecond, which deletes the cache file
and the `--build` and `--refresh` flags with it.

## Group 1: pickers

These need a fuzzy matcher (`nucleo`) and a TUI (`ratatui`). They're the only
group where the port changes what the user feels: a warm `prefix+?` is 93ms
today, of which 50ms is fzf starting up and 33ms is `display-popup`. Drop fzf
and it's about 38ms.

| Subcommand | Replaces | zsh lines | Needs |
| --- | --- | --- | --- |
| `keys` | `keys.zsh` | 120 | matcher, TUI, `list-keys` parse, usage log |

| `cheatsheet` | `cheatsheet.zsh` | 99 | box layout, usage counts |
| `project` | `project-session.zsh`, `project-preview.zsh`, `zoxide-window.zsh`, `short-path.zsh` | 272 | matcher, TUI, zoxide read, theme map |
| `theme` | `choose-tmux-theme.zsh`, `preview-tmux-theme.zsh` | 300 | matcher, colour parse, swatch render |
| `run` | `run-command-pane.zsh` | 235 | matcher, history parse, pane control, resize animation, exit dialog |
| `open` | `open-from-text.zsh` | 277 | URL and `file:line:col` extraction, pane reuse |

`theme` is the one with a head start: `preview-tmux-theme.zsh` parses
`colour125`, the eight base names and hex triplets, and `src/tmux/format.rs`
already does colour work for the status bar.

`open` is the most self-contained and the least like the others, since it has
no picker in two of its four modes and mostly does regex extraction and one
tmux call.

### What `run` has to keep

It's the one picker whose behaviour isn't obvious from its name, so the details
are here rather than in a diff nobody reads later. Pick a command from history,
run it in a new full-height pane on the right at 33% of the window width, and
when it exits show a dialog centred over that pane offering Close, View and
Restart, with Close preselected on success and Restart on a non-zero exit.

The parts that are easy to drop and then miss:

- the pane opens at one column and slides out to its width over five ease-out
  steps in about 150 ms, because tmux has no animation primitive and a stepped
  `resize-pane` is the closest thing to an IDE sliding a panel out; the step
  count is a tradeoff against the neighbour pane's shell repainting on every
  `SIGWINCH`
- what you typed becomes the command when nothing matches, and `M-Enter` runs
  the query even when something does, which is what makes the picker usable for
  a command that was never in the history
- history is deduplicated, most recent first, and `fc -ln` escapes real
  newlines, so an entry is unescaped when it's a selection and never when it's
  the typed query
- `display-popup -E` blocks until it closes, which is what makes the choice
  file complete when it returns; a fifo deadlocks there because both sides
  block, and the comment saying so stays in the Rust
- if the popup can't open, on a tiny or detached client, the dialog falls back
  to an inline one-key prompt rather than failing
- View puts the pane in `copy-mode` read-only and any key brings the dialog
  back, Restart loops without reopening the pane, and Close animates the pane
  back down to one column before exiting

Its config: which side and what width, the animation in steps and milliseconds
or off entirely, the default button per exit status, the shell the command runs
under, and the history source, which is `zsh` today and wants `bash`, `fish` and
`atuin` as options since anybody using atuin has no `.zsh_history` worth
reading.

## Group 2: orchestration

I argued against moving these on the grounds that they're ten tmux commands in
a row and Rust buys nothing. That's true measured on its own, and beside the
point if the aim is one binary: leaving four scripts behind means the config
still needs zsh, still needs a `comrades` directory, and the thing is still not
a companion.

| Subcommand | Replaces | zsh lines | Notes |
| --- | --- | --- | --- |
| `close-project` | `close-project.zsh` | 87 | quits nvim with `:xa`, waits, falls back; the wait loops want a real timer |
| `toggle` | `toggle-tool.zsh` | 25 | edit and ai windows, falls back to `last-window` |
| `autosave` | `session-autosave.zsh` | 80 | becomes a daemon task, so the lock file and the sleep loop both go |

`autosave` is the one that gets better rather than merely equivalent. It's a
detached zsh loop with a PID lock today, guarding against `prefix+r` starting a
second copy. Inside a daemon that already runs tokio it's a scheduled task, and
the whole class of problem disappears.

## Group 3: generators and probes

One-shot tools that don't need a TUI, and each one removes a dependency.

| Subcommand | Replaces | lines | Removes |
| --- | --- | --- | --- |
| `theme gen` | `scripts/tmux-theme-gen.py` | 176 | the python3 dependency |
| `probe keys` | `scripts/probe-key-sequence.sh` | 49 | raw termios handling in shell |
| `probe cells` | `scripts/probe-cell-width.sh` | 69 | the DSR round-trip, which already fixed one bug about reading from /dev/tty rather than stdin |

`theme gen` computes WCAG contrast to choose black or white text per theme and
mints shades along the xterm cube, and it produced the 76 theme files that
exist now. It's pure arithmetic with no I/O beyond reading and writing files,
which makes it the easiest thing here to port and the easiest to unit test.

## Group 4: logic currently inline in tmux.conf

| What | Where | Why it should move |
| --- | --- | --- |
| vim and fzf detection by `ps -o state= -o comm=` | `User0` bind and four navigator mappings | the same regex in five places, and `vim_bg.rs` already knows how to ask this question |
| clipboard switch between `pbcopy` and `xclip` | two `if-shell` branches on `uname` | one binary picks the right clipboard, which is one less thing the Linux branch has to special-case |
| the `bind z` pane-or-window toggle | a `run-shell` wrapping an `if-shell` | shelling out to tmux from tmux to ask how many panes there are |
| theme applied on `session-created` | a hook calling the theme script | becomes `theme apply --session` |

## What stays in tmux.conf

Bindings, options, hooks, styles and formats stay declarations. So does the
right-aligned `message-format` with its left-align escape for `:` and `(`,
because it's a format string rather than code, and the mouse-wheel `if-shell`
chains, which are tmux conditionals with nowhere else to live. Popup geometry
stays on the binding.

## Order

0. phase 0 above, all of it, before any of the following
1. `theme gen`, because it's pure arithmetic and drops python3 on its own
2. `keys` and `cheatsheet`, because they share the matcher and the usage log and
   they're the pair where the 93ms becomes 38ms
3. `project`, once the matcher and the picker chrome are proven by `keys`
4. `autosave` and `toggle`, small, and `autosave` gets structurally better
5. `theme` and `run`, the two biggest pickers
6. `open`, `close-project`, the probes and the tmux.conf logic

Each of these carries the documentation row and the configuration row from the
tables above, plus its before and after numbers in `BENCHMARKS.md`, and is not
finished without all three.

## Branches, and what lands in main

A phase is weeks of work and it can't sit on one branch until the end, because
the tmux.conf on this laptop has to keep working the whole time and a branch
nobody merges is a branch nobody rebases either. So the unit is not the phase,
it's the smallest change that leaves `main` installable: the binary builds,
the tests pass, the bar renders, and `docs/tmux.conf.example` still describes
what the binary does.

One branch per row of the phase tables, not one per phase. Phase 0 has eleven
steps and they're independent enough to land separately, which is what
`port/0-fmt-clippy-toolchain` landing on a Tuesday and
`port/0-config-loader` landing the following weekend looks like in practice.

Naming: `port/<phase>-<thing>`. `port/0-typed-args`, `port/0-config-loader`,
`port/0-sh-jobs`, `port/1-theme-gen`, `port/2-keys`, `port/2-cheatsheet`.
The phase number in the branch name is what makes `git branch --list 'port/2-*'`
answer a question worth asking a year from now.

Each branch merges with the repo rule: `git merge --squash`, one commit whose
message summarises the branch, then `git branch -m port/2-keys
parked/port/2-keys` so the commit-by-commit history stays reachable without
sitting in the branch list. Nothing gets deleted. The parked branches are local
and don't get pushed.

What a branch has to carry before it merges, which is the same list as the
phase rows and worth repeating in one place because a branch is where it gets
forgotten:

- the code, and `cargo fmt`, `clippy -D warnings` and `cargo test` clean
- its documentation row, in the same branch rather than a follow-up
- its config keys in `docs/config.example.toml`, which CI checks anyway
- its before and after numbers in `BENCHMARKS.md`, for a ported command
- no review marker left in anything it touched

A commit inside a branch is a working state, so `cargo test` passes at every
one of them. That matters more here than usual, because a bisect over the port
is the only way to find which of twelve subcommands slowed the bar down.

Two exceptions to the one-branch-per-row rule. A change that touches every
file mechanically, `cargo fmt` and the `//!` headers being the two in phase 0,
goes in alone and merges the same day, since it conflicts with everything and
holding it costs more than it saves. And a pair that has to land together
merges together: the `vim-bg` to `sh-jobs` rename and the config table it reads
from make no sense apart, so they're one branch with two commits in it.

`CHANGELOG.md` gets its entry in the merge commit rather than in the branch,
which is the one place a conflict is guaranteed if every branch edits it.

## What the plan left open

Eight things a reading of the crate turned up that the sections above don't
answer. Most are small and all of them get worse after twelve subcommands land.

### Linux

`README.md` line 45 says the battery segment is macOS and `DESIGN.md` line 88
calls it "macOS ioreg battery", and both are stale: `segments/battery.rs` goes
through the `battery` crate 0.7 and `segments/network.rs` through `sysinfo`
0.39, and both of those carry Linux backends.

Phase 0 step 2 checked it rather than assuming. `cargo check --target
x86_64-unknown-linux-gnu --all-targets` compiles the crate and its tests clean
on 1.98.0 with no `cfg` changes and nothing stubbed out, so the Linux arm of
the matrix is expected to build. What that does not prove is runtime: `check`
never links, no test reads hardware, and a Linux box with no battery, no tmux
and no `$XDG_RUNTIME_DIR` is a different question from one that compiles. The
socket path is the first thing to look at there, since
`/tmp/tmux-companion-<uid>.sock` isn't where a Linux user expects it when
`$XDG_RUNTIME_DIR` exists.

### An old daemon answering a new client

The client started a server when the socket was absent and did nothing when a
server from the previous build was already listening, which cost a stale render
and, after the port, would have cost a `keys` request to a daemon that had
never heard of `keys`.

Phase 0 step 10 closed it. Every request and response carries a build id, which
is the version plus a stamp `build.rs` writes at compile time, because during
development every build is 0.1.0 and the question a client needs answered is
whether the daemon is running the binary that was just installed. A client that
sees a different build asks the daemon to shut down, waits for the socket to
go, and retries once. Both fields default to empty, so a new client and a
daemon from before the field can still talk to each other, which is the case
that matters during the upgrade itself.

### The pickers don't run in the daemon

A `ratatui` picker owns a terminal and the daemon has none, so six of the
ported subcommands run their TUI in the client process and the daemon only
answers with rows. That's worth a paragraph in `explanation/architecture.md`,
because "one binary the config talks to" reads like the daemon does the work,
and for the whole of group 1 it can't. It also means the `current_thread`
runtime that saves 3.5 ms per client spawn is what the pickers get, which is
fine, and the decision should be written down rather than rediscovered.

### No logs and no doctor

A picker that misbehaves inside `display-popup -E` sends its stderr wherever
the popup went, which is nowhere.

`tmux-companion doctor` exists as of phase 0 step 10 and prints the binary and
its build, whether a daemon is running and which build it is, the socket with
its mode and owner, the config file in use, the glyph preset, the state
directory with the last config error in it, the tmux version and the platform.
It reads without starting or replacing anything, because a diagnostic that
changes the answer while reading it is not a diagnostic.

`[general] log` is in the config and nothing writes to it yet, which is the
half still missing.

### The socket becomes an execution surface

This one's worth stating plainly rather than in passing. Today a request makes
the daemon read git state and hardware counters. After `run` and `open` land, a
request makes the daemon spawn a process as me, and the socket sits at
`/tmp/tmux-companion-<uid>.sock`, in a directory every user on the machine can
write to.

Phase 0 step 10 did the first half: the socket is chmod 0600 after bind, and a
client checks the owner before connecting rather than trusting the path.
`doctor` prints the mode it found, which is how the live socket turned out to
be 0755. What is left is the window between bind and chmod, which closes
properly only by creating the socket inside a 0700 directory, and that turns
out to be the same change as preferring `$XDG_RUNTIME_DIR` where it exists. `open` also takes text out of a pane and
hands it to an opener, so the extraction has to reject anything that isn't a
URL or a `file:line:col`, and the spawn has to be a `Command` with an argument
vector and no shell anywhere in it.

### Running both for the weeks it takes

The plan has an order and no cutover. Six pickers and three orchestration
scripts can't all be swapped on one evening, so each binding gets the Rust
command next to the zsh one under a different key first, the old binding stays
until the new one has survived a week of use, and `comrades` stays in the
config until its last script is gone. This is also the answer to a question the
plan raises and drops, which is what happens to somebody who clones the repo
halfway through the port: the full tmux.conf example only lists what is built.

### The benchmark row

`BENCHMARKS.md` is the best writing in the repo and the port quotes one number
out of it, the 93 ms warm `prefix+?` with 50 ms of fzf in it, without making
anything depend on it. Each ported command records a before number taken from
the zsh version and an after number from the Rust one, in `BENCHMARKS.md`, in
the commit that does the port. A command that comes out slower than the script
it replaced does not merge, which is the rule that keeps "one binary" from
becoming the only reason anything happened.

### The usage log

"usage log" appears in the `keys` and `cheatsheet` rows as a requirement and
nowhere as a design. It needs a location, `$XDG_STATE_HOME/tmux-companion/`, a
format, a bound on how large it gets, and an off switch in `[usage]`, since it
records what somebody presses and that's their business and not the tool's.

## What this costs

Iteration. Today a script is edited and the next keypress runs the new code,
and during one afternoon these files changed more than a dozen times. Every one
of those becomes a rebuild, which is the real price and it's worth saying out
loud before starting rather than discovering it in week two.

The tmux quirks don't go away either. `list-keys -T <table> <key>` answering
with nothing, `list-keys <key>` answering for some keys and not others, and
`list-keys -N -T copy-mode-vi` printing to the client's message area when it
runs under `run-shell` are all properties of tmux, and a Rust client meets every
one of them the same way a zsh script does.
