> Planning record from the port, kept for history; the current behaviour is in the reference docs.

# Port checklist

Status for every row in `comrades-port.md`. One line per item, updated in the
commit that does the work. `blocked` carries the reason on the same line.

## Phase 0

| # | Item | Status |
| --- | --- | --- |
| 0 | transfer the repository to `lonkar-org` | done — 2026-09-22, badge added, `origin` and the crates.io `repository` field point at the new path |
| 1 | `cargo fmt`, the two clippy lints, `rust-toolchain.toml` | done |
| 2 | GitHub Actions: `fmt --check`, `clippy -D warnings`, `test` | done — badge in the README; unverified until somebody pushes |
| 3 | `src/lib.rs`, `main.rs` down to argument parsing, `tests/` | done — main.rs 287 lines to 32 |
| 4 | one typed args struct per command | done — output byte-identical, checked with compare-output.sh |
| 5 | integration test over the real socket, plus the singleton | done — 9 tests in tests/socket_round_trip.rs |
| 6 | `//!` and `///` everywhere, `#![warn(missing_docs)]` | done — 197 items documented, rustdoc job in CI |
| 7 | `LICENSE`, `CONTRIBUTING.md`, "Adding a command", test-count sentence | done — plus crates.io metadata, docs/ skeleton, requirements.md |
| 8 | the config loader and its tables | done (loader, errors, `config` subcommand, `[general]`, `[dirs]`, `[git]`, `[network]`, `[battery]`, `[glyphs]` with nerd-font-v3 and ascii, `[git] parts`, `[status.right]`); `[[layout]]` waits for `project` in phase 3 |
| 9 | `vim-bg` becomes `sh-jobs` with a job table | done — `vim-bg` hidden alias for one release |
| 10 | wire version, socket 0600, `doctor` | done — build stamp from build.rs, owner check, `__shutdown` |

## The port, in order

| # | Item | Status |
| --- | --- | --- |
| 1 | `theme gen` | done — report matches the python on all 76 themes |
| 2 | `keys`, `cheatsheet` | done — both render what the zsh did, bar the two bindings tmux stopped reporting |
| 3 | `project` | done — 156 rows live, sessions and zoxide merged, `[[layout]]` drives the windows |
| 4 | `autosave`, `toggle` | done — autosave is a daemon task, toggle cycles the session's windows by index |
| 5 | `theme`, `run` | done — run's history is byte-identical to `fc -ln` over 1127 commands |
| 6 | `open`, `close-project`, the probes, the tmux.conf logic | done — plus `clipboard` and `zoom`, and docs/tmux.conf.full.example |
| 7 | `panes`, and the `agents` segment | done — one `list-panes -a` shared by the picker and the bar, `[agents]` is the one list of what an agent is, and the restore headline reads it too |

Each row also carries its documentation row from the table in
`comrades-port.md`, its config row, and its before and after numbers in
`BENCHMARKS.md` where it replaces a script.

## Taking on the session store

The design is `docs/dev/tmux-resurrect-port.md`. One row per step.

| # | Item | Status |
| --- | --- | --- |
| 0 | `command_for` and the `default-command` wrapper | done — every pane on a macOS server was graded `Exact` while holding a command that opens a shell inside a shell |
| 1 | the snapshot file and its generations | done — atomic write, a pointer that moves last, `prune` pure over stamps and times |
| 2 | `[sessions]` config, with a floor of 10 under the interval | done — three modes, and the cost of a short one written down in the example config |
| 3 | whole-server capture | done — three listings in, one snapshot out, tested against this laptop's own output |
| 4 | `sessions save`, `list`, `show` | done — pane history is a directory per generation, no new dependency |
| 5 | the restore table | done — matches the whole command, default deny, ten shipped rows |
| 6 | restore, non-interactive | done — refuses a live server, `--merge` adds what is missing, `--dry-run` prints the real command list |
| 7 | waiting for the prompt mark before sending keys | done — `capture-pane -F` marks a prompt line, 2s ceiling for a shell that never will |
| 8 | the summary screen | done — opens on what the restore does not know, never on a pane count |
| 9 | `shutdown` and `restart`, for the daemon and for the server | done — both refuse from inside tmux, and `sessions restart` bounces the daemon by default so config.toml is reread |
| 10 | the autosave timer and the crash marker | done — the shared poll was dropped: measuring it after `aggressive` was cut left it saving a rounding error |
| 11 | importing the old format | done — two passes, because the file writes every pane before every window |
| 12 | `project close`, and the cutover | done — `close-project` hidden for one release, `[autosave]` deprecated and off by default |

## What is left

The cutover, and it is not code.

The transfer happened on 2026-09-22, so the README badge is in, `origin` and
the crates.io `repository` field point at `lonkar-org/tmux-companion`, and
GitHub redirects the old path. That redirect dies the moment somebody takes
the name back, so `yogeshlonkar/tmux-companion` must never be recreated.

Every command here has been run against the live machine and none of them is
bound to a key yet. `docs/tmux.conf.full.example`
is what a full set of bindings looks like; the port plan's advice is to move
one binding at a time, leave the zsh one under a different key for a week, and
keep `comrades` in the config until its last script is gone.

## What the port did not bring across

Said plainly rather than left to be discovered.

- The elaborate mock status bar `preview-tmux-theme.zsh` drew. The theme picker
  previews a theme by listing its settings with a swatch against each, which is
  enough to choose by and is not the same thing.
- `probe keys` reports what crossterm decided a keypress was rather than the
  raw bytes, because by the time this code runs the parse has happened. What it
  prints is what any program acting on the key will see.
- `zoxide-window.zsh` was recorded here as folded into `project`, "the same
  list with a different verb on the end". That was wrong: `project` makes and
  switches sessions and never grew the window verb, so prefix+c had nothing to
  bind to and the feature was dropped rather than ported. It came back as
  `new-window`, which is the same list, the same resolution order for a typed
  path, and the pane's own directory prefilled so the key and enter mean what
  tmux's own prefix+c meant.
