# Three gaps in the capped-CPU bench

Deferred 2026-10-10, found in the re-baseline of the merged release build
(`CONFIG=mine`, loads 0, 16 and 32, call and push). They look like the
harness rather than the tool, and the frame numbers stay trustworthy.

## To do

- [ ] `switch sessions`: frame is `nan` in every round and type-ahead reads
      1.2-3.6 s. The sessions view commits on Enter, not on a pause, so the
      bench is probably waiting for a pause-commit, or for a frame marker the
      sessions view never draws. Give it the right marker and the right
      commit key.
- [ ] `new-window` "never drew its marker" in 7 of 10 rounds (frame and
      type-ahead still measured). Find which marker it waits for and why the
      merged build stopped drawing it.
- [ ] `seen` reads 19-28 ms, lower than `spawn`, for project, panes and
      switch sessions in most rounds after the first: the marker probably
      matches a frame from the previous press. Clear the pty or match on
      something unique to this press.
- [ ] Rerun `CONFIG=mine just bench-capped` after the fixes and replace the
      release section's numbers in `docs/BENCHMARKS.md` if they move.
