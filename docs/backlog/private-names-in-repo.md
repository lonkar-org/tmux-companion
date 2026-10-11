# Names of a private setup in the public repository

Deferred 2026-10-10. Commits made since 2026-10-09 keep private repository
names and paths out of the tree, but older ones did not: the name of the
dotfiles repository this tool grew out of appears in 23 files today (find
them with `rg -il '<name>' --glob '!target'`).

## To do

- [ ] Test fixtures and unit tests (`tests/fixtures/*.tsv`, `src/restore.rs`,
      `src/config.rs`): a neutral session name.
- [ ] The window segment's special logo for that path
      (`src/segments/window.rs`): make it a config entry or drop it.
- [ ] The example output in `skills/tmux-companion/SKILL.md` and
      `docs/dev/tmux-resurrect-port.md`: neutral session names.
- [ ] `scripts/bench-bar.py`, the `justfile` comment and `docs/BENCHMARKS.md`:
      the old bar's scripts are read from that repository's history. Keep the
      method reproducible (a path from an environment variable) without the
      name.
- [ ] Decide whether history needs rewriting or only the tree.
