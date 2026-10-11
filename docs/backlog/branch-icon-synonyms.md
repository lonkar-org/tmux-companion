# Branch-icon synonyms from a thesaurus

Deferred 2026-10-10. A branch prefix outside the curated table is looked up
as a Nerd Font md glyph name, with a six-entry synonym list for words that
aren't glyph names (`docs/dev/design-branch-icons.md`). The `thesaurus` crate
(WordNet, MIT) would reach far more words, but as a runtime dependency it
adds several MB to the binary and the daemon's memory, its synonyms are loose
("fix" gives jam, pickle, hole) and it hasn't had a release since 2022.

## To do

- [ ] `scripts/gen-branch-synonyms` (a Rust example or a small bin behind a
      dev feature): for each word in a list of common branch prefixes, the
      WordNet synonyms that are md glyph names, skipping glyphs the bar
      already draws.
- [ ] Output a small sorted TSV in `src/presets/`, beside
      `nf-md-names.tsv`, committed and reviewed by hand like any diff.
- [ ] The hand-written list wins over generated entries; the generated table
      is consulted after it and before the plain name match.
- [ ] Unit tests pin a handful of generated mappings; a `just` recipe
      regenerates the file.
