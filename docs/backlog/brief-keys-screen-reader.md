# The brief's keys in screen-reader mode

Deferred 2026-10-06. When `screen-reader-mode` lands, this file gets its row in
that branch's `docs/backlog/README.md`.

**What.** Two parts of `9a8eef0` (branch `brief-keys`) that didn't come to
main with the keys:

- `asked` and `render_spoken` in `src/brief.rs`: in screen-reader mode the
  brief prints as numbered lines with no icons or padding and asks a
  question, the same keys typed as an answer (`2`, `c 3`), printed again after
  an action that stays. `question` and `choose` already take a typed answer,
  and `act` still takes `paint: None` for this path.
- "Last messages": the brief repeats tmux-companion's last three messages,
  through `readbar::our_messages`.

**Why it waits.** Both call `accessibility::screen_reader` or `readbar`,
which only `screen-reader-mode` has. Landing them first would have brought
half that branch to main ahead of the rest.

**What brings it back.** `screen-reader-mode` landing on main. Restore them
from `git show 9a8eef0:src/brief.rs` with their tests
(`screen_reader_mode_asks_the_same_keys_as_a_sentence`,
`the_spoken_brief_is_numbered_lines_with_no_chrome_or_padding`,
`the_last_three_of_our_messages_come_in_the_order_they_were_said`), and put
the manual's paragraph on the typed answer back under `brief`.
