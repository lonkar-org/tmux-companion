# Benchmarks

## What actually costs anything

The status bar's cost is set by **how many processes tmux spawns**, not by what
they compute. Three measurements establish that, and they are the reason for
every change in the current design:

1. **A fork/exec costs 12.4 ms of CPU** — 14.6 ms as tmux actually runs it,
   wrapped in the `sh -c` that `#()` jobs go through. That is the floor. No
   amount of server-side optimisation touches it.
2. **tmux gates `#()` to `status-interval` per attached client**, per *distinct
   command string*. Five different `#()` calls at `status-interval 1` is five
   spawns a second; the same call repeated costs one.
3. **Server-side work is cheap by comparison.** Computing the entire right-hand
   side — git status, bandwidth and battery — takes 2.6 ms.

So collapsing five `#()` calls into one is worth more than any amount of tuning
inside them, and the only lever that touches the fork/exec half is making the
process itself cheaper to start.

## Method

Two independent measurements, both in [`bench-cpu.py`](./bench-cpu.py).

**Server CPU** comes from the server's own `getrusage(2)`, exposed on the socket
as the `__rusage` command: `utime stime cutime cstime request_count`, in
microseconds, self plus reaped children. This exists because nothing outside the
process can measure per-call server CPU at the resolution the question needs —
`top` quantises to 10 ms and `ps` to a whole second, while a segment costs
single-digit milliseconds. Requests go straight down the unix socket, so no
process spawn is counted.

**Spawn CPU** comes from `RUSAGE_CHILDREN` deltas around blocks of real
fork/exec — 4 interleaved blocks of 40, so drift and thermal state hit every arm
equally.

**End to end** is a real tmux (`tmux -L tcmeasure`) drawing a real status bar
with one attached client at `status-interval 1`, for 45 seconds, counting the
requests the server actually served and the CPU it actually burned. Total =
server CPU + (calls/s x spawn CPU via `sh -c`).

Every run uses `TMUX_COMPANION_SOCK` to put the benchmark server on its own
socket, so a measurement never touches the live status bar.

```sh
python3 bench-cpu.py target/release/tmux-companion "after" --tmux
python3 bench-cpu.py /path/to/old/binary  "before" --tmux
```

`bench.sh` is the older wall-clock harness and still works; it answers a
different question (how long a client waits) from this one (how much CPU the
machine spends).

Hardware: Apple Intel MacBook, 16 cores, macOS 25.5.0 Darwin. Measured
2026-08-11.

---

## Headline: 15.4% of a core → 1.8%

| | before | after |
|---|---|---|
| `#()` calls per second | 5.00 | **0.98** |
| server CPU | 80.90 ms/s | **7.39 ms/s** |
| spawn CPU | 72.88 ms/s | **11.04 ms/s** |
| **total** | **153.77 ms/s** | **18.43 ms/s** |
| **share of one core** | **15.38%** | **1.84%** |

An 8.3x reduction. "Before" is the five-call configuration — `clients`,
`vim-bg`, `gst` (with `#{pane_pid}`), `net`, `battery`. "After" is one
`status-right` call with a native-tmux left side.

## Where the win comes from

| Change | Saved | Kind |
|---|---|---|
| Five `#()` calls → one | ~58 ms/s of spawn | spawn count |
| Parse args before building the tokio runtime | 3.3 ms per spawn | per-spawn cost |
| Drop `#{pane_pid}` from the gst path | 18.5 ms per gst call | server |
| Cache `rev-parse --is-inside-work-tree` | 7.5 ms per gst call | server |
| `clients` + `vim-bg` off the bar | 21 ms per refresh | both |
| Delete SQLite | ~0 | build only — see below |

### Server CPU per request

| Segment | before | after | note |
|---|---|---|---|
| `status-right` (combined) | — | **2.59** | all three segments, concurrently |
| `gst` (no pane_pid) | 7.55 | **0.09** | rev-parse fork now cached |
| `gst` (with pane_pid) | 26.01 | 16.00 | process scan, only when asked by hand |
| `net` | 1.05 | 1.10 | never cached; it is a rate |
| `battery` (30 s cache) | 0.06 | 0.05 | |
| `clients` | 4.93 | 5.14 | off the bar |
| `vim-bg` | 16.25 | 16.00 | off the bar |

The `gst` line is the striking one: 7.55 ms → 0.09 ms, an 84x drop, entirely
from caching `git rev-parse --is-inside-work-tree`. That check sat *ahead* of the
status cache, so every cache hit still paid a full `git` fork for it. A path's
repo-ness effectively never changes; it is cached for 5 minutes rather than
forever so that `git init` in a watched directory is not misremembered until the
server restarts.

### Spawn CPU per fork/exec

| Shape | before | after | change |
|---|---|---|---|
| `/bin/echo` (floor) | 2.32 | 2.43 | — |
| client round trip | 9.78 | **6.56** | −32.9% |
| client via `sh -c` (as tmux runs it) | 14.58 | **11.29** | −22.6% |

`#[tokio::main]` built a multi-threaded runtime — one worker thread per core,
sixteen here — before clap had even parsed the arguments, on every client
invocation. Parsing first and giving clients a `current_thread` runtime is the
only change that touches the fork/exec half of the bill.

---

## Deleting SQLite: honest accounting

Replacing `rusqlite` with an in-memory TTL map on `ServerState` **is not a CPU
win**. SQLite was 0.62 ms of the 27.7 ms a cached `gst` cost. It was never the
problem.

What it actually buys:

| | before | after |
|---|---|---|
| crates in the dependency graph | 156 | **120** (−36) |
| clean release build (`-j 4`) | 59.8 s | **39.1 s** (−35%) |
| binary size | 6.33 MB | **4.01 MB** (−37%) |

`rusqlite`'s `bundled` feature compiles SQLite from C and was the single largest
item in a clean build. Removing it, along with the `tokio-tungstenite` and
`futures-util` dependencies of the deleted `speak` subcommand, takes 36 crates
out of the tree.

The cost is that the cache no longer survives a server restart. That is one
51 ms cache miss, once — not load-bearing.

---

## Why `status-interval` stays at 1

Measured against the server's own CPU time, 30-second windows, alternating, two
rounds each:

```
interval=1 -> 5.60%, 5.76% of one core
interval=5 -> 5.63%, 5.90% of one core
```

Identical within noise. tmux redraws the status line on pane output and activity
as well as on this timer, and in a working session those events, not the timer,
set the redraw rate. The cost to attack is the cost *per redraw*. Since a longer
interval buys nothing and the clock prints seconds, it stays at 1.

---

## Historical: individual segment latency

Wall-clock round trips including process spawn, from `bench.sh`. Kept for
continuity with earlier versions; the CPU figures above are the ones that
matter, since wall clock on an idle machine hides the cost that a busy one pays.

| Segment | min | avg | max | Notes |
|---|---|---|---|---|
| `gst` (cached) | 29ms | 30ms | 35ms | pre-rework, SQLite TTL hit |
| `gst --force` (cache miss) | 47ms | 48ms | 51ms | full `git status --porcelain=v2` |
| `battery` (cached ≤30s) | 38ms | 43ms | 44ms | IOKit pre-warmed at server start |
| `net` | 21ms | 22ms | 23ms | `sysinfo::Networks`, no subprocess |
| `clients` | 23ms | 26ms | 32ms | `tmux list-clients` |
| `vim-bg` | 47ms | 48ms | 49ms | `sysinfo::System` process scan |
| `window` | 17ms | 18ms | 22ms | pure in-process path abbreviation |

### Earlier work: native libraries replacing subprocesses

`netstat` → `sysinfo`, `pgrep`+`ps` → `sysinfo`, `ioreg`+`plist` → `battery`
crate. The headline there was not CPU but a stall: the old `net` segment hit a
`netstat` hang roughly one call in three, and worst-case refresh went from
>1100 ms to 57 ms.

| Change | Benefit |
|---|---|
| `sysinfo::Networks` replaces `netstat` | eliminates the ~1-in-3 netstat stall |
| `sysinfo::System` replaces `pgrep`+`ps` | two fewer sequential subprocesses per vim-bg |
| `battery` crate replaces `ioreg`+`plist` | with a 30 s cache, ~14x less server CPU |
| 5 s timeouts on remaining subprocesses | a hung `git` or `tmux` cannot block the server |
| battery pre-warm at server startup | no 600 ms IOKit cold start on first refresh |

## The port: `keys`

Measured 2026-09-22 on this machine, the same one every other figure here came
from.

| Path | Median | Minimum |
| --- | --- | --- |
| `tmux-companion keys --print`, warm daemon | 8.3 ms | 7.2 ms |
| `cat keys-cache.tsv \| fzf --filter`, the zsh equivalent | 18.8 ms | 18.6 ms |

Both are the non-interactive shape of the same work: read the rows, apply the
opening query, print what matched. The interactive halves aren't comparable
that way, since one of them waits for a person.

What the number leaves out matters as much as what it says. The 93 ms a warm
`prefix+?` took was 50 ms of fzf starting, 33 ms of `display-popup` and about
10 ms of everything else, so this replaces the fzf half and doesn't touch
`display-popup`, which is tmux's own cost and isn't going anywhere.

The zsh row is also flattered by its cache being warm and current. When
`tmux.conf` is newer, that path rebuilds with six `tmux list-keys` calls at
about 70 ms before it can show anything, and the daemon does the same work once
per config change rather than once per keypress.
