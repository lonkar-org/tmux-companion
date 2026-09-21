# Requirements

Everything here was checked on 2026-09-22 against the versions named.

## Rust

| | |
| --- | --- |
| Minimum | 1.85, the first release with edition 2024 |
| Developed and tested against | 1.98.0, pinned in `rust-toolchain.toml` |

`Cargo.toml` carries `rust-version = "1.85"`, so cargo says which toolchain is
needed rather than reporting a type error inside the edition. The README used
to say 1.82, which can't build the crate at all.

## tmux

3.0 or newer for the status bar, which is all the binary needs today, and it's
worth saying that this is the version the segments were written against rather
than a floor anybody has gone looking for.

The pickers the comrades port adds will need **3.2**, because they run in
`display-popup -E`. That line moves here when the first one lands rather than
before.

Developed against tmux 3.7c.

## Fonts

The default glyph preset is Nerd Fonts **v3**, and its codepoints are in the
private use area, so a font without the patch draws boxes. Any v3 patched font
works; this repository is developed against FiraCode Nerd Font.

If you don't have a patched font, set a different preset rather than living
with the boxes:

```toml
[glyphs]
preset = "ascii"
```

## Platforms

| Platform | State |
| --- | --- |
| macOS | developed and used here, on aarch64 and x86_64 |
| Linux | compiles clean for `x86_64-unknown-linux-gnu`, tests included, and runs in CI |

Nothing in the crate is macOS-only: battery goes through the `battery` crate and
bandwidth through `sysinfo`, and both carry Linux backends. What hasn't been
checked is a Linux machine with no battery and no `$XDG_RUNTIME_DIR`, so if you
run it there, say what happened.

Windows isn't supported and isn't planned, since the whole thing is a unix
socket and a `SIGWINCH`.

## System libraries

None. `cargo build --release` needs a Rust toolchain and nothing else.
