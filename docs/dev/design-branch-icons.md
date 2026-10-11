# Branch icons

Status: draft
Problem: `docs/…`, `perf/…`, `parked/…` and `bench/…` all draw the plain
branch glyph. Only six prefix groups had an icon, and adding one meant copying
all six into the config, because the config list replaced the built-ins.

## What changes

1. A curated table of 25 prefix groups ships instead of 6.
2. A prefix the table doesn't know is looked up among the Nerd Font
   Material Design glyph names, so `cleanup/` finds `nf-md-broom`.
3. What the lookup picked is written to a file in the state directory, so the
   same prefix draws the same glyph after a restart or an upgrade.
4. `[[git.branch_types]]` adds and overrides instead of replacing.

## The curated table

Glyphs are md where a new one was needed: one family on one grid, in a plane
nothing else on the bar uses. None of them is a codepoint `src/tmux/icons.rs`
already draws for something else; a unit test holds that.

| Constant     | Glyph                   | Prefixes                                                         |
| ------------ | ----------------------- | ---------------------------------------------------------------- |
| `FEATURE`    | fa-lightbulb_o (as was) | `feat/` `feature/` `features/`                                   |
| `BUGFIX`     | fa-bug (as was)         | `fix/` `fixes/` `bugfix/` `bugfixes/`                            |
| `HOTFIX`     | fa-fire (as was)        | `hotfix/`                                                        |
| `CHORE`      | as was                  | `chore/` `chores/`                                               |
| `RELEASE`    | as was                  | `release/` `releases/`                                           |
| `TAG`        | oct-tag (as was)        | `tag/` `tags/`                                                   |
| `DOCS`       | md-file_document        | `docs/` `doc/` `documentation/`                                  |
| `PERF`       | md-speedometer          | `perf/` `performance/`                                           |
| `REFACTOR`   | md-recycle              | `refactor/` `refactoring/`                                       |
| `TEST`       | md-test_tube            | `test/` `tests/` `testing/`                                      |
| `CI`         | md-cog_sync             | `ci/`                                                            |
| `BUILD`      | md-hammer               | `build/` `builds/`                                               |
| `DEPS`       | md-package_variant      | `deps/` `dep/` `dependencies/` `bump/` `dependabot/` `renovate/` |
| `STYLE`      | md-format_paint         | `style/` `styles/`                                               |
| `REVERT`     | md-undo                 | `revert/` `reverts/`                                             |
| `WIP`        | md-progress_wrench      | `wip/`                                                           |
| `EXPERIMENT` | md-flask                | `experiment/` `experiments/` `exp/` `spike/` `spikes/` `poc/`    |
| `BENCH`      | md-chart_line           | `bench/` `benchmark/` `benchmarks/`                              |
| `PARKED`     | md-parking              | `parked/`                                                        |
| `SECURITY`   | md-shield_lock          | `security/` `sec/`                                               |
| `A11Y`       | md-human_handsup        | `a11y/` `accessibility/`                                         |
| `I18N`       | md-translate            | `i18n/` `l10n/` `translation/` `translations/`                   |
| `DESIGN`     | md-pencil_ruler         | `design/`                                                        |
| `RESEARCH`   | md-microscope           | `research/`                                                      |
| `BACKUP`     | md-archive              | `backup/` `backups/`                                             |
| `PREFIXED`   | md-label_outline        | any other `word/` the name lookup can't place                    |

The six that were there keep their glyphs and prefix lists, so a bar on one of
them is byte for byte what it was; a test pins that.

## The name lookup

The list is Nerd Fonts 3.4.0's `glyphnames.json`, md set only, 6880 names,
written by `scripts/nerdfont-names.sh` into `src/presets/nf-md-names.tsv`
(name, tab, codepoint, sorted in byte order) and embedded with
`include_str!`. The file is MIT (it sits at the root of the Nerd Fonts
repository, outside the folders with their own licences); its header carries
the notice. The names come from Material Design Icons, Apache 2.0. Names and
codepoints only, no glyph shapes. It costs the release binary 151,552 bytes
(13,290,328 with the list emptied, 13,441,880 with it), about 1.1%.

The rule, for a branch `<word>/<rest>` with a non-empty rest:

1. The word is 2 to 24 characters of letters, digits, `-` and `_`, with at
   least one letter. It's lowercased and `-` becomes `_`, the md spelling.
   Anything else isn't looked up and draws `PREFIXED`.
2. Candidates, tried in this order, first hit wins:
   1. the synonym list (below), when it has the word;
   2. the word;
   3. a singular: `-ies` to `-y`, then `-es` dropped, then `-s` dropped
      (not after `ss`);
   4. from `-ed`: `-ing`, then the stem, then the stem plus `e`
      (`parked` to `parking`, `fixed` to `fix`, `archived` to `archive`);
   5. from `-ing`: the stem, then the stem plus `e`;
   6. the word plus `s`.
3. A candidate shorter than two characters is skipped, and so is one whose
   codepoint is a glyph the bar already draws (the md alert is the health
   mark, the md robot is the agent count, and every glyph in the table
   above): a branch prefix shouldn't look like a warning or like `build/`.
4. No hit: `PREFIXED`.

Synonyms, where the word people use isn't a glyph name:

| Word              | md name       |
| ----------------- | ------------- |
| `user`, `users`   | account       |
| `infra`, `ops`    | server        |
| `deploy`          | rocket_launch |
| `demo`            | presentation  |
| `cleanup`         | broom         |
| `migration`       | database_sync |

What some common prefixes not in the table resolve to under this rule (unit
tests pin these): `cleanup/` broom, `deploy/` rocket_launch, `infra/` server,
`api/` api, `web/` web, `database/` database, `clouds/` cloud, `update/`
update, `locked/` lock. `hack/`, `alice/` and `db/` find nothing and draw the
catch-all. `alert/` and `robot/` would find the health mark and the agent
glyph, and `hammer/` the build glyph, so they're skipped and draw the
catch-all too.

### Prefix kept or cut

A table or name-matched glyph stands for the word, so the prefix is cut, as it
always was: `parked/old-idea` draws the parking glyph and `old-idea`. The
catch-all stands for nothing in particular, so it keeps the whole name:
`alice/topic` draws the label glyph and `alice/topic`. Cutting it would leave
a bar that no longer says whose branch it is.

## Storage

`$XDG_STATE_HOME/tmux-companion/branch-icons.toml`, the directory
`server::state_dir` gives everything else. One `[[pick]]` per prefix the
lookup placed: the prefix, the glyph name and the codepoint in hex, so the
file reads without a patched font and the codepoint is what's kept if a later
Nerd Fonts renames the glyph. Written through a temporary file and a rename,
mode 0600, only when a new pick is made. Read once when the daemon starts.

Only lookup picks are stored. The curated table and the config are already
fixed by the build and the file. A catch-all result isn't stored either, so a
later list that gains the word gives that prefix a real glyph.

A curated entry or a config entry wins over a stored pick: the table is tried
first. That is on purpose: a glyph somebody chose beats a guess.

Clearing: delete the file, or one `[[pick]]` in it, and restart the daemon.
Keeping one: `doctor` prints each pick with the `[[git.branch_types]]` entry
that makes it yours.

The no-daemon path (`gst --no-daemon`) reads the file and runs the same
lookup, but never writes.

## Config semantics

`[[git.branch_types]]` used to replace the built-in list. Now the effective
list is the config's entries first, then every built-in entry the config
didn't repeat, then the stored picks, then the lookup. First match wins, so:

- an entry wins for the prefixes it names, and every other prefix keeps its
  built-in or looked-up glyph;
- a config that copied the six built-ins (the old advice) behaves exactly as
  it did for those six, and gains the new ones;
- `icon = ""` claims its prefixes for the plain branch glyph with the name
  drawn whole, which is how you turn a built-in off and is what deleting it
  from the list used to do;
- `[git] branch_icon_matching = false` turns the lookup off: an unknown
  prefix draws the catch-all, no file is read or written.

The default value of `branch_types` is still the whole table, so
`config dump` prints it in the shape you edit, and a dump pasted back is
still the defaults.

The one behaviour that moves: somebody who deleted a built-in from their list
to get rid of it gets it back, and needs `icon = ""` now. The changelog says
so.

## The ascii preset

Each new constant has a 7-bit form in `src/presets/ascii.toml`, a short word
like the six had (`docs `, `perf `, `a11y `). A looked-up glyph is a
codepoint the preset can't name, so under `preset = "ascii"` the lookup is off
and stored picks aren't applied: an unknown prefix draws the catch-all, whose
ascii form is empty, with the whole name kept. `parked/x` reads `parked x`
(table) and `alice/x` reads `alice/x`.

## Accessibility

The glyph is decoration and the branch name stays text. For a table or
looked-up glyph the prefix word is cut, as it was for the six; the ascii
preset puts the word back, and the catch-all never cuts. No screen-reader
path on main prints the git segment today (`read-bar` is on the paused
screen-reader branch); when it lands it should say the prefix as a word
rather than drop it with the glyph, which is a limit to carry there.

## Cost

Nothing per render that wasn't there. The daemon builds the effective table
once at start and holds it behind an `Arc`, so a render clones a pointer
where it used to clone the config's `Vec`. The lookup runs once per distinct
prefix word for the daemon's life: a set of words already considered is one
hash probe per render, and a binary search over the embedded list (about 13
line comparisons) happens only on a word's first sighting. A write happens
only when that search finds something.

`status-right` for the six existing prefixes, and for an unprefixed branch,
is byte for byte what it was; tests pin both.
