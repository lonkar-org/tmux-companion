# Performance

This was done to make sure tmux-companion doesn't contribute to my devices throttling conditions, if your interested I have post [about it](https://yogesh.lonkar.org/posts/macbook-monitor-throttle/) which later lead to this.

This page is for my own and claude's reference if in future this question comes again cause BENCHMARKS become stale and conditions and what trigger it never gets documented.

| When         | What changed                                                                                                                                                                              | Why                                                                                      |
| :----------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :--------------------------------------------------------------------------------------- |
| June 2026    | tmux-companion replaced the zsh scripts behind the status bar and the window names                                                                                                        | the window segment and the bar as a whole were slow                                      |
| August 2026  | the status bar collapsed to one `#()` call, 15.4% of a core to 1.8%                                                                                                                       | the same                                                                                 |
| October 2026 | pickers fast under a capped CPU and the bar pushed by the daemon (`perf`), then pickers on screen sooner and the bar right straight after a switch (`perf/perceived`, `perf/bar-instant`) | the monitor throttling incident in the post above, then cleaning up the MacBook after it |

| What     | Value/state                                                                                                                                                                      |
| :------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Machine  | MacBookPro16,1, Intel i9-9980HK, 16 logical CPUs, no efficiency cores                                                                                                            |
| System   | macOS 26.6, tmux 3.7c, on mains power unless said                                                                                                                                |
| Measured | 2026-10-09, before and after the same day                                                                                                                                        |
| Setup    | 8 sessions, 16 windows: 7 coding-agent panes (2 printing as they work), 6 editors, 3 shells, 166 directories in the directory history, a real configuration with 179 theme files |
| Load     | none, 16 or 32 busy loops at normal priority, which made the machine throttle itself to 62-74% of its clock                                                                      |
| Presses  | 15 per picker per condition, two rounds; the figure is the middle press                                                                                                          |
| Clock    | from the key going down to the first row's characters reaching the terminal                                                                                                      |

Load 32: For each loaded run of benchmark, scripts/bench-capped.sh started 32 copies of yes > /dev/null, which does nothing but spin one CPU core at full speed, forever. It stops them when the run ends.
Load 16: is 16 copies.
No load: is none.

What that does on this my machine:

- Load 16: the MacBook has 16 logical CPUs, so 16 loops keep every one of them busy.
- Load 32: twice as many busy programs as CPUs. tmux, the picker and the daemon have to share each core with about two spinning loops, so each gets roughly a third of the CPU it would get on an idle machine.
- Heat: the loops also heated the machine. macOS cut its own CPU speed to 62–74%, which pmset -g therm recorded during the loaded runs. So the 32 column means contention plus a slower clock.

**My own apps were also running on top of the loops**, such as chrome, edge, music and 1-2 more.

| Picker                           | before, no load | after, no load | before, load 32 | after, load 32 | change at load 32 |
| -------------------------------- | --------------: | -------------: | --------------: | -------------: | ----------------: |
| project                          |          249 ms |          78 ms |          446 ms |         103 ms |              -77% |
| panes                            |           86 ms |          50 ms |          194 ms |          69 ms |              -64% |
| new-window                       |           91 ms |          95 ms |          152 ms |         134 ms |              -12% |
| keys                             |           45 ms |          47 ms |           71 ms |          67 ms |               -6% |
| the popup alone, tmux's own cost |           24 ms |          23 ms |           33 ms |          32 ms |                   |

| Picker     | picker on screen before, load 32 | picker on screen after, load 32 |
| ---------- | -------------------------------: | ------------------------------: |
| project    |                           446 ms |                           63 ms |
| new-window |                           152 ms |                           65 ms |

new-window got 4 ms slower with no load (91 to 95) because it now draws itself before its list;
the list comes from the directory source, which nothing here touched.

## Picker on screen

First row is what a timer likes, but what I notice first is the popup showing up with its border and title, and zsh-bench counts a shell's first prompt under 50 ms as no different from instant so I took 50 ms at load 32 as the budget for that.
Every picker started a tmux process only to ask for its theme colour, 15-20 ms under load, now it asks the daemon which already knows, and the panes picker listed every pane before it drew anything, now it draws its frame first like project and new-window do.

| Picker on screen, p50 | before, no load | after, no load | before, load 32 | after, load 32 |
| --------------------- | --------------: | -------------: | --------------: | -------------: |
| project               |        40-46 ms |       32-38 ms |        62-64 ms |          48 ms |
| new-window            |        39-47 ms |       31-37 ms |        63-64 ms |          48 ms |
| keys                  |        47-48 ms |          39 ms |           66 ms |       48-52 ms |
| panes                 |        50-51 ms |          38 ms |        68-69 ms |          49 ms |

A range is the two rounds disagreeing. Keys at 52 in one round is the one over the budget.
No key typed early got lost in any run.

I had written here that after a window or pane switch the bar keeps showing the old place for about a second in both modes and that was the bench's own bug: my config sets base-index 1, the bench switched to window 0 that didn't exist and never checked so what it timed was the clock redrawing a second later.
With the switch fixed, call mode was never that slow, tmux runs the `#()` again as soon as the pane's path in it changes.
Push was the slow one since it wrote one value per session on its one second tick, now the daemon also writes a value on each pane that's in a different directory, so tmux already has the right text when it redraws for the switch.

| Bar after a window switch, p50 / p95 | no load              | load 32              |
| ------------------------------------ | -------------------- | -------------------- |
| `call`                               | 40 / 53 ms           | 62 / 81 ms           |
| `push` before the per-pane value     | 133-193 / 168-303 ms | 203-227 / 257-276 ms |
| `push` now                           | 16 / 17 ms           | 22 / 27 ms           |

20 switches a round, two rounds alternated (call at load 32 is one round). So there's no trade-off left, push is the faster and the cheaper of the two, and the per-pane value made it cheaper still: 24.9-27.5 ms of CPU a second instead of 30.5-35.6 with no load, and 30.6-33.4 instead of 36.8-43.7 at load 32.

| Status bar, one attached client, idle                  |                 no load |           load 32 |
| ------------------------------------------------------ | ----------------------: | ----------------: |
| `call`: tmux starts a process every second             | 31.2 ms of CPU a second | 41-43 ms a second |
| `push`: the daemon writes the bar, tmux starts nothing |        24.9 ms a second |  31.5 ms a second |
| each further client in `call`                          |       +10.5 ms a second | +19.7 ms a second |
| each further client in `push`                          |                      +0 |                +0 |

| Low Power Mode on battery, after | no load | load 32 |
| -------------------------------- | ------: | ------: |
| project                          |   91 ms |  135 ms |
| panes                            |   61 ms |   94 ms |
| new-window                       |  118 ms |  176 ms |
| keys                             |   58 ms |   89 ms |

I have no idea how it will behave on Apple Silicon, my blind guess is it will be faster than mine.

It did run on a Raspberry Pi 5 though (4 cores, 8 GB, Debian 13, tmux 3.4) with 0, 4 and 8 busy loops, and the case fan wasn't connected so it sat at 90-96 C and throttled itself to 250-1500 MHz instead of 2.4 GHz the whole time, still the picker was on screen in 12-16 ms with no load, 24-33 ms at 4 loops and 51-109 ms at 8 in call mode, [BENCHMARKS.md](../BENCHMARKS.md#raspberry-pi-5-no-fan) has the rest with the heat caveat.
With the fan plugged in it stayed at 57-70 C and never throttled and the picker was on screen in 9-11 ms with no load, 15-18 ms at 4 loops and 21-26 ms at 8 so I've kept the first run as the worst case, a Pi with no fan inside a plastic case that doesn't let the heat out and [the fan run](../BENCHMARKS.md#raspberry-pi-5-fan-connected) sits next to it.

```sh
CONFIG=mine just bench-capped          # loads 0, 16 and 32, two rounds
BAR=push CONFIG=mine just bench-capped # the status bar in push mode
just bench-ab <revision>               # a revision against the working tree, alternated
```

- [BENCHMARKS.md](../BENCHMARKS.md): every table, the phases inside a press, the method
- [design-capped-cpu.md](../dev/design-capped-cpu.md): the design, the budgets, what was tried and dropped

**The bench runs in a throwaway home and never touched my own tmux or daemon**; It is part of script so anyone running should not worry about there setup/configurations or process.
