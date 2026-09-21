# Contributing

Patches welcome, including the ones that tell me I got something wrong.

## Before you push

Three commands, and CI runs the same three, so a pull request that's going to
fail does it here rather than in review:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

On a machine somebody is using, keep the job count down and the priority low,
because a full build otherwise saturates every core:

```sh
nice -n 15 cargo test -j 4
```

The toolchain is pinned in `rust-toolchain.toml`. With rustup installed you get
it automatically, and without it your formatting will differ from mine, which
the first command will tell you before CI does.

## What a change needs

- **Tests in the same file as the code**, in a `#[cfg(test)] mod tests` at the
  bottom. Pure functions are extracted from the I/O around them precisely so
  they can be tested without a subprocess or a hardware read, and the whole
  suite runs in well under a second. Keep it that way.
- **A documented public item.** `#![warn(missing_docs)]` is on and CI runs
  `cargo doc` with `-D warnings`, so an undocumented `pub` fails the build.
- **No change to the rendered bar unless that is the point of the change.**
  `./compare-output.sh old-binary new-binary` runs two builds against their own
  sockets and diffs what they draw. If your change moves a byte, say so in the
  commit message and say why, because somebody's bar is going to shift and
  they'll want to know it was deliberate.
- **A measurement, for anything performance-shaped.** `BENCHMARKS.md` explains
  how the numbers were taken and `bench-cpu.py` is the instrument. A claim that
  something's faster wants a number beside it.

## Things worth knowing before you start

- The invariants in `CLAUDE.md` are load-bearing and several of them record a
  bug that's already happened once. The lock discipline in particular: every
  `ServerState` method is synchronous so that a guard cannot be held across an
  `.await`, and the combined status side would deadlock itself if one were.
- Adding a segment and adding a command each have a checklist in `CLAUDE.md`.
- Icon codepoints live in `src/tmux/icons.rs` and tests refer to them by name,
  so changing one needs no test edits.
- `docs/comrades-port.md` is where the current work is planned, and
  `docs/port-checklist.md` says what is done.

## Reporting something broken

Include the output of `tmux-companion --version`, your tmux version, and your
platform. If a glyph looks wrong, say which font you're using, because the
default preset assumes a Nerd Fonts v3 patch and you'll get boxes without one.

## Licence

MIT, the same as the rest of the repository. By sending a patch you're agreeing
it can ship under that licence.
