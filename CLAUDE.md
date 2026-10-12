# CLAUDE.md

## Companion first

`docs/dev/companion-first.md` says why: a companion first, and second, every
job it does channeled to people who don't use their eyes, or rely on them
less. The rules below come out of it. They bind any change to something
somebody sees or hears: a picker, the bar, a popup, a message, a key.

**The primary focus now** is a screen reader and a keyboard in a terminal.
For motor and attention, remove traps rather than designing a second
interface: nothing timed, nothing mouse-only, no focus moved without saying
so, no flashing or moving output.

### Rules

1. Every job has a path a screen reader reads top to bottom: plain lines in a
   stable order, and nothing said by colour, glyph or position alone. The
   command (`inbox`, `brief`, `panes --print`, `read-bar`) is the source of
   truth, and a popup is one view of it.
2. A new surface drawn as a grid, a popup or with box drawing ships with that
   path in the same change. When it can't, its manual entry and its
   CHANGELOG line say `a11y: limited` and why.
3. A limit is written down whatever its cause. No time is a reason to come
   back to it, never a reason to leave the note out. A limit that comes from
   the approach or the design stays an open issue.
4. Screen-reader mode never reads chrome: no `─`, `│`, icons or column
   padding in what it prints.
5. Same job, different channel is the bar. A feature only for screen-reader
   users is welcome, and not required.
6. Design for information through speech, braille and keys, not for one
   mental model of the screen. One closed-eyes session is a failure mode to
   design against, not a picture of every screen-reader user.

**Answer these in the commit message for any such change:**

- What job does it serve?
- Which command does that job with output a screen reader reads top to
  bottom?
- Does it depend on colour, an icon, pane geometry, a timer, or focus that
  isn't announced?
- If it does, what is the other path, or where is the limit written down?
- Does screen-reader mode stay free of `─│`, icons and padding?

`docs/dev/accessibility-checklist-status.md` says where the work stands.

## Build & test

The justfile is the list of commands, and `just` alone prints it. Every recipe
that compiles already runs under `nice -n 15` with four jobs, because a bare
`cargo build --release` on a machine someone is using saturates every core.

```sh
just build       # release binary → target/release/tmux-companion
just test-unit   # the fast half: --lib --bins, no tmux, no daemon, no socket
just test-e2e    # the four integration suites; e2e and remote drive a real tmux
just test        # both, --no-fail-fast, with the daemon reaper on the way out
just lint        # clippy --all-targets -D warnings, the way ci.yml runs it
just lint-md     # markdownlint over every .md, the way markdown.yml runs it
just capnp       # regenerate schema/link_capnp.rs after editing schema/link.capnp
just test-tmux 3.4   # every test with one pinned tmux first on PATH
just test-matrix # every test on each pinned tmux, 3.4, 3.5a and 3.7c
just ci          # review-marks, capnp-check, rustfmt, clippy, test-matrix and rustdoc
just install     # build, install to PREFIX, move the daemon onto the new binary
```

`just lint` is the lint step, and CI fails on a warning, so run it before
pushing.

tmux is pinned, not installed. CI tests every OS against tmux 3.4 (the floor,
Ubuntu 24.04's), 3.5a and 3.7c, each built from its release tarball by
`scripts/tmux-build.sh`, whose list is the one list; `just ci` runs the same
three here through `test-matrix`. A behaviour that differs between tmux
versions has to fail on this machine before a push, not in CI after one. A
feature that needs a newer tmux says so in the tool and gates its tests on it,
as key routing does on 3.5. `TC_SKIP_E2E=1 cargo test` skips the tmux tests with one printed line;
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

Same binary, two modes. `server` binds a Unix socket
(`/tmp/tmux-companion-<uid>.sock`) and serves requests forever. Every other
subcommand is a client: it connects (auto-starting the server if the socket is
absent), writes one JSON line, reads one JSON line, prints the output field, and
exits. The server keeps everything mutable in `ServerState` behind an
`Arc<tokio::sync::Mutex<_>>`: the bandwidth previous-sample, the dir-aliases
map, and the three caches (git status, is-inside-work-tree, battery). There is
no database; the caches are in-memory and die with the server, which is the
intended lifetime -- one cold `git status` after a restart costs 51 ms, once.

## The library surface

The crate ships on crates.io as a binary and a library, and docs.rs shows six
modules: `daemon`, `proto`, `segments` (`git`, `network`), `keyroute`
(`spell`), `sessions` (`capture`, `import`, `portable`, `store` and the
snapshot types) and `tmux` (`format`, `icons`). Those follow semver, which
before 1.0 means a breaking change to one bumps the minor version and gets a
CHANGELOG line. Everything else is `#[doc(hidden)]` in `src/lib.rs`: public
only so `tests/` links against it, with no promise attached.

- A new module is hidden by default. Making one visible is a decision about
  supporting it, made on purpose, with a doctest in its module docs.
- A visible signature must not name a type from a hidden module. Re-export it
  next to the item that uses it (`segments::git` re-exports `BranchType` and
  `GitPart`, `sessions` re-exports `Confidence`) or hide the item.
- Library code never spawns `current_exe()`: inside somebody else's program
  that is their binary. `daemon::Daemon` connects and nothing else, which is
  why it exists beside `client`.
- The examples in the docs are tests. `just doc` and the rustdoc CI job run
  `cargo test --doc`, which `--all-targets` skips.

## Key invariants

- **`ARROW_RIGHT`** (`src/tmux/icons.rs`) — currently `\u{e0bc}`. The glyph
  rendered depends on the Nerd Font variant installed. Changing this constant
  is intentional and the tests use the constant (not a hardcoded codepoint), so
  they track changes automatically.

- **`colored_segment` ARROW_RIGHT special case** — in no-tmux mode (`no_tmux=true`),
  calling `colored_segment(true, fg, bg, ARROW_RIGHT)` emits only the ANSI color
  escape codes, not the arrow glyph. This matches the Go original's behaviour.
  See `tmux/format.rs` and the test `colored_segment_no_tmux_arrow_right_special_case`.

- **Branch truncation** — truncates when `char_count > 20` (strictly greater),
  and the tail is the last 10 chars (`TAIL_LEN + 1`). This matches Go's
  `branch[lastIndex-tailLen:]` formula exactly. See `short_branch` in
  `segments/git.rs` and the `short_branch_exactly_max_len_not_truncated` test.

- **`ServerState` has only synchronous methods, and a lock guard is never held
  across an `.await`.** The state lives behind a `tokio::sync::Mutex` and the
  combined `status-right` handler polls three segments concurrently under
  `tokio::join!`; if any of them could hold the guard across a suspension point
  they would deadlock each other. Keeping every `ServerState` method sync makes
  that mistake impossible to write. Compute first, then take the lock for one
  statement.

- **The TTL belongs to the reader, not the store.** `TtlMap` records when each
  entry was written and nothing else; freshness is `age < ttl` evaluated at
  read time with a TTL the caller supplies. That is what lets `--ttl` be a
  runtime flag, and it means there is no cold-start special case — the first
  entry written expires exactly like the thousandth.

- **Cache skip on `force`** — `render` skips the cache read when `opts.force` is
  set but still writes the fresh result back, so the next normal call gets a
  warm cache.

- **`status-right` must never pass a `pane_pid` to the git segment.** Doing so
  calls `has_suspended_nvim`, which enumerates the whole process table and cost
  18.5 ms of the segment's 26.0 ms. The suspended-nvim marker is deliberately
  given up on the status bar; `gst <path> <pid>` by hand still shows it.

- **Never cache the assembled status side, only its segments.** `net` is a
  rate. A cached assembly would replay one measurement window's average for as
  long as the entry lived, which looks like a frozen bar rather than a quiet
  network.

## Performance under a capped CPU

The pickers are judged on a throttled machine, not an idle one: at 32 busy
loops the project picker took 446 ms to its first row against 249 idle, and
381 of those were spent collecting rows before anything was drawn.
`docs/dev/design-capped-cpu.md` has the measurements and the reasoning; these
are the rules that come out of it, for any picker, loop or segment.

1. **No process and no file read per row before the first frame.** Per-row
   work (a preview, a capture, a listing) is done for the selected row when
   the selection lands on it, and cached for the picker's life, or it comes
   from a daemon cache. A loop of `.await`s over rows before `picker::run`
   is the bug this rule exists for.
2. **Chrome before rows.** When collecting rows can take longer than the
   floor, the picker draws its border, title and prompt first, with one
   static loading line where the rows go. Nothing animates and the line is
   replaced once.
3. **One poll per tmux listing.** The daemon runs `list-panes -a` once on a
   timer and every consumer reads that. A new loop does not add its own
   `list-panes`, `list-windows` or `list-clients` on a timer, and a picker
   asks the socket before it asks tmux for something the daemon already has.
4. **Fewer processes, not lower-priority ones.** Background work costs what
   it starts, so the fix for a loop is starting less: a shared listing, a
   change-only write, a longer cache. Two obvious fixes were measured and
   made things worse: lowering children's priority (a `pre_exec` hook stops
   `posix_spawn`, so every child forks the whole daemon, and a lowered tmux
   client holds tmux's one thread longer), and stretching a loop's interval
   under pressure (the inbox reads an agent's silence in its own intervals,
   so a slower inbox saw working agents stop). A loop is slowed only once
   what it detects no longer depends on how often it looks.
5. **A frame is one write.** Input already waiting is read before the next
   frame; nothing is re-parsed when it has not changed. No debounce timer:
   draining what is there is not waiting for what might come.
6. **Measured, not argued.** A new picker or loop adds itself to
   `scripts/bench-capped.py`, and its commit carries `just bench-capped 0 32`
   before and after against the budgets in the design doc. A miss says by
   how much and why.

`just bench-capped` is isolated: its own HOME, XDG directories, daemon socket
and tmux socket. Run nothing by hand against the live server instead; a bare
`tmux` from inside a pane talks to that pane's server whatever `TMUX_TMPDIR`
says, and that has killed a live server once.

## Adding a new segment

1. Add a function in `src/segments/<name>.rs`. Extract all I/O-free logic into
   a pure `fn` so it can be unit-tested.
2. Add `pub mod <name>;` to `src/segments/mod.rs`.
3. Handle the new `cmd` string in `src/server/handlers.rs`.
4. Add the subcommand variant to `Cmd` in `src/cli.rs`, add an args struct to
   `src/proto.rs`, and send it with `Request::build`. The handler reads it
   back with `req.parse_args::<YourArgs>()`; never index `req.args["key"]`.
5. Write unit tests in the same file.
6. If the segment belongs on the status bar, add it to the **combined**
   `status-right` response rather than giving it its own `#()` call — one more
   `#()` costs ~14.6 ms of CPU per second per attached client, which is more
   than any segment here costs to compute. Put anything expensive and slow to
   change behind a `TtlMap` on `ServerState`.
7. Give the file a `//!` module doc whose first sentence says what it owns,
   and update `docs/tmux.conf.example` and `docs/tmux-companion.1`.

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

## The changelog

`CHANGELOG.md` has an `## Unreleased` section at the top, and a change adds
its line there in the same commit, under Added, Changed or Fixed. When a
release is cut the section becomes `## x.y.z - date` and gets a prelude first:
a line or two on what matters in it, not a list of everything, so somebody who
reads only that knows whether to upgrade.

## Adding a new command

A command is not a segment: it does something to tmux rather than returning
bytes for the bar. The steps are the same shape, with one extra.

1. Add the `Cmd` variant and its clap flags in `src/cli.rs`.
2. Add an args struct to `src/proto.rs` with `#[serde(deny_unknown_fields)]`
   and a `#[serde(default)]` on every optional field. The struct is the only
   definition of the command's arguments; there is no second one.
3. Send it with `Request::build("name", &args)` in the dispatch in `cli.rs`.
4. Read it back in `src/server/handlers.rs` with
   `req.parse_args::<YourArgs>()`. Never index `req.args["key"]`: a key that
   was never sent reads back as `None` and changes behaviour silently, which is
   the bug class the args structs exist to remove.
5. Write the pure parts as free functions and unit-test them in the same file.
   Give it a paragraph in `docs/tmux-companion.1`; the suite fails without one.
6. If the command talks to the terminal (a picker, a dialog), it runs in the
   **client** process, not the daemon. The daemon has no terminal; it answers
   with rows and the client draws them.
7. Add a row to `docs/reference/cli.md` and a line to `docs/dev/port-checklist.md`.
8. A picker meets "Performance under a capped CPU" above: nothing per row
   before its first frame, and a line in `scripts/bench-capped.py`.

## Workflows

The docs site, releasing the Claude Code plugin and changing icon codepoints
are in the `repo-workflows` skill (`.claude/skills/repo-workflows/SKILL.md`).
