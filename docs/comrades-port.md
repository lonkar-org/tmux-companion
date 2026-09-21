# Porting comrades into tmux-companion

`~/.config/tmux/comrades` is 1,495 lines of zsh across 13 scripts, plus 294
lines of generators and probes in `mysetup/scripts`. This is the inventory of
what moves in here, so the companion stops being a status-bar renderer and
becomes the one binary the config talks to.

The port is phase 1. Phase 0 is the crate it lands on, because a foundation
that holds 2,671 lines isn't automatically one that holds twice that with a
TUI on top. Running alongside both is a documentation track, one step per
phase, which is what turns a personal tool into one somebody else can send a
patch to.

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

`cargo fmt --check` reports 37 diffs across 12 files. `cargo clippy --all-targets
-- -D warnings` fails on two lints, a `collapsible_if` at `segments/git.rs:159`
and an inherent `to_string` at `tmux/format.rs:158` that should be a `Display`
implementation. Both are one-line fixes and neither's interesting. What is
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

124 public items carry 19 `///` comments, which is 15%. Two files out of
eighteen have a `//!` module header, `cache.rs` and `preview.rs`, and both are
good enough to show what the other sixteen are missing. The comments that do
exist are unusually good, several of them record the bug that caused them, and
the gap is coverage rather than quality.

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
```

`CLAUDE.md` stays where it is and shrinks to the invariants, which is the part
that's genuinely for an agent rather than a person.

### One step per phase

The rule is that a phase isn't finished until its row is done. The work lands
next to the code that motivated it, which is the only time anybody knows what to
write.

| Phase | Documentation step |
| --- | --- |
| 0 | `LICENSE`, `CONTRIBUTING.md`, the `docs/` skeleton, `reference/requirements.md` with a verified minimum, `rust-version` and the rest of the crates.io metadata in `Cargo.toml`, `README.md` cut down to a front door |
| 1 `theme gen` | `reference/cli.md` started with the first subcommand, and the contrast arithmetic written up in `explanation/` while it's fresh |
| 2 `keys`, `cheatsheet` | `how-to/add-a-command.md`, written from having just done it twice, and `tutorial/getting-started.md` extended to the first picker |
| 3 `project` | `reference/configuration.md`, which is where the theme map and the project map file formats finally get written down |
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
  (`src/segments/vim_bg.rs`)
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
| `run` | `run-command-pane.zsh` | 235 | matcher, zsh history parse, pane control, exit dialog |
| `open` | `open-from-text.zsh` | 277 | URL and `file:line:col` extraction, pane reuse |

`theme` is the one with a head start: `preview-tmux-theme.zsh` parses
`colour125`, the eight base names and hex triplets, and `src/tmux/format.rs`
already does colour work for the status bar.

`open` is the most self-contained and the least like the others, since it has
no picker in two of its four modes and mostly does regex extraction and one
tmux call.

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

Each of these carries the documentation row from the table above, and is not
finished without it.

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
