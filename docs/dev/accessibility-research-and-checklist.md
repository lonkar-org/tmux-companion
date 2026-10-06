# tmux-companion accessibility research and checklist

Oct 2, 2026 · Yogesh Lonkar

## Summary

The biggest opening for tmux-companion is to be the speech and event layer that tmux itself lacks. The daemon already knows every event a blind, low-vision or distracted user needs: a long command finished out of sight, an agent asked a question, a health failure, an idle session. Today all of that is delivered visually, by repainting the bar or flashing the message line. Route the same events to the screen reader's own voice, to a linear log, and to plain-language summaries, and the tool does something no tmux plugin does. I searched for one and found none.

As of 0.7.0 (2026-09-30) the base is strong. The work so far includes an `ascii` glyph preset, `NO_COLOR` that keeps bold and reverse video, every theme held to WCAG contrast, and `--print` TSV on most pickers. `notify` is limited to what finished out of sight, and `brief` and `inbox` put "what needs me" on one screen.

The same release added risk. Picker rows now carry Nerd Font icons and colour-coded tones, and the recommended `status-interval 1` repaints the bar every second, the net segment included. Both are exactly what screen readers struggle with. The checklists below separate what to verify in your VoiceOver pass from what to build.

This audit is based on the public `main` branch at 0.7.0 plus two commits after it. The unpushed work on your machine isn't reflected here.

## What the research says

The consistent finding is that a linear stream of text is accessible and a redrawn grid is not. tmux-companion's pickers and bar are grids.

- **Stream vs grid.** A blind developer's 2026 essay draws the line between the CLI and the TUI. The CLI is an append-only stream that screen readers like Speakup handle well. The TUI treats the terminal as a 2D canvas and redraws it. Their gemini-cli case study shows a spinner and timer moving the hardware cursor every second. The user hears fragments like "Responding… Time elapsed 1s" interleaved with history, and long histories crashed NVDA. ([xogium.me](https://xogium.me/the-text-mode-lie-why-modern-tuis-are-a-nightmare-for-accessibility))
- **What works in old TUIs.** The same essay names three patterns that still work. `nano` and `vim` let you suppress cursor-position noise. `menuconfig` keeps focus in one column with the cursor pinned to the list. `irssi` appends new lines through VT100 scrolling regions instead of rewriting the screen.
- **The Google study (CHI 2021).** 12 developers using screen readers found navigating terminal output hard. Commands with no progress output left them with no feedback at all. Tables forced them to memorise columns, and they asked for CSV export instead. A common workaround was copying output into an editor to read it. ([ACM](https://dl.acm.org/doi/fullHtml/10.1145/3411764.3445544), [dev.to summary](https://dev.to/baspin94/two-ways-to-make-your-command-line-interfaces-more-accessible-541k))
- **GitHub CLI (2025).** GitHub's preview targets three groups: screen reader users, users needing high contrast, and users needing customisable colour. It moved prompts to Charm's `huh` accessible mode and replaced the braille-dot spinner with static text like "Working…". It also kept colours within the 16 ANSI colours, so users can remap them in their terminal. It's enabled through `gh a11y`. ([GitHub blog](https://github.blog/engineering/user-experience/building-a-more-accessible-github-cli/))
- **VoiceOver on macOS.** Power users report VoiceOver often fails to follow new terminal output, reading just "new line" or the last few words. Some prefer TDSR for automatic reading and fall back to VoiceOver inside interactive tools. ([AppleVis](https://www.applevis.com/forum/macos-mac-apps/tips-power-usersanyone-does-more-read-emailslight-web-browsing-be-more)) A recent iTerm2 issue describes VoiceOver's review cursor jumping to the top on new output; Terminal.app behaves better. ([iTerm2 #12920](https://gitlab.com/gnachman/iterm2/-/issues/12920))
- **Screen readers can be spoken to.** Several readers accept text to speak from outside the terminal. VoiceOver takes it over AppleScript once "Allow VoiceOver to be controlled with AppleScript" is ticked ([AppleVis](https://www.applevis.com/guides/making-voiceover-announce-time-date-your-mac)). Fenrir, a Linux console reader, takes `command say <text>` and `command interrupt` over a unix socket ([fenrir man page](https://manpages.ubuntu.com/manpages/stonking/man1/fenrir.1.html)). This is the hook that makes the differentiators below possible.
- **Voice control.** Talon is the common voice-control stack for programmers with RSI. Its community command set switches context by application and window title, and has terminal tags. ([talonhub/community](https://github.com/talonhub/community))
- **ADHD.** I found no research specific to terminals. The ADHD recommendations below are design reasoning, not evidence, and are best checked with real users.

## Audit of 0.7.0

Most features help someone. The pickers and the bar are where it hurts screen reader users today.

| Feature                               | Helps                                                                             | Hurts or unverified                                                                                                                                                                                                |
| ------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Status bar (`status-right`)           | Low vision: AAA themes, `ascii` preset                                            | Screen reader: repaints every second at the recommended `status-interval 1`, and the net segment changes each tick. Powerline caps and the `+N` health token read as noise                                         |
| Pickers (ratatui, `display-popup`)    | Motor: fuzzy matching saves keystrokes. Everyone: `--print` TSV                   | Screen reader: a redrawn grid with a preview pane beside the list, so a line read mixes row and preview. 0.7.0 added icons and colour-only tones (amber, green, red). Where the hardware cursor sits is unverified |
| `notify`                              | ADHD and blind: finished-out-of-sight only, threshold and ignore list             | Default output is `display-message`, which vanishes after `display-time` and isn't reliably spoken                                                                                                                 |
| `brief`, `inbox`, `health`            | ADHD: one "what needs me" screen. Blind: `--print` is linear                      | Popup layout, icons in the TTY form                                                                                                                                                                                |
| `agent` hooks, inbox capture          | Blind: the agent's question is captured from a pane you're not in                 | Captured lines include box drawing and spinner frames from agent TUIs                                                                                                                                              |
| `search`, OSC 133 `shell-init`        | Blind: scrollback search across panes. The prompt marks enable per-command output | No command yet to fetch the last command's output as clean text                                                                                                                                                    |
| `zen`, `pocket`, `run`                | ADHD: single focus. Blind: `zen` removes split-pane mixing                        | The pocket opens a split, so its lines interleave with the pane beside it                                                                                                                                          |
| `cheatsheet`, `keys`, `which-key` doc | Motor and ADHD: learn keys gradually; any binding can be run from search          | Four-box layout reads poorly linearly; `--print` is fine                                                                                                                                                           |
| `setup`, `doctor`                     | Everyone: clear state, nothing turns on silently                                  | `doctor` recommends `status-interval 1`, which is wrong for screen reader users                                                                                                                                    |
| README, playground                    |                                                                                   | The demo is a GIF and an asciinema cast with no text transcript                                                                                                                                                    |

## Things no one else is doing

These build on what the daemon already knows. I found no tmux tool, and few CLIs of any kind, that do them.

1. **Speak events through the user's own screen reader.** Add a `[speech]` sink beside `notify.command`. It sends the event text to VoiceOver over AppleScript, to Fenrir's socket (`command say`), or to Speech Dispatcher for Orca. It falls back to `say` or `spd-say` when no screen reader is running.
   - Copy the web's `aria-live` semantics: _polite_ events queue behind current speech (command finished, idle session), _assertive_ ones interrupt (an agent asked, a build failed).
   - Coalesce bursts into one sentence, for example "3 commands finished: cargo test in api, …".
   - Use the screen reader's voice, rate and verbosity rather than a second voice talking over it.
2. **A spoken "where am I".** One key gives the facts a sighted user glances at the bar for, in one short sentence: session, window name, pane 2 of 3, the program in front, directory, branch with ahead and behind, and the last command's exit status and duration. Speak it, or print it as one line. This replaces the bar for people who can't glance.
3. **The last command's output as a document.** The OSC 133 marks `shell-init` already writes let you cut out exactly one command's output. Offer it in `less`, on the clipboard, or as a temp file the user's editor opens. That turns the Google study's copy-into-an-editor workaround into one keystroke, and "previous" and "next" commands give command-by-command navigation.
4. **An event log instead of a bar.** `tmux-companion events --follow` prints one dated plain-text line per change: branch switched, battery low, network down, agent asked, command finished. A blind user keeps it in its own window and reviews it like an irssi channel. The bar becomes optional, not the only channel.
5. **Agent questions, read cleanly.** Blind developers increasingly drive coding agents whose TUIs are among the worst offenders, as the gemini-cli case study shows. The inbox already captures the question from a pane you're not looking at. Strip box drawing and spinner frames, then speak or print just the question and its numbered options.
6. **A linear picker mode.** A `huh`-style accessible mode: a numbered list printed once, a filter prompt that reprints matches only when asked, and a typed number to choose. Rows read as sentences ("api, live session, idle 5 days"), not columns, with no preview pane. `--print` already proves the data path.
7. **Window titles that voice control can use.** Set the terminal title to a stable, structured string, for example "tmux: api / editor / nvim". Talon contexts and VoiceOver window announcements can then key off it. Ship a Talon command file that maps phrases straight to subcommands, so "project api" runs `tmux-companion project api`.
8. **Earcons.** Optional distinct short sounds for done, asked and failed. They help blind users as a pre-attentive cue, and ADHD users who won't read a status bar.
9. **A one-switch profile.** `tmux-companion setup --profile screen-reader` (and `low-vision`, `motor`, `focus`) applies the whole bundle as one fenced block. It explains each line, as `setup` already does, and reverts cleanly.

## Checklist: blind and screen reader users

Each item is a test or a change. Tick off whatever your VoiceOver work has already covered.

**The bar**

- [ ] A static bar mode that renders only when a value changes. No per-second segments: net off or coarse, no clock seconds.
- [ ] `doctor` and `setup` recommend `status-interval 0` or a long interval when the screen-reader profile is on, not `1`.
- [ ] Segment text as words ("branch main, 2 ahead"), not glyphs or arrows, through a new `spoken` glyph preset beside `ascii`.
- [ ] The health mark says what failed ("health: sessions timer failed"), not `timer +2`.
- [ ] No powerline caps or separators in the screen-reader preset; a plain separator like " | " reads as a pause.

**Pickers**

- [ ] Verify where the hardware cursor sits while a picker is open. Pin it to the selected row, the way `menuconfig` does, or to the query line. Never let it jump to the preview.
- [ ] Preview off by default in the screen-reader profile (`[picker] preview = "none"`).
- [ ] Icons off and tones carried in words ("asked 3m", not amber).
- [ ] When the focused row changes, speak the row (polite) instead of relying on the screen reader to find the redraw.
- [ ] Empty states are announced, not only written to the message line.
- [ ] A linear picker mode (differentiator 6) for all seven pickers.
- [ ] Escape and Enter behave the same in every picker, and closing a picker announces where focus landed ("switched to api").

**Messages and popups**

- [ ] Every `display-message` the tool emits can also go to the speech sink, since `display-time` makes them vanish.
- [ ] A `messages` command that lists the tool's recent messages in order, so a missed one can be reread.
- [ ] `brief` and `doctor` in a popup have a plain mode: no boxes, one fact per line, headings as text lines.

**Panes and output**

- [ ] "Last command output" and previous/next command navigation (differentiator 3).
- [ ] The pocket and `run` can open as a window, not a split, so their lines don't interleave.
- [ ] Agent questions captured without box drawing (differentiator 5).

**Docs and launch**

- [ ] A text transcript of the demo cast, step by step, linked beside the GIF.
- [ ] An accessibility page in `docs/how-to/` that says plainly what works with which screen reader and what doesn't yet.
- [ ] Alt text on README images, plus a sentence saying the badges are decorative.

## Checklist: low vision

Contrast is already ahead of most tools. The gaps are colour-only meaning, magnifier reach and glyph legibility.

- [ ] Every coloured state also has a word or shape. Picker tones (amber, green, red), git ahead/behind and the health mark must read correctly in greyscale.
- [ ] A colourblind-safe tone set: avoid red against green as the only distinction, offer blue/orange, and test under a deuteranopia simulation.
- [ ] A theme option that uses only the 16 ANSI colours, so the terminal's own palette, which users already tuned, decides. GitHub CLI made this choice.
- [ ] The cursor band keeps 7:1 contrast. Also check that the selected row is visible with colour off: reverse video already does this under `NO_COLOR`.
- [ ] Magnifier reach. A zoomed user following the cursor never sees a bottom-right bar. Offer `status-position top`, and announce alerts as a centred popup or near the cursor, not only on the bar.
- [ ] Glyph legibility at high zoom. Nerd Font icons blur into each other, so the `spoken` preset doubles as a large-print preset.
- [ ] Popup size scales with the window, as a percentage rather than fixed columns, so large fonts don't clip rows. Clipped rows end in an ellipsis plus a way to read the full row.
- [ ] No motion. Already done for the pocket slide; keep it a rule, and add a `reduce_motion` config key so a future animation respects it.
- [ ] Test at 200% and 400% terminal zoom and with macOS Zoom following keyboard focus.

## Checklist: motor impairments and voice control

The win here is removing chords and timeouts. Every feature being a plain subcommand is already an advantage for voice users.

- [ ] A modal "companion mode" key table. Press the prefix once and stay in the mode, where single keys run commands, until Esc. No chords, no holding modifiers.
- [ ] Every shipped binding also works prefix-less from that table, and `keys` shows which table a binding lives in.
- [ ] No timing requirements. Check `repeat-time` on any repeatable (`-r`) binding the examples ship, and recommend a longer `escape-time`-safe setup for slow typists. Prompts never time out.
- [ ] `kill --ask` and other confirmations accept one key, not a typed word.
- [ ] Click targets on the bar are wide enough to hit. The agents and health ranges should be several cells, not one glyph, for users of head pointers or eye tracking.
- [ ] Pickers accept number selection and arrow keys, not only fuzzy typing, for switch users and on-screen keyboards.
- [ ] Structured terminal titles for Talon contexts (differentiator 7).
- [ ] A Talon command file in `docs/` or a separate repo: "companion project \<name>", "companion where am I", "companion last output", "companion inbox".
- [ ] Dictation-friendly names. Session and window names should be pronounceable words, so `project` could suggest a spoken alias for directories like `x9-api-v2`.
- [ ] Test with macOS Voice Control, Talon, Sticky Keys on, and a one-handed layout.

## Checklist: ADHD and attention

This is where tmux-companion is furthest ahead already: `brief`, `inbox`, `journal`, idle sessions, `zen`, quiet hours and the cheat sheet that learns. These items are design reasoning to test with users, not research findings.

- [ ] A "where was I" card on `start` and on attach to a project. Show the last journal entries for that project, the last command and its result, uncommitted changes, open agent questions and any note left. It answers the re-entry question that costs the most after an interruption.
- [ ] One-key capture of a stray thought into `note` without leaving the pane: a parking lot so the thought stops pulling at attention.
- [ ] A notification budget. Batch polite events and deliver them at a natural break, such as the next prompt or switching windows, not mid-typing. Assertive events still interrupt.
- [ ] Time awareness, opt-in. Show time spent in this project today, plus a gentle line after a long stretch ("2h in api since 14:10"). No streaks, scores or guilt wording.
- [ ] A low-stimulation profile: zen by default, bar reduced to session name and one "needs you" count, icons and colour tones off.
- [ ] `brief` sorted by what needs action, capped at a few items, with "and 4 more" rather than a wall.
- [ ] Consistent words everywhere. The same state is always "asked", never "waiting" in one place and "pending" in another. The current naming is already careful; keep a glossary.
- [ ] Everything stays opt-in and reversible, as now. Surprises cost more for this group than missing features.

## Testing matrix and who to involve

VoiceOver with Terminal.app is the right first target. The tool also runs on Linux, and Linux console screen reader users are the core terminal audience, so cover them before a launch post claims anything.

| Screen reader | Terminal                  | Platform         | Why it matters                                                             |
| ------------- | ------------------------- | ---------------- | -------------------------------------------------------------------------- |
| VoiceOver     | Terminal.app              | macOS            | Your current pass; Terminal.app tracks output better than iTerm2           |
| VoiceOver     | iTerm2                    | macOS            | Popular with developers; known review-cursor issues                        |
| TDSR          | any                       | macOS, Linux     | Preferred by some Mac terminal users for automatic reading                 |
| Orca          | GNOME Terminal (VTE)      | Linux desktop    | Default Linux GUI reader                                                   |
| Fenrir        | Linux console or PTY      | Linux            | Console reader with a speech socket; the first target for the speech sink  |
| Speakup       | Linux console             | Linux            | Kernel-level; the reader the stream-vs-grid essay is about                 |
| NVDA          | Windows Terminal over SSH | Windows to Linux | The most common screen reader overall, reaching your Linux binary remotely |
| BRLTTY        | Linux console             | Linux            | Braille users: check one-line displays against the bar and picker rows     |

Who to involve, before and after building:

- [ ] Ask two or three blind developers who use tmux to try the playground with the screen-reader profile, and pay for their time. Communities to ask: the Fenrir and Stormux users, AppleVis forums, and blind programmer mailing lists.
- [ ] Make the playground container work with a screen reader: document running it in Terminal.app with VoiceOver, and check the tour steps read in order.
- [ ] Ask one Talon user to run through the bindings by voice.
- [ ] Open a pinned GitHub issue or discussion for accessibility feedback and label issues `a11y`. The gemini-cli experience shows that leaving these to a stale bot is what drives this audience away.
- [ ] Say what's tested and what isn't, by reader and version, in the accessibility doc. Claims without that are what this community has learned to distrust.

## Sources

- [lonkar-org/tmux-companion](https://github.com/lonkar-org/tmux-companion): README, CHANGELOG 0.5.0–0.7.0, `docs/reference/cli.md`, `docs/config.example.toml`, `src/picker`, `src/theme`
- [The text mode lie: why modern TUIs are a nightmare for accessibility](https://xogium.me/the-text-mode-lie-why-modern-tuis-are-a-nightmare-for-accessibility) (The Inclusive Lens, 2026-01-05)
- [Building a more accessible GitHub CLI](https://github.blog/engineering/user-experience/building-a-more-accessible-github-cli/) (GitHub blog, 2025-05-02)
- [Accessibility of Command Line Interfaces](https://dl.acm.org/doi/fullHtml/10.1145/3411764.3445544) (Sampath, Merrick, Macvean, CHI 2021). The page blocked direct reading; taken from its abstract and a [summary on dev.to](https://dev.to/baspin94/two-ways-to-make-your-command-line-interfaces-more-accessible-541k)
- [AppleVis: VoiceOver power-user tips and terminal discussion](https://www.applevis.com/forum/macos-mac-apps/tips-power-usersanyone-does-more-read-emailslight-web-browsing-be-more)
- [AppleVis: making VoiceOver announce text via AppleScript](https://www.applevis.com/guides/making-voiceover-announce-time-date-your-mac)
- [iTerm2 issue 12920: VoiceOver review cursor and new output](https://gitlab.com/gnachman/iterm2/-/issues/12920)
- [fenrir(1) man page: remote control socket](https://manpages.ubuntu.com/manpages/stonking/man1/fenrir.1.html)
- [talonhub/community](https://github.com/talonhub/community)
