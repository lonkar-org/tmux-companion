# Quiet follows Focus

Status: decided 2026-10-12 and built; this is the design record.
Problem: quiet hours are a timer and a daily schedule. macOS already knows when you don't want to be bothered, it's called a Focus and today tmux-companion doesn't know about it so a Work or Sleep Focus still gets a nudge from the inbox and a banner from `[notify]`.

The goal is one line of behaviour: when a Focus is on tmux-companion is quiet, when it ends it isn't. On Linux the same setting follows the desktop's Do Not Disturb, see "Linux" below.

## How a program can know the Focus

Checked on macOS 26.6 on 2026-10-12, read-only. Nothing in System Settings, TCC, Shortcuts or Focus was changed.

| Mechanism | Needs | Reliability | Cost |
| --- | --- | --- | --- |
| `~/Library/DoNotDisturb/DB/Assertions.json`, named through `ModeConfigurations.json` | the file read; [focus-cli](https://pkg.go.dev/github.com/legnoh/focus-cli) says Full Disk Access for the reader | undocumented format, Apple's own; the shape below held on this machine, and scripts from 2021 ([alexwlchan](https://alexwlchan.net/projects/scripts/files/macos/get_focus_mode), [Six Colors](https://sixcolors.com/link/2021/11/help-a-mac-automation-find-focus/)) read the same keys | one `stat` per look, a read of two files of about 5 and 7 KB only when one changed |
| A Shortcuts personal automation, "When Work turns on / off", running a shell command | the automation set up by hand, once per Focus; Shortcuts' Run Shell Script allowed | push, from the system's own trigger; macOS 26 Shortcuts has Focus among its Mac triggers ([MacMost](https://macmost.com/an-introduction-to-shortcuts-automation-in-macos-tahoe.html)) | nothing between changes; one `tmux-companion` client run per change |
| `INFocusStatusCenter` | an app bundle with the Communication Notifications entitlement and the user's authorization ([Apple forums](https://developer.apple.com/forums/thread/682143)); a bare CLI can't carry it ([infocus](https://github.com/bartreardon/infocus) wraps one in an app) | documented, but only says focused or not, never which Focus | a helper app to build, sign and ship |
| `defaults read com.apple.controlcenter`, `com.apple.ncprefs` | nothing | no Focus key on this machine; the old `dnd_prefs` went with Big Sur | n/a |
| The notification centre database | Full Disk Access | holds delivered notifications, not the Focus | n/a |
| kqueue or FSEvents on the database directory | the same read access as the file | push, but macOS writes the file by rename, so the watch has to be on the directory and re-armed | a new dependency or raw kqueue code, to save a `stat` every two seconds |

What the file looks like, from its keys and never its values:

- `Assertions.json`: `data[].storeAssertionRecords[]`, each with `assertionDetails.assertionDetailsModeIdentifier` (`com.apple.focus.work`) and `assertionStartDateTimestamp` in seconds since 2001. A record is there while the mode is on and moves to `storeInvalidationRecords` when it ends.
- A Focus turned on by its schedule writes an assertion too: the invalidation records here carry `assertionDetailsLifetimeType = "schedule"`. So the scheduled triggers in `ModeConfigurations.json` don't have to be evaluated which the 2021 scripts did.
- `ModeConfigurations.json`: `data[].modeConfigurations[<id>].mode.name`, the name the user sees: `Do Not Disturb`, `Work`, `Sleep`, `Personal`, `Driving`, `Fitness`, `Reduce Interruptions` and any custom one.

On this machine both files are mode 644 and the shell this was checked from could read them. That shell's process tree has Full Disk Access (it can list `~/Library/Safari`) so it doesn't answer whether a daemon started from tmux can which depends on the terminal app tmux was started in.

Checked 2026-10-12 on macOS 26.6: `head -c1 ~/Library/DoNotDisturb/DB/Assertions.json` run inside the maintainer's tmux server (`tmux run-shell`) printed `{` and exited 0, so on that setup the daemon reads it without Full Disk Access granted for it. Other setups may need Full Disk Access for the terminal tmux runs in, and the manual says so.

## Recommendation

Both push paths into the same state and no new process on any timer.

1. `tmux-companion quiet --focus [NAME]` and `quiet --focus-off`: a Focus started or ended. Works everywhere, needs no permission and is what a Shortcuts automation or a Linux hook calls.
2. `[quiet] follow_focus = true`: on macOS the daemon follows the database itself, one `stat` of the two files every two seconds and a read only when the modification time or size changed.

The Shortcuts automation is the recommended setup on his machine because it needs no Full Disk Access; `follow_focus` is for a terminal that has it already and needs no clicking per Focus.

Decided: `follow_focus` is off by default.

## How it combines with the timer and the schedule

Quiet is on when any of the three is: the `quiet 2h` timer, a `[quiet] daily` window, or a Focus. A Focus has no end anyone can count to so it's kept as its own flag, not folded into the timer.

Decided: quiet is a Focus or `daily` or the timer.

- `quiet --off` ends the timer and leaves the Focus on. The Focus is the system's call and only its end, or `--focus-off`, takes that part away. `brief`'s `z` says the same: "Still quiet while Focus Work is on; ending the Focus ends it."

Decided: `quiet --off` leaves a Focus on.

- Every Focus counts. There's no list of which ones do.

Decided: every Focus quiets; there is no `focus_only` list.

- With both the push and the file on, the last one to say something wins: a file change overwrites what a hook said, a hook overwrites the file until the next change.

## What it says

`quiet` and `health` print one line, words only so a screen reader gets the same as the bar:

```text
quiet while Focus Work is on
quiet while Focus Sleep is on, and for 1h more
quiet while Do Not Disturb is on
quiet while a Focus is on
quiet for 45m more
not quiet
```

The third is Do Not Disturb, said without the word Focus. The fourth is `quiet --focus` with no name. The bar keeps the crossed-out bell it draws for any quiet so nothing new depends on a glyph or a colour: `health`, `read-bar` and `brief` read the line above.

Decided: `quiet while Focus Work is on`, and `quiet while Do Not Disturb is on` for Do Not Disturb on both systems.

## Config

```toml
[quiet]
daily = []
follow_focus = false
```

No hook command in the config. On Linux `follow_focus` covers GNOME, KDE Plasma, dunst, mako and swaync over the session bus (below), and anything else runs its hook outside the daemon (a keybinding, a script the notification daemon runs) and calls `quiet --focus` itself so the daemon still starts nothing.

Decided: no `[quiet] focus_command` on Linux; a hook calls the CLI.

## Failure modes

- The database isn't there: a Mac where Focus was never used. Not quiet from Focus, nothing said.
- It's there and can't be read (no Full Disk Access) or doesn't parse (Apple changed the format): one health reason, `a timer failed: focus: cannot read the macOS Focus state: ...; quiet follows the timer and [quiet] daily only`, filed once, not on every tick. It goes after the health window or on the next change that reads. The Focus part stays as it was.
- A schedule-started Focus that somehow wrote no assertion isn't seen. Not seen on this machine; noted in case.
- `follow_focus = true` on a system that's neither macOS nor Linux does nothing and says nothing.

## The manual step

For the Shortcuts path, once per Focus that should quiet tmux-companion:

1. Shortcuts, Automation, New Automation, Focus, pick the Focus, "When turning on", Run Immediately.
2. Action: Run Shell Script, with `/opt/homebrew/bin/tmux-companion quiet --focus Work` (the full path, Shortcuts doesn't read the shell's `PATH`; `command -v tmux-companion` says which).
3. A second automation for "When turning off" running `tmux-companion quiet --focus-off`.

Decided 2026-10-12: shipped as written. The step names come from MacMost's Tahoe walkthrough and haven't been checked against a real Shortcuts screen; fix them if a reader reports a difference.

For `follow_focus`: set `follow_focus = true`, `tmux-companion restart`, and give the terminal app tmux starts in Full Disk Access if the health mark says the database can't be read.

## Cost

The loop starts only with `follow_focus` on macOS: two `stat` calls every two seconds and a read of two small files when one changed. No `tmux` listing, no process. `just bench-capped` wasn't run for it, since the bench's config leaves it off and with it on the work is two system calls a tick.

Decided: the macOS check runs every two seconds.

## Linux

Researched on 2026-10-12 from each project's source and docs, not on a desktop: the only Linux machine to hand is a server with no desktop session so no real GNOME or KDE was toggled. The bus half is tested against a private `dbus-daemon` playing each desktop (`follow_focus_on_linux_follows_the_desktop_on_its_bus` in `tests/e2e.rs`).

`follow_focus` keeps its meaning: follow the system's do-not-disturb. Linux has no one place for it, each desktop or notification daemon keeps its own so the daemon opens one session-bus connection, subscribes to each source's change signal once and asks a source only when its signal fired. No timer and no process, either per look or per change.

| Desktop | Where it lives | How it's followed | Source |
| --- | --- | --- | --- |
| GNOME | `org.gnome.desktop.notifications show-banners`, false while Do Not Disturb is on | dconf's `ca.desrt.dconf.Writer.Notify` signal on `/ca/desrt/dconf/Writer/user`, then a read of `~/.config/dconf/user` (GVDB) when the write named that key | gnome-shell [dateMenu.js](https://gitlab.gnome.org/GNOME/gnome-shell/-/blob/main/js/ui/dateMenu.js) reads `doNotDisturb = !show-banners`; dconf [dconf-engine.c](https://gitlab.gnome.org/GNOME/dconf/-/blob/main/engine/dconf-engine.c) and [dconf-engine-source-user.c](https://gitlab.gnome.org/GNOME/dconf/-/blob/main/engine/dconf-engine-source-user.c) for the signal, path and file; GLib's [gvdb-format.h and gvdb-reader.c](https://gitlab.gnome.org/GNOME/gvdb/-/tree/main/gvdb) for the format |
| KDE Plasma | `Inhibited` on `org.freedesktop.Notifications`, a Plasma extension the applet sets from its effective state, Do Not Disturb included | `PropertiesChanged` on `/org/freedesktop/Notifications`, carrying the value | [server_p.cpp](https://invent.kde.org/plasma/plasma-workspace/-/blob/master/libnotificationmanager/server_p.cpp) emits it; the applet's [Globals.qml](https://invent.kde.org/plasma/plasma-workspace/-/blob/master/applets/notifications/global/Globals.qml) assigns it; [D24486](https://phabricator.kde.org/D24486) is the change that made Do Not Disturb count |
| dunst | `paused` on `org.dunstproject.cmd0`, same path | `PropertiesChanged`, carrying the value; emitted when the property is set which is what `dunstctl set-paused` does | [dbus.c](https://github.com/dunst-project/dunst/blob/master/src/dbus.c), [dunstctl](https://github.com/dunst-project/dunst/blob/master/dunstctl) |
| mako | a mode, by the manual's convention `do-not-disturb` | `PropertiesChanged` (invalidation only) for `Modes` on `/fr/emersion/Mako`, then `ListModes`, the call `makoctl mode` makes | [mako.5](https://github.com/emersion/mako/blob/master/doc/mako.5.scd), [dbus/mako.c](https://github.com/emersion/mako/blob/master/dbus/mako.c), [mode.c](https://github.com/emersion/mako/blob/master/mode.c), [makoctl.c](https://github.com/emersion/mako/blob/master/makoctl.c) |
| SwayNotificationCenter | its Do Not Disturb, method `GetDnd` on `org.erikreider.swaync.cc` | the `SubscribeV2(count, dnd, cc_open, inhibited)` signal on `/org/erikreider/swaync/cc` | [swayncDaemon.vala](https://github.com/ErikReider/SwayNotificationCenter/blob/main/src/swayncDaemon/swayncDaemon.vala), [main.vala](https://github.com/ErikReider/SwayNotificationCenter/blob/main/src/main.vala) |

Looked at and not used:

- The freedesktop [Desktop Notifications spec](https://specifications.freedesktop.org/notification-spec/latest/) has no do-not-disturb or inhibition so there's nothing common to follow. `Inhibited` is Plasma's.
- The settings portal, `org.freedesktop.portal.Settings`, would carry GNOME's value in its `SettingChanged` signal, but xdg-desktop-portal-gnome only serves a fixed list of schemas and `org.gnome.desktop.notifications` isn't on it ([settings.c](https://github.com/GNOME/xdg-desktop-portal-gnome/blob/main/src/settings.c)).
- `gsettings monitor`, `dunstctl is-paused`, `makoctl mode`, `swaync-client -D`: a process each which the signals make unnecessary.
- A stat loop on `~/.config/dconf/user`, the macOS shape: it covers GNOME only and still needs the bus for the rest.

Every source is asked first whether its name has an owner (`NameHasOwner`) so a read never starts a notification daemon by bus activation. A `NameOwnerChanged` for `org.freedesktop.Notifications` (which swaync also owns) re-reads every service so one started after the daemon is picked up. Quiet is on when any source says so and the Focus is written to the daemon's state only when that answer changes so the last to speak still wins between the desktop and a hook.

`quiet` then says `quiet while Do Not Disturb is on`.

Decided: "Focus" is dropped when the name is Do Not Disturb, on both systems.

### What it costs

Off by default, as on macOS, and with it off nothing new runs. With it on:

- one connection to the session bus and five match rules, made once;
- a wakeup per dconf write anywhere, since zbus re-checks an `arg0path` rule on its own side by plain equality and would drop dconf's directory-form `Notify` so the rule is the whole Writer path and `dnd::dconf_touches` decides; the database is read only when the write named `show-banners` or a directory above it;
- a property read or two per change on KDE, dunst, mako or swaync.

No loop, so `just bench-capped` has nothing to measure: the bench config leaves `follow_focus` off and turned on the daemon is idle between signals.

The dependency is [zbus](https://crates.io/crates/zbus) 5, MIT, pure Rust, with only its `tokio` feature so it runs on the daemon's runtime and brings no second executor. It's a Linux-only dependency, 24 crates, and the static `x86_64-unknown-linux-musl` build (`just build-linux`) builds with it. Writing a D-Bus client by hand (SASL, marshalling, match rules) was the alternative and not worth owning.

Decided: zbus, as a Linux-only dependency, for all five.

### Limits

- KDE's `Inhibited` is the applet's effective state so a fullscreen window or a mirrored screen that Plasma's own settings say inhibits notifications makes tmux-companion quiet too.

Decided: KDE's `Inhibited` is followed as it is, fullscreen inhibition included.

- mako has no do-not-disturb of its own. Only the mode named `do-not-disturb`, the manual's example, counts.

Decided: mako by the fixed mode name `do-not-disturb`.

- dunst paused by `killall -SIGUSR1 dunst`, the old way, emits no signal and isn't seen until the next `PropertiesChanged`.
- GNOME: a `show-banners` set in a system dconf database or locked by an administrator isn't read, only the user's. A GSettings backend other than dconf (the keyfile backend) isn't followed.
- No session bus (a daemon started over ssh, or a tmux that lost `DBUS_SESSION_BUS_ADDRESS` and has no `$XDG_RUNTIME_DIR/bus`) is one health reason, `focus: no session bus to follow do-not-disturb on: ...`. A bus that goes away later is one too and the daemon doesn't reconnect until it restarts.
- `follow_focus` on with none of the five running says nothing: there's no way to tell "no desktop we know" from "the desktop's do-not-disturb is off".
