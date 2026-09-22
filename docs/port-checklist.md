# Port checklist

Status for every row in `comrades-port.md`. One line per item, updated in the
commit that does the work. `blocked` carries the reason on the same line.

## Phase 0

| # | Item | Status |
| --- | --- | --- |
| 0 | transfer the repository to `lonkar-org` | blocked — needs Yogesh's GitHub account |
| 1 | `cargo fmt`, the two clippy lints, `rust-toolchain.toml` | done |
| 2 | GitHub Actions: `fmt --check`, `clippy -D warnings`, `test` | done — unverified until somebody pushes; README badge waits for the transfer |
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
| 4 | `autosave`, `toggle` | done — autosave is a daemon task, toggle cycles the layout |
| 5 | `theme`, `run` | todo |
| 6 | `open`, `close-project`, the probes, the tmux.conf logic | todo |

Each row also carries its documentation row from the table in
`comrades-port.md`, its config row, and its before and after numbers in
`BENCHMARKS.md` where it replaces a script.
