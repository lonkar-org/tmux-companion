# The chunk clock's banner command, configurable

Deferred 2026-10-10. The chunk clock's banner is `osascript -e 'display
notification …'` on macOS and `notify-send` elsewhere (`banner_command` in
`src/chunk.rs`). On macOS a notification posted by `osascript` belongs to
Script Editor, so its "Show" button opens an empty Script Editor window, and
there is no way to attach anything to it.

## To do

- [ ] A `[chunk] banner_command` (or a shared notifier setting, if `notify.rs`
      and the earcons want the same) taking an argv with `{title}` and
      `{body}` placeholders; the built-in commands stay the default.
- [ ] The same setting used by every banner the tool posts, not only the
      chunk clock: check `notify.rs` for its own.
- [ ] Unit tests for the placeholder substitution and quoting; no shell in
      between, the argv is run as given.
- [ ] Manual paragraph, `docs/config.example.toml`, configuration reference,
      CHANGELOG line.
