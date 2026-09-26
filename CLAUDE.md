# CLAUDE.md

## Build & test

The justfile is the list of commands, and `just` alone prints it. Every recipe
that compiles already runs under `nice -n 15` with four jobs, because a bare
`cargo build --release` on a machine someone is using saturates every core.

```sh
just build       # release binary → target/release/tmux-companion
just test-unit   # the fast half: --lib --bins, no tmux, no daemon, no socket
just test-e2e    # the three integration suites; e2e drives a real tmux
just test        # both, --no-fail-fast, with the daemon reaper on the way out
just lint        # clippy --all-targets -D warnings, the way ci.yml runs it
just ci          # review-marks, rustfmt, clippy, tests and rustdoc, native
just install     # build, install to PREFIX, move the daemon onto the new binary
```

`just lint` is the lint step, and CI fails on a warning, so run it before
pushing. `TC_SKIP_E2E=1 cargo test` skips the tmux tests with one printed line;
with `CI` set they fail instead of skipping, which is on purpose (see the note
on `test-e2e` in the justfile).

## Running manually

Move the daemon onto the new binary before testing, or the client keeps
talking to the old one:

```sh
./target/release/tmux-companion restart
./target/release/tmux-companion gst /path/to/repo
```

Not `pkill -f tmux-companion`: the daemon handles SIGTERM now, so that works,
but `restart` is the clean path. It waits for the old daemon to go, starts the
new one, and exits 1 with the reason when the new binary refuses its config,
which a pkill-and-respawn reports as nothing at all.

Or let the client auto-start the server:

```sh
./target/release/tmux-companion gst .
```

## Architecture in one paragraph

Same binary, two modes.  `server` binds a Unix socket
(`/tmp/tmux-companion-<uid>.sock`) and serves requests forever.  Every other
subcommand is a client: it connects (auto-starting the server if the socket is
absent), writes one JSON line, reads one JSON line, prints the output field, and
exits.  The server keeps everything mutable in `ServerState` behind an
`Arc<tokio::sync::Mutex<_>>`: the bandwidth previous-sample, the dir-aliases
map, and the three caches (git status, is-inside-work-tree, battery).  There is
no database; the caches are in-memory and die with the server, which is the
intended lifetime -- one cold `git status` after a restart costs 51 ms, once.

## Module responsibilities

One row per file under `src/`, in the order `ls src src/*/` gives. The "owns"
column is the first sentence of the file's `//!` doc, so the file is the
source and this is the index; four files have no module doc yet and keep the
description they had.

| Path | Owns |
|------|------|
| `src/autofetch.rs` | fetching in the background, so ahead and behind mean something |
| `src/autoreload.rs` | sourcing tmux's config when it changes |
| `src/cache.rs` | in-memory TTL maps for the server's segment caches |
| `src/cheatsheet.rs` | the cheat sheet: four boxes in a 2x2 grid, showing the bindings somebody wrote rather than the ones tmux ships |
| `src/cli.rs` | the `Cmd` enum clap parses into, and the dispatch that turns a variant into a request to the server |
| `src/client.rs` | find the socket, start a server if nothing answers, send one JSON line and read one back |
| `src/close.rs` | closing a project session by letting every window exit on its own |
| `src/config.rs` | the configuration file: where it lives, how it is parsed, and what happens when it cannot be |
| `src/dirsource.rs` | where the project picker's directory list comes from |
| `src/doctor.rs` | `tmux-companion doctor`: the first thing to ask for on an issue from a stranger |
| `src/keys.rs` | key bindings, parsed out of `tmux list-keys` into rows a picker can search |
| `src/lib.rs` | one binary in two modes; the library `tests/` links against |
| `src/local.rs` | running a segment in this process, with no daemon and no socket |
| `src/main.rs` | the binary: parses arguments, picks a runtime, hands over to the library |
| `src/notify.rs` | telling you a long command finished in a pane you were not looking at |
| `src/open.rs` | open a URL or a file reference found in text |
| `src/panes.rs` | every pane on the server, as a list to jump from, and the agents among them |
| `src/picker/mod.rs` | the picker: a fuzzy-matched list in a terminal, shared by every chooser in the tool |
| `src/picker/ansi.rs` | enough ANSI to draw a preview |
| `src/picker/screen.rs` | the picker's own screen: the state behind it, the matching, and the drawing |
| `src/picker/style.rs` | what a picker looks like, as settings rather than as a shape compiled into the drawing |
| `src/presets/ascii.toml` | the 7-bit fallback: one value per Nerd Font glyph name, for a machine whose font nobody controls |
| `src/preview.rs` | renders the git segment for a spread of repo states in every color style |
| `src/probe.rs` | probes: ask the terminal what it does, rather than assuming |
| `src/project.rs` | projects: one tmux session each, with the windows a layout asks for |
| `src/proto.rs` | `Request` / `Response` serde types, one args struct per command, and the build id (no module doc) |
| `src/restore.rs` | what a restore will run in each pane, decided before anything runs |
| `src/run.rs` | run a command from history in a pane beside the one you are in |
| `src/saved.rs` | per-project layouts: the file a key writes and `project` reads back |
| `src/segments/mod.rs` | one module per thing the status bar can draw |
| `src/segments/agents.rs` | how many coding agents are running, and how many are waiting on you |
| `src/segments/battery.rs` | battery percentage and icon, read through the `battery` crate |
| `src/segments/clients.rs` | how many other clients are attached to this server, session and window |
| `src/segments/git.rs` | git status: running the command, parsing porcelain v2, and rendering it into a tmux segment |
| `src/segments/network.rs` | network bandwidth: a counter read, the arithmetic that turns two reads into a rate, and the IEC formatting |
| `src/segments/sh_jobs.rs` | jobs stopped under a pane: which ones, and what to draw for each |
| `src/segments/vim_bg.rs` | the old name for `sh_jobs`, kept one release |
| `src/segments/window.rs` | path abbreviation, index icons, `render` (no module doc) |
| `src/server/mod.rs` | bind the socket, accept forever, hand each line to a handler |
| `src/server/handlers.rs` | `req.cmd` → segment fn; `assemble_right`, the `__rusage` probe (no module doc) |
| `src/server/state.rs` | `ServerState` + caches + TTL constants + dir-aliases loader (no module doc) |
| `src/sessions/mod.rs` | snapshots of the whole tmux server, and the generations they are kept in |
| `src/sessions/capture.rs` | turning three tmux listings into one snapshot |
| `src/sessions/cli.rs` | the client half of `sessions resurrect`: the confirm screen, the countdown and the crash acknowledgement, which run in the terminal and so cannot live in the daemon |
| `src/sessions/idle.rs` | sessions nobody has looked at for days: which they are, and the picker that closes one |
| `src/sessions/import.rs` | reading the tab-separated format tmux-resurrect writes |
| `src/sessions/restore.rs` | rebuilding a server from a snapshot |
| `src/sessions/store.rs` | where snapshots live, how many are kept, and which one is newest |
| `src/sessions/summary.rs` | the screen a restore shows when it does not know something |
| `src/sessions/timer.rs` | the daemon's half: a snapshot on a timer, and a marker saying it is alive |
| `src/shell.rs` | the OSC 133 prompt marks, and the shell code that emits them |
| `src/tasks.rs` | background work the daemon does on a timer, and the small tmux commands that do not need one |
| `src/theme/mod.rs` | theme arithmetic: the readable text colour for a theme, a visible border colour, and the lighter and darker siblings of a cube colour |
| `src/theme/cli.rs` | the client half of `theme`: the subcommands that read a themes directory, run the picker and source a file into tmux, which talk to the terminal and so run in the client rather than the daemon |
| `src/tmux/mod.rs` | everything that knows about tmux's own formatting language |
| `src/tmux/format.rs` | colours, styles and the string builder every segment renders through |
| `src/tmux/icons.rs` | Nerd Font codepoints the status bar draws with |
| `src/window_names.rs` | naming windows after what is running in them, from the job table |

## Key invariants

- **`ARROW_RIGHT`** (`src/tmux/icons.rs`) — currently `\u{e0bc}`.  The glyph
  rendered depends on the Nerd Font variant installed.  Changing this constant
  is intentional and the tests use the constant (not a hardcoded codepoint), so
  they track changes automatically.

- **`colored_segment` ARROW_RIGHT special case** — in no-tmux mode (`no_tmux=true`),
  calling `colored_segment(true, fg, bg, ARROW_RIGHT)` emits only the ANSI color
  escape codes, not the arrow glyph.  This matches the Go original's behaviour.
  See `tmux/format.rs` and the test `colored_segment_no_tmux_arrow_right_special_case`.

- **Branch truncation** — truncates when `char_count > 20` (strictly greater),
  and the tail is the last 10 chars (`TAIL_LEN + 1`).  This matches Go's
  `branch[lastIndex-tailLen:]` formula exactly.  See `short_branch` in
  `segments/git.rs` and the `short_branch_exactly_max_len_not_truncated` test.

- **`ServerState` has only synchronous methods, and a lock guard is never held
  across an `.await`.**  The state lives behind a `tokio::sync::Mutex` and the
  combined `status-right` handler polls three segments concurrently under
  `tokio::join!`; if any of them could hold the guard across a suspension point
  they would deadlock each other.  Keeping every `ServerState` method sync makes
  that mistake impossible to write.  Compute first, then take the lock for one
  statement.

- **The TTL belongs to the reader, not the store.**  `TtlMap` records when each
  entry was written and nothing else; freshness is `age < ttl` evaluated at
  read time with a TTL the caller supplies.  That is what lets `--ttl` be a
  runtime flag, and it means there is no cold-start special case — the first
  entry written expires exactly like the thousandth.

- **Cache skip on `force`** — `render` skips the cache read when `opts.force` is
  set but still writes the fresh result back, so the next normal call gets a
  warm cache.

- **`status-right` must never pass a `pane_pid` to the git segment.**  Doing so
  calls `has_suspended_nvim`, which enumerates the whole process table and cost
  18.5 ms of the segment's 26.0 ms.  The suspended-nvim marker is deliberately
  given up on the status bar; `gst <path> <pid>` by hand still shows it.

- **Never cache the assembled status side, only its segments.**  `net` is a
  rate.  A cached assembly would replay one measurement window's average for as
  long as the entry lived, which looks like a frozen bar rather than a quiet
  network.

## Adding a new segment

1. Add a function in `src/segments/<name>.rs`.  Extract all I/O-free logic into
   a pure `fn` so it can be unit-tested.
2. Add `pub mod <name>;` to `src/segments/mod.rs`.
3. Handle the new `cmd` string in `src/server/handlers.rs`.
4. Add the subcommand variant to `Cmd` in `src/cli.rs`, add an args struct to
   `src/proto.rs`, and send it with `Request::build`.  The handler reads it
   back with `req.parse_args::<YourArgs>()`; never index `req.args["key"]`.
5. Write unit tests in the same file.
6. If the segment belongs on the status bar, add it to the **combined**
   `status-right` response rather than giving it its own `#()` call — one more
   `#()` costs ~14.6 ms of CPU per second per attached client, which is more
   than any segment here costs to compute.  Put anything expensive and slow to
   change behind a `TtlMap` on `ServerState`.
7. Update `docs/tmux.conf.example`, the module table above, and
   `docs/tmux-companion.1`.

## The manual

`docs/tmux-companion.1` is mdoc, the macro set tmux's own page uses, and it is
part of the change that adds a feature rather than a thing to write afterwards.
A command, a config section, a key binding or a file path that the tool gains
gets its paragraph in the same commit.

Two tests enforce it, so this is not a rule anybody has to remember:
`the_manual_documents_every_subcommand` reads `--help` and fails on a command
the page does not name, and `the_manual_names_every_configuration_section`
does the same for `docs/config.example.toml`. Both are in `tests/e2e.rs`.

Read it while writing it:

```sh
man ./docs/tmux-companion.1
```

It ships three ways and all three have to keep working: `scripts/install.sh`
puts it in `$PREFIX/share/man/man1`, the release workflow copies it into every
archive, and the playground image installs it so `man tmux-companion` answers
in the container.

`docs/tmux.conf.starter.example` exists alongside `docs/tmux.conf.example` and
`docs/tmux.conf.full.example`, and the e2e checks in `tests/e2e.rs` run over
all three.

## The skill

`skills/tmux-companion/SKILL.md` is the instruction sheet a coding agent loads
before it touches this tmux server, and the repository ships it as a Claude Code
plugin: `.claude-plugin/plugin.json` and `.claude-plugin/marketplace.json` make
the checkout its own marketplace, and `.claude/skills/tmux-companion` is a
symlink so a session started here picks it up with no install.

An agent acts on that file without checking it, so a command removed from the
CLI has to leave the skill in the same commit.
`the_skill_names_no_command_that_went_away` in `tests/e2e.rs` reads every
backticked span and fenced line in the file and holds the commands, subcommands
and long flags against `--help`, which is the same trick the manual tests use.

The plugin has its own version and its own tag namespace,
`tmux-companion--v0.3.0`, because SKILL.md changes on a different schedule from
the binary and `release.yml` triggers on `v*`:

```sh
just plugin-check              # is the skill ahead of its last tag?
just plugin-release 0.3.0      # bump both manifests, validate, commit, tag, push
```

## Adding a new command

A command is not a segment: it does something to tmux rather than returning
bytes for the bar.  The steps are the same shape, with one extra.

1. Add the `Cmd` variant and its clap flags in `src/cli.rs`.
2. Add an args struct to `src/proto.rs` with `#[serde(deny_unknown_fields)]`
   and a `#[serde(default)]` on every optional field.  The struct is the only
   definition of the command's arguments; there is no second one.
3. Send it with `Request::build("name", &args)` in the dispatch in `cli.rs`.
4. Read it back in `src/server/handlers.rs` with
   `req.parse_args::<YourArgs>()`.  Never index `req.args["key"]`: a key that
   was never sent reads back as `None` and changes behaviour silently, which is
   the bug class the args structs exist to remove.
5. Write the pure parts as free functions and unit-test them in the same file.
   Give it a paragraph in `docs/tmux-companion.1`; the suite fails without one.
6. If the command talks to the terminal (a picker, a dialog), it runs in the
   **client** process, not the daemon.  The daemon has no terminal; it answers
   with rows and the client draws them.
7. Add a row to `docs/reference/cli.md` and a line to `docs/dev/port-checklist.md`.

## Changing icon codepoints

Edit `src/tmux/icons.rs`.  Icon constants are used directly in tests via the
constant name, so a codepoint change automatically propagates to all tests —
no manual expected-string updates needed.

After changing a codepoint that appears in tmux output, rebuild and do a
side-by-side visual check with the previous tool:

```sh
just build
./target/release/tmux-companion restart   # not pkill; see "Running manually"
./target/release/tmux-companion gst /some/repo
yrl gst /some/repo   # or the previous shell script
```
