# Changelog

Kept in the shape [keep a changelog](https://keepachangelog.com) suggests, one
entry per phase of the comrades port.

## Unreleased

### Added

- Released binaries for four targets: macOS on Apple silicon and Intel, Linux on
  ARM and on Intel or AMD. The Linux pair link statically against musl, so one
  binary runs on any distribution rather than tracking a glibc version.
- `scripts/install.sh`, which picks the archive for the machine it runs on,
  verifies it against the release's `checksums.txt`, and installs it. It refuses
  rather than warns when the checksum does not match, and falls back to building
  from source when no release fits.
- `tmux-companion.tmux`, so tpm can install it. It binds no keys and sets no
  options.
- `--version`, reporting the build stamp the daemon handshake compares rather
  than the crate version alone.
- `.github/workflows/release.yml`: a tag builds the four binaries, and
  publishing waits on the `release` environment so an admin approves first.
- [docs/how-to/install.md](docs/how-to/install.md), covering all three ways in
  and how to remove it again.

- Panes in a layout window. `[[layout.window.pane]]` with a command, a cwd and
  a focus flag, under a `layout` naming one of tmux's five presets or carrying
  a raw tmux layout string, so a window you arranged by hand can be pasted in
  rather than described in a new syntax.
- `project save`, `project forget` and `project show`, which capture the
  session you are in as that project's layout and say which file decided. A
  saved layout wins over `[[layout]]`. `close-project` captures on the way out,
  before anything is asked to quit.
- `shell-init` for zsh, bash and fish, printing the OSC 133 prompt marks that
  tmux's `next-prompt` and `previous-prompt` have been waiting for since 3.3.
- `[git.autofetch]`, fetching the repositories the bar has drawn so ahead and
  behind mean something. Off by default, since it is the only part of this that
  touches a network.
- `[autoreload]`, sourcing tmux's config when it changes, over the file the
  daemon already stats for `keys`.
- `[notify]`, saying when a long command finished in a pane you were not
  looking at, with tmux's own `display-message` as the notifier so nothing has
  to be installed.
- `[window_names]`, naming windows from the `[[sh_jobs.job]]` table. It never
  takes a name away from a window somebody pinned.

- `keys` and `cheatsheet`, replacing the fzf-driven pair. The rows are parsed
  from `tmux list-keys` and cached against the config's mtime, so six
  `list-keys` calls happen once per config change rather than once per
  keypress. Warm, `keys --print` measures 8.3 ms against 18.8 ms for the
  equivalent `fzf --filter`.
- `project`, replacing four scripts: live sessions newest-attached-first, then
  the directories zoxide knows, coloured through the project map.
- `run`, which picks from shell history into a pane that slides out beside you
  and offers Close, View or Restart when the command exits, and whose history
  parsing is byte-identical to `fc -ln` over all 1127 commands after three
  attempts that each got zsh's format wrong in a different way.
- `theme pick`, `theme apply` and `theme gen`, which drops the python3
  dependency. Every one of the 76 themes resolves to the same colour the Python
  computed.
- `open`, `close-project`, `toggle`, `autosave`, `clipboard`, `zoom` and the
  two `probe` subcommands.
- `doctor`, which prints what an issue reporter would otherwise be asked for one
  question at a time.
- A TOML config file: glyph presets for readers without a Nerd Font, an ordered
  `[git] parts` list, a configurable `[status.right]`, `[[layout]]` for what a
  project session starts with, and an off switch on the usage log.

### Changed

- `vim-bg` is `sh-jobs`, with a config-driven job table rather than one
  hardcoded editor. The old name works for one more release and says so.
- `autosave` runs in the daemon instead of a detached shell loop, which removes
  its PID lock file, the stale-lock takeover and the liveness check between
  sleeps.
- Every request and response carries a build id, so a client talking to a
  daemon from an older build replaces it rather than quietly getting an older
  answer.
- The socket is created 0600 and its owner is checked before a client connects.

### Fixed

- `TMUX_COMPANION_SOCK` that is empty or too long for a unix socket address now
  exits 2 instead of warning and connecting to the default socket, which meant
  a harness asking for an isolated server quietly got the live one.

- `Duration::from_secs_f64` panicked on a negative or non-finite `--ttl`, which
  arrives from a client.
- A misspelled argument used to read back as `None` and change behaviour
  silently. Arguments are typed structs on both ends of the wire now, and an
  unknown field is an error naming the field.
