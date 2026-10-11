# Benchmarks on a Raspberry Pi 5

Deferred 2026-10-09, on branch `bench/raspberry-pi`. The capped-CPU and
perceived-latency numbers so far come from one Intel MacBook; a Pi 5 is a
second machine with a CPU that is capped by design.

## The machine

- Raspberry Pi 5, Raspberry Pi OS Lite 64-bit (aarch64, Debian-based), no
  desktop.
- On the same network as the machine driving it.

## To do

- [ ] `scripts/bench-capped.sh` and `scripts/bench-capped.py` portable to
      Linux: no `pmset`, `sysctl hw.ncpu`, `sysctl vm.loadavg` or
      `machdep.cpu.brand_string`; `nproc`, `/proc/loadavg`, `/proc/cpuinfo`
      instead.
- [ ] Loads default to one and two times the core count (0, 4, 8 on a Pi 5)
      rather than 0, 16, 32.
- [ ] Throttling recorded around each run from `vcgencmd get_throttled`,
      `vcgencmd measure_clock arm` and `/sys/class/thermal/thermal_zone0/temp`,
      where macOS uses `pmset -g therm`.
- [ ] Prerequisites on the Pi: `cc`, `python3`, `zoxide`, `git`, Rust, and the
      pinned tmux from `scripts/tmux-build.sh 3.4`.
- [ ] `scripts/bench-remote.sh HOST` with a `just` recipe: copy or pull the
      branch on the Pi, build the release binary there, run `bench-capped`,
      fetch `target/bench/<run>` back. SSH errors surface, never swallowed.
- [ ] A baseline on the Pi: pickers (seen, frame, type-ahead, input20), the
      bar after a window switch in call and auto, idle cost.
- [ ] The Pi's numbers next to the MacBook's in `docs/BENCHMARKS.md` and the
      performance page.
