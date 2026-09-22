# Install

Three ways, and the first one's the one to take.

## Download the binary

```sh
curl -fsSL https://raw.githubusercontent.com/lonkar-org/tmux-companion/main/scripts/install.sh | bash
```

It works out which archive this machine wants, downloads it, checks it against
the `checksums.txt` published with the release, and installs it. A mismatch
stops the script rather than warning and carrying on, since the case that file
exists for is the download not being what the release published.

Piping a script off the internet into a shell is a thing you're allowed to
dislike. Read it first, or download and run it:

```sh
curl -fsSL -o install.sh https://raw.githubusercontent.com/lonkar-org/tmux-companion/main/scripts/install.sh
less install.sh
bash install.sh
```

The binary lands in `/usr/local/bin` when that's writable and `~/.local/bin`
otherwise. `--prefix` picks somewhere else, `--version v0.1.0` pins a release,
and `--force` reinstalls over what is already there.

There are four archives:

| | |
| --- | --- |
| `x86_64-unknown-linux-musl` | any Linux on Intel or AMD |
| `aarch64-unknown-linux-musl` | any Linux on ARM |
| `x86_64-apple-darwin` | macOS on Intel |
| `aarch64-apple-darwin` | macOS on Apple silicon |

The Linux builds are static against musl, so one binary runs on any
distribution rather than tracking whichever glibc the build machine had.

## Through tpm

If you already keep your plugins in [tpm](https://github.com/tmux-plugins/tpm):

```tmux
set -g @plugin 'lonkar-org/tmux-companion'
```

`prefix + I` clones it and installs the binary the same way the script does,
falling back to building from source when no release matches your machine,
which needs Rust 1.85. The build runs detached, so tmux doesn't sit there
looking hung while a compiler works.

The plugin binds no keys and sets no options. What goes on your status bar and
which key opens which picker is yours, and a plugin that decided for you would
be the thing this tool exists to avoid. Copy what you want from
[docs/tmux.conf.example](../tmux.conf.example).

Two options, both set before the plugin line:

```tmux
set -g @tmux-companion-install-prefix "$HOME/.local"
set -g @tmux-companion-auto-install "off"   # tell me, do not install
```

## Build it yourself

```sh
git clone https://github.com/lonkar-org/tmux-companion
cd tmux-companion
cargo build --release
sudo install -m 755 target/release/tmux-companion /usr/local/bin/
```

Rust 1.85 or newer, no system libraries. `./scripts/install.sh --build` does the same
thing and puts the binary wherever `--prefix` says.

## Then

```sh
tmux-companion doctor
```

That prints what a bug report needs, and on a fresh install it's also the
quickest way to find out whether the binary can see tmux.

One line in `tmux.conf` gets you the bar:

```tmux
set -g status-right "#(tmux-companion status-right --branch-max-len 40 #{pane_current_path})"
```

## Upgrading

Run the install script again. It compares what's on PATH against the latest
release and does nothing when they match. Through tpm, `prefix + U` updates the
checkout and the next tmux start picks up the new binary.

## Removing it

```sh
rm "$(command -v tmux-companion)"
rm -rf ~/.local/state/tmux-companion    # saved layouts and the usage log
rm -f ~/.config/tmux-companion/config.toml
```

The daemon dies with the socket. There's nothing else on disk.
