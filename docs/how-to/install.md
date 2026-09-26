# Install

Four ways, and the first one's the one to take.

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

## Through Homebrew

```sh
brew tap lonkar-org/tap
brew trust lonkar-org/tap
brew install tmux-companion
```

The trust line is Homebrew 7 and newer: it won't load a formula out of a
third-party tap until you say that tap is one of yours, and the error it prints
otherwise names the command, so the worst a skipped line costs you is one
message.

The formula downloads the same archive the script would, checks it against the
sha256 the tap recorded when the release was published, and installs the
manual, so `man tmux-companion` works straight after. `brew upgrade
tmux-companion` takes you to the next release, and `brew uninstall` takes the
binary and the manual back out, leaving your config and saved layouts alone.

The formula lives in
[lonkar-org/homebrew-tap](https://github.com/lonkar-org/homebrew-tap) and the
release workflow rewrites it after each release, so a new version reaches brew
a couple of minutes after it reaches the Releases page. The `homebrew-` on the
front is Homebrew's convention rather than a second tap: `brew tap
lonkar-org/tap` is what you type, and it expands the name itself.

## Through tpm

If you already keep your plugins in [tpm](https://github.com/tmux-plugins/tpm):

```tmux
set -g @plugin 'lonkar-org/tmux-companion'
```

`prefix + I` clones it and installs the binary the same way the script does,
falling back to building from source when no release matches your machine,
which needs Rust 1.95. The build runs detached, so tmux doesn't sit there
looking hung while a compiler works.

The plugin binds no keys and sets no options. What goes on your status bar and
which key opens which picker is yours, and a plugin that decided for you would
be the thing this tool exists to avoid. Copy
[docs/tmux.conf.starter.example](../tmux.conf.starter.example) to start with;
it is the bar and eight bindings, and says where the rest are.

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

Rust 1.95 or newer, no system libraries. `./scripts/install.sh --build` does the same
thing and puts the binary wherever `--prefix` says.

## Then

```sh
tmux-companion doctor
```

That prints what a bug report needs, and on a fresh install it's also the
quickest way to find out whether the binary can see tmux.

Two lines in `tmux.conf` get you the bar; the second is the background the
segments draw against, and without it tmux's default green shows through:

```tmux
set -g status-style bg=colour233,fg=colour251
set -g status-right "#(tmux-companion status-right --branch-max-len 40 #{pane_current_path})"
```

[docs/tmux.conf.starter.example](../tmux.conf.starter.example) is those plus
the bindings.

## Starting it

From a shell that is not in tmux yet:

```sh
tmux-companion start
```

That opens the project picker, live sessions first and then every directory
your jumper knows — zoxide by default, and
[`[project] dirs_source`](../reference/configuration.md#where-the-directory-list-comes-from)
if you use `z`, `cdr`, `ghq`, something else or nothing at all — and attaches to
what you pick. It is worth an alias, because it is the thing you type instead
of `tmux`:

```sh
alias t='tmux-companion start'
alias tl='tmux-companion start --last'
```

`tmux` on its own gives you a session called `0` with one bare shell in it, and
everything here is a keystroke further on from that. If you would rather keep
typing `tmux`, one hook makes it land in the same place:

```tmux
set-hook -g client-attached 'run-shell "tmux-companion start --hook"'
```

That opens the picker only when the session is one tmux named itself, meaning
a name that is all digits, with one window, one pane and a shell in it. A session you
asked for by name, or one with anything already running, is left alone.

## Upgrading

Run the install script again, then `tmux-companion restart`. The script
compares what's on PATH against the latest release and does nothing when they
match. Through tpm, `prefix + U` updates the checkout and installs the new
binary; the restart is still yours to run, because the daemon is one long-lived
process and a new binary on disk changes nothing until the old one exits.
`tmux-companion doctor` says which build is running and which is on disk.

## Removing it

```sh
tmux-companion shutdown                  # the daemon does not die with the binary
rm "$(command -v tmux-companion)"
rm -rf ~/.local/state/tmux-companion    # saved layouts, snapshots, the usage log, daemon.log
rm -f ~/.config/tmux-companion/config.toml
rm -rf ~/.config/tmux/themes            # only if `theme init` wrote them
```

The socket under `/tmp` goes with the daemon. Nothing else is on disk.

See also [things tmux already does](things-tmux-already-does.md), for the
things people install a plugin for that tmux 3.x does on its own.
