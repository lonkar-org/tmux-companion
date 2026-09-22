# Design

## The problem

A tmux config that shells out pays for the shelling out, not for the work.

tmux gates `#()` to `status-interval` per attached client, so a bar that calls
six programs a second costs six fork-and-execs a second whatever those programs
compute. A fork and exec is 12.4 ms of CPU, 14.6 as tmux runs it through
`sh -c`. Computing every segment on the bar takes 2.6 ms.

The same sum runs the other way for a keybinding. A popup that starts zsh, reads
a config file and pipes into `fzf` spends most of its latency before the first
frame, and none of that's the search.

So: one process that stays up and holds its state, and clients that connect,
send a line, read a line and exit.

## Shape

Same binary, two modes.

```
tmux-companion server        # binds the socket, serves until killed
tmux-companion <cmd> [args]  # connects, sends one line, prints, exits
```

The client starts the server on first use. It tries to connect; on failure it
forks the server as a detached child and retries with backoff, ten attempts at
50 ms.

```
   tmux status-interval                    a keybinding
   #(tmux-companion status-right …)        display-popup -E "tmux-companion keys"
              │                                        │
              └──────────── unix socket ───────────────┘
                              │
                    tmux-companion server
                      tokio, one task per connection
                      ServerState behind an Arc<Mutex<_>>
                        caches, the bandwidth anchor, the key rows
                      background tasks, each off by default
```

The wire is newline-delimited JSON, one request per connection.

```jsonc
{"cmd": "gst", "args": {"path": "/repo"}, "version": "0.1.0+1790054855"}
{"output": "#[fg=color025,…]", "error": null, "version": "0.1.0+1790054855"}
```

Each command owns one serde struct with `deny_unknown_fields`, used by the clap
flags, the wire and the handler. A key that was never sent used to read back as
`None` and change behaviour in silence; now it's an error that names the field.

The `version` is a build stamp from `build.rs` rather than the crate version,
during development every build carries the same version number, and the question
a client actually needs answered is whether the daemon's running the binary that
was just installed. A mismatch replaces the daemon and retries once.

### Singleton

Before binding, the server connects to its own socket. If that succeeds another
instance is up and this one exits. If it fails, a stale socket file's removed and
the bind attempted. Two concurrent starts are benign: one bind wins and
the loser exits. The socket is created 0600.

## Where work happens

Three places, and which one a thing belongs in is the main design decision in
the repository.

**In the daemon.** Anything that returns bytes for the bar, anything with a
cache worth keeping warm, and anything on a timer. The daemon hasn't got a terminal, so it never draws.

**In the client.** Anything with a terminal: the pickers, the dialogs, the
`ratatui` screens. The daemon answers with rows and the client draws them, which
is also why a picker's testable against a `TestBackend` without a socket.

**In a pure function.** Everything else. Parsing, formatting, arithmetic and
ordering come out of the async functions that do the I/O, so they run in tests
without a subprocess, a repository or a clock. Building a tmux session is a
dozen ordered commands, and `session_commands` returns them as data precisely so
the order can be asserted.

## Invariants

These are the ones that cost something to rediscover.

**Every `ServerState` method is synchronous.** The combined `status-right`
handler polls its segments concurrently under `tokio::join!`, and any one of
them holding the mutex guard across a suspension point would deadlock the
others. Keeping them sync makes that impossible to write rather than merely discouraged. Compute first, take the lock for one statement.

**The TTL belongs to the reader.** `TtlMap` records when an entry was written
and nothing else; freshness is `age < ttl` evaluated at read time with a TTL the
caller supplies. That is what lets `--ttl` be a runtime flag, and it means there's
no cold-start case: the first entry expires exactly like the thousandth.
Expired entries are swept on insert, so a daemon running for weeks across many
repositories cannot grow without bound.

**Never cache the assembled bar, only its segments.** `net` is a rate. A cached
assembly would replay one measurement window's average for as long as the entry
lived, which reads as a frozen bar rather than a quiet network.

**`status-right` never passes a pane pid to the git segment.** Doing so calls
`has_suspended_nvim`, which walks the whole process table and cost 18.5 ms of
that segment's 26.0. The marker's given up on the bar, and it's still there from `gst <path> <pid>` by
hand.

**The assembled bar is pinned byte for byte.** `assemble_right` is pure and its
output is asserted in full, including the literals and the trailing space, so a
refactor that drops a separator fails a test rather than a glance. Across the
port, `compare-output.sh` checked the same claim against the previous binary on
every commit.

## Configuration

One TOML file, found at `~/.tmux-companion.toml` or
`$XDG_CONFIG_HOME/tmux-companion/config.toml`, parsed once at startup and never
per request. A daemon exists partly to stop repeated config reads, so reading one per render
would be a poor joke. `reload` is what picks up an edit.

Parsing uses `deny_unknown_fields`, and an unknown key's answered with the
field names serde already knows, so a typo names itself. A bad file stops the
daemon starting and says which line and what it expected, cause a status bar
that quietly ignores half a config is worse than one that refuses to start.

Glyph substitution happens once at the response edge rather than at the 263
places a glyph is written, so a preset for people who haven't got a Nerd Font is one lookup on the way out.

## Background tasks

Four, each a tokio task spawned at startup only when its config says so:
fetching repositories the bar has drawn, sourcing tmux's config when it changes,
naming windows from the job table, and announcing a long command that finished
out of sight. All four default to off. A daemon that starts reaching a remote,
renaming windows or sourcing a config on its own is a surprise, and the argument
for one binary is that it costs less than the plugins, not that it decides more.

This is also what replaced a detached shell loop with a PID lock file. A task
lives as long as the process and stops when it stops, which is the whole of the
lifetime management the zsh version needed that lock file for.

## Testing

620 unit tests and 22 integration tests, the whole suite under a second, no
network and no fixtures.

The pattern throughout is that the pure core's tested and the I/O is a thin
wrapper over it. `TtlMap` takes the instant explicitly in its `*_at` form, so
expiry and sweeping are tested without sleeping. `rate` and `advance` in the
network segment take an injected clock. `scan` in `open` takes an `exists`
closure. The pickers render to a `TestBackend`.

The integration tests speak to a real socket in a temporary directory rather
than setting `TMUX_COMPANION_SOCK`, because that variable's process-global and the tests run in parallel.

Where a port replaced a script, it was checked against the script rather than
read alongside it: the theme report matches the Python on all 76 themes, `run` is
byte-identical to `fc -ln` over 1127 commands, and `cheatsheet` lists the same
34 bindings.
