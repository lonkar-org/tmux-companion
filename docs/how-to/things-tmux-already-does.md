# Things tmux already does

This page exists because of a number. `docs/dev/after-the-port.md` records
`tmux-plugins/tmux-logging` at 1258 stars, and that plugin is a wrapper around
`pipe-pane`, a command that has been in tmux the whole time. A wrapper doesn't
get installed that often when the thing it wraps is easy to find, so the next
time somebody asks this project for a logging segment, or a menu, or a way to
move a pane between windows, the answer is a section below rather than a
feature. Everything here is checked against `man tmux` for 3.7c on my machine,
and where I couldn't find a flag in the page I haven't written it.

## pipe-pane: log a pane to a file

`pipe-pane` connects the output of a pane to a shell command, and with no
command it closes the pipe that's there, so the same command turns logging on
and off. The `-o` flag only opens a pipe when none exists, which is what makes
one key a toggle, and the manual's own example is the one to copy:

```tmux
bind-key C-p pipe-pane -o 'cat >>~/output.#I-#P'
```

By hand, the same on and off:

```sh
tmux pipe-pane -t edit.1 'cat >>~/edit-1.log'   # on
tmux pipe-pane -t edit.1                        # off
```

A pane can only feed one command at a time, so a second `pipe-pane` closes the
first before it starts. Without a flag the direction is `-O`, pane output into
the command's stdin, and `-I` goes the other way, so whatever the command
prints lands in the pane as if you'd typed it.

## display-menu: a menu on a key

You've been using this already if you've ever pressed `prefix >` or
right-clicked a pane with the mouse on: both open a `display-menu`, and so does
`prefix <` for the window. A menu is a list of name, key, command triples, an
empty name is a separator, and a name that starts with a hyphen is shown dim and
can't be picked:

```tmux
bind-key g display-menu -T "git" \
    "status" s "display-popup -E 'git status; read'" \
    "" \
    "-fetch (soon)" f ""
```

`-x` and `-y` place it; `P` is the bottom left of the pane, `M` the mouse
position, `W` the window's spot on the status line, and `C` the centre. The
`Zoom` item on the default pane menu runs `resize-pane -Z`, which is the same
command `tmux-companion zen` sends, anchored to the pane the key was pressed
in.

## customize-mode: browse every option

`prefix C` puts the pane into customize mode, a list of every option and key
binding with its current value, which you can search with `C-s`, change with
`s`, set globally with `S`, and put back to the default with `d`. It's the
quickest way I know to find out what an option is called when you half
remember it, and it's bound by default to `customize-mode -Z`, so it takes the
whole window while it's open.

```sh
tmux customize-mode -Z
```

The values it shows are for the active pane in the current window, so open it
from the pane whose settings you're asking about.

## allow-passthrough: escape sequences through tmux

`allow-passthrough` is a pane option. Set to `on`, a program in the pane may
send tmux the envelope `\ePtmux;...\e\\` and whatever is inside goes straight to
the outer terminal, but only while the pane is visible; `all` allows it from a
pane that's hidden too. The default on 3.7c is `off`.

```tmux
set -g allow-passthrough on
```

The manual says only what the option does: programs in the pane bypass tmux
with a `\ePtmux;...\e\\` sequence, and `on` lets it through while the pane is
visible where `all` lets it through regardless. What a program wraps in that
sequence is its own business.

## link-window: one window in two sessions

A window belongs to a session list, and `link-window` adds it to a second one,
so the same shell with the same scrollback is window 1 in `work` and window 9
in `notes`, and `unlink-window` takes it out of one session while the other
keeps it:

```sh
tmux link-window -s work:1 -t notes:9
tmux unlink-window -t notes:9
```

If the destination index exists already that's an error unless you pass `-k`,
which kills what was there, and `-d` links without switching to it. This is
how I'd keep a build log in view from two projects without running the build
twice.

## join-pane -s: move a pane between windows

`join-pane` is `split-window` that moves an existing pane into the new space
instead of starting a shell there, and it's the reverse of `prefix !`, which
breaks a pane out into its own window. `-h` puts it beside, `-v` below, `-b`
to the left or above:

```sh
tmux join-pane -h -s logs.0 -t edit.1
tmux break-pane -s edit.2                # and back out again
```

With no `-s`, the marked pane is used, so `prefix m` on the pane you want,
then `join-pane` in the window you want it in. `tmux-companion run` opens its
side pane with `split-window -fh`, and joining is what to reach for when the
pane you want beside your editor already exists.

## respawn-pane -k: restart what a pane runs

`respawn-pane` runs the command a pane was created with again, in the same
pane, and `-k` kills whatever is still running there first, so a dev server
that has wedged restarts in place without losing its position in the layout.
Give it a new command and that one replaces the old:

```sh
tmux respawn-pane -k -t edit.1
tmux respawn-pane -k -t edit.1 'npm run dev'
```

Without `-k` the pane has to be dead already, which is what `remain-on-exit`
is for: a pane with it `on` stays open when its program exits, `failed` keeps
it only on a non-zero exit, and the default pane menu's `Respawn` item is
exactly `respawn-pane -k`. `tmux-companion project close` goes the other way
and uses `send-keys` to ask an editor to quit, because a killed editor is one
with unsaved work in it.

## select-pane -T: name a pane

A pane has a title separate from the window name, `select-pane -T` sets it,
and `pane-border-status` draws it on the border, where the default
`pane-border-format` already shows `#{pane_title}` in quotes:

```sh
tmux select-pane -t edit.1 -T tests
tmux set -g pane-border-status top
```

Programs can set the title themselves through the terminal, which
`allow-set-title` controls, so a title you set by hand can be overwritten by
the next program that wants to.
`tmux-companion panes` shows this title beside the program in each row, which
is what makes naming one worth the keystroke.

## `%if` and `#{version}`: one config across versions

A tmux.conf line that uses an option an older tmux doesn't have is a
configuration error on that machine every time the server starts, and `%if` is
how you keep one file for both machines. Its argument is a format, `#{version}` is the server
version, and `>=` is a string comparison, which is fine for `3.2` against
`3.7c` and worth remembering for `3.10`:

```tmux
%if "#{>=:#{version},3.2}"
set -g allow-passthrough on
%endif
```

`%elif` and `%else` exist too, and a guard can sit on one line:

```tmux
%if #{==:#{host},myhost} set -g status-style bg=red %endif
```

I haven't measured what a guard costs at load time and I assume it's nothing
anyone would notice, since the file is read once when the server starts.

Hope this helps, and if there's a plugin you install for something tmux does
on its own, open an issue and it can have a section here.
