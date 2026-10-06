# Accessibility checklist: where each item stands

Against [accessibility-research-and-checklist.md](accessibility-research-and-checklist.md),
as of 2026-10-02.

Where the work lives:

- **main**: committed in `fc610cb` (cursor, timing, contrast, colour,
  confirms) and `2f0fe8a` (the playground walkthrough). Not pushed yet.
- **branch**: `screen-reader-mode`, not committed yet (line-mode pickers,
  `jump` as a question, `read-bar`, the `prefix A` toggle).

What each status means:

| Status | Meaning |
| --- | --- |
| done | built, with tests |
| easy | an hour or two |
| build | real work: a day or more, or a design question first |
| trial | something you can check yourself on the Mac, or in a Linux VM |
| user | needs somebody who lives with the barrier, because a trial by a sighted person can't settle it |

## Blind and screen-reader users

| Item | Status | Notes |
| --- | --- | --- |
| A static bar that renders only on change | easy | A recipe: drop `net` from `[status.right]`, a long `status-interval`. Part of a screen-reader profile |
| `doctor` and `setup` stop recommending `status-interval 1` when screen-reader mode is on | easy | `doctor.rs:143` recommends `1` unconditionally |
| Segment text as words, through a `spoken` glyph preset | easy | The preset mechanism exists (`ascii`). `read-bar` on the branch already says the bar in sentences, on demand |
| Health mark says what failed, not `timer +2` | easy | `health.rs:132` draws `+{more}`. `read-bar` says the reasons in full |
| No powerline caps in the screen-reader preset | easy | Caps are glyph constants, so the `spoken` preset can blank them |
| Verify the hardware cursor; pin it to the selected row | done | On main. Your VoiceOver trial showed Terminal.app doesn't follow it, so line mode is the real answer |
| Preview off in screen-reader mode | done | Branch: line mode has no preview pane, `p 3` reads one on demand |
| Icons off, tones as words | done | Branch: icons dropped, and spoken as words when they differ between rows |
| Speak the focused row as it changes | build | Needs the speech sink. Line mode has no moving focus, so it matters less now |
| Empty states announced | easy | Printed and put on the message line already, but a popup with `-E` closes before a screen reader gets to it |
| A linear picker mode for every picker | done | Branch: every picker goes through `picker::run`. The cheat sheet isn't a picker and has `--print` |
| Same Esc and Enter everywhere; say where focus landed | easy | Keys are unified on main. "Switched to api" after a pick isn't said yet |
| Every message can also go to a speech sink | build | Differentiator 1. On main, `message_ms = 0` keeps messages up until a key |
| A `messages` command | easy | tmux's own `prefix ~` lists them, checked. A filtered `tmux-companion messages` would be a thin wrapper |
| `brief` and `doctor` in a plain mode | easy | `brief --print` exists; `doctor` and the popup forms need a screen-reader branch |
| Act from the brief, not only read it | done, drawn | Single keys in the drawn brief, on main. The same actions as a typed answer in screen-reader mode wait for that branch: `docs/backlog/brief-keys-screen-reader.md` |
| Last command output, previous and next | build | Differentiator 3. The OSC 133 marks are already there |
| Pocket and `run` as a window, not a split | build | Both use `split-window`; needs a setting and a second code path |
| Agent questions without box drawing | easy | `inbox.rs:159` already drops rule lines. Box characters and spinner frames inside the question still get through |
| Text transcript of the demo cast | easy | Can be generated from the `.cast` file's output events |
| An accessibility page in `docs/how-to/` | trial | The draft is held back until your trials say what works |
| Alt text on README images; badges called decorative | easy | The GIF has alt text and the badges have labels. The sentence about the badges is missing |

## Low vision

| Item | Status | Notes |
| --- | --- | --- |
| Every coloured state also has a word or shape | done | On main: battery %, rate arrows without colour, state icons in pickers. One known gap left, a branch with no upstream (easy) |
| Colourblind-safe tone set, tested under deuteranopia | trial | macOS Colour Filters, all three. Rework only if something reads wrong |
| A theme of the 16 ANSI colours only | build | Themes and the bar use 256-colour indices throughout |
| Cursor band at 7:1; selection visible with colour off | done | Already computed (`paint.rs`); reverse video under `NO_COLOR` |
| Magnifier reach: status line on top, alerts near the cursor | trial | Try `status-position top` under macOS Zoom. Alerts as a centred popup would be a build |
| Glyph legibility at high zoom | easy | Same `spoken` preset as above |
| Popups sized by percentage; clipped rows say so | easy | Percentages done. Clipped rows are cut with no ellipsis; line mode reads them in full |
| No motion; a `reduce_motion` key | done | On main the battery no longer animates. A key for future animation is easy whenever one exists |
| Test at 200% and 400% zoom, and macOS Zoom following focus | trial | |

## Motor and voice control

| Item | Status | Notes |
| --- | --- | --- |
| A modal "companion mode" key table | build | A tmux key table entered once with single keys inside. A tmux-side design, small code |
| Every binding reachable from that table; `keys` shows the table | build | Part of the above. `keys` already shows the table a binding lives in |
| No timing requirements | done | No `-r` bindings shipped, pickers have no timeouts, resurrect can wait (`confirm_wait`), `hold_ms` can be raised. Advice on `escape-time` is easy to add |
| Confirmations take one key | done | `confirm-before` y/n for kill, close and forget; `sessions idle` asks on a line |
| Wide click targets on the bar | done | The click range covers the whole segment, not one glyph |
| Pickers take numbers and arrows | done | Arrows on main, numbers in line mode on the branch |
| Structured terminal titles for Talon | easy | `set-titles` and `set-titles-string` in the example configs |
| A Talon command file | user | Easy to draft, but only a Talon user can say whether it's usable |
| Dictation-friendly names and spoken aliases | user | A design question to settle with voice users first |
| Test with Voice Control, Talon, Sticky Keys, one-handed | trial / user | Voice Control and Sticky Keys you can try; Talon needs a Talon user |

## ADHD and attention

The research says these are reasoning, not findings, so most end in "user".

| Item | Status | Notes |
| --- | --- | --- |
| A "where was I" card on attach to a project | build | `journal` and `brief` hold the data; the per-project card on attach is new |
| One-key capture of a thought into `note` | done | `note` on `prefix N` |
| A notification budget, batched at natural breaks | build / user | Needs the speech sink's polite queue, and users to say whether it helps |
| Time awareness, opt-in | build / user | Easy to compute from the journal; whether it helps or nags is a user question |
| A low-stimulation profile | easy | On main: the calm picker recipe, `[quiet] daily`, `[bar] colour = false`. Needs bundling as a profile |
| `brief` sorted by action, capped with "and N more" | easy | Check what `brief` does with a long list |
| Consistent words, a glossary | easy | Mostly careful already. "asked" and "waiting" are both used for an agent that stopped |
| Everything opt-in and reversible | done | Every setting added this round defaults off |

## The differentiators

| # | Item | Status |
| --- | --- | --- |
| 1 | Speak events through the user's screen reader | build, then user |
| 2 | A spoken "where am I" | partly: `read-bar` on the branch prints it. Pane count and last exit status still to add (easy) |
| 3 | Last command's output as a document | build |
| 4 | An event log, `events --follow` | build |
| 5 | Agent questions read cleanly | easy |
| 6 | A linear picker mode | done, on the branch |
| 7 | Window titles for voice control | easy, then user |
| 8 | Earcons | agents and health done (`[earcons]`, `earcon`); commands wait on tmux 3.8; then user |
| 9 | One-switch profiles (`setup --profile`) | build. Each setting exists; `setup` doesn't bundle them yet |

## Testing matrix

| Reader and terminal | Who |
| --- | --- |
| VoiceOver, Terminal.app | trial, in progress: round one done, line mode next |
| VoiceOver, iTerm2 | trial |
| VoiceOver, Ghostty | trial: unknown whether Ghostty exposes its text |
| TDSR | trial: install and run the same walkthrough |
| Orca, GNOME Terminal | trial in a Linux VM; a daily Orca user to confirm |
| Fenrir, Speakup, BRLTTY | user |
| NVDA over SSH from Windows | user |

## Who to involve

| Item | Status |
| --- | --- |
| Two or three paid blind developers on the playground | user, once line mode has passed your trial |
| Playground works with a screen reader | done: `just playground-a11y` |
| One Talon user | user |
| A pinned accessibility issue and an `a11y` label | easy, but it's public, so it's your call |
| Say what's tested, by reader and version | after the trials, in the accessibility page |
