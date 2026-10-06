# Chunk clock

Status: draft
Owner: tmux-companion
Problem: focused work runs past the time it should. A timer that must be started loses to the work.

The chunk clock shows how long the aggregate of session has been in the focus of this sitting, paints that on the status line, and notifies when the budget is over. It does not track tasks, does not start a pomodoro, and does not kill panes.

## Terms

- Focus time. Seconds an attached client had the OS focus (tmux's `focused` client flag, which needs `focus-events on`) and saw a key or click within `break_after`. A gap shorter than that counts as work so reading a long agent reply doesn't stop the clock. This is the number on the bright clock.
- Process age. Seconds since the pane's current command started. Secondary. Does not spend budget.
- Sitting. One budget interval across every session. Ends after `break_after` away, or on `chunk reset` or `chunk close`. The next focus starts a new sitting at 0.
- Budget. Configured limit for a sitting. Default 50 minutes.
- Overdue. Focus time past the budget. Stays visible. Does not roll into a new sitting by itself.

## What it shows

Status segment, right side, every window of an attached session:

```
󱫐 38m
󱫌 50m+12
󱫐 22m · 1h14m
```

- Focus time in the sitting (`38m`, `1h06m`), after `󱫐` (nf-md-timer_check).
- At or past budget: `50m+12` (budget, then overdue), after `󱫌` (nf-md-timer_alert).
- Agent pane only: dim process age after a middle dot (`1h14m`), unless `agent_age = false`. Agents are the ones the bar's agent count already finds (`panes::is_agent`) which catches claude by its version-named process where a `^claude` pattern never would.

Colors:

- ok: under 80% of budget
- warn: 80% to budget
- overdue: past budget

Colour isn't the only channel. `chunk status`, `read-bar` and `brief` say it in words: `12 min for break` under budget, `1 hr 7 min passed break time` over it.

No segment when no client is attached. Detached sessions do not accumulate focus time.

## Clock rules

Focus time accrues only while all of these hold:

1. A client attached to any session has the OS focus.
2. That client saw a key or click within `break_after`.

Away for less than `break_after` (no focused client, or no key) the sitting carries on and the gap counts. At `break_after` it's a break, the sitting ends and the next focus starts one at 0. Switching session, window or pane changes nothing.

A terminal that doesn't report focus reads as focused forever so there only the key rule applies. `doctor` warns when `focus-events` is off.

Process age is read from the pane's current command start. It is display-only. A `claude` pane at 72 minutes while the user is in another window does not spend the budget.

Creating a pane, restoring a session, or a long-lived shell does not start focus time. Focus does.

## Notifications

Fired by the daemon, not by the status redraw.

- At budget the cue waits for the next boundary: a prompt coming back (OSC 133), an agent finishing its turn, or a window switch. It fires at most `boundary_grace` late.
- The cue is a sound, an earcon from `earcons.rs` (brought over from screen-reader-mode). A desktop banner is off by default and `banner = true` turns it on; its body is `50m sitting over`.
- `chunk snooze` grants one extension of `snooze`, then one more cue, then only the bar.
- Quiet hours keep the sound and drop the banner. The cue only comes while you're at the keyboard so it can't wake a display.
- No notify for process age.
- No notify while away.
- A sitting notifies at budget once. Reset or a new sitting arms it again.

## Commands

```
tmux-companion chunk status            # one line, or nothing if no sitting is running
tmux-companion chunk status --json
tmux-companion chunk snooze            # one extension of `snooze`, once per sitting
tmux-companion chunk reset             # new sitting, focus time 0
tmux-companion chunk close             # end the sitting, clear the segment until focus returns
tmux-companion chunk budget <dur>      # override budget until reset
```

`M-z` in the root table runs `chunk snooze`.

Durations accept `45m`, `1h`, `90s`. No start command. The clock runs when tmux has the focus.

`chunk status` prints the segment text, or nothing if no sitting is running. Exit 0 always when the daemon is up.

## Config

In `config.toml`:

```toml
[chunk]
enabled = true
budget = "50m"
warn_at = 0.8
break_after = "10m"
boundary_grace = "5m"
snooze = "5m"
sound = true
banner = false
agent_age = true
```

`chunk budget` overrides the file until `chunk reset` or daemon restart. File wins again after restart.

`enabled = false` removes the segment and stops notifies. Existing focus time is kept.

## Status integration

The clock is one more segment in the combined `status-right`, placed with `[[status.right.segments]]` like the others. It gets no `#()` of its own and writes no segment files because one more `#()` costs about 14.6 ms of CPU a second per attached client.

## State

In-memory in the daemon. Persisted only so a daemon restart does not zero a live sitting:

```toml
# state/chunk.toml
focus_s = 2280
budget_s = 3000
notified_budget = true
snoozed = false
last_focus = 2026-10-06T10:40:00+05:30
```

Wiped on `chunk reset` and `chunk close`. Not part of `sessions save`.

## Out of scope

- Task lists, estimates, history, reports.
- Starting or stopping the user's process.
- Locking the pane or forcing a break.
- Per-pane budgets. Process age is the per-pane signal.
- Sync across machines.
- Chrome. Stretchly or `pomodoro` remains the interrupt outside tmux.

## Acceptance

- Work 5 minutes in `forgebolt`, switch to `vachan` for 5 more: segment shows about 10m.
- Switch to the browser for 4 minutes and come back: the sitting carries on and the 4 minutes count.
- Away 10 minutes, in the browser or with no key: the next focus starts a sitting at 0.
- Cross budget while a command runs: no cue until its prompt comes back, or 5 minutes, whichever is first. The bar shows `50m+N` and `chunk status` says `N min passed break time`.
- After the cue, `M-z`: one more cue 5 minutes later, then nothing but the bar.
- In quiet hours the cue is the sound with no banner.
- Agent pane focused: segment includes dim process age. A busy agent in another window doesn't spend budget.
- `chunk reset` zeros focus time and re-arms the budget cue.
- Daemon restart mid-sitting restores focus time.
- `focus-events off`: `doctor` says so.
- `enabled = false` clears the segment and sends nothing.
