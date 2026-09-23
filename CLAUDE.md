# CLAUDE.md

## Build & test

```sh
cargo build --release          # binary → target/release/tmux-companion
cargo test                     # unit and integration tests, no external deps
```

On a machine someone is using, keep the job count down and the priority low --
a full build otherwise saturates every core:

```sh
nice -n 15 cargo build --release -j 4
nice -n 15 cargo test -j 4
```

No lint step is configured yet; `cargo clippy` is fine to run but not required.

## Running manually

Kill any stale daemon before testing so you pick up the new binary:

```sh
pkill -f tmux-companion; sleep 0.1
./target/release/tmux-companion server &
sleep 0.3
./target/release/tmux-companion gst /path/to/repo
```

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

| Path | Owns |
|------|------|
| `src/main.rs` | argument parsing and the runtime choice, nothing else |
| `src/lib.rs` | the library every module hangs off, so `tests/` can link it |
| `src/cli.rs` | CLI (`Cmd` enum via clap), dispatch to client or server |
| `src/client.rs` | connect-with-retry, spawn server, `send` and send/print |
| `src/server/mod.rs` | UnixListener accept loop |
| `src/server/handlers.rs` | `req.cmd` → segment fn; `assemble_right`, the `__rusage` probe |
| `src/server/state.rs` | `ServerState` + caches + TTL constants + dir-aliases loader |
| `src/cache.rs` | `TtlMap` — timestamped map, reader-supplied TTL, sweep on insert |
| `src/segments/git.rs` | git status parse, format; `GstOptions`, `render(opts, state)`, cached is-inside-work-tree |
| `src/segments/battery.rs` | ioreg plist parse, `format_battery_output` |
| `src/segments/network.rs` | `sample` (counter read), `rate`/`advance` (pure arithmetic), IEC format |
| `src/segments/clients.rs` | tmux list-clients, `format_client_output` |
| `src/segments/sh_jobs.rs` | jobs under a pane, the config job table, `format_jobs` |
| `src/segments/vim_bg.rs` | deprecated shim over `sh_jobs`, kept one release |
| `src/segments/window.rs` | path abbreviation, index icons, `render` |
| `src/tmux/format.rs` | `Segment`, `colored_segment`, `powerline_segment`, color consts |
| `src/tmux/icons.rs` | Nerd Font codepoints |
| `src/proto.rs` | `Request` / `Response` serde types, and one args struct per command |
| `src/config.rs` | the TOML config: search order, parse, did-you-mean errors, `dump` |
| `src/doctor.rs` | the report to ask an issue reporter for |
| `src/theme.rs` | WCAG contrast, the xterm cube, theme file rewrite |
| `src/keys.rs` | `list-keys` parse, the row cache, the usage log |
| `src/picker.rs` | fuzzy match state machine and the ratatui screen |
| `src/cheatsheet.rs` | the four boxes, their ordering and the grid |
| `src/project.rs` | session and directory rows, short paths, session names |
| `src/dirsource.rs` | where the directory list comes from: named sources, their parsers, visits |
| `src/tasks.rs` | the autosave timer, the calendar, the toggle target |
| `src/run.rs` | shell history parsing, the pane slide, the exit dialog |
| `src/open.rs` | URL and `file:line:col` extraction from text |
| `src/close.rs` | which pane gets which farewell |
| `src/probe.rs` | key bytes and DSR cell measurement |

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
7. Add a row to `docs/reference/cli.md` and a line to `docs/port-checklist.md`.

## Changing icon codepoints

Edit `src/tmux/icons.rs`.  Icon constants are used directly in tests via the
constant name, so a codepoint change automatically propagates to all tests —
no manual expected-string updates needed.

After changing a codepoint that appears in tmux output, rebuild and do a
side-by-side visual check with the previous tool:

```sh
cargo build --release
pkill -f tmux-companion
./target/release/tmux-companion gst /some/repo
yrl gst /some/repo   # or the previous shell script
```
