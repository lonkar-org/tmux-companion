# Changelog

Kept in the shape [keep a changelog](https://keepachangelog.com) suggests, one
entry per phase of the comrades port.

## Unreleased

### Added

- `setup`: what the tool offers and the state of each, with the lines to add
  one. Enter copies them and asks before writing a fenced block into the
  tmux.conf tmux loaded, or merging into config.toml. `--print` prints the
  rows. `doctor` counts what is open, and the brief does for the week after a
  build first runs.
- `shell-init` sets the pane option `@tmux-companion-marks` on the first
  prompt, which is how `setup` sees the marks are on.
- `kill --ask` looks before it asks. A shell at its prompt is refused with
  no question, and anything else gets tmux's confirmation naming the program
  and its pid. The `K` binding in `docs/tmux.conf.full.example` uses it.
- `docs/how-to/which-key.md`: every key in the full example, the question
  it answers, its `--help` line, and the tmux default it replaces.

### Changed

- The cheat sheet shows the bindings you keep looking up in `prefix ?`, most
  lookups first, and drops one after `[usage] learned_after_days` (14) with
  no lookup. Companion keys fill three boxes; the fourth holds the tmux and
  plugin keys you've looked up. `cheatsheet --print` writes rows as TSV.
  `docs/explanation/cheatsheet.md` says why.
- Binding notes start `companion: ` rather than `custom: `. A note still
  written `custom: ` is read as `companion: ` for one release, so an older
  tmux.conf keeps its keys in the search and on the cheat sheet.
- `[journal]` and `[usage]` are off until turned on, like every other
  background task. `journal` says it's off rather than showing an empty day.
- `DESIGN.md` and `BENCHMARKS.md` moved under `docs/`.
- The playground image's two base images are pinned by digest, and
  `.github/dependabot.yml` bumps them, and the workflow actions, weekly.
- The workflows pin every action to a commit SHA, with the version in a
  trailing comment.
- The playground image runs as `USER 1000:1000`, strips any setuid or setgid
  bit, and drops `man-pages`, `findutils`, `coreutils` and `grep` for
  busybox's. Its seeded home lives read-only at `/opt/playground/home` and
  the entrypoint copies it into `/home/play` on start.
- `scripts/playground.sh` runs the playground read-only, with tmpfs for
  `/tmp` and the home, no network, no capabilities, `no-new-privileges`, and
  a pids and memory cap; `PLAIN=1` runs it bare. The workflow smokes the
  image both ways, and `docs/how-to/playground.md` gives the command.
- The playground build fails on a HIGH or CRITICAL vulnerability that has a
  fix, scanned with Trivy. The published image carries an SBOM and a
  `mode=max` provenance attestation, and is signed keyless with cosign;
  `docs/how-to/playground.md` has the `cosign verify` line.

### Fixed

- A picker with nothing to show (`sessions idle`, `inbox`, `ports`, `panes`,
  `journal`, `search`, `keys`, `cheatsheet`) also puts its line on the tmux
  message line, so `display-popup -E` closing doesn't take it away. `--print`
  is unchanged.
- A machine with no `/sys/class/power_supply` at all, such as a container,
  reads as having no battery. It used to count as a failed battery read, so
  the health mark said `timer` on every render and hid whatever came after.
- `sessions save`, `shutdown` and `restart` work from a terminal outside
  tmux with no UTF-8 locale, and so does the sessions timer in a daemon a
  service manager started. tmux printed every tab in its listings as `_`
  there, so nothing could be read and the save refused with `alpha_/tmp/..._0`.
  Every tmux call now passes `-u`, except `attach`, which leaves the terminal
  to your locale.
- `status-right` computes only the segments `[status.right]` lists. Git, net
  and battery used to run on every render whatever the list said, so a bar
  without a battery still queried it and a failed read still put `battery
  segment` on the health mark. The daemon no longer warms the battery at
  start either unless the bar draws it. `gst`, `net` and `battery` by hand
  compute as before.

## 0.5.2 - 2026-09-28

The evening 0.5.1 went out the bar said `timer` over something that wasn't
wrong. This is what changed so it doesn't and so a failure that's been read
can be taken off the mark.

### Added

- `health`: why the health mark is up, one reason a line or `ok`. `health
ack` forgets the timers and segments that failed and prints each one so
  the mark comes down when you've read it and not an hour later. It leaves
  what's still true and lists it as `still on the mark`: an edited
  `config.toml`, a newer binary, quiet hours or a network that's gone.
  `doctor` names the command on its health line when a failure is there.

### Fixed

- The sessions timer called it a failure when there was no tmux server to
  save. I closed every session, started tmux again seven minutes later and
  the bar said `timer` for the rest of the hour because the daemon had
  outlived the server and remembered, so no server, or one with no session
  in it, is nothing to save now and nothing on the mark. Anything else tmux
  says when asked for its sessions is still a failure, a socket it may not
  read among them.

- A timer that failed and then worked kept its failure on the mark until the
  hour ran out. It takes its own back on the next run that works and leaves
  any other timer's standing. The mark also held one failure at a time so a
  second one replaced the first; it holds one per timer and per segment and
  `+N` counts them.

## 0.5.1 - 2026-09-27

0.5.0 was tagged and never published. The release gate runs the suite on the
runner's tmux 3.4 before it builds anything, one e2e test failed there that
passes on the 3.7c I have here, so there are no 0.5.0 binaries and the tap
never saw it. This is 0.5.0 with the test corrected, and what's new since
0.4.0 is the entry under this one.

### Fixed

- The e2e test for `search --first` compared the whole of
  `#{copy_cursor_line}` with the needle. tmux 3.4 gives that line back with
  the copy-mode position indicator on it when the line is on the top row,
  which is where the oldest line in the history lands, so the test reads the
  first three words now. `search` had put the pane in copy mode on the line
  and selected it on both versions.

## 0.5.0 - 2026-09-27

The agents say what they're doing, where 0.4.0 read it off how long a window
had been quiet. The rest is what six tmux plugins did before they stopped
being pushed, `search`, `ports`, `kill`, `promote`, the options `doctor` ends
on and the `[online]` mark, and along with those the pocket, a shell you pull
out beside the editor and put away again, and a layout that travels with the
checkout.

### Added

- `agent busy`, `agent asked`, `agent done`: what an agent is doing, said by
  the agent itself from one of its hooks, kept per pane by the daemon and
  winning over the window's quiet time on the bar, in the inbox, in `panes`
  and in the brief. `agent hooks claude` prints the `settings.json` block
  that wires claude up: a prompt sent or a tool finished is busy, a
  permission prompt or an `AskUserQuestion` is asked, the end of a response is
  done; `agent clear` forgets a pane's last word. Silence alone could not
  tell a two-minute build from a question, and called every idle agent
  waiting.

- `.tmux-companion.toml` in a project root: the layout that checkout opens
  with, travelling with the repository. `[[window]]` rows in the shape of
  `[[layout.window]]`, or `layout = "name"` naming one of the reader's own.
  Its commands run only under a path `[project] trusted` lists; anywhere
  else the window names count and every command is blanked, and `project
show` says which happened. `config check` inside the checkout reads it.

- `pocket`: a shell that slides out beside the pane you're in, goes away
  into a window called `_pocket` on the second press, and comes back on the
  third with its process and its scrollback. `pocket NAME` keeps more than
  one. `toggle` cycles past the window, and `project save` leaves it and any
  pocket that is out off the layout it writes.

- `search`: every line of every pane's scrollback in one list, newest
  first, and enter lands on the line in copy mode with the line selected.
  A pattern narrows it, `--kind url|path|sha|ip` is a stored search,
  `--first` goes to the newest match without the list and `--print` is the
  same rows for a script. tmux-copycat did the stored searches and hasn't
  been pushed since May 2023.

- `ports`: the TCP ports something is listening on, each with the pane
  that started it, and enter goes to the pane. `--all` adds the ports no
  pane started, `--udp` the UDP sockets bound and not connected, `--print`
  is the rows for a script, and `--kill` makes enter stop the program.

- `doctor` ends on tmux's own options: the ones that cost something as
  they are set, each with its value, what that costs and the line for
  tmux.conf. It sets nothing. `docs/tmux.conf.full.example` now sets
  `escape-time`, `history-limit`, `display-time` and `focus-events`, which
  is what tmux-sensible did for people before it stopped in April 2024.

- `promote`: give a pane a session of its own, named for its directory
  the way `project` names one, with its process and scrollback. A session
  of that name that's already there is joined instead.

- `[online]`: the health mark reads `offline` when two TCP connections in a
  row to `probe` went unanswered, and `doctor` and the brief say for how
  long. Off by default, since the probe leaves the machine.

- `kill`: stop a program that stopped answering `C-c` without closing its
  pane. TERM to the terminal's foreground group, KILL after `--grace`
  seconds if it's still there, and a shell at its prompt is refused.

- The terminal bell as a signal: an agent pane whose window rang the bell and
  has not been visited since reads as `asked`, which covers any agent with no
  hooks that can ring one.

- `[agents] show`: which count follows the total on the bar. `waiting`, as
  before; `busy`, drawn with the progress glyph, for somebody who keeps a
  dozen agents open and works one at a time; or `both`. `BUSY` joins the
  glyph names under `[glyphs.icons]`.

- The journal writes an `answered` line when an agent that reports through
  its hooks works one turn past `[journal] agent_min_secs`, five minutes by
  default, and says it is done: the program, how long, and the last line of
  the answer. A turn under that is a chat and gets nothing.

- `docs/tmux.conf.full.example` says the prefix, copy mode and
  synchronised panes on the left of the bar with tmux's own formats, and
  carries the two lines that hand a tmux-fingers match to `open`.
  `how-to/things-tmux-already-does.md` has a section on the first.

### Fixed

- The question an inbox row, a nudge and a journal line carry was the last
  line on the agent's screen, which for claude is its mode line, so a
  notification read `⏵⏵ auto mode on`. It's now the last thing the agent
  said: the input box and everything under it are cut, then the lines
  `[agents] question_skip` matches, and a question inside a dialog loses the
  box's edges.

- A nudge is no longer sent about an agent that said `done`.

- A process named by its version is shown as `claude` in rows, nudges and
  the journal, where it read `2.1.283`.

- The journal writes a question once per pane rather than once per stop: an
  agent with no hooks that redrew its status line and went quiet again wrote
  the same line every thirty seconds.

### Changed

- A click on the agent count opens the agent list rather than the inbox when
  `[agents] show` is `busy`, since the number clicked is the working agents
  and the inbox holds the stopped ones.

- The inbox and the brief say how an agent stopped: `asked`, `done` or
  `waiting`. A `done` agent is listed so the answer can be read, is not
  coloured, and is not news for `brief --hook`; `panes` reads the same words.
  The brief's numbers line counts busy as well as waiting.

## 0.4.0 - 2026-09-26

The attention half of the name. 0.3.0 taught the daemon where the agents are
and when they've stopped; this is what it does with that for you, and what it
remembers.

### Added

- `inbox`: the agents waiting on you, longest wait first, each with the
  question it asked. The daemon captures an agent's last screen lines the
  moment it stops drawing, so the question is on record for a window nobody
  has looked at since; one capture per stop, not one per read. Enter jumps
  there. `[agents] nudge_after_secs` says it out loud once, through
  `nudge_command` or tmux's own `display-message`, and the skill file tells
  an agent to put its question on the last line and print nothing after it.

- `brief`: what needs you, on one screen: the agents waiting and what each
  asked, the reasons the health mark is up, the sessions nobody has touched
  for three days, and one line of numbers with the last snapshot's age.
  `--hook` on `client-attached` opens it only when something is waiting or
  wrong, so attaching to a quiet server stays quiet.

- `quiet 45m`: no notifications, no nudges and no agent count on the bar for
  a while, with the health mark saying `quiet` in their place so the silence
  reads as chosen. `quiet` alone says how long is left, `off` ends it. The
  daemon keeps the clock, so every timer and every client agree.

- `journal`: what happened in each project, written down as it happens by
  the daemon: a command that ran past `[journal] min_secs` with how long it
  took, an agent that stopped and what it asked, a project opened or closed.
  Newest first, Enter goes to the session, `-t SESSION` is the standup
  answer for one project, `--days` reaches back. `journal.tsv` in the state
  directory, in local time, rotated like the daemon log.

- `sessions export` and `sessions import`: a generation as one file that
  reads on another machine, every path under home spelled `~`, the project
  colours bundled, the screens left behind. Import stores it as a new
  generation named after its source and adds the colours for the projects
  this machine hasn't coloured; then `sessions resurrect` names the
  directories that aren't there.

- A note left on a pane with `note` goes through a snapshot and comes back on
  restore, set with `select-pane -T` before anything runs in the pane.

- `[agents] style = "glyphs"`: the robot and the count, an arrow, an
  hourglass and the waiting count, `󰚩 2  1`, for a bar that already has
  words on it. The three glyphs are `AGENT`, `AGENT_TO` and `WAITING` under
  `[glyphs.icons]`.

- A click on the bar. The `agents` and `health` segments carry a
  `range=user|NAME` mark, the way tmux marks its own window list, so
  `bind -T root MouseDown1StatusRight run-shell "tmux-companion click
'#{mouse_status_range}'"` reaches `click` with the segment's name: the
  inbox for the count, the brief for the mark, or whatever
  `[[status.right.segments]] on_click` says. The other segments carry no
  mark, so the default side's output is byte for byte what it was.

### Changed

- `sessions list` says `imported` and where from on a generation that came
  in through `import`, since its stamp is when it landed here.

- `[agents] interval_secs` now paces the inbox as well as the bar's count,
  and `[journal]` is a new table: `enabled`, `interval_secs`, `min_secs`.

### Fixed

- A process named only by its version counts as an agent. claude sets its
  process title to its version, so tmux reported its pane as `2.1.283` and
  nothing in `[agents] programs` matched it; on the laptop this was written
  on it was the only agent running and the bar, the inbox, the brief,
  `panes --agents` and the nudge all said none. The same rule keeps `[notify]`
  and the journal from treating a claude session ending as a command
  finishing.

## 0.3.0 - 2026-09-26

The work of 2026-09-25 and 26, most of it from a review of the whole tool for the places where it went quiet when it shouldn't have. The one that
started it: the `[autosave]` timer had been failing every fifteen minutes for
thirty hours, because a `~` in its script path was never expanded and the
daemon's stderr went to `/dev/null`, so nothing on screen changed and I found
it by accident while chasing an `M-a` bug.

### Added

- The daemon has a log, `daemon.log` in the state directory, one line per
  start and one per failure. `[general] log` moves it. `doctor` prints the
  log's last line, whether either autosave timer is on and when it last ran,
  and whether the script it points at exists.

- A health mark on the bar. `[[status.right.segments]] name = "health"` draws
  one glyph and a word when a timer failed in the last hour, `config.toml` was
  edited after the daemon started, or the binary on disk is newer than the one
  running, and nothing at all otherwise. `doctor` has a `health` line that
  names every reason in full, asked of the daemon itself.

- `panes`, a picker over every pane on the server: `session:window.pane`, the
  program with the pane's title beside it, `busy` or `waiting 3m` for an agent
  and `active` or `idle 3m` for the rest, the directory, and the pane's last
  lines as the preview. Enter jumps there. `--agents` keeps the coding agents,
  `-t` one session, `--print` gives TSV. Bound to `prefix g` in the examples,
  `prefix G` for the agent list. The skill file tells an agent to find the
  others with it instead of a hand-matched `list-panes -a`.

- `[agents]`: which programs are coding agents, how many seconds of silence
  make one `waiting`, and how often the bar re-reads the list. The `agents`
  segment draws `3 agents · 1 waiting` when listed under `[status.right]`,
  from one `list-panes` every two seconds however many clients are attached,
  and the restore summary counts agents from the same table.

- `note`: a one-line note on a pane, as tmux's pane title. `panes` shows it
  beside the program, the pane border draws it when `pane-border-status` is
  on, no text prints the note that is there and `--clear` takes it off.

- `sessions idle`: the sessions with no client and no activity for N days,
  three by default, most idle first; Enter runs `project close` on the pick so
  the layout is captured on the way out. The project picker shows `idle 5d` on
  a detached session a day or more old.

- `keys --unused`: the bindings you wrote that the usage log has no press for,
  with a line saying how many of how many and how many presses the log holds.
  The log carries no dates, so "unused" means "since the log began".

- `[theme] default = "by-name"`: an unclaimed session gets one of the six
  bundled themes chosen from its name, the same one every time and on every
  machine (FNV-1a, not the standard hasher, whose output may change between
  Rust releases). The project picker shows a directory in the colour its
  session will get. A theme picked by hand still wins.

- `config init` writes a fifteen-line starter and refuses to overwrite.
  `config dump` stays for seeing what a key is called.

- `toggle --last` flips to the previous window, tmux's `last-window`, bound
  to `M-A` beside `M-a` in the examples. `project show` and `project forget`
  take a directory, so a project can be inspected from outside its session.

- `docs/tmux.conf.starter.example`: the bar, `status-style`, and eight
  bindings, with the `-N "custom: <group> ..."` note convention that `keys`
  and `cheatsheet` depend on written down for the first time. The other two
  examples say what they are. `docs/tutorial/first-hour.md` walks install to
  the first picker. `docs/how-to/things-tmux-already-does.md` is the page the
  port notes planned: `pipe-pane`, `display-menu`, `customize-mode`,
  `allow-passthrough`, `link-window`, `join-pane`, `respawn-pane`,
  `select-pane -T` and the `%if` guards, each checked against `man tmux` 3.7c.

- Saved layouts carry `format = 1` and the build that wrote them; snapshots
  say "a newer tmux-companion wrote it" instead of "unknown field" when the
  format is ahead of the build. The daemon writes `VERSION` to the state
  directory on start. The build id carries the short commit and a `-dirty`
  flag after the seconds.

- `just install` builds, installs and restarts the daemon; `just test-unit`
  and `just test-e2e` split the fast suite from the fifty-second one, and
  `TC_SKIP_E2E=1` skips the tmux tests. CI runs an MSRV job and a check that
  fails on any review marker left in the tree; the release workflow gains
  a gate that runs fmt, clippy, the tests and the tag-matches-Cargo.toml
  check before it builds anything.

- Design notes under `docs/dev/`, for a `.tmux-companion.toml` in the
  project root (names only, no commands, so nothing to trust) and a pocket
  pane per session (park it in a hidden window with `break-pane`, bring it
  back with `join-pane`). Nothing built yet; the notes say what would change
  my mind.

### Changed

- Every path in `config.toml` expands a leading `~`, through one function.
  Four places did it by hand and the autosave script path wasn't one of them.

- `restart` exits 1 with the reason when the new daemon refuses its config.
  It said "daemon restarted" over a dead daemon, which is the message that
  sends you looking everywhere but the file you just edited.

- Every command that reads the config says so on stderr, once, when the file
  doesn't parse and the defaults are in use. `project show` reported the
  defaults as if they were yours.

- Segment commands exit 1 on a daemon error instead of 0 with the error on
  stderr; the status bar ignores exit codes, so `tmux.conf` doesn't care.
  `gst --style bogus` is refused with the list. `keys` and `cheatsheet` with no
  `-N "custom: ..."` binding say what they need instead of showing an empty
  picker.

- A client older than the running daemon no longer kills it; only a newer
  client replaces one, and a daemon too old to report a version counts as
  older. Two builds in one tmux, `target/release` and `/usr/local/bin`, used
  to kill each other's daemon on every call.

- The shipped default layout is no layout, a plain shell. It was `nvim`
  beside `claude`, which is what one laptop runs, and a machine without either
  got two windows of "command not found" on its first `start`. The pair stays
  in `config.example.toml` as the example to copy; `[project] preview_window`
  defaults to empty to match.

- `project show` names a saved-layout file that is there and not used, with
  the reason: it doesn't parse, it has no windows, or an older build wrote
  tmux's `default-command` where every pane's program should be. All of those
  collapsed into "the config decided" while the file sat there.

- `project --print` with a directory is refused; it opened the session and
  printed nothing. A directory before a subcommand is refused too; both were
  dropped in silence.

- `toggle` cycles the session's windows by index instead of matching names
  against the saved layout, which failed with two windows called `zsh` and
  with any window the layout didn't name. The `WINDOW` argument is ignored so
  old bindings still parse.

- `project close` names its own command in messages, quits the vi family and
  `hx` in their own language and sends everything else `exit`, and refuses a
  session that isn't there. emacs and nano were on the list for a while and
  got `:qa!`, which they ignore.

- `[[status.right.segments]]` knows `agents` and `health`. `--print` is the
  print-and-exit flag on every picker; `--plain` still works as an alias.

- The crash marker means a crash. It used to mean "a daemon is running", so
  every restore thought it followed one; now a start moves a marker whose pid
  is dead to `sessions/crashed`, and only that file counts. SIGTERM and SIGINT
  clear the marker and unlink the socket, so `pkill` no longer reads as a
  crash. Snapshots taken by the timer say `taken while running` instead of
  `no clean shutdown recorded`.

- `keys`, `cheatsheet` and `[autoreload]` find `~/.tmux.conf` when there is no
  `~/.config/tmux/tmux.conf`. `keys-usage.tsv` compacts to counts past 5000
  lines.

- `rust-version` is 1.95, what the locked dependencies build on (`sysinfo`
  0.39.3); the source itself needs 1.88 for let-chains. The MSRV job checks
  with the lockfile in place instead of regenerating it.

- `resurrect` and `run_theme` moved out of `cli.rs` into `sessions/cli.rs`
  and `theme/cli.rs`; `cli.rs` is 850 lines shorter. `CLAUDE.md`'s module
  table is regenerated from the tree and its build section names the recipes.

- The reference docs match the tool: `configuration.md` has a section for
  every table, including `[sessions]` and `[[restore.program]]`, which had
  none, and no longer tells you to turn on the deprecated `[autosave]`. The
  five port-planning documents and `coverage.md` moved to `docs/dev/` with a
  banner.

### Removed

- `[git] branch_tail_len`. It was documented and never read; the tail the
  ellipsis keeps is fixed in `segments/git.rs`. `[git] branch_max_len` is now
  read when the `--branch-max-len` flag is absent, which it never was.

### Deprecated

- Top-level `autosave` says so in its help and reports status with no flags.
  `[sessions] autosave` is the timer to use; `config check` has said so since
  0.2.0.

### Fixed

- `[autosave] script = "~/..."` failed every run with "is missing" while
  `ls` found the file. See the top of this entry.

- Opening a project typed `"reattach-to-user-namespace -l /bin/zsh"` into the
  new shell, where the quotes made it one word. Builds before 93d60a9 saved
  tmux's `default-command` as every window's program; those files are now
  read as no layout, so the project opens from its `[[layout]]` instead of as
  bare shells.

- `M-a` did nothing with two windows of the same name and cycled wrong with
  three.

- A first `project` open printed tmux's `can't find session` before
  succeeding.

- `doctor` says `run tmux-companion restart` on a version mismatch and when
  `config.toml` is newer than the daemon.

## 0.2.0 - 2026-09-24

### Changed

- The pickers draw their own screen instead of handing it to skim. skim has
  `border_label`, `preview_label` and `footer` fields and accepts the fzf flags
  that set them, so it looks for a while as though it can draw what fzf draws;
  all three are private, carry `#[builder(setter(skip))]` and
  `arg(hide = true)`, and appear nowhere in its drawing code. They are there so
  an fzf command line parses, not so it renders. The matching is now
  nucleo-matcher, which was already a dependency and used nowhere, and the
  drawing is ratatui, which was already there for the colours.

  What that buys is `[picker]`: the border, where the picker's label sits on
  it, which end the hint line and the query are at, which end the list starts
  from, the counter, the rules, the cursor marker, the column order, and the
  preview's side, share, border and label. Each of them can be written again
  under `[picker.keys]`, `[picker.project]`, `[picker.window]`,
  `[picker.theme]`, `[picker.run]` or `[picker.open]` for the one picker that
  wants a different answer, along with the words that picker says. How big the
  popup is stays with `display-popup` in tmux.conf, which is the only place
  that knows.

### Added

- The playground image has a description on Docker Hub, pushed from the release
  rather than pasted into a web form once. It is the page anybody who finds the
  image lands on first and it was empty, and it is the one thing about the
  image that cannot ship inside it, because it lives in Hub's own database. It
  names the two things a stranger otherwise finds out the hard way: run the
  container from a terminal that is not already in tmux, or ctrl-b reaches
  their own server and every binding in the tour looks broken, and the font is
  the one thing the image cannot supply.

- The theme picker previews a theme rather than listing it. The card is the
  three colours it is built from, a ramp showing where the main one can go, and
  then the four places tmux actually paints: the status line, a message, a
  copy-mode selection and the pane borders. A list of `@theme-color-*` values
  answers none of the question somebody opens a preview to ask, which is
  whether the text on that background can be read. The values follow the card
  for the person editing the file rather than choosing from it.

- `open --choose`, or `-i`, asks which application opens what it found, from
  `[[open.application]]`. A browser is launched and left alone and an editor
  gets a pane beside the one you are in, which is what `pane` decides.

- `theme apply --all` repaints every session. Sourcing tmux.conf resets the
  global options a theme sets, so a reload without this leaves every session
  wearing whatever the file says instead of its own colour.

- The exit dialog after a `run` command is a popup with three buttons, moved
  between with the arrows or Tab and picked with Enter, rather than a line of
  bracketed letters. `q` closes it alongside `c`, and Esc leaves the pane open
  and read-only so nothing is lost while you work out what happened. The
  one-line prompt is still there for a client too small or too detached for a
  popup, which is when `display-popup` fails.

### Changed

- `[picker]` is a layer rather than a set of values. A setting nobody wrote is
  now telling apart from one somebody wrote to the value that happens to be the
  default, which it was not: writing `preview_label_position = "bottom-center"`
  under `[picker]` quietly stopped every picker using its own answer for the
  other settings too. Four layers now, narrowest last: the tool's defaults, the
  shape that picker is built for, `[picker]`, then `[picker.<name>]`.

- Each picker has its own preview shape before anybody configures it, because
  what goes in a preview is not the same thing twice: three lines of tmux
  command, a screen of whatever another session is doing, a directory listing,
  a theme card, nothing at all. `keys` gets 30% underneath, `project` 80%
  beside, `window` 40% beside, `theme` 70% beside and framed, `run` none.
  `[picker] preview` and `preview_percent` are now unset by default and
  override all six at once when written, which is almost never what anybody
  wants; `[picker.<name>]` is the place.

- `preview_border` says how much of a border the preview gets — `none`, `edge`
  or `full` — rather than whether it gets one. fzf has the same three, and
  which is right depends on what is in the pane: a directory listing wants a
  divider, a theme card wants a frame.

### Fixed

- `just act-ci` runs clippy and the tests rather than quietly skipping them.
  act skips a job whose runner platform it has no mapping for, and skips a
  matrix job whole, so naming only `ubuntu-latest` meant the one job worth
  running was dropped while rustfmt and rustdoc passed and the run ended green.
  Both labels are mapped now, to the same Linux image: what act checks is that
  the workflow's steps are right, and the platform difference is what the real
  runner is for.

- The test suite writes its themes into its own sandbox instead of into
  `$HOME`. `themes_dir` resolves from `HOME` as well as `XDG_CONFIG_HOME`, and
  the harness set only the second, so with a `~/.tmux.conf` present every test
  wrote into one shared directory in the home of whoever ran them. On GitHub's
  ubuntu runner, which has that file where this laptop does not, whether a
  theme was in place depended on which other tests had run; it read as a tmux
  3.4 difference for two pushes and was not one.

- The theme card draws a theme in its own colours rather than in black. It read
  `@theme-color-black` and `@theme-color-secondary`, two names carried over
  from the shell script it replaced and written by no theme file the generator
  produces, so every sample fell back to literal black on the theme's own
  background. On a dark theme that is black on `#00005f`, a contrast ratio of
  1.1 to 1: the samples exist to show whether the text can be read and they
  could not be read. It reads `@theme-color-on-main` and `@theme-color-border`
  now, which are what `theme gen` writes and what `_apply.tmux` uses, and
  computes a readable colour when a theme carries neither.

- The theme card's two pane-border samples are the colours tmux will draw.
  `_apply.tmux` gives the active border `@theme-color-border` and the inactive
  one `@theme-color-secondary`, which it pins to colour242 with its reasoning
  beside it. The card had them the wrong way round and drew the active border
  in the main colour, which on a dark theme is very nearly the background, so
  the two lines looked alike and both were wrong.

- A preview is clipped at the pane edge rather than reflowed. The theme card's
  sample bars are built wider than any pane on purpose, so they reach its edge,
  and wrapping folded the overhang onto the next line as a stray block of
  colour. A preview is either a capture of somebody else's screen, which is
  already the shape it wants, or a card drawn to a width it chose before the
  pane existed; neither is improved by reflowing.

- The theme card says how readable the text is, as the WCAG ratio, because a
  pair of colour names does not answer the only question anybody opens a theme
  preview to ask. Below 4.5 to 1 it says so.

- The arrows mean down and up the screen. With `list_from = "bottom"` the list
  is drawn in reverse, so the index meaning "further down" is the smaller one,
  and Up moved the cursor down in every picker. Paging had it too.

- A preview beside the list stays beside it unless the list would be genuinely
  too narrow to read. The rule was a popup-width constant of 96 columns, and
  tmux's own default popup on a 170-column terminal is 83, so a theme list
  configured to sit beside its card was stacked above it instead. What decides
  it now is the columns the list keeps rather than the width of the popup --
  the same popup leaves 37 columns at a 55% preview and 25 at 70% — and
  `min_list_width` is the setting, zero being fzf's behaviour of splitting
  whatever it is given.

- The record of a config the daemon refused to start on is removed when one
  starts cleanly. It was written on a bad start and never taken away, so
  `doctor` went on reporting a parse error somebody had already fixed, and a
  client that failed to connect for an unrelated reason blamed the config.

- An empty query keeps the rows in the order they were built. They were being
  ranked, and with nothing to match on the ranking fell through to row length,
  which scattered the live sessions through the directories in the project
  list. That order is the whole of what that list is: the sessions first, then
  the directories by how often they are visited.

- The watchdog notices a socket that was replaced on Linux. It compared device
  and inode, and Linux hands an inode number straight back out after the file
  using it is unlinked, so a new socket at the same path came back looking
  like the old one. The inode's change time is in the comparison now. macOS
  does not reuse them that eagerly, which is why this passed locally and
  failed on the Linux runner.

- A picker with `border = "none"` keeps the frame round its preview. The line
  was taken from the outer border, so turning that off — which the theme
  picker does, because its popup already has one — silently took the preview's
  box with it.

- `tmux-companion --help` opens with a description of the tool. clap takes a
  doc comment's first line as `about` and the rest as `long_about`, so the note
  above `Cli` — which is addressed to whoever is editing that file, and is
  about why `--version` reports a build stamp — was what the help opened with.
  The first thing somebody saw after installing it read like somebody else's
  memo. `about` is now the sentence the manual's `.Nd` already used, and three
  tests keep the doc comment out of both the short and the long help.

- The directory list a new window opens at previews what is in the directory.
  The preview was the path, which is what the row already says: a pane of one
  line repeating the line you are looking at.

- A preview beside the list carries its label on the outer border, under its
  own columns, rather than inside the pane. Inside, it landed on the last line
  of whatever was being previewed and read as attached to nothing.

- Themes sort by file name rather than by the name they carry, so a theme and
  its lighter and darker siblings land together: `amber-light.tmux` sorts
  before `amber.tmux` because `-` is below `.`, where sorting by "Amber Light"
  and "Amber" scattered a family across the list.

- Daemons accumulated instead of exiting, one per test run, benchmark or
  recording. Two bugs, and either alone was enough. The daemon unlinks the
  socket file before it binds, because that is how a crashed predecessor's
  file gets cleared, and unlinking is exactly what makes `AddrInUse`
  unreachable: two starts racing each other both passed the connect check,
  both unlinked, and both bound, with the second unlink taking the first
  daemon's socket file away. And nothing bounded a daemon's life, so a harness
  that pointed `TMUX_COMPANION_SOCK` at a sandbox and then deleted the sandbox
  left a daemon holding an unlinked inode with no way to reach it and no
  reason to stop. A start now takes an exclusive lock on `<socket>.lock`
  before it touches the socket file, and a daemon exits once the socket file
  it bound is gone or has been replaced.

- `__shutdown` removes the socket file on its way out. A daemon that exited
  and left the file behind read as a live daemon to anything that stats the
  path rather than connecting to it, and an interrupted suite left one such
  file per test.

### Added

- `scripts/reap-daemons.sh`, and `just reap`, which kills the daemons nothing
  can reach any more and leaves the live one alone. It classifies by the
  socket a process holds rather than by its command line, because
  `pkill -f 'tmux-companion server'` matches the live daemon too, and a
  leftover socket file does not mean a live daemon, so each one is probed by
  connecting rather than stat'ed. `just test` runs it on the way out however
  the run ends, since the runs that leak a daemon are the ones that failed or
  were interrupted.

- The end-to-end tests told the tool which config to read and which daemon
  socket to use, but never which tmux to talk to, so every tmux command the
  binary ran under test went to the default socket rather than to the server
  the test had started. `scripts/repro-ci.sh` runs the suite under the CI
  job's conditions and is what found it.

- A broken config reached the person as `Connection reset by peer (os error
104)` instead of the name of the key that was wrong, on a machine loaded
  enough to lose a race. The daemon bound its socket before it parsed the
  config, so it was reachable for as long as the parse took: a client that
  connected inside that window was accepted and then dropped when the daemon
  gave up, and the client only consulted the recorded error when the _connect_
  had failed. The daemon parses before it binds now, so a refusal leaves no
  socket at all, and a connection that dies mid-request is explained by the
  daemon's own last words wherever it left any.

- `project save` and the save `close-project` does on the way out are all or
  nothing. Three things could each lose part of a layout while reporting
  success: a pane or window line tmux answered with that did not parse was
  dropped silently, so a smaller layout replaced a larger one; a tmux command
  that failed outright returned an empty string, which parsed as a session with
  no windows and was written over a good file, and because a saved layout wins
  over the config that empty file then shadowed the `[[layout]]` the project
  used to open with; and the write truncated the old file before writing the
  new one, so an interruption left half a layout. Now a failed tmux read is an
  error, a line that cannot be parsed is quoted back and nothing is written, a
  layout with no windows is refused, and the file is written under a temporary
  name and renamed over the old one. A layout file with no windows that is
  already on disk reads as no layout rather than as an empty one.

## 0.1.0 - 2026-09-23

### Added

- Released binaries for four targets: macOS on Apple silicon and Intel, Linux on
  ARM and on Intel or AMD. The Linux pair link statically against musl, so one
  binary runs on any distribution rather than tracking a glibc version.
- `scripts/install.sh`, which picks the archive for the machine it runs on,
  verifies it against the release's `checksums.txt`, and installs it. It refuses
  rather than warns when the checksum does not match, and falls back to building
  from source when no release fits.
- `tmux-companion.tmux`, so tpm can install it. It binds no keys and sets no
  options.
- `--version`, reporting the build stamp the daemon handshake compares rather
  than the crate version alone.
- `.github/workflows/release.yml`: a tag builds the four binaries, and
  publishing waits on the `release` environment so an admin approves first.
- [docs/how-to/install.md](docs/how-to/install.md), covering all three ways in
  and how to remove it again.
- [docs/BENCHMARKS.md](docs/BENCHMARKS.md) and `just bench`, which measure from inside a
  real tmux against the zsh this replaced: a key is pressed, tmux runs the
  binding, and the clock stops when the first row reaches the terminal. The bar
  is 3.0% of a core against 34.4%, and the pickers cost half the CPU while
  opening no faster, which is the number the old socket-side measurements could
  not see. The port's own before and after numbers move to
  [docs/BENCHMARKS-before-port.md](docs/BENCHMARKS-before-port.md).

- Panes in a layout window. `[[layout.window.pane]]` with a command, a cwd and
  a focus flag, under a `layout` naming one of tmux's five presets or carrying
  a raw tmux layout string, so a window you arranged by hand can be pasted in
  rather than described in a new syntax.
- `project save`, `project forget` and `project show`, which capture the
  session you are in as that project's layout and say which file decided. A
  saved layout wins over `[[layout]]`. `close-project` captures on the way out,
  before anything is asked to quit.
- `shell-init` for zsh, bash and fish, printing the OSC 133 prompt marks that
  tmux's `next-prompt` and `previous-prompt` have been waiting for since 3.3.
- `[git.autofetch]`, fetching the repositories the bar has drawn so ahead and
  behind mean something. Off by default, since it is the only part of this that
  touches a network.
- `[autoreload]`, sourcing tmux's config when it changes, over the file the
  daemon already stats for `keys`.
- `[notify]`, saying when a long command finished in a pane you were not
  looking at, with tmux's own `display-message` as the notifier so nothing has
  to be installed.
- `[window_names]`, naming windows from the `[[sh_jobs.job]]` table. It never
  takes a name away from a window somebody pinned.

- `keys` and `cheatsheet`, replacing the fzf-driven pair. The rows are parsed
  from `tmux list-keys` and cached against the config's mtime, so six
  `list-keys` calls happen once per config change rather than once per
  keypress. Warm, `keys --print` measures 8.3 ms against 18.8 ms for the
  equivalent `fzf --filter`.
- `project`, replacing four scripts: live sessions newest-attached-first, then
  the directories zoxide knows, coloured through the project map.
- `run`, which picks from shell history into a pane that slides out beside you
  and offers Close, View or Restart when the command exits, and whose history
  parsing is byte-identical to `fc -ln` over all 1127 commands after three
  attempts that each got zsh's format wrong in a different way.
- `theme pick`, `theme apply` and `theme gen`, which drops the python3
  dependency. Every one of the 76 themes resolves to the same colour the Python
  computed.
- `open`, `close-project`, `toggle`, `autosave`, `clipboard`, `zoom` and the
  two `probe` subcommands.
- `doctor`, which prints what an issue reporter would otherwise be asked for one
  question at a time.
- A TOML config file: glyph presets for readers without a Nerd Font, an ordered
  `[git] parts` list, a configurable `[status.right]`, `[[layout]]` for what a
  project session starts with, and an off switch on the usage log.
- `open` finds what is under the cursor. The copy-mode binding passes
  `#{copy_cursor_line}` and `#{copy_cursor_x}`, so `o` opens the path or URL
  the cursor is on without selecting it first, and a line naming two paths
  opens the one you are actually on rather than whichever came first.
- `[project] dirs_source`, so the project picker and `new-window` list
  directories from `z`, zsh's own `cdr` or `ghq` as well as zoxide, and
  `[project] dirs_command` for anything else that prints one absolute path per
  line. `z` and `cdr` are read as files rather than run as commands, because
  both are shell functions. `[project] visit_command` is the matching write.

### Changed

- `theme gen --shades` writes every colour in tmux's 6x6x6 cube whose text
  clears a contrast rung rather than one lighter and one darker sibling of each
  theme already on disk, each named after the colour it sits nearest to so
  `ember-04` and `pine-11` group in the picker. It answers "show me what there
  is" instead of "vary what I have", and it no longer depends on which files
  happen to be in the directory. The rung is the flag's value: `aa` for 216
  themes, `aaa` for 151, `a4` for 105, `a5` for 75 and `a6` for the original
  eighteen. On its own the flag means `aaa`, because the worst colour in the
  cube scores 4.60:1 and an AA filter therefore keeps all 216 and removes
  nothing. An unknown rung is an error rather than a silent fall back, since a
  typo that generates 145 files instead of 18 is a directory somebody cleans up
  by hand.

- `vim-bg` is `sh-jobs`, with a config-driven job table rather than one
  hardcoded editor. The old name works for one more release and says so.
- zoxide is documented as optional rather than assumed. It was already possible
  to run without it, and the picker already degraded to live sessions and a
  typed path, but nothing outside `config.example.toml` said so -- `[project]` is
  now covered in the manual, `docs/reference/requirements.md` and the README.
- `autosave` runs in the daemon instead of a detached shell loop, which removes
  its PID lock file, the stale-lock takeover and the liveness check between
  sleeps.
- Every request and response carries a build id, so a client talking to a
  daemon from an older build replaces it rather than quietly getting an older
  answer.
- The socket is created 0600 and its owner is checked before a client connects.

### Deprecated

- `zoom` is `zen`. The name described half of what it does: with other panes it
  zooms, and with none it takes the status bar, because a lone pane already
  fills the window and tmux's own `prefix z` does nothing there. Both halves
  mean "clear everything but what I am working on", which is one idea and now
  has one name. The old name works for one more release, and the binding is
  still `prefix z`.

- `[project] zoxide`. It is the old spelling of `dirs_source = "none"`, still
  works, still wins over everything else in the section, and goes away in the
  next release. `config check` names it, so nobody meets the removal first.

### Fixed

- `shell-init zsh` emitted a prompt mark that tmux then threw away, so
  `previous-prompt` and `next-prompt` did nothing on zsh -- the shell this was
  written on and tested with. zsh has `PROMPT_SP` and `PROMPT_CR` on by
  default: after `precmd` returns it prints a partial-line indicator and a
  carriage return and redraws the prompt line, taking the mark recorded against
  that line with it. The mark now goes in `PS1`, where the redraw cannot reach
  it. bash was never affected, because it has no such redraw.
  `scripts/check-prompt-marks.sh` drives a real tmux and fails when the cursor
  does not move, which is the only thing that catches this: the hook installs,
  defines its functions and prints every byte either way.
- `[run] history` defaulted to `zsh`, so on bash the command picker read a
  `~/.zsh_history` that was not there and came up empty with nothing said. It
  defaults to `auto` now and reads whichever shell `$SHELL` names.
- `[run] shell` defaulted to `zsh`, so on a machine without zsh -- most Linux
  boxes -- the pane slid out and the command never ran. Empty now, meaning
  `$SHELL`.
- `open` and `zoom` acted on the wrong pane. Both ran their tmux commands with
  no target, and tmux then resolves "current" as the most recently used session
  on the server, so with a second session touched more recently the editor
  opened in a window nobody was looking at and the zoom key moved a pane on
  another screen. `$TMUX_PANE` is no help here: tmux runs these from bindings
  through `run-shell`, where it holds the most recently active pane on the
  server and `run-shell -t` does not change it. Both commands now take
  `--pane`, and the shipped bindings pass `#{pane_id}`.
- `new-window` created its window in whichever session the server had used
  last, for the same reason. It targets its own session now.

- `TMUX_COMPANION_SOCK` that is empty or too long for a unix socket address now
  exits 2 instead of warning and connecting to the default socket, which meant
  a harness asking for an isolated server quietly got the live one.

- `Duration::from_secs_f64` panicked on a negative or non-finite `--ttl`, which
  arrives from a client.
- A misspelled argument used to read back as `None` and change behaviour
  silently. Arguments are typed structs on both ends of the wire now, and an
  unknown field is an error naming the field.
