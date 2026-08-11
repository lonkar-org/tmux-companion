# Design

## Problem

tmux's `status-interval` fires every second.  The original setup called six
separate programs per tick — a Go binary and five zsh scripts.  Each involves
process-fork overhead, shell interpreter startup, and repeated reads of config
files or system interfaces.  On a busy machine this causes visible status-line
lag.

The goal: one persistent daemon that holds all state in memory, with clients
that connect over a Unix socket, send one JSON line, read one JSON line, and
exit.

## Architecture

```
┌──────────────────────────────────────────────────────┐
│  tmux status-interval (every 1 s)                    │
│                                                      │
│  #(tmux-companion gst …)  #(tmux-companion battery)  │
│         │                        │                   │
└─────────┼────────────────────────┼───────────────────┘
          │  Unix socket           │
          ▼                        ▼
┌─────────────────────────────────────────────────────┐
│  tmux-companion server                              │
│                                                     │
│  tokio async runtime, one task per connection       │
│                                                     │
│  ServerState (Arc<Mutex<_>>)                        │
│    net_previous:  Option<NetSample>                 │
│    net_last_render: String                          │
│    dir_aliases:   HashMap<PathBuf, String>          │
│    battery_cache: Option<(String, Instant)>         │
│    git_cache:     TtlMap<PathBuf, GitStatus>        │
│    repo_check:    TtlMap<PathBuf, bool>             │
└─────────────────────────────────────────────────────┘
```

### Same binary, two modes

```
tmux-companion server        # binds socket, accepts connections
tmux-companion <cmd> [args]  # connects, sends request, prints output, exits
```

The client auto-starts the server on first use: it tries to connect; on failure
it forks the server as a detached child, then retries with exponential backoff
(up to 10 attempts, 50 ms intervals).

### IPC protocol

Newline-delimited JSON, one request per connection.

```jsonc
// request  (client → server)
{"cmd": "gst", "args": {"path": "/repo", "force": false}}

// response (server → client)
{"output": "#[fg=color025,bg=color120] …", "error": null}
```

Each connection is handled by a spawned tokio task.  The task reads one line,
dispatches, writes one line, closes.

### Singleton guarantee

Before binding the socket, the server tries to connect to it.  If that
succeeds, another instance is running and the new process exits immediately.
If the connect fails, the stale socket file (if any) is removed and a fresh
`UnixListener::bind` is attempted.  A second concurrent startup race is benign:
only one bind wins; the loser exits.

## Module map

```
src/
  main.rs              clap CLI (Cmd enum), dispatch to client or server
  proto.rs             Request / Response serde types
  client.rs            connect-with-retry, spawn_server, send_and_print
  server/
    mod.rs             UnixListener accept loop
    state.rs           ServerState: net_previous, dir_aliases
    handlers.rs        req.cmd → segment fn
  segments/
    git.rs             git status parse + format + cache (main segment)
    battery.rs         macOS ioreg battery
    network.rs         netstat bandwidth delta
    clients.rs         tmux list-clients count
    vim_bg.rs          pgrep children, state=T + nvim
    window.rs          window title: index icons, path abbreviation, flags
  tmux/
    format.rs          Segment builder, colored_segment(), powerline_segment()
    icons.rs           Nerd Font Unicode codepoints
  cache.rs             TtlMap: timestamped map, reader-supplied TTL
```

## Git status segment

The main segment, ported from the Go `yrl gst` command.

### Caching

Git status is cached in memory on `ServerState`, keyed by the canonicalized
repository path, with a TTL that defaults to 5 seconds and is settable per
request with `--ttl`.  A cache hit skips all git subprocess invocations — the
round-trip is then socket connect → JSON deserialize → map lookup → JSON
serialize → socket write, which costs 0.09 ms.

`git rev-parse --is-inside-work-tree` is cached separately, for 5 minutes.  It
sits ahead of the status cache, so before it was cached every warm call still
paid a full `git` fork for it — 7.5 ms of the 7.6 ms a warm call cost.  A path's
repo-ness effectively never changes; the TTL exists only so that `git init` in a
watched directory is not misremembered until the server restarts.

The `--force` flag on `gst` skips the cache read (the result is still written
back so the next normal call benefits).  `--ttl 0` disables the cache entirely.

The cache does not survive a server restart, which is the intended lifetime: one
cold `git status` costs 51 ms, once.

### Parsing

Runs `git status --untracked-files=all --branch --porcelain=v2` and
`git rev-parse --path-format=absolute --git-dir` concurrently via `tokio::join!`.
A second join concurrently resolves `is_gone` (`git branch -r`) and counts
stash entries (line count of `.git/logs/refs/stash`).

### Formatting

`status_line_mode(status, no_tmux)` is a direct port of yrl's `StatusLine` Go
function.  It builds a `Segment` (a `Vec<String>` whose elements are joined on
`to_string()`) and calls `colored_segment` / `powerline_segment` to produce
either `#[fg=colorXX,bg=colorYY]` (tmux format) or `\x1b[38;5;XXm\x1b[48;5;YYm`
(ANSI/no-tmux format).

One special case inherited from the Go original: in no-tmux mode,
`colored_segment(fg, bg, ARROW_RIGHT)` emits only the color-change escape codes,
not the arrow glyph itself.  The arrow appears only from the terminal reset
sequence appended at the end.

### Branch truncation

Matches Go's `shortBranch` logic exactly:
- Strip known prefix (`feat/`, `bugfix/`, `hotfix/`, `chore/`, `release/`) and
  prepend the matching icon.
- Truncate when `char_count > 20`: keep first 8 chars + `...` + last 10 chars.
  (Go iterates byte indices 0..len-1; truncation fires when index ≥ 20, i.e.
  when len > 20.  Tail is `branch[len-1-9:]` = last 10 chars.)

## Window segment

Port of `window-status.zsh`.

**Path abbreviation** (`abbreviate_path`):
1. Strip home prefix → replace with `~`.
2. Discard `RootDir` component; treat it as a prefix `/`.
3. Abbreviate every component except the last to its first character
   (dot-prefixed components keep two characters: `.c` for `.config`).
4. Ellipsize basename at 17 chars (head 7 + `…` + tail 7).
5. Apply dir-logo substitutions in priority order (most specific first):
   `~/g/mysetup`, `~/g/`, `~/b/`, `~/`, `~`, `/`.

**Dir aliases** are loaded once at server startup from `~/.yrl/lib/dir-aliases`
into `ServerState.dir_aliases` and cloned per request.

**Index icons**: 10 selected (filled) and 10 unselected (outline) number-circle
icons.  Codepoints extracted directly from `window-status.zsh` via byte
inspection.

**Process animation**: when `process ≠ zsh` and the window is not current, the
index color cycles through 20 colours keyed on `epoch_secs % 20`.

## Network monitor

`ServerState.net_previous` holds a `NetSample` — `rx_bytes`, `tx_bytes` and the
`Instant` they were read — from the previous call.  On the first call the sample
is stored and an empty string is returned.  On subsequent calls the delta
divided by the elapsed time gives the throughput.  Speeds below 20 480 B/s are
suppressed.

The segment splits into three pieces, so that each is independently testable and
the expensive one can run concurrently with the other segments:

- `sample()` reads the cumulative counters via `sysinfo::Networks` (no
  subprocess; loopback interfaces are excluded) and touches no shared state.
- `rate(delta, elapsed)` divides, using `as_secs_f64`.  It previously used
  `as_secs().max(1)`, which truncated: a 1.1-second interval was reported as one
  second and every rate came out about 10% high, while any interval shorter than
  a second was divided by a whole one and came out low.
- `advance(previous, last_render, rx, tx, now)` is the state machine.  Below
  200 ms of elapsed time it returns the previous rendering unchanged and
  deliberately does *not* move the anchor, so the next call still has a
  full-length interval to divide by.  Without that guard two clients refreshing
  back to back would divide a handful of bytes by a few milliseconds and spike.

`net` is never cached.  It is a rate, the user wants it live, and at 1.05 ms per
call there is nothing worth caching.

## Caching

`src/cache.rs` holds `TtlMap<K, V>`: a `HashMap` whose values carry the `Instant`
they were written.  Freshness is decided by the *reader* — `age < ttl`, with a
TTL the caller supplies — which is what lets `--ttl` be a runtime flag rather
than a constant compiled into the store, and means there is no cold-start
special case: the first entry written expires exactly like the thousandth.
Expired entries are swept on insert, so a server running for weeks across many
panes and repositories cannot grow without bound.

There is no database.  SQLite was removed because it earned nothing: it cost
0.62 ms of the 27.7 ms a cached `gst` took, while `rusqlite`'s `bundled` feature
compiled SQLite from C and was the largest single item in a clean build.
Dropping it took 36 crates out of the dependency graph, 21 s off a clean build
and 2.3 MB off the binary.

`ServerState` carries three caches: git status (5 s, flag-settable),
is-inside-work-tree (5 min) and battery (30 s).  Every `ServerState` method is
synchronous, which makes it impossible to hold the mutex guard across an
`.await` — the combined `status-right` handler polls three segments
concurrently, and any one of them holding the guard across a suspension point
would deadlock the others.

## One `#()` call

The status bar makes a single `#()` call, `status-right`, which renders the git,
bandwidth and battery segments concurrently under `tokio::join!` and joins them
with the tmux literals that used to sit between the separate calls in
`tmux.conf`.  `assemble_right` is a pure function so those exact bytes are
pinned by unit tests.

The reason is measurement, not tidiness: a fork/exec costs 12.4 ms of CPU
(14.6 ms as tmux runs it, via `sh -c`), tmux gates `#()` to `status-interval`
per attached client per distinct command string, and the entire server-side
computation for all three segments is 2.6 ms.  Spawn count is the whole bill.
The TTL is applied per segment inside the call and never to the assembled
string: `net` is a rate, and caching the assembly would replay one measurement
window's average until the entry expired.

## Testing

255 unit tests.  All pure functions are extracted from async render functions so
they can run without spawning any subprocesses or reading hardware.

Key test patterns:
- `segments/git.rs` — ports all 17 Go status_line test cases from
  `yrl/pkg/git/statusline_test.go`, plus parsing, Area, and helper tests.
- `cache.rs` — every method takes the current `Instant` explicitly in its `*_at`
  form, so hit, expiry, boundary and sweep are all tested without sleeping.
- `server/handlers.rs` — `assemble_right` is pinned byte for byte, including
  the literals and the trailing space.
- `segments/network.rs` — `rate` and `advance` are pure, so the fractional-second
  arithmetic and the sub-200 ms guard are tested against an injected clock.
- `tmux/format.rs` — verifies every `Segment` method and both output modes of
  `colored_segment`, including the ARROW_RIGHT special case.
- `segments/window.rs` — tests `abbreviate_path` for root, home, deep nesting,
  dotfiles, truncation, and all dir-logo substitutions; tests `render` for icon
  selection, color, flags, and process animation.
