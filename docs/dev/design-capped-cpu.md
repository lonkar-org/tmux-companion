# Capped CPU

Status: draft
Owner: tmux-companion
Problem: the pickers are quick on an idle machine and slow on a throttled one. The tool does not cause the throttling, but the work it does before a picker can draw, and the work it does in the background, grows with every process it starts, and a capped CPU charges for each one.

This is the design for making the pickers fast under a capped CPU and keeping the background cheap enough not to add to the cap. The baseline it starts from is in [BENCHMARKS.md](../BENCHMARKS.md), "Under a capped CPU"; `just bench-capped` reproduces it.

## Terms

- Capped CPU. Less CPU than the work wants, from a thermal limit, Low Power Mode, or other work on the machine. The bench's load N (N busy loops) stands in for it, and on the measuring machine it also triggered a real thermal limit (`CPU_Speed_Limit` 64-68%).
- First row. The moment the first result row's bytes reach the terminal. What a person waits through after a key.
- Floor. What every picker pays before its own work: tmux opening the popup, the shell, exec and the loader (spawn), and `Paint::detect` (paint).
- Per-row work. Anything a picker does once per row: a tmux call, a directory listing, a file read.

## What the baseline says

Keypress to first row, median ms, from the baseline:

| Picker      | load 0 | load 32 | rows phase at 32 |
| ----------- | -----: | ------: | ---------------: |
| project     |    249 |     446 |              381 |
| panes       |     86 |     194 |              125 |
| new-window  |     91 |     152 |               86 |
| keys        |     45 |      71 |                4 |
| empty popup |     24 |      33 |                  |

1. Per-row work before the first frame is the cost. The project picker runs `list-windows` and `capture-pane -e` one after the other for each session and lists each of 166 directories, all awaited in a loop (`cli.rs`, `project_preview(r).await` per row), before drawing anything. The panes picker runs one `capture-pane` per pane, concurrently, against a tmux server that is short of CPU too. Every preview exists before the first frame, though a person sees one.
2. The floor is about 70 ms at load 32: spawn 40-48, paint 16-18, the popup itself. Paint is a blocking `tmux list-clients` (and sometimes `display-message`) plus a second parse of the config, which the caller already parsed.
3. Typing is fine. Five keys at once draw within 25 ms at load 32. The loop draws once per key, with no coalescing, and the preview's ANSI is parsed on every frame; neither shows yet, both will once rows are cheap and a frame is the larger share.
4. The daemon's idle cost is flat with load, 4-5 ms/s, and the tmux server's 4-6 ms/s. The background's real cost under a cap is the processes it starts:
   - three loops (inbox and notify every 2 s, journal every 5 s) each run their own `list-panes -a`, and the pickers then run it again;
   - window names and the chunk clock each run their own listing every 5 s;
   - battery is read every second while charging, because a charging battery is never cached;
   - the agents segment's cache TTL equals the inbox interval, so the bar sometimes races the inbox loop into an extra `list-panes`;
   - `?` after a daemon start or a tmux.conf edit runs nine `list-keys` in a row;
   - the status bar's `#()` is one `sh -c` and one client process per second per attached client, the largest steady cost (19 ms/s in the September bench).
5. Nothing runs at a lower priority. The daemon's loops, the snapshots and the processes they start compete with the picker a person is waiting on at the same priority.

## Rules

These go into CLAUDE.md as "Performance under a capped CPU", so a new picker or loop is held to them.

1. A picker starts no process and reads no file per row before its first frame. Per-row work is done for the selected row, when it is selected, and cached for the life of the picker; or it is read from a daemon cache.
2. A picker's first frame shows its chrome (border, title, prompt) before its rows are collected, when collecting them can take longer than the floor. The rows replace a static "loading" line; nothing animates.
3. One poll per tmux listing in the daemon. A loop that needs the pane list reads the shared one; a new loop does not add its own `list-panes`, `list-windows` or `list-clients` on a timer.
4. Data a picker needs and the daemon already has is served from the daemon. A picker asks the socket before it asks tmux.
5. Background work is made cheaper by starting fewer processes, not by lowering the priority of the ones it starts; see "What was measured and dropped".
6. A loop is slowed under pressure only once what it detects does not depend on how often it looks. Until then it runs at its own interval.
7. A frame is one write. Input that is already waiting is read before the next frame is drawn. Nothing is re-parsed when it has not changed.
8. A new picker or a new loop adds itself to `scripts/bench-capped.py` and its numbers at load 0 and 32 go in the commit message, against the budgets below.

### Budgets

Proposed from the baseline; each is a decision for the owner.

- Keypress to first row, any picker, load 32: 120 ms p50.

- Rows phase, any picker, load 0: 20 ms p99.

- Paint phase, load 32: 5 ms p50.

- Five keys typed at once, load 32: 30 ms a ceiling to the frame after the last.

- Idle, daemon plus tmux server with the bar ticking: 8 ms/s.

## The fixes, ranked

Ranked by what they take off a person's wait at load 32. Each lands as its own commit with the bench at loads 0 and 32 before and after.

1. **Lazy previews.** `Item` takes a preview source rather than a string: either text already in hand, or a closure that builds it. The picker builds the selected row's preview when the selection lands on it and keeps it for the picker's life. The project picker stops awaiting two tmux calls per session and a listing per directory before its first frame; the panes picker captures one pane, not every pane. Expected: project rows phase from ~380 to tens of ms at load 32, panes from ~125 likewise.
2. **Chrome first.** The picker enters the alternate screen and draws its border, title and prompt, then collects rows, then draws them. Keys typed in between sit in the terminal's input and are read once the rows exist. Perceived: the popup is a picker at the floor rather than at floor plus rows.
3. **Key coalescing.** After a key, read every event `poll(Duration::ZERO)` reports before refiltering and drawing once. No timer and no debounce: input already waiting is drained, nothing waits for input that might come.
4. **One write per frame, and the preview parsed once.** The terminal writes through a 64 KiB `BufWriter` flushed per frame instead of `LineWriter`'s 1 KiB pieces, which each cost the tmux server a parse. `ansi::into_lines` runs when the selection or the preview changes, not every frame.
5. **Paint off the critical path.** The config is parsed once and handed to `Paint::detect`. The accent and the other values `detect` asks tmux for come from the daemon, which already knows the theme, with the tmux call as the fallback when the daemon cannot answer.
6. **Keys warm at start.** The daemon builds the keys rows when it starts and when tmux.conf changes, and `keys::collect` asks tmux for fewer listings: one `list-keys -N` and one full `list-keys`, split by table, in place of two per table.
7. **One pane poll.** One loop in the daemon runs `list-panes -a` with every field any consumer needs and publishes the result; inbox, notify and journal read it, and so does the agents segment. A `__panes` request serves it to the panes picker, which then needs tmux only for the selected row's capture. The daemon keeps the last few lines of each agent pane it already captures for the inbox, so the agents among the panes have previews without a call.
8. **Project rows from the daemon.** Sessions, the zoxide list and the theme colours change slowly; the daemon keeps them behind a TTL and the project picker asks the socket. The 179 theme files and the zoxide process stop being per-press costs.
9. **The cheap ones.** Battery cached for 10 s while charging. The agents TTL is the inbox interval plus one second, so the bar never races the loop. One `sysinfo::Networks` kept and refreshed rather than rebuilt every second. Concurrent git refreshes for one path share one run.
10. **Background priority and backoff.** Tried, measured and dropped; see "What was measured and dropped".
11. **The bar, pushed.** See below.

### Later: control mode

One long-lived `tmux -C` client in the daemon replaces most of what is left: commands go down a pipe instead of a fork each, `%window-*` and `%session-*` notifications make window names and the session list event-driven, and `refresh-client -B` subscriptions (tmux 3.2 and later, so inside the 3.4 floor) can deliver `pane_current_path` changes without a poll. It is the largest change here and the riskiest: a control client is a client, it shows in `list-clients`, it has to be told apart from a person's in the clients segment, the chunk clock and `brief`, and its pipe must never block the daemon. It waits until the fixes above are measured, so its benefit is measured against a cheap baseline rather than an expensive one.

## What was measured and dropped

Each with `just bench-ab` against the commit before it, alternated, two pairs.

- **Background priority for the loops' children.** A task-local mark made `tmux::command` lower a child between fork and exec: nice 10, and on macOS `PRIO_DARWIN_BG`. At load 32 the pickers' p95 rose from 68-81 ms to 116-157 with the Darwin band, and the daemon's own CPU from 4.9 to 6.4 ms/s; nice alone moved no latency and still cost the daemon 5.2 -> 6.8 ms/s. A `pre_exec` hook stops Rust from using `posix_spawn`, so every child is forked from a large multi-threaded process; and a lowered tmux client holds the single-threaded tmux server longer, which is where a picker's own calls queue. Wrapping each call in `nice` or `taskpolicy` is an exec more per call. The thread-QoS runtime the fix list described was not built: it would have reached the children only through the same hook.
- **Backoff under pressure and with nobody attached.** The inbox, notify, journal and window-name loops waited twice as long under thermal pressure or Low Power Mode (read from `pmset` every 30 s; two calls cost 0.65 ms/s) and four times as long with no client. Idle cost went up, 2.0 -> 3.1 tmux calls/s, not down: the inbox decides an agent stopped by its silence over its own intervals, so a slower inbox saw working agents stop, captured their screens, and would have nudged. Backing off needs that detection made independent of the poll interval first.
- **Building the key rows at daemon start.** `list-keys` with no server starts one, and rows taken while tmux.conf is still being sourced stay cached until the file's mtime changes; the bench's keys picker came up with somebody else's bindings. The first `?` collects instead, in one round of five calls.
- **A `__panes` request for the panes picker.** Its two calls already run side by side; a list up to a loop interval old would miss a pane split a moment before the picker opened.
- **Project rows from a daemon cache.** After the three reads went side by side, what is left is about the slower of `list-sessions` and zoxide; the session list has to be current, and the daemon would run zoxide in the tmux server's environment rather than the popup's.
- **The theme accent with the keys rows.** The daemon has no cheap, current source for it (a theme change does not touch tmux.conf). It costs the keys picker 10-15 ms of paint and waits for control mode.

## The status bar both ways

Today `status-right` is `#(tmux-companion status-right ... #{pane_current_path})`: every `status-interval`, for every attached client, tmux starts `sh -c` and a client process, which asks the daemon over the socket and prints the answer. That stays, unchanged, as `[bar] mode = "call"`.

`[bar] mode = "push"` turns it round. The daemon renders the right side once per session with an attached client, on the same one-second tick, and sets it in one tmux invocation for all of them:

```sh
tmux set -t s1 @tc-right '...' \; set -t s2 @tc-right '...' \; refresh-client -S
```

and the tmux.conf line becomes `set -g status-right "#{@tc-right}"`, which tmux expands without starting anything. The path each session's git segment needs is that session's active pane's, which the shared pane poll already has. Two clients on one session see the same bar, which is what they see today, since `pane_current_path` is the session's active pane in both.

What it costs and saves, to be measured rather than assumed:

- One `tmux` process per second for the whole server, against one `sh` and one client per second per attached client. With one client the process count is the same and the difference is the shell and the socket round trip; with three clients it is one against six.
- A pane switch shows on the bar at the next tick, up to a second, where today it shows at the next `status-interval`, also up to a second. A `after-select-pane` hook that asks for a push can make it immediate.
- Under control mode the one process per second goes away too.
- If the daemon stops, the option keeps its last value and the bar freezes. Push mode clears `@tc-right` on shutdown and the health mark is part of what is pushed, so a frozen bar is a bar that says the daemon is gone, which `doctor` then explains.

Measured (BENCHMARKS.md, "after the fixes"), steady state, one client: the bar costs 31.2 ms/s in call mode and 24.9 in push at load 0, 41-43 and 31.5 at load 32, and each further client adds about a call's cost to call mode and nothing to push. Push is the recommended mode on that evidence, with the fallback line, so a bar whose daemon stopped keeps working. It is not the default yet: a pushed bar with its daemon killed outright shows its last figure until a new daemon replaces it, which call mode cannot do, and the e2e suite has no attached client to test the loop end to end.

Two measured differences from the design above: no `refresh-client -S`, since `status-interval 1` already re-expands `#{@tc-right}` every second and a refresh needs a client target the daemon does not have; and the set-option target is `=session:`, with the colon, because `set-option -t` takes a pane target and refuses `=session` alone.

The three example `tmux.conf` files use the fallback line, and the default mode is `auto`: the daemon pushes when tmux's global `status-right` reads `@tc-right` and tmux is 3.4 or newer, rechecked once a minute, and leaves the bar to the call otherwise. A `status-right` still on the call-only line therefore pays nothing for push, which a plain `push` default would have charged it. `doctor` has a `bar` line saying which mode is in effect and why, including both reasons for not pushing; the health mark carries only the tmux-too-old one, since a `status-right` that does not read the option is a choice, not a fault.

### When the daemon crashes

A pushed bar shows what the daemon last set, so a daemon that dies without cleaning up would freeze it, and nothing on a pushed bar would start a new one. The daemon therefore writes a heartbeat, the global option `@tc-alive`, every 10 seconds while it pushes, and the recommended line reads the pushed value only while it is set and the heartbeat is under 30 seconds old:

```tmux
set -g status-right "#{?#{&&:#{@tc-right},#{e|<:%s,#{e|+:#{@tc-alive},30}}},#{@tc-right},#(tmux-companion status-right --branch-max-len 40 #{pane_current_path})}"
```

`%s` is the status line's own clock, so tmux itself is the canary and no other process is needed: within 30 seconds of a crash the line runs the call, the call starts a daemon, and the bar is pushed again. A clean stop (a signal, `restart`, an upgrade's `__shutdown`) unsets both options at once. A daemon that finds a heartbeat on start knows the last one stopped uncleanly: it writes that to the daemon log and a line to `crashes.tsv` in the state directory, and the health mark says so; two within a minute adds a second reason. Both stay for the failure window or until `health ack`. No popup: it would take focus without saying so, and the health mark is already read out by `brief`, `health` and `doctor`.

Rehearsed on a private server with a real client: `kill -9` of a pushing daemon, a new one 23-28 s later, both unclean stops logged and counted, the repeat reason after the second, and a `restart` afterwards counted as nothing. The format arithmetic was checked on tmux 3.4 and 3.7c.

## Where it ended

Against the budgets above, at load 32 unless said:

| Budget | Result |
| --- | --- |
| first row, any picker, load 32, 120 ms p50 | project 103, panes 69, keys 67 meet it; new-window 134 misses by 14 (its directory source) |
| rows phase, any picker, load 0, 20 ms p99 | panes 16, keys 3 meet it; project 44-57 and new-window 64-66 miss, and include drawing the loading frame now (slowest of 15, so a p99 only nominally) |
| paint, load 32, 5 ms p50 | 0.1-0.3 for every picker but keys, 15 (the accent; waits for control mode) |
| five keys at once, load 32, 30 ms ceiling | 18-19 for every picker |
| idle, daemon and tmux server, 8 ms/s | 8.4 in call mode, 7.2 in push, at load 0 |

In Low Power Mode on battery (BENCHMARKS.md) the 120 ms first-row budget at load 32 is missed by project, 135, and new-window, 176; panes 94 and keys 89 still meet it. The floor every picker pays rises with the clock: spawn 39 idle and 60 at load 32, against 32 and 42 on mains.

What is left, in the order it would pay: new-window's directory source; the accent for keys; the daemon's own idle polling (git status for the bar, window names, the chunk clock and the inbox's captures, 12-15 ms/s of the processes it starts); loops whose detection does not depend on their interval, which is what backing off needs; and control mode, which removes most of the rest.

## Accessibility

Companion first holds for every fix here; most of them change nothing a person sees or hears, and the ones that do:

- Chrome first draws the frame before the rows. A screen reader reading the popup hears the title and an empty list, then the rows. The "loading" line is a plain line in the list position, no spinner, no animation, and it is replaced once, never updated in place. The command that is each picker's source of truth (`panes --print`, `inbox`, `brief`) does not change.
- Lazy previews change when a preview is built, not what it says. The selected row's preview is there when the selection is; a screen reader moving down the list hears the same text as before.
- Key coalescing drops no keys and reorders none; it draws fewer intermediate frames, which a screen reader announces less of, which is better.
- Push mode changes how the bar is filled, not what it says. `read-bar`, the bar's screen-reader path, is not on main yet; when it lands it asks the daemon for the segments rather than reading `@tc-right`, so it reads the same in both modes, and a test holds it to that.
- Backoff is announced: the health mark carries "slowed: thermal" or "slowed: low power", which `brief` reads out, and `read-bar` will.
- Nothing here adds a timer a person has to beat. The backoff timers are the daemon's, not the person's.

## Out of scope

- Rewriting the matcher. Matching is a few ms at load 32 and the bench does not show it.
- A persistent picker process kept running between presses. It would hide spawn, but it is a terminal-owning process the daemon would have to manage, and the floor it removes is mostly tmux's own popup.
- Showing last time's rows and swapping fresh ones in. Rows changing under a cursor a screen reader is following is focus moving without being announced.

## Acceptance

- Every picker meets the budgets above at load 32 in `just bench-capped`, or the commit that misses says by how much and why.
- `just bench-capped` covers every picker that collects rows per row.
- The idle cost, daemon plus tmux server, is no higher than the baseline at any load, and lower with push mode.
- Push mode's bar says what call mode's says, segment for segment, held by a test; `read-bar` joins that test when it lands.
- CLAUDE.md carries the rules, and a new picker or loop that breaks one fails review on the rule rather than on a person noticing.
