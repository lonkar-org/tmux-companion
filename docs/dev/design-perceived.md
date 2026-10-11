# Perceived latency

Status: draft
Owner: tmux-companion
Problem: the capped-CPU work cut what a timer measures, keypress to first row. What a person feels is a different set of moments: the popup appearing, the keys they typed early landing, each key while they type, and the bar catching up after they move. This is the design for measuring those moments the way a person meets them, holding them to thresholds with sources, and fixing the ones over the line.

[design-capped-cpu.md](./design-capped-cpu.md) is the work this builds on; [BENCHMARKS.md](../BENCHMARKS.md) has its numbers.

## What a person meets

Each moment with the nearest measured or published threshold. A threshold is the latency below which a delay was not told apart from none, or below which an action felt instant, in the source named; none of them was measured on a terminal picker.

| Moment | What it is here | Nearest threshold | Source |
| --- | --- | --- | --- |
| Frame | key to the picker's border and prompt on screen | 50 ms, first prompt of a shell indistinguishable from zero | zsh-bench, one person's blind self-test ([how fast is fast](https://github.com/romkatv/zsh-bench#how-fast-is-fast)) |
| First row | key to the first row of results on screen | ~96 ms, tapping on a touchpad first detected (indirect, closest to a key opening something); 0.1 s "feels instantaneous" | Deber et al., CHI 2015, via [arXiv 2408.02525](https://arxiv.org/html/2408.02525v1); [Nielsen](https://www.nngroup.com/articles/response-times-3-important-limits/), itself Miller 1968's judgement rather than a measurement |
| Type-ahead | key, then a query typed at once, to the filtered list on screen with none of the query lost | 150 ms, first command of a shell typed before its prompt | zsh-bench |
| Input | one more key into a long query, to the frame showing it | 20 ms | zsh-bench |
| Bar | a pane switch or a `cd`, to the status bar saying the new place | 10 ms is zsh-bench's command lag, the weakest fit: nothing in the sources redraws on its own after an action | zsh-bench |
| Worth doing | how big a cut must be to be noticed at all | ~8.3 ms improvement noticeable across baselines | Deber et al. 2015, same |

What the sources do not say: there is no psychophysical threshold for text appearing after a keystroke (the one keyboard study found 200 ms did not hurt copying text but did hurt correcting it, and fast typists noticed more: [Schmid et al. 2023](https://hci.uni-regensburg.de/publications/effects_of_text_input_latency_on_performance_and_task_load_2024)); Ng et al. 2012's 6 ms dragging figure and Jota et al. 2013's tapping figure could not be confirmed from a primary source and are not used.

### Budgets

- Frame, any picker, load 32: 50 ms median.

- Type-ahead, load 32: 150 ms p95, no key lost.

- Input, load 32, a 20-character query: 20 ms p95.

- Bar after a pane switch, idle: 50 ms median.

## How it is measured

The way zsh-bench measures and our bench already does: a real tmux client on a pty, the clock from the key being sent to the first byte on that pty matching a marker only the moment in question produces. Never an internal "ready" timer: zsh-bench's `exit time` shows one drifting from what a person sees (agnoster exits in 2 ms and leaves the screen empty for 32). Unlike zsh-bench, which keeps the best of 16 runs, this reports the median and p95, since the tail is what is felt.

- Frame: the picker's title on the pty.
- Type-ahead: the binding key and then a query of characters no row contains, sent at once as a fast typist would; the clock stops when the query is on the pty. Lost keys show as a query that never appears. The pane under the popup has echo off, so keys that went to it cannot be mistaken for keys that reached the picker.
- Input: a 20-character query typed and drawn, then one key more, timed by the picker's own `key` and `drawn` marks; the one place an internal mark is used, since the pty cannot tell which frame carried which key.
- Bar: two windows, one in a git repository and one not; select the other one and time until the branch name appears on, or leaves, the pty, in call and in push.

## What the sources say to do, and not

- Draw what cannot change first, the real thing as soon as it exists, and nothing that will be replaced by something different: powerlevel10k's instant prompt and zsh-bench's argument for it, and powerlevel10k's warning against prompts that show stale state and refresh later. A count or a preview that changes after the first frame is out.
- Show rows as they arrive rather than all at once: fzf reads its input asynchronously by default and makes waiting for all of it an opt-in, `--sync` ([fzf(1)](https://mankier.com/1/fzf)).
- No spinner and no skeleton. Under a second no indicator is wanted and a looped animation distracts ([NN/g](https://www.nngroup.com/articles/progress-indicators/)); skeleton screens are not shown to help and did worst in one small test ([Viget](https://viget.com/articles/a-bone-to-pick-with-skeleton-screens)); screen readers handle a redrawn spinner badly, which is why the GitHub CLI replaced its own with static text ([GitHub](https://github.blog/engineering/user-experience/building-a-more-accessible-github-cli/)).
- Never drop or misroute a key typed early. tmux sends keys pressed after a popup's binding into the popup's pty, where they wait for the process to read them (`popup_display` and `popup_key_cb` in [popup.c](https://github.com/tmux/tmux/blob/3.5a/popup.c)); only a `TCSAFLUSH` when entering raw mode would throw them away ([tcsetattr](https://pubs.opengroup.org/onlinepubs/9799919799/functions/tcsetattr.html)), and nothing here uses one. A test holds it there.
- A cut under about 8 ms is not worth a feature's complexity on its own.

## Candidates

Ranked by the moment they move and how far over its budget that moment is; each lands with the bench before and after, and is dropped if the number does not move by more than about 8 ms.

1. Frame first for every picker, not only project and new-window: the panes and keys pickers draw their chrome before collecting rows. Done for panes (`cc6d007`): frame 68 to 49 ms at load 32. Keys needs none once 2 landed: its rows take 3 ms.
2. The accent: every picker's paint waited 15-20 ms under load for a tmux call. Done (`0a41eb0`): the daemon answers from its pane list, keyed by the session id at the end of the popup's `$TMUX`; frame 62-66 to 48 ms at load 32 for project, new-window and keys. A colour set by hand reaches a picker within 3 s. `display-popup -e` and the popup command are not format-expanded (tmux 3.7c), so a binding could not hand it over.
3. The popup's own cost, 23-33 ms of every press: measure what part is tmux running the command through `default-shell -c`, and whether `exec` in the binding or a smaller shell moves it.
4. Bar after a pane switch in push mode: push on the switch, not on the next tick, through a hook that reaches the daemon without starting a process per switch, or show that the tick is already under the budget. Done without a hook: tmux expands `#{@tc-right}` for the client's current pane and looks on the pane before the window and the session, on 3.4, 3.5a and 3.7c, so the daemon writes a pane option on every pane of an attached session whose directory is not the session's, and the switch's own redraw shows it. Still one `tmux` process a tick, changed values only; the render runs once per directory of an attached session instead of once per session. Measured before: p50 979-988 ms in both modes, n=20.
5. Rows as they arrive, for a picker whose rows come from more than one source (project: sessions, then directories), if the first of them is far enough ahead to matter, and only if the cursor never moves under a screen reader.

## Accessibility

- The loading line stays one plain line, replaced once; the frame that replaces it is the list, which a screen reader reads where the cursor is, as WCAG 4.1.3 asks of status messages ([status messages](https://www.w3.org/WAI/WCAG22/Understanding/status-messages)).
- Rows arriving in more than one batch (candidate 5) would move what a screen reader is reading. The cursor stays on the row it was on, by identity, or the batch waits; otherwise candidate 5 is dropped.
- Nothing animates and nothing flashes, whatever it saves.
- Type-ahead keeps every key in order; a key lost is worse for somebody who cannot see where it went.

## Out of scope

- A picker process kept running between presses: it would hide the spawn, but it is a terminal-owning process the daemon would have to manage, as the capped-CPU design already concluded.
- Changing tmux.

## Acceptance

- `just bench-capped` reports frame, type-ahead (with keys lost), input and bar for every picker it covers.
- Every candidate that lands moves its moment by more than 8 ms at load 32, or is reverted with the number in the commit.
- A test sends a query before the picker is ready and finds every key of it.
