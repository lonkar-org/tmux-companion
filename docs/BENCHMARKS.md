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

## What it costs

Measured from inside tmux on one machine, against the zsh this replaced, with
the method beside the numbers below.

|                                    |                                   |
| ---------------------------------- | --------------------------------- |
| the bar                            | 29.71 ms/s, 3.0% of a core        |
| the zsh bar it replaced            | 344.01 ms/s, 34.4% of a core      |
| a picker, keypress to first row    | 86 ms, and the zsh took 82        |
| a picker, CPU per press            | 33.3 ms, against 66.7 for the zsh |
| the whole right side, computed     | 1.43 ms                           |
| one fork and exec, as tmux runs it | 13.07 ms                          |

The picker rows are the honest part. Opening one is not faster: what a person
waits through is `display-popup` at 21 ms, a process starting and a terminal
painting, and none of that got cheaper. What halved is what it costs to do.

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
| --- | --- | --- |
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
| --- | --- | --- |
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
| --- | --- | --- |
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
| --- | --- |
| `status-right` (all three segments, concurrently) | **1.43 ms** |
| `gst`, no `pane_pid` | 0.14 ms |
| `gst`, with `pane_pid` | 18.25 ms |
| `net` | 1.22 ms |
| `battery`, 30 s cache | 0.06 ms |
| `clients` | 7.26 ms |
| `vim-bg` | 17.84 ms |

And what a client costs before it has asked anything:

| shape | CPU |
| --- | --- |
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

## Under a capped CPU: the baseline, 2026-10-09

The pickers are quick enough on an idle machine and slow on a throttled one,
so this measures the same pickers under load, phase by phase, before any of
the fixes in [design-capped-cpu.md](./dev/design-capped-cpu.md) land.

Same MacBookPro16,1 (i9-9980HK, 16 logical CPUs, no efficiency cores), macOS
26.6, tmux 3.7c, on AC power, Low Power Mode off, commit `746c3b2`. Load N is
N busy loops at normal priority for the length of the run; each condition
ran twice, alternated, 15 presses per picker, and the numbers are the mean
of the two rounds' medians (the rounds agreed within a few percent, except
`keys` at no load, 40 and 50 ms). The load was enough to make the machine
throttle itself: `CPU_Speed_Limit` fell to 64-68% after every loaded run, so
the 16 and 32 columns are contention and a lower clock together, which is
what a capped CPU looks like in use.

The setup is a working day's server, built by `scripts/bench-capped.sh` in a
throwaway home: 8 sessions, 16 windows, 7 agent panes (2 printing as they
work), 6 editors, 3 shells, 166 directories in zoxide, and a copy of a real
configuration with 179 theme files.

Keypress to first row, ms, median:

| Picker | load 0 | load 16 | load 32 | 32 vs 0 | rows phase at 32 |
| --- | ---: | ---: | ---: | ---: | ---: |
| project | 249 | 363 | 446 | 1.8x | 381 |
| panes | 86 | 153 | 194 | 2.3x | 125 |
| new-window | 91 | 123 | 152 | 1.7x | 86 |
| keys | 45 | 59 | 71 | 1.6x | 4 |
| empty popup (tmux alone) | 24 | 30 | 33 | 1.4x | |

Where the time goes, ms, median, every picker:

| Phase | load 0 | load 16 | load 32 |
| --- | ---: | ---: | ---: |
| spawn: key to the picker's `main` (tmux, shell, exec) | 25-32 | 34-40 | 40-48 |
| paint: `Paint::detect`, a tmux call and a config read | 8-11 | 12-16 | 16-18 |
| draw: the first frame | 1-2 | 2 | 2-4 |
| five keys typed at once, to the frame after the last | 10-16 | 16-19 | 19-25 |
| rows | see the table above | | |

Idle, with the bar ticking every second: the daemon 4.3-5.0 ms/s and the tmux
server 4.3-6.0 ms/s, about the same at every load.

What it says:

- Collecting rows is most of the wait wherever there is any. The project
  picker spends 214 of its 249 ms there idle and 381 of 446 ms at load 32:
  a session preview is two tmux calls in a row for each session, and a
  directory preview a listing of each of 166 directories, all before the
  first frame.
- The panes picker degrades most, 2.3x, because its rows are one
  `capture-pane` per pane, all at once, against a tmux server that is itself
  short of CPU.
- `keys` reads its rows from the daemon's cache in 3-4 ms. What is left of its
  71 ms is spawn, paint and the popup, the floor every picker pays. That
  floor is about 70 ms at load 32, and paint is the part of it this tool
  owns.
- Typing is not the problem: five keys cost 25 ms at worst.
- The daemon's idle cost does not grow with load. What it adds under a
  capped CPU is the work it hands to the tmux server and the processes it
  starts, which is measured per fix rather than here.

Reproduce with `CONFIG=mine just bench-capped` (loads 0, 16 and 32, two
rounds), or without `CONFIG=mine` for the default configuration.

## Under a capped CPU: after the fixes, 2026-10-09

The same machine, the same setup, the same loads and two rounds, on the branch
with every fix in [design-capped-cpu.md](./dev/design-capped-cpu.md) that
survived measurement. Medians, mean of the two rounds, ms:

| Picker | before 0 / 16 / 32 | after 0 / 16 / 32 | at load 32 |
| --- | --- | --- | ---: |
| project | 249 / 363 / 446 | 78 / 90 / 103 | 4.3x faster |
| panes | 86 / 153 / 194 | 50 / 57 / 69 | 2.8x faster |
| new-window | 91 / 123 / 152 | 95 / 111 / 134 | 1.1x faster |
| keys | 45 / 59 / 71 | 47 / 55 / 67 | about the same |
| empty popup (tmux alone) | 24 / 30 / 33 | 23 / 27 / 32 | |

- The project and new-window pickers now draw their frame before their rows,
  46 ms after the key idle and 63-65 ms at load 32, so the time to a picker on
  screen is about half the time to its first row. New-window's first row is
  4 ms later than before at no load for the same reason: the frame is drawn
  first, and its rows are the directory source, which no fix here reached.
- Previews are built for the selected row only. That is most of project's and
  all of panes' gain.
- Typing is unchanged: five keys at once draw in 14 ms idle and 18-19 under
  load.
- Colour lookup (paint) left the critical path for every picker but keys,
  whose rows come from the daemon faster than the lookup can finish.

The bar, idle, one attached client, everything it costs per second: the
daemon, the processes it starts, the tmux server, and in call mode the shell
and client tmux starts for `#()` (priced at 10.5-19.7 ms each by running them):

| Mode | load 0 | load 32 |
| --- | ---: | ---: |
| call (`#()`, the default) | 31.2 ms/s | 41-43 ms/s |
| push (`[bar] mode = "push"`) | 24.9 ms/s | 31.5 ms/s |

Push starts nothing for the bar, and each further client adds about a call's
cost to call mode and nothing to push. These are steady-state figures, taken
after three minutes of picker presses; a run that measures idle straight after
start-up includes the inbox capturing every agent for the first time and reads
higher in both modes.

In Low Power Mode, on battery, the same branch, two rounds, call mode. The
machine reported `CPU_Speed_Limit` 100 throughout, so what Low Power Mode caps
here does not show in that figure; it shows in everything below:

| Picker | load 0 | load 32 | vs mains at load 32 |
| --- | ---: | ---: | ---: |
| project | 91 | 135 | +31 |
| panes | 61 | 94 | +25 |
| new-window | 118 | 176 | +42 |
| keys | 58 | 89 | +22 |
| empty popup (tmux alone) | 30 | 48 | +16 |

Spawn, the part of every press before the picker's own code runs, is 39 ms idle
and 60 at load 32 here, against 32 and 42 on mains; five keys at once take 28-30
ms at load 32. The bar costs 42-44 ms/s idle and 57 at load 32 in call mode.

Reproduce with `CONFIG=mine just bench-capped`, and `BAR=push` for the second
row of the bar table. `just bench-ab REV` compares a revision with the working
tree.

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
