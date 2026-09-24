# Changelog

Kept in the shape [keep a changelog](https://keepachangelog.com) suggests, one
entry per phase of the comrades port.

## Unreleased

### Fixed

- The end-to-end tests told the tool which config to read and which daemon
  socket to use, but never which tmux to talk to, so every tmux command the
  binary ran under test went to the default socket rather than to the server
  the test had started. `scripts/repro-ci.sh` runs the suite under the CI
  job's conditions and is what found it.

- A broken config reached the person as `Connection reset by peer (os error
  104)` instead of the name of the key that was wrong, on a machine loaded
  enough to lose a race. The daemon bound its socket before it parsed the
  config, so it was reachable for as long as the parse took: a client that
  connected inside that window was accepted and then dropped when the daemon
  gave up, and the client only consulted the recorded error when the *connect*
  had failed. The daemon parses before it binds now, so a refusal leaves no
  socket at all, and a connection that dies mid-request is explained by the
  daemon's own last words wherever it left any.

- `project save` and the save `close-project` does on the way out are all or
  nothing. Three things could each lose part of a layout while reporting
  success: a pane or window line tmux answered with that did not parse was
  dropped silently, so a smaller layout replaced a larger one; a tmux command
  that failed outright returned an empty string, which parsed as a session with
  no windows and was written over a good file, and because a saved layout wins
  over the config that empty file then shadowed the `[[layout]]` the project
  used to open with; and the write truncated the old file before writing the
  new one, so an interruption left half a layout. Now a failed tmux read is an
  error, a line that cannot be parsed is quoted back and nothing is written, a
  layout with no windows is refused, and the file is written under a temporary
  name and renamed over the old one. A layout file with no windows that is
  already on disk reads as no layout rather than as an empty one.

## 0.1.0 - 2026-09-23

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
- [BENCHMARKS.md](BENCHMARKS.md) and `just bench`, which measure from inside a
  real tmux against the zsh this replaced: a key is pressed, tmux runs the
  binding, and the clock stops when the first row reaches the terminal. The bar
  is 3.0% of a core against 34.4%, and the pickers cost half the CPU while
  opening no faster, which is the number the old socket-side measurements could
  not see. The port's own before and after numbers move to
  [BENCHMARKS-before-port.md](BENCHMARKS-before-port.md).

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
- `open` finds what is under the cursor. The copy-mode binding passes
  `#{copy_cursor_line}` and `#{copy_cursor_x}`, so `o` opens the path or URL
  the cursor is on without selecting it first, and a line naming two paths
  opens the one you are actually on rather than whichever came first.
- `[project] dirs_source`, so the project picker and `new-window` list
  directories from `z`, zsh's own `cdr` or `ghq` as well as zoxide, and
  `[project] dirs_command` for anything else that prints one absolute path per
  line. `z` and `cdr` are read as files rather than run as commands, because
  both are shell functions. `[project] visit_command` is the matching write.

### Changed

- `theme gen --shades` writes every colour in tmux's 6x6x6 cube whose text
  clears a contrast rung rather than one lighter and one darker sibling of each
  theme already on disk, each named after the colour it sits nearest to so
  `ember-04` and `pine-11` group in the picker. It answers "show me what there
  is" instead of "vary what I have", and it no longer depends on which files
  happen to be in the directory. The rung is the flag's value: `aa` for 216
  themes, `aaa` for 151, `a4` for 105, `a5` for 75 and `a6` for the original
  eighteen. On its own the flag means `aaa`, because the worst colour in the
  cube scores 4.60:1 and an AA filter therefore keeps all 216 and removes
  nothing. An unknown rung is an error rather than a silent fall back, since a
  typo that generates 145 files instead of 18 is a directory somebody cleans up
  by hand.

- `vim-bg` is `sh-jobs`, with a config-driven job table rather than one
  hardcoded editor. The old name works for one more release and says so.
- zoxide is documented as optional rather than assumed. It was already possible
  to run without it, and the picker already degraded to live sessions and a
  typed path, but nothing outside `config.example.toml` said so -- `[project]` is
  now covered in the manual, `docs/reference/requirements.md` and the README.
- `autosave` runs in the daemon instead of a detached shell loop, which removes
  its PID lock file, the stale-lock takeover and the liveness check between
  sleeps.
- Every request and response carries a build id, so a client talking to a
  daemon from an older build replaces it rather than quietly getting an older
  answer.
- The socket is created 0600 and its owner is checked before a client connects.

### Deprecated

- `zoom` is `zen`. The name described half of what it does: with other panes it
  zooms, and with none it takes the status bar, because a lone pane already
  fills the window and tmux's own `prefix z` does nothing there. Both halves
  mean "clear everything but what I am working on", which is one idea and now
  has one name. The old name works for one more release, and the binding is
  still `prefix z`.

- `[project] zoxide`. It is the old spelling of `dirs_source = "none"`, still
  works, still wins over everything else in the section, and goes away in the
  next release. `config check` names it, so nobody meets the removal first.

### Fixed

- `shell-init zsh` emitted a prompt mark that tmux then threw away, so
  `previous-prompt` and `next-prompt` did nothing on zsh -- the shell this was
  written on and tested with. zsh has `PROMPT_SP` and `PROMPT_CR` on by
  default: after `precmd` returns it prints a partial-line indicator and a
  carriage return and redraws the prompt line, taking the mark recorded against
  that line with it. The mark now goes in `PS1`, where the redraw cannot reach
  it. bash was never affected, because it has no such redraw.
  `scripts/check-prompt-marks.sh` drives a real tmux and fails when the cursor
  does not move, which is the only thing that catches this: the hook installs,
  defines its functions and prints every byte either way.
- `[run] history` defaulted to `zsh`, so on bash the command picker read a
  `~/.zsh_history` that was not there and came up empty with nothing said. It
  defaults to `auto` now and reads whichever shell `$SHELL` names.
- `[run] shell` defaulted to `zsh`, so on a machine without zsh -- most Linux
  boxes -- the pane slid out and the command never ran. Empty now, meaning
  `$SHELL`.
- `open` and `zoom` acted on the wrong pane. Both ran their tmux commands with
  no target, and tmux then resolves "current" as the most recently used session
  on the server, so with a second session touched more recently the editor
  opened in a window nobody was looking at and the zoom key moved a pane on
  another screen. `$TMUX_PANE` is no help here: tmux runs these from bindings
  through `run-shell`, where it holds the most recently active pane on the
  server and `run-shell -t` does not change it. Both commands now take
  `--pane`, and the shipped bindings pass `#{pane_id}`.
- `new-window` created its window in whichever session the server had used
  last, for the same reason. It targets its own session now.


- `TMUX_COMPANION_SOCK` that is empty or too long for a unix socket address now
  exits 2 instead of warning and connecting to the default socket, which meant
  a harness asking for an isolated server quietly got the live one.

- `Duration::from_secs_f64` panicked on a negative or non-finite `--ttl`, which
  arrives from a client.
- A misspelled argument used to read back as `None` and change behaviour
  silently. Arguments are typed structs on both ends of the wire now, and an
  unknown field is an error naming the field.
