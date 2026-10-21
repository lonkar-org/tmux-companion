# Switcher

Status: draft

Problem: `M-a` sweeps a session's windows by index once the first jump is
made, and `M-s` opens the whole project picker to go back one session. Neither
is the Cmd-Tab shape, where the order is the order you used things in, a press
moves a highlight, and you only go somewhere when you let go.

The switcher is that shape for windows (`M-a`, `M-A` backwards) and sessions
(`M-s`, `M-S` backwards):

1. The order is most recently used. One press is where you just were, each
   further press the next older, and wherever you land becomes the most
   recent. Shift walks the other way.
2. While pressing, a small list in that order with the candidate on it. You
   move only when you commit. Esc leaves you where you were.
3. Windows commit on Enter, and with `commit = "pause"` also when the
   presses pause (`pause_ms`, 500). Sessions commit on Enter only, and a letter typed into the session
   list turns it into the project picker's search, in the same popup.

It replaces `toggle`, which is gone, with its `[toggle]` section (ignored if
a config still has one).

## Where the order comes from

tmux keeps one previous window per session (`window_last_flag`) and one
previous session per client (`client_last_session`). That is the first step
of an MRU list and none of the rest. `window_activity` is output in the
window, not focus, and `session_last_attached` is whole seconds: three
`switch-client`s in a row gave three sessions the same stamp on every pinned
version.

Three ways to get the rest:

| Source | Precise | Process per switch |
| --- | --- | --- |
| The daemon's shared `list-panes` poll, reading `window_active` each second | no: two switches inside a second lose the first | none new, but it reads only what is active when it looks |
| A hook running `run-shell "tmux-companion ..."` | yes | one, on every switch, whether or not anybody ever presses `M-a` |
| A hook running tmux commands only | yes | none |

The third is the one used. Three hooks, each two `set-option`s with format
expansion, run inside the server and start nothing:

```tmux
set-hook -g 'session-window-changed[73]' 'set -gF @tc_seq "#{e|+:#{@tc_seq},1}" ; set -wF @tc_win_used "#{@tc_seq}"'
set-hook -g 'client-session-changed[73]' 'set -gF @tc_seq "#{e|+:#{@tc_seq},1}" ; set -F @tc_ses_used "#{@tc_seq}"'
set-hook -g 'client-attached[73]' 'set -gF @tc_seq "#{e|+:#{@tc_seq},1}" ; set -F @tc_ses_used "#{@tc_seq}"'
```

`@tc_seq` is one server-wide counter. Each window entered takes the next
number as `@tc_win_used`, each session switched to takes it as
`@tc_ses_used`, and sorting by it, highest first, is the MRU order. A counter
rather than a time because tmux has no millisecond clock in a format, and
because a counter can't tie.

The window and the session options have different names on purpose. A format
looks an `@` option up pane, then window, then session, then global, so a
session option called the same as a window one reads back as the current
window's value in `list-sessions`. The first try did exactly that.

Checked on an isolated server for each pinned tmux, 3.4, 3.5a and 3.7c, with
the same results on all three:

- `select-window` from a command client, with nobody attached, fires the
  window hook and the `-w` set lands on the window selected.
- `new-window` without `-d` stamps the new window; with `-d` it stamps nothing,
  which is right, since nobody went there.
- Killing the current window stamps the one tmux falls back to.
- `switch-client` fires the session hook and the plain `set` lands on the
  session switched to.
- An attach from outside fires `client-attached` (and `client-session-changed`
  as well), and the stamp lands on the session attached to.
- The index `[73]` sits beside a hook the person set themselves at `[0]`, and
  setting it twice leaves one entry.

Who installs them: `switch` puts all three in the same tmux call it already makes
on every press, so they cost no extra process and a binding is all a person
needs. The example configs set them too, so the history starts with the
server rather than with the first press. Before a window has a stamp it sorts
after every window that has one, the session's last window first and then by
index, which makes the first press right even on a server that has never run
the hook. Sessions without a stamp fall back to `session_last_attached`.

A window linked into two sessions is one window to tmux, so focusing it in one
session moves it up the other's list too. Linked windows are rare enough to
leave that written down here rather than worked round.

## How repeated presses reach one process

A terminal never sees a key released, so the switcher can't know when Alt goes
up. What it can know is the next press. Two places a press can go:

- Before any popup is open, to tmux's root table, which runs `switch windows`
  through `run-shell`: one process per press.
- Once a popup is open, to the popup. tmux hands an overlay every key before
  the key tables see it (`server_client_handle_key`, then `popup_key_cb`), and
  a popup with `-E` and a running job writes the key to the job's pty. On all
  three pinned versions, `M-a`, `M-A`, `Enter` and `Escape` sent to a client
  with a popup open arrived in the popup as `ESC a`, `ESC A`, `\r` and `ESC`,
  with nothing run from the root table.

So the first popup press is the last one that goes through `run-shell`; from
then on the presses are key events in one process, which counts them, moves
the highlight, and commits.

`display-popup` run from a `run-shell` child waits until the popup closes (on
all three versions a command chained after it had not run 0.8 s later), so the
child that opens the popup starts `tmux display-popup` with its output on
`/dev/null` and does not wait for it. `run-shell` would otherwise hold the
client's command queue for as long as the popup is up.

There is a gap between the press that asks for the popup and the moment tmux
puts it up. A press inside that gap still goes to the root table. It is not
lost: the session option `@tc_switch` holds the time of the last press, the
window the presses began in, whether a popup is on its way, and how many
presses arrived for it. A press that finds a popup on its way adds one to the
count, the popup reads the count when it starts and again when it commits,
and the window it commits to is the one the presses counted to. The highlight
can trail that late press by one frame; the destination doesn't.

Popup cost: `just bench-capped` measured an empty popup at 25 to 33 ms to its
first byte. That is paid on the second press, not the first, under the flip
below.

## The quick flip

Today one tap of `M-a` goes to the last window at once, and that is most of
what `M-a` is pressed for. With a commit on pause, one tap would wait the pause
before anything moved.

Two shapes:

- **flip** (`first = "flip"`): the first press goes to MRU #1 at once, with no
  popup, exactly as fast as the `toggle` it replaces. A second press inside
  `pause_ms` opens the list on MRU #2, with the order as it was before the first press, and further
  presses move from there. Commit goes back through the window the presses
  began in, so the order afterwards is what Cmd-Tab would leave: the window
  committed to, then the one you started from. Esc goes back to where the
  presses began. A single tap never shows a list.
- **list** (`first = "list"`): every first press opens the list on MRU #1 and
  nothing moves until commit. A single tap shows the list for `pause_ms` and
  then goes. Nothing behind the popup changes while it is up.

A quick second press used to land on top of the flip: the first press had
already moved before the list opened. Since 2026-10-21 the first press waits
`flip_wait_ms`, 150, before it goes. A second press inside the wait opens the
list from where the presses began with nothing moved; one after the wait but
inside `pause_ms` opens it over the flip as before. 150 ms is about as long
as a single tap can wait before it feels slow, so it catches a quick double
press and not a deliberate slow one. The wait is a detached process the
press starts, because a press that slept in its own `run-shell` would hold
the client's keys, the second press among them. It is a debounce, the thing
the capped-CPU rules say not to add for a frame; here it costs one sleeping
process per tap and replaces a visible move, and `flip_wait_ms = 0` takes it
out. It asks nothing new of the person pressing: the second press already
had to come within `pause_ms`.

Decided: **flip** is the default. The single tap is the common press, and it stays as
fast as it is now and draws nothing. Cmd-Tab and VS Code's Ctrl-Tab both show
nothing on a quick tap either. What it costs: during a longer run the window
behind the list is the one the first press went to rather than the one you
started in, and the second press has to come inside `pause_ms` to open the
list, which is a timing rule `list` doesn't have.

`list` stays, behind `[switcher] first`, as the shape with no clock in the
second press.

Sessions always open the list (there is no flip): `M-s` already opens a popup
today, and a session switch with a pause in it would be the same timing rule
for a much bigger move.

## Committing

`[switcher] commit` is `"enter"`, the default, or `"pause"`. It was
`"pause"` until 2026-10-19: a list that goes on its own left no time to read
which window was the candidate before pressing on, so knowing where you were
in the list meant racing the clock.

- `pause`: the window list commits `pause_ms` after the last `M-a` or `M-A`,
  or at once on Enter. Any other key (a letter, an arrow) stops the clock, and
  then only Enter commits: once somebody is searching or arrowing, they are
  doing it on purpose and slowly.
- `enter`: only Enter commits. Nothing in the switcher is timed.

The session list commits on Enter only, whatever `commit` says.

## Sessions

The session list is the project picker in a different opening state, so typing
needs no second popup and every path the picker has keeps working, the typed
path from 08bf2ab included:

- Every session is listed, attached elsewhere or not. Another client's session
  is a place you can go, and a session idle for days carries its `idle 5d`
  column as it does in the project picker.
- The order is `@tc_ses_used`, then `session_last_attached`. The session you
  are in is the first row and the cursor starts on the second.
- The directories are there from the start and hidden while the query is
  empty. The first letter typed brings them in after the sessions, and a
  search filters without ranking: sessions by last use, then projects by last
  jump (zoxide's `last_accessed`, read from `db.zo`, since `zoxide query -l`
  only gives frecency). Until 2026-10-12 a search ranked sessions and
  projects together by match score, which mixed the two.
- `M-s` and `M-S` move the cursor; Enter goes; Esc stays.

Only the sessions are collected before the first frame; the directories come
after it (see Performance).

## Accessibility

What job: moving to a window or session you were just in, without the prefix.

- The plain path is `switch windows --print` (index and name) and
  `switch sessions --print` (the name): one to a line, most recent first,
  nothing else on the line.
- The list is the shared picker, so the terminal's cursor sits on the
  candidate row, which is what a screen reader and a braille display follow.
  The row is also marked by more than colour: the current window or session
  carries the word `now`.
- Each press puts the candidate's name and place, `ai, 2 of 4`, on tmux's
  message line (`[switcher] announce`). While the switcher is up the message
  line covers nothing the person is using, unlike `toggle`'s, which covered
  the window list mid-sweep. The flip announces where it landed the same way.
- Timed: under `first = "flip"`, the second press opening the list, and
  `commit = "pause"` when it is set. The untimed path is `first = "list"` with
  `commit = "enter"`: each press opens or moves the list, Enter commits,
  nothing happens on a clock. That limit and that path go in the manual, the
  configuration page, the CHANGELOG line and the checklist status.
- The popup draws a border like every picker. Screen-reader mode, still on its
  own branch, will read the list as plain lines the same way it reads the
  other pickers; until then `--print` is the chrome-free path. Written in the
  CHANGELOG line as `a11y: limited` for that reason.

## Configuration

```toml
[switcher]
first = "flip"      # or "list": the first M-a goes, or opens the list
commit = "enter"    # or "pause": the window list commits only on Enter, or on a pause too
pause_ms = 500
flip_wait_ms = 150  # how long the first M-a waits for a second; 0: at once
announce = true     # the candidate on the message line, each press
```

A `[toggle]` section is accepted and ignored, and `config check` and `doctor`
say it can go.

## CLI and bindings

```text
switch windows [SESSION] [--reverse] [--client NAME] [--print]
switch sessions [--reverse] [--print]
```

In a terminal (inside a popup) `switch windows` draws the list. Without one
(from `run-shell`) it is a press: it flips, or opens the popup for itself, or
counts toward a popup already on its way. The hidden `--pressed` tells the
popup a press opened it and left `@tc_switch` for it to read. The session is
passed as `#{session_id}`, which needs no quoting, so a press costs one tmux
call before it acts; with only `--client` it asks for the session first.
The popup is sized to the list: six rows of chrome and one per window up to
twenty, as wide as the line of keys.

```tmux
bind -n M-a run-shell "tmux-companion switch windows '#{session_id}' --client '#{client_name}'"
bind -n M-A run-shell "tmux-companion switch windows '#{session_id}' --reverse --client '#{client_name}'"
bind -n M-s display-popup -B -E -w 80% -h 70% "tmux-companion switch sessions"
bind -n M-S display-popup -B -E -w 80% -h 70% "tmux-companion switch sessions --reverse"
```

`setup`'s catalog follows the full example, so its `toggle`, `toggle-last`
and `project` items became `switch-windows`, `switch-windows-back`,
`switch-sessions` and `switch-sessions-back`.

Decided: one picker is enough. `M-s` is the project picker, titled
`[ Sessions/Projects ]`, and there is no separate setup item for the bare
`project` picker; the command stays for `project DIR` and anybody who binds
it. Directory rows carry nf-md-puzzle_edit (U+F14D3, `PROJECT` in
`src/tmux/icons.rs`, `p` in the ascii preset) instead of the folder, which
read too much like the session icon beside it.

Decided: `toggle` and `[toggle]` are removed now rather than deprecated.
The `toggle` command shipped in 0.9.1, so a binding that still calls it
fails as an unknown command; `[toggle]` never shipped, and a config with it
still loads.

## Performance

- Windows: one tmux call before the first frame (`list-windows` and the
  session's state in one invocation, with the hooks chained on). No per-row
  work, no preview.
- Sessions: one tmux call before the first frame, `list-sessions` with the
  hooks chained in front, and no loading frame, since that call is no
  slower than the window list's. The directories (the source's process and
  every theme file) were collected before the first frame too, for rows
  hidden until a letter is typed: measured on this branch with `RUNS=10`
  and `CONFIG=mine`, `switch sessions` was seen at p50 78 ms at load 0 and
  108 at load 32, 58 of them collecting rows. They now come from a thread
  the picker starts once its first frame is up (`picker::Later`), so they
  don't compete with the drawing for a core; they join the list when they
  arrive, and the first letter typed waits for them if they haven't, so a
  search never runs over half the list.
- Both lists' `draw` phase was 6 ms at load 32 against about 2 for the other
  pickers. The likely cause is the announcement: the first candidate was
  said before the first frame, and saying is a `tmux display-message`
  process, so its spawn landed between the paint and the frame, the only
  work in that phase the other pickers don't have. It is said once the
  frame is up now. Traced on an idle machine (debug build), the spawn cost
  about 1 ms and setting up the terminal 0.3; whether that accounts for all
  4 ms at load 32 is for the next `bench-capped` run to say.
- Both added to `scripts/bench-capped.py`'s `PICKERS` (`M-o` for sessions,
  `M-w` for fx-01's windows with the clock off so the typed burst is
  measured). The flip itself is a `run-shell` process and one tmux call, the
  same as `toggle` was; a popup press is that plus the popup.

Measured on the merged release build, 2026-10-10, `CONFIG=mine`, two rounds
(no before: the switcher is new). Windows: frame p50 53-54 ms idle and 74-77
at load 32, over the 50 ms budget; its frame is its first row. Type-ahead 55
and 82-85. Sessions isn't measured yet: the bench waits for a commit the
sessions view doesn't make (`docs/backlog/bench-harness-gaps.md`).

## Left open

- The highlight can trail a press made in the moment the popup is being put up
  (see above); the destination is right.
- Linked windows share their stamp across sessions.
