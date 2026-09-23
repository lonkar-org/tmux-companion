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

## Directory jumpers

Optional, every one of them. Nothing here has to be installed for the tool to
run, and nothing errors when it is missing.

The project picker and `new-window` list live sessions and then directories,
and the directories come from whichever jumper `[project] dirs_source` names.
Four are built in:

| Name | Needs | Read as |
| --- | --- | --- |
| `zoxide` | [zoxide](https://github.com/ajeetdsouza/zoxide), the default | `zoxide query -l` |
| `z` | rupa/z, zsh-z or z.lua | the `~/.z` file, `$_Z_DATA` honoured |
| `cdr` | zsh with `chpwd_recent_dirs` on | `~/.chpwd-recent-dirs`, `$ZDOTDIR` honoured |
| `ghq` | [ghq](https://github.com/x-motemen/ghq) | `ghq list -p` |

Anything else that prints one absolute path per line goes in
`[project] dirs_command` without this tool knowing its name — autojump, fasd,
jump, an `fd`, or a glob through `sh -c`. Which flag each of those wants is in
its own manual; this page does not repeat them, because they do not agree.

With none of them installed, and nothing configured, the picker lists live
tmux sessions and opens whatever path you type. That is the floor, and it needs
nothing but tmux.

[reference/configuration.md](configuration.md) has the table of what each one
reads and why `z` and `cdr` are read as files rather than run as commands.

## Contrast

Text on a theme's own block clears WCAG 2.1 AA, 4.5:1 for body text, and an
active pane border clears 3:1 against your terminal's real background.
`theme gen` reports anything that doesn't and `--apply` fixes it.
[how-to/themes.md](../how-to/themes.md) has the detail.

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
