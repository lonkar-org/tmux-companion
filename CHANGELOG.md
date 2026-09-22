# Changelog

Kept in the shape [keep a changelog](https://keepachangelog.com) suggests, one
entry per phase of the comrades port.

## Unreleased

### Added

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

- `Duration::from_secs_f64` panicked on a negative or non-finite `--ttl`, which
  arrives from a client.
- A misspelled argument used to read back as `None` and change behaviour
  silently. Arguments are typed structs on both ends of the wire now, and an
  unknown field is an error naming the field.
