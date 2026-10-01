# Design: one key, routed to the layer that should have it

Status: plan drafted by Claude on 2026-10-01 from a session in `mysetup`, review gaps closed and all six steps built the same day. The tmux hold is spiked and works on 3.7c (see Spike). The first publisher, for nvim, is built in `mysetup` (`home/.config/nvim/lua/keyroute/`, plan in `home/.config/nvim/KEY_ROUTING_PLAN.md`); nothing in tmux-companion is built. This file owns the pane-option contract, and every publisher, nvim's included, is written against it.

## The problem

Some of the keybindings in tmux, nvim, fzf and claude etc. collide. Some of them are intentionally like search, next occurrence etc. those are muscle memories that should stay as is across apps but tmux being the top layer should handle it with grace and apps in tmux can't tell tmux what to do on runtime so this is one directional problem.

What's true today:

- tmux's root table sees a key before the pane does so `M-a`, `M-A` and `M-s` never reach nvim or fzf. fzf-lua's default `alt-a` is toggle-all, and it switches the tmux window instead. (As of 2026-10-01 `mysetup`'s fzf-lua replaces the defaults, so `alt-a` isn't bound there; `keys collide` shows it.)
- vim-tmux-navigator shares `M-h/j/k/l` by running `ps` against the pane's tty on every press and it only knows that nvim or fzf is running, not whether it wants the key.
- Keys are aligned by hand. The comment above `M-a` in `mysetup`'s tmux.conf lists the four layers that were checked (nvim, claude, zsh vicmd, tmux) and nothing re-checks them when one of those configs changes.
- The goal is one key meaning one thing across layers, e.g. `M-1` opens nvim-tree in nvim and a session > window > pane tree in tmux, aligned on purpose as more shared keys turn up.

## What tmux-companion owns and what it doesn't

The feature has to work for somebody who installed from brew and has never seen `mysetup`, and for an app nobody has thought of yet (nano, a REPL, a TUI). So tmux-companion owns everything app-agnostic:

- the pane-option contract and the `keys claim` / `keys release` commands that write it
- discovery: finding out what each app binds without the app doing anything new (see Discovery)
- static claims per app, from discovery or declared in config, which is all an app with no publisher ever needs
- the collision report and the router

No app has to change its behaviour for any of that. A publisher, a piece of code inside an app that keeps its claims current by mode, is the one optional extra, and its only job is setting `@kc_claim`. It lives with its author: nvim's lives in `mysetup` and is the reference implementation, and may become its own public repo later. The nvim discovery adapter is tmux-companion code that talks to nvim's existing API; nothing else in tmux-companion knows about any publisher.

## The routing rule (decided)

| Who claims the key in this pane right now | What happens |
| --- | --- |
| only the app | app gets it, no wait |
| only tmux | tmux runs its binding, no wait |
| both | tmux holds it for `hold_ms` (default 170, configurable). Timeout runs the tmux binding. A second press inside the window sends one key to the app. Any other key inside the window counts as a timeout: the tmux binding runs, then that key is delivered |
| neither | passes through as today |

Scope is every key tmux's root table binds in `M-*`, `C-*` and `F*`, with any of the `S-` and `C-`/`M-` modifiers combined (`S-F5`, `C-M-a`). The prefix key is excluded: it's not a root binding and can't be wrapped. Mouse keys and `User*` keys are out.

Routing is off by default, like `[autoreload]` and `[usage]`: it rewrites the root table, and that isn't something to do to somebody's bindings without being asked. It needs tmux 3.4 for `send-keys -K`; below that `keys route` writes nothing and says why, and `doctor` reports whether routing is on, the tmux version it needs and how many keys are wrapped. The rest of the tool keeps its 3.2 floor.

## Pieces

### Registry: what each key is meant to do

Two places, same schema, and both count:

- a `[keys]` section in `config.toml`, for somebody routing one or two keys
- `~/.config/tmux-companion/keys.toml`, the same tables without the `keys.` prefix, for a map that spans tmux and several apps

They merge. The same key, app or default defined in both is a `config check` error that names both places, so nothing is shadowed without a word. Both go through config.rs: `deny_unknown_fields`, `config check`, and `restart` refusing a config it can't read. Moving a table from one file to the other is a cut and paste and a prefix.

```toml
# config.toml spelling; keys.toml drops the "keys." prefix
[keys]
route = false             # the router is off until this is true
hold_ms = 170

[keys.key."M-1"]
intent = "tree"           # a label for the report, nothing reads it
tmux = "choose-tree"      # drift check: a substring of the binding's note, then its command
nvim = "NvimTreeToggle"   # the same, against the discovered row's desc, then its rhs

[keys.key."M-a"]
intent = "next"
route = "hold"            # hold | tmux | app; hold is the default when both claim
hold_ms = 200             # per-key override

[keys.key."M-h"]
route = "app"             # navigator keys: the app gets it and hands off to tmux itself

[keys.app.nvim]
modes = ["n"]             # which discovered modes count as static claims; normal is the default for modal editors

[keys.app.nano]
claims = ["C-o", "C-x", "C-w"]   # declared on top of what discovery reads from nanorc

[keys.app.claude]
claims = ["C-g"]          # counts while `keys claim --app claude --owner` named the pane
```

`route = "tmux"` means tmux always wins and the app's claim is reported as a collision. `route = "app"` means the app always wins while it claims the key, which is what vim-tmux-navigator does today without the `ps`.

Drift is a case-insensitive substring match: the registry's value against the discovered row's note or desc first, then its command or rhs. An exact match breaks on every reworded desc.

Shipped defaults cover `fzf` (fzf's own binds) and `claude` (a short hand-kept list, see Discovery). Config extends a default and never has to repeat it.

### Discovery: what each layer actually binds

`tmux-companion keys discover [--layer L]` writes `keys-discovered.json` under the state dir, one row per (layer, mode, key, description, source, rung). The file is tmux-companion's own cache, read by `keys collide` and `keys route`; no app reads or writes it.

No app is asked to produce anything. Each app has an adapter in tmux-companion that tries three rungs, best first, and records which one answered:

1. **Live.** Ask a running instance through an interface it already has. It sees maps made at runtime, which a config read can't. Only a fixed list of read-only calls is ever sent, never an expression from config or the command line.
2. **Config.** Read the app's own config the way the app does, over a shipped table of its defaults, or start the app headless with the user's config and ask it, when nothing is running.
3. **Declared.** `[keys.app.<name>] claims` in the registry, for an app with no adapter. Declared keys are added to whatever rungs 1 and 2 found, for any app.

| Layer | Live | Config | Notes |
| --- | --- | --- | --- |
| tmux | `keys::collect()`, reading through wrappers | | root, prefix, my-keys, copy-mode-vi. A wrapped key reports the binding in `kc-tmux` with its own note |
| nvim | the running instance's socket: `nvim --server <sock> --remote-expr` calling `nvim_get_keymap` per mode | `nvim --headless` with the user's config | tested on 0.12.5 against a live editor: 13 ms, no plugin, and it returned maps made in a plugin's `config()` |
| vim | | `vim -Es` with the user's vimrc, `:map` per mode, parsed | vim has no socket unless built with clientserver |
| emacs | `emacsclient --eval` when its server is up | | |
| claude | | `~/.claude/keybindings.json` over a shipped default list | the list holds only claude's defaults in scope (`M-*`, `C-*`, `F*`), unversioned; `doctor` names the claude version it was last checked against |
| fzf (shell) | | `FZF_DEFAULT_OPTS` `--bind` over fzf's own defaults | as the discover command's environment sees it, so it runs in the client, not the daemon |
| nano | | `bind` lines in `~/.nanorc` and `/etc/nanorc` over nano's defaults | |
| zsh | | `zsh -ic 'bindkey -L -M emacs; bindkey -L -M viins; bindkey -L -M vicmd'` with a timeout | report-only, see below |
| any other app | | | rung 3 only |

Finding nvim's socket: the pane's `pane_pid` is the shell, `pane_current_command` names the nvim UI process under it, and since nvim 0.10 the API is served by its `nvim --embed` child. The socket is `nvim.<embed pid>.0` under `$XDG_RUNTIME_DIR` on Linux or `$TMPDIR/nvim.$USER/*/` on macOS. An nvim started with `--listen` somewhere else isn't found, and the adapter drops to rung 2. Every open nvim pane is asked, so buffer-local maps from different projects all show up in the report, each against its pane.

Discovery runs on demand, in the client. The daemon doesn't spawn editors or shells or connect to them: nothing about tmux.conf's mtime says an editor's maps changed, and a headless editor with a full config can install plugins or go to the network. `keys route` reads the last `keys-discovered.json` rather than discovering again, so a newly added nvim map counts once `keys discover` has run.

What discovery found is also a static claim list for routing. For each app, the keys from the modes in `[keys.app.<name>] modes` count as claimed whenever the app is in front: normal mode by default for a modal editor (nvim, vim), every mode for anything else. So an nvim with no publisher still gets `M-1` held, with no plugin installed. The cost is over-claiming: `M-1` is held in insert mode too, although only normal mode maps it. A publisher removes that by telling tmux the claims for the mode it's actually in.

zsh is report-only. It's the command in every idle pane and it binds most of `C-*`, so if its keys counted as claims every tmux key it shares would wait 170 ms at the prompt. It shows up in `collide` so a collision is visible, and tmux always wins.

### Report: where they disagree

`tmux-companion keys collide` opens a picker (the `keys` picker's style) with three groups:

- collisions: a key claimed by tmux and one or more apps, with the route the registry gives it, or "unrouted"
- drift: the registry says `M-1` is `NvimTreeToggle` and discovery finds something else, or nothing
- free: keys in scope nobody claims, for when a new shared action needs a home

`--json` for scripts. This is the discovery tool that feeds alignment decisions so it's the first thing worth shipping.

The commands are subcommands of `keys`: `keys discover`, `keys collide`, `keys route`, `keys claim`, `keys release`. A bare `keys`, with or without its flags, still opens the bindings picker.

### Router: generated tmux bindings

tmux.conf ends with:

```tmux
run-shell "tmux-companion keys route"
```

`keys route` reads the live root table, so it sees tmux.conf's binds as they are after this source, plus the registry and the static claims. It applies the wrapping straight to the server, with no generated file to source. A file would hold a copy of each original command, and the next source of an edited tmux.conf would put the old command back over the new one until something regenerated it.

Every root key in scope is wrapped, not only the ones discovery or the registry knows. A key costs one format evaluation when nobody claims it, and a key an app only claims at runtime (fzf-lua's `alt-a`, through the publisher's `want()`) would otherwise never be held. The registry only overrides the route and `hold_ms`.

A wrapped binding carries its original note, so the `keys` picker, `cheatsheet` and `keys --unused` keep finding it. `keys::collect()` reports the binding in `kc-tmux`, never the wrapper. The router recognises its own wrapper by its shape (a root binding whose command switches to `kc-tmux`), not by a note prefix, so a second run doesn't wrap twice. When tmux.conf is sourced again, root is back to the originals and they get wrapped fresh. A key that leaves the scope, or a registry route set to `tmux`, gets its original binding moved back to root.

With `route = false`, `keys route` unwraps whatever it wrapped before and exits.

The decision is made with format conditions and no fork. This is the shape the spike runs on tmux 3.7c, for `M-a` with a 170 ms hold:

```tmux
# the original binding from tmux.conf, moved untouched into a table of its own
bind -T kc-tmux M-a run-shell "tmux-companion toggle '#{session_name}'"

bind -n M-a {
  if -F "#{&&:#{==:#{pane_current_command},#{@kc_owner}},#{m:*|M-a|*,#{@kc_claim}}}" {
    switch-client -T kc-hold-M-a
    set -gF @kc_gen "#{e|+:#{@kc_gen},1}"
    set -gF "@kc_hold_#{client_pid}" "#{@kc_gen}"
    run-shell -b -d 0.17 -C "if -F '##{&&:##{==:##{client_key_table},kc-hold-M-a},##{==:##{@kc_hold_#{client_pid}},#{@kc_gen}}}' { switch-client -c '#{client_name}' -T kc-tmux ; send-keys -c '#{client_name}' -K M-a }"
  } {
    switch-client -T kc-tmux ; send-keys -K M-a
  }
}
bind -T kc-hold-M-a M-a { switch-client -T root ; send-keys }
bind -T kc-hold-M-a Any { switch-client -T kc-tmux ; send-keys -K M-a ; send-keys -K }
```

The condition above is the dynamic-claim half. The generated condition ORs in one term per static claim: `#{==:#{pane_current_command},nano}` for an app named by its process, `#{==:#{@kc_app},claude}` for one named by its hook, baked into the wrapper for each key in that app's static list (discovered from the configured modes, plus declared). A dynamic `@kc_claim` replaces the static list for its owner while the owner is in front: the static term is ANDed with `#{==:#{@kc_claim},}`, so it only counts when no publisher has spoken.

What each part is for, all of it checked in the spike:

- The original command never gets copied into a string. `send-keys -K M-a` with the client in `kc-tmux` runs it as a real key press, so its formats expand when it runs, with the client and pane it would have had, and nothing needs escaping. The router only has to move a binding, not rewrite it.
- `run-shell -b`. Without `-b` the delayed command blocks the client's queue, so the second press waits behind the timer and lands after it, which turns every double press into two tmux actions.
- `##{` in the timer. `run-shell` expands its whole string when it starts, which is at press time. A single `#` there would read the key table as `kc-hold-M-a` every time. The doubled ones survive to the timer; the single `#{client_pid}`, `#{client_name}` and `#{@kc_gen}` are meant to expand at press time.
- `@kc_gen` stops an old timer from firing into a new hold: `M-a M-a` then `M-a` again 100 ms later left the first timer firing while the third press was being held.
- `@kc_hold_<client_pid>` and the timer's `-c`. A single global `@kc_gen` let a second client's press void the first client's timer: the first client stayed in `kc-hold-M-a`, and its next key, whenever it came, ran the `M-a` action late. tmux expands formats in an option's name, so each client keeps the generation of its own last hold, and the timer names the client it acts on.
- `send-keys` with no key, in the hold table, sends the key it's bound to, so the double press delivers one `M-a` to the pane.
- `Any` plus a bare `send-keys -K` is how another key keeps its order. A non-root table drops a key it doesn't bind, it doesn't fall back to root. `Any` catches it, runs the tmux action, then sends the same key back through the client's key table, which is root again, so `M-a` then `M-s` ran the `M-a` action and then the `M-s` binding.

The `#{==:#{pane_current_command},#{@kc_owner}}` guard means a claim left behind by an app that crashed or was suspended with `C-z` stops counting as soon as the shell is back in front. The spike checked it with `@kc_owner=nvim` on a pane running something else: no hold, straight to tmux.

A pane in copy mode never reaches root for a key copy mode doesn't bind, claim or no claim, so the wrapper doesn't run there and needs no `pane_in_mode` term.

### Claude and other apps named by a hook

fzf is easy since `pane_current_command` reads `fzf`. Claude isn't: today's panes report its version, `2.1.283`, as the command. `agent hooks claude` already wires hooks in, so a `SessionStart` hook runs `tmux-companion keys claim --app claude --owner` and `SessionEnd` runs `keys release`.

`--owner` also sets `@kc_owner` to the pane's `pane_current_command` as tmux reads it at that moment, the version string, so the owner guard covers claude the way it covers nvim: a claude that is killed and never runs `SessionEnd` stops counting once the shell is back in front. Without that, a stale `@kc_app` would hold claude's keys in a plain shell.

The hook is the only supported way for now. If it turns out to be too permissive, matching `pane_current_command` against a version number (the rule `panes.rs` already uses for `VERSION_NAMED`) is the fallback, with no hook.

## Pane-option contract

| Option | Set by | Value |
| --- | --- | --- |
| `@kc_owner` | any publisher, or `keys claim --owner` | the `pane_current_command` tmux shows for the app while it's in front |
| `@kc_claim` | any publisher, or `keys claim` | keys claimed right now, tmux spelling, pipe-delimited with a leading and trailing pipe: `\|M-a\|M-1\|C-q\|`. Unset or empty means the app's static list applies |
| `@kc_app` | `keys claim --app` | a name from `[keys.app.*]`, for apps whose process name doesn't say what they are |

Keys use tmux's spelling (`M-a`, `M-A` for shift, `C-Tab`, `F5`). Converting from an app's own spelling is the publisher's job. A publisher takes the list of keys worth claiming from `tmux list-keys -T root`, which with routing on is exactly the wrapped set; there's no dependency on `keys-discovered.json`.

`keys claim [--app NAME] [--owner] KEY...` and `keys release` run in the client with no daemon round-trip. They spell and pipe the value, so a hook or a wrapper script never hand-formats it. A publisher on a hot path, like nvim's on every mode change, can set the options with `tmux set-option` itself; it costs the same one fork.

The contract gets its own section in `docs/tmux-companion.1`, since publishers are written against it from outside this repo.

## Hot path cost

A key only one layer claims costs one format evaluation, no fork. A held key adds one table switch and one `run-shell -b -d -C`, still no fork. That's an improvement on the `ps` fork vim-tmux-navigator does on every `M-h/j/k/l` press so moving those four keys onto the router with `route = "app"` is worth doing once the rest works.

A publisher's claim can lose a race with a key pressed right after a mode change. nvim's debounces 20 ms plus one fork, and `<Esc>` to an Alt key is about 120 ms by hand, so it hasn't been worth fixing.

## Spike, 2026-10-01

Run on tmux 3.7c, macOS. The harness nests two servers: an outer `tmux -L` whose pane runs an inner client, so `send-keys` on the outer pane arrives at the inner server as typed input and goes through its key tables, which `send-keys` straight at the inner server would skip. The inner pane records every byte with `dd bs=1` so the order of the tmux action (it types `[T]` into the pane) and the forwarded key is visible. A second outer window attaches a second inner client to the same session for the two-client case.

| Pressed | Claim | Pane got |
| --- | --- | --- |
| `M-a` | none | `[T]` at once |
| `M-a` | `\|M-1\|` only | `[T]` at once |
| `M-a M-a` | `\|M-a\|`, owner not running | `[T][T]`, the stale-owner guard |
| `M-a` | `\|M-a\|` | `[T]` after the hold |
| `M-a M-a` | `\|M-a\|` | `ESC a`, no `[T]` |
| `M-a` 120 ms `M-a` | `\|M-a\|` | `ESC a` |
| `M-a` 230 ms `M-a` | `\|M-a\|` | `[T][T]`, two holds that each timed out |
| `M-a x`, `M-a Enter`, `M-a C-c` | `\|M-a\|` | `[T]` then the key |
| `M-a M-s` | `\|M-a\|` | `[T]` then `M-s`'s own root binding ran |
| `M-a M-a`, 100 ms, `M-a` | `\|M-a\|` | `ESC a [T]`, the first timer saw a newer generation and did nothing |
| `M-a` on client 1, 50 ms, `M-a` on client 2 | `\|M-a\|` | `[T][T]`, each timer resolved its own client |
| copy mode, `M-a M-a` | `\|M-a\|` | tmux action ran 0 times; copy mode never reaches root |

Press to action measured 182 to 185 ms for `-d 0.17` over 6 runs, and 262 to 265 ms for `-d 0.25`, which includes the harness's own `tmux send-keys` exec and the nested client. So `hold_ms` is honoured to within the cost of measuring it.

Three dead ends on the way, all in the notes above: `run-shell -d` without `-b` serialises the second press behind the timer, single `#{...}` in the timer expands at press time, and one global generation strands a client when two hold at once.

Every row of the table is now an e2e test in `tests/e2e.rs` (the key-hold section), loading the shape from `tests/fixtures/key-hold.conf`. It uses the same nested-server trick, with a 400 ms hold and gaps that are fractions or multiples of it, so a loaded runner can't move a press across the edge of the window. Putting the global generation back fails the two-client test. The tests skip below tmux 3.4. The timing measurement above was a one-off and wasn't kept.

Not covered: mouse events reaching `Any` inside the window, and a real publisher's claims (nvim's is built; it hasn't been run against a generated route yet).

## Order of work

Each step lands with its paragraph in `docs/tmux-companion.1`, a CHANGELOG line, a row in `docs/reference/cli.md` and a line in `docs/dev/port-checklist.md`, and leaves `the_skill_names_no_command_that_went_away` green.

1. Done: `keys discover` for tmux, nvim (live socket, then headless) and declared `[keys.app.*]` claims, and `keys collide` as a picker. Useful on its own, with no routing.
2. Done: the spike as an e2e test, the cases in the table above against `tmux -L` servers of their own, using the nested-client trick since key tables need typed input.
3. Done: `keys route`, applied from the `run-shell` line at the end of tmux.conf, off by default, wrapping every routable root key; `keys claim` / `keys release`; the `doctor` line. Root's notes are read with `list-keys -a -N -T root`, since `-N -T root` without `-a` came back empty on a fresh 3.7c server whose root bindings carried notes; a key with no note lists its command there, so a note equal to the command is dropped. Keys with punctuation in their name are reported and left alone for now. Static claims by process name hold only while no publisher speaks for the app (`@kc_owner` isn't the app), and `@kc_app` claims count while the command the hook recorded in `@kc_owner` is in front.
4. Done: the registry in both places, with `route`, `hold_ms` and drift per key, and the duplicate check. A layer field is a free-form name, so `KeyEntry` takes unknown fields as layers rather than rejecting them; a misspelt `rout = "app"` shows up as drift for a layer called `rout` instead of an error.
5. Done: claude, fzf, vim, nano and zsh adapters, and the claude hook calling `keys claim --app claude --owner --quiet` on `SessionStart` and `keys release --quiet` on `SessionEnd`. nano ships no defaults: nothing installed lists them. fzf-lua's binds come from the live nvim, read from `package.loaded['fzf-lua.config'].globals.keymap.fzf`, which is the effective table; a setup `keymap.fzf` replaces fzf-lua's defaults unless its first element is `true`, which is why `alt-a` (toggle-all, a default) isn't bound in `mysetup`'s fzf-lua today.
6. Done 2026-10-01 in `mysetup`: vim-tmux-navigator's tmux half is out of tpm, `M-h/j/k/l` are plain `select-pane` bindings in root and copy-mode-vi, `keys.toml` routes them `app`, and `run-shell "tmux-companion keys route"` follows tpm as tmux.conf's last line. A publishing nvim claims them in normal mode, an nvim started before the publisher was loaded is covered by the static claim from discovery, and anywhere else, nvim's insert mode included, tmux moves at once. No `ps` on any press.

Tests: unit for the registry parser and the merge of both files, the spelling normaliser, each discoverer's parser, the wrapper recogniser and the condition generator. e2e for hold timeout, double press, other key inside the window, a stale claim after the owner exits, two clients holding at once, and a re-source of an edited tmux.conf picking up the new action.

## Left open

- Mouse events inside the hold window.
- nvim's own `:terminal` running claude or fzf. `mysetup`'s plan calls claude in an nvim terminal won't do; fzf in one is the publisher's call, since the terminal job's keys go through nvim's claim.
