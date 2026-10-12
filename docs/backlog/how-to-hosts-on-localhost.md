# How-to: try hosts against localhost

Deferred 2026-10-12, out of 0.10.0. A how-to page under `docs/how-to/` for
trying remote sessions on one machine, with Remote Login on, before a real VM.
The steps and commands below are checked against `--help`; the page's prose
is still to be written by the maintainer.

## Steps

1. System Settings > General > Sharing > Remote Login.
2. `ssh localhost true`, once, to accept the host key (the link runs with
   `BatchMode` and can't answer the prompt).
3. `tmux-companion hosts add local -- ssh localhost`, then
   `tmux-companion hosts connect local`.
4. `M-s`: the host's line and its sessions under it; pick one to open a
   mirror session such as `api@local`.
5. `tmux-companion hosts` and `tmux-companion switch sessions --print`.
6. `tmux-companion hosts forget local` to clean up.

## Open questions

- [ ] Why try this machine first, and what it shows before a real VM.
- [ ] What Remote Login is for here, that it's macOS only, and what a Linux
      reader turns on instead.
- [ ] Who to allow under "Allow access for", and whether to turn it off again.
- [ ] Does `ssh localhost` find `tmux-companion` on PATH in a non-interactive
      shell, or does the host command need the full path? Untested: Remote
      Login was off on the development Mac.
- [ ] What `local` names and why it isn't saved (no `--save`: kept until this
      tmux server exits).
- [ ] What `hosts connect local` prints when it works, and what to do when it
      doesn't.
- [ ] What the mirror bar shows: the host's git, agents and health, and this
      machine's as `here: <reason>`.
- [ ] What the host's line says in `hosts` (link up, pushing or polled every
      5s, last heard).
- [ ] What `switch sessions --print` gives a screen reader in place of the
      switcher.
- [ ] On localhost the bridge answers from the same daemon, so your own
      sessions show twice, once local and once under the host. Say so?
- [ ] When to forget the host, and that a mirror session to it has to be
      closed first.
