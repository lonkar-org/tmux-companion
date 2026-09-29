# Benchmarks

What this costs on a working machine, measured from inside tmux: a key is
pressed, tmux runs the binding it would run for a person, and the clock stops
when the first row of results reaches the terminal.

That is the whole difference from
[BENCHMARKS-before-port.md](./BENCHMARKS-before-port.md), which measured the
socket. Measuring the socket answers "is the code fast" and measuring the
session answers "is it fast to use", and the two answers here are not the same.

Everything below is one machine on one day: a MacBookPro16,1, 16 cores, macOS
26.6.2, tmux 3.7c, zsh 5.9, fzf 0.74.3, measured 2026-09-23. Reproduce it with
`just bench`.

## The before arm is the real thing

Both arms are the code that actually ran, not a reconstruction of it.

The pickers are the zsh still installed at `~/.config/tmux/comrades`. The bar
is read out of the `mysetup` repository at `18e8db9^` -- the parent of the
commit that replaced those scripts with this tool -- so `battery-life.zsh`,
`net-monitor.zsh`, `check-clients.zsh`, `check-vim-in-background.zsh` and
`~/go/bin/yrl gst` are the same files that drew the bar before the port.

Neither is vendored into this repository. They are somebody's dotfiles, they
exist only to reproduce a number, and a copy kept here would drift from the
original with nothing to notice. `scripts/bench.sh` checks for them and, on any
machine that does not have them, measures the new arm alone and says so.

## Headline: the bar, 34.4% of a core to 3.0%

45 seconds, one attached client, `status-interval 1`, idle session.

| | before | after |
|---|---|---|
| `#()` spawns per second | 4.87 | **1.00** |
| spawn CPU | 335.78 ms/s | **19.02 ms/s** |
| tmux server CPU | 8.22 ms/s | **3.11 ms/s** |
| daemon CPU | — | 7.58 ms/s |
| **total** | **344.01 ms/s** | **29.71 ms/s** |
| **share of one core** | **34.40%** | **2.97%** |

11.6x. Three numbers are added rather than one measured, because no single
counter sees all of it: a reaped child's CPU is invisible from outside the
process that reaped it, so the spawn half is counted (each wrapper records its
own name) and priced separately (`RUSAGE_CHILDREN` around blocks of the same
command), and the tmux server and the daemon are read from `ps` and from the
daemon's own `getrusage`.

### What each call cost

| call | per spawn | per second |
|---|---|---|
| `battery-life.zsh` | 118.49 ms | 115.90 ms/s |
| `net-monitor.zsh` | 83.60 ms | 79.91 ms/s |
| `yrl gst` | 72.66 ms | 71.07 ms/s |
| `check-vim-in-background.zsh` | 37.09 ms | 36.27 ms/s |
| `check-clients.zsh` | 33.36 ms | 32.63 ms/s |
| `window-status.zsh` | 18.03 ms | 0.00 ms/s |
| **`tmux-companion status-right`** | **19.02 ms** | **19.02 ms/s** |

One call now does what the top five did, for less than any one of them.

`window-status.zsh` reads zero per second and that is a real reading, not a
missing one. tmux re-runs the window-status format when the window list is
redrawn, and an idle session with one window does not redraw it. It costs
18 ms per window each time something does.

The 4.87 rather than 5.00 spawns per second is the same arithmetic from the
other side: tmux gates `#()` to `status-interval` per attached client per
distinct command string, five distinct strings, and the missing 0.13 is the
window either side of a 45 second sample.

## The pickers: no faster, half the CPU

15 presses each, both arms, same session. The clock starts when `send-keys -K`
returns and stops when the bytes for the first result row reach the client.

| | before | after |
|---|---|---|
| project picker, keypress to first row | 102.2 ms | 103.3 ms |
| new-window picker, keypress to first row | 81.8 ms | 86.0 ms |
| project picker, CPU per press | 96.7 ms | **47.3 ms** |
| new-window picker, CPU per press | 66.7 ms | **33.3 ms** |
| `display-popup` on its own | 23.0 ms | 21.1 ms |

**The pickers do not open faster.** One is a millisecond behind and the other
four, which on fifteen presses is the spread rather than a difference. What
halved is what opening one costs the machine.

This is worth saying plainly because the obvious claim -- rewrote it, so it
must be quicker -- is not what the measurement says. What a person waits
through is `display-popup` at 21 ms, then a process starting, then a terminal
painting a screenful. None of that got cheaper, and the row computation that
did get cheaper was never the part being waited on.

Two things follow. The CPU column is the one that matters on a laptop, where
every press of a key charged to the battery, and 15 presses an hour at half the
price is worth having. And the fzf arm is flattered here by the metric: fzf
streams, drawing rows as they arrive, while this builds its list and then
renders, so "first row" arrives earlier for the streaming design even when the
list takes longer to finish.

CPU is measured with zsh's `times` builtin, which reports the children's user
and system time, written by a wrapper after the picker exits. It quantises to
10 ms, so those are means over 15 presses and not single readings.

## Inside the daemon

Per request, from the daemon's own `getrusage`, sent straight down the socket
so no process spawn is counted. `just bench-segments`.

| request | CPU |
|---|---|
| `status-right` (all three segments, concurrently) | **1.43 ms** |
| `gst`, no `pane_pid` | 0.14 ms |
| `gst`, with `pane_pid` | 18.25 ms |
| `net` | 1.22 ms |
| `battery`, 30 s cache | 0.06 ms |
| `clients` | 7.26 ms |
| `vim-bg` | 17.84 ms |

And what a client costs before it has asked anything:

| shape | CPU |
|---|---|
| `/bin/echo`, the floor | 2.52 ms |
| client round trip | 8.03 ms |
| client via `sh -c`, as tmux runs it | 13.07 ms |

Those two tables are the 19.02 ms the bar pays per second, taken apart: 13 of
it is the shell and the process, 1.4 is the answer, and the rest is the client
writing to a terminal.

`gst` with a `pane_pid` costs 18.25 ms against 0.14 ms without one, because the
pid makes it look for a suspended editor and that enumerates the process table.
This is why the bar never passes one, and it is the single largest item in
either table.

## Method, and three ways it was wrong first

`scripts/bench-keys.py` starts a private tmux server, attaches a real client on
a pty, sets that pty's window size, and fires the real binding with
`send-keys -K -c <client>`. It watches the pty's byte stream rather than asking
tmux what is on screen.

That is not a preference. **`capture-pane` cannot see a popup**: verified on
tmux 3.7c, while a popup is drawing, the active pane and every pane on the
server come back empty. The pty is the only place those bytes appear. It is
also the cheaper end -- each `capture-pane` poll is a fork of its own, so at a
2 ms poll interval the measurement costs more than the thing it measures.

Three bugs, each of which produced a plausible number before it was caught:

- **A picker that opened in 15.8 ms.** It had not. The popup that closed a
  moment earlier was still repainting the session underneath it, those bytes
  arrived after the buffer was drained, and the next run matched them before
  its own popup drew anything. The drain now reads until the client has been
  quiet for 150 ms.
- **Every press timing out at ten seconds.** The first attempt at the CPU
  column wrapped each picker in `/usr/bin/time -l … 2>>log`. `time` writes its
  report to stderr and the child inherits it, so the picker's own drawing went
  into the log file instead of the terminal and nothing ever appeared on
  screen. `times`, a shell builtin, costs no process and goes nowhere near the
  picker's file descriptors.
- **A bar bill built from an average.** The five calls were priced at the mean
  of the six commands times the number of spawns. `battery-life.zsh` costs
  118 ms and `window-status.zsh` costs 18, so averaging them charged each one
  the other's price. Each call is now counted and priced on its own.

The marker is the first row naming a benchmark directory, not a particular
row: the two pickers order and scroll their lists differently, so waiting for
one named row would time the scroll position rather than the draw.

Both arms read a private zoxide database of twenty directories
(`_ZO_DATA_DIR`), so the measurement neither reads the author's real history
nor writes a visit into it, and both pickers are given the same list. Twenty
because both filter by the pane's own directory the moment they open, and a
three-entry database measures a picker with nothing to do.

## Running it

```sh
just bench              # pickers and the bar, both arms
just bench-bar          # the bar only
just bench-pickers      # the pickers only
just bench-segments     # daemon CPU per request

RUNS=25 SECS=90 just bench     # longer, for a quieter number
```

A run takes about six minutes. The old arm needs `fzf`, `zoxide`,
`~/.config/tmux/comrades`, `~/git-repos/mysetup` and `~/go/bin/yrl`; without
them the new arm is measured on its own.
