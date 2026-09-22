# Requirements

Everything here was checked on 2026-09-22 against the versions named.

## Rust

Only if you're building it yourself. The released binaries need no toolchain,
and [how-to/install.md](../how-to/install.md) has the download.

| | |
| --- | --- |
| Minimum | 1.85, the first release with edition 2024 |
| Developed and tested against | 1.98.0, pinned in `rust-toolchain.toml` |

`Cargo.toml` carries `rust-version = "1.85"`, so cargo says which toolchain it wants
rather than reporting a type error from inside the edition.

## tmux

3.0 or newer for the status bar, and **3.2** for the pickers, which run in
`display-popup -E`. Both are the versions the features were written against
rather than floors anybody has gone looking for.

`shell-init`'s prompt marks only do anything under tmux 3.3, which is when
`next-prompt` and `previous-prompt` arrived.

Developed against tmux 3.7c.

## Fonts

The default glyph preset is Nerd Fonts **v3**. Its codepoints live in the private
use area, so a font without the patch draws boxes where the icons should be.

Start at [nerdfonts.com](https://www.nerdfonts.com/font-downloads), or
[ryanoasis/nerd-fonts](https://github.com/ryanoasis/nerd-fonts) if you'd rather
patch your own. Any v3 patched font works.

What the recordings and the screenshots were made with is
[lonkar-org/firacode-nfc-tweaked](https://github.com/lonkar-org/firacode-nfc-tweaked),
which is Fira Code patched with Nerd Fonts and merged with Noto Sans
Devanagari. Reach for it if you want the bar looking exactly like the one in the
recordings, or if you write Devanagari in your terminal. For everything
else a stock Nerd Font is the same experience.

If you haven't got a patched font, set a preset rather than living with the
boxes:

```toml
[glyphs]
preset = "ascii"
```

## Platforms

Four binaries are published per release, one per target:

| Target | For | State |
| --- | --- | --- |
| `aarch64-apple-darwin` | macOS, Apple silicon | developed and used here |
| `x86_64-apple-darwin` | macOS, Intel | developed and used here |
| `x86_64-unknown-linux-musl` | Linux, Intel and AMD | compiles clean, tests run in CI |
| `aarch64-unknown-linux-musl` | Linux, ARM | compiles clean, tests run in CI |

The Linux builds link statically against musl, so one binary runs on any
distribution rather than tracking whichever glibc the build machine had. The
crate has no C dependencies, which is what makes that available.

None of it's macOS-only: battery goes through the `battery` crate and
bandwidth through `sysinfo`, and both carry Linux backends. What hasn't been
checked is a Linux box with no battery and no `$XDG_RUNTIME_DIR`, so if you run
it there, say what happened.

Windows isn't supported and isn't planned, since the whole thing is a unix
socket and a `SIGWINCH`.

## System libraries

None, either way. A released binary needs nothing installed, and
`cargo build --release` needs a Rust toolchain and nothing else.

`curl` and `tar` are needed to run `scripts/install.sh`, and `sha256sum` or
`shasum` to verify what it downloaded. The script won't install anything it couldn't check.
