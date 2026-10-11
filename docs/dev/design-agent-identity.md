# Agent identity

Status: implemented on `fix/agent-identity`
Problem: whether a pane is a coding agent was decided by matching tmux's
`pane_current_command` against `[agents] programs`. That name is the kernel's
and for most agents it isn't the name anybody typed.

## What tmux reports

tmux names a pane after the foreground process group of its terminal
(`tcgetpgrp`) and asks the system for that group's name in two different
ways (`osdep_get_name` in tmux's `osdep-darwin.c` and `osdep-linux.c`):

- macOS: `pbsi_comm` from `proc_pidinfo`, the name of the file that was
  executed, after symlinks, cut to 15 bytes.
- Linux: `argv[0]` from `/proc/<pgrp>/cmdline` and tmux takes its last
  path component.

So the same agent can be three different names and an interpreter-based one
is always the interpreter's.

## What each agent looks like

Observed is a process read on a macOS machine running it. Launcher is read
from the installed launcher or the package's published `bin` entry, without
running it.

| Agent | Install | macOS tmux name | Linux tmux name | argv | Rule that names it | Source |
| --- | --- | --- | --- | --- | --- | --- |
| claude | native, `~/.local/bin/claude` → `versions/2.1.296` | `2.1.296` | `claude` | `claude --continue` | `argv[0]` | observed |
| claude | npm | `node` | `node` | `node /usr/local/bin/claude` | script | launcher |
| grok | native, `~/.grok/bin/grok` → `downloads/grok-1.0.50-macos-x86_64` | `grok-1.0.50-mac` | `grok` | `grok` | `argv[0]` | observed |
| agy | native, `~/.local/bin/agy` | `agy` | `agy` | `agy` | name, `argv[0]` | launcher |
| gemini | npm, `#!/usr/bin/env node` | `node` | `node` | `node /usr/local/bin/gemini` | script | launcher |
| gemini | its own restart with a bigger heap | `node` | `node` | `node --max-old-space-size=N …/@google/gemini-cli/dist/index.js` | package, `-cli` dropped | launcher |
| codex | npm | `node` | `node` | `node /usr/local/bin/codex` | script | launcher |
| codex | native (brew, release binary) | `codex` | `codex` | `codex` | name | launcher |
| opencode | native | `opencode` | `opencode` | `opencode` | name | launcher |
| opencode | npm | `node` | `node` | `node /usr/local/bin/opencode` | script | launcher |
| cursor-agent | `~/.local/bin/cursor-agent` → a shell launcher that execs its own node | `node` | `node` | `…/cursor-agent/versions/<v>/node --use-system-ca …/versions/<v>/index.js` | package | launcher |
| aider | pip, pipx, uv: console script with a python shebang | `Python` (framework build) or `python3.N` | `python3` | `…/bin/python /Users/you/.local/bin/aider` | script | observed shape (a Python console script running here) |
| aider | `python -m aider` | as above | `python3` | `python3 -m aider` | module | launcher |

## The rule

1. **Fast path.** tmux's name matched against `[agents] programs`, as before,
   with the bare-version (`2.1.283`) and release-suffix (`grok-1.0.50-mac`)
   rules kept as the fallback for a process that can't be read.
2. **Only names that can hide a program are looked into.** A bare version, a
   name with a release after a hyphen, an interpreter (`node`, `nodejs`,
   `bun`, `deno`, `python*`, `Python`, `pypy`, `ruby`, `perl`), or a name of 15
   bytes which on macOS may be a longer one cut short. A shell, an editor,
   `cargo` or `claude` costs nothing.
3. **For those, read what tmux read, with the arguments.** The pane's own
   process (`#{pane_pid}`, added to the one listing) holds the terminal, so
   its record says which group is in front: `e_tpgid` from `proc_pidinfo` on
   macOS, field 8 of `/proc/<pid>/stat` on Linux. That group's leader's
   arguments come from `KERN_PROCARGS2` or `/proc/<pid>/cmdline`.
4. **Name from the arguments** (`identity::name_from_argv`, pure): the last
   component of `argv[0]`; for an interpreter or a shell given a script, the
   script's file name without its extension; for `-m module`, the module's
   first part; for a generic script name (`index`, `main`, `cli`,
   `__main__`), the package directory above it, past `dist`, `lib`, version
   directories and the like, with a trailing `-cli` dropped. Code on the
   command line (`-c`, `-e`) leaves the interpreter's name.
5. That name is `Pane::identity` and every consumer asks `Pane::name()`:
   the agents segment, inbox, brief, pickers, `[notify]`, the journal and
   the chunk clock's agent check.

Why it holds across agents: an agent either is a native binary, whose
`argv[0]` is the name it was started by whatever the file behind the symlink
is called, or is run by an interpreter, whose first non-flag argument is the
script path found on the way in and a package's bin entry is named for the
command. Both systems expose argv for a process of the same user without
privileges and both expose the terminal's foreground group in the shell's
own record so neither needs a scan of the process table.

## The version-suffix rule from `fix/grok-agent`

Kept, as the fallback and as display cleanup: a process that can't be read
(another user's, `/proc` mounted with `hidepid`, an unsupported OS) still has
only tmux's name and `grok-1.0.50-mac` should still count and still show as
`grok`. With the identity in place it no longer carries the detection.

## Cost

Per listing, for each pane whose name can hide a program: one system call on
macOS or one `/proc` read on Linux, to learn the foreground group.
The arguments are read once per foreground group and name, kept in a table of
at most 256 entries so the daemon's two-second listing re-reads nothing while
the same agent runs. Both calls together measured 10 µs on the machine this
was written on. No process is started and the bar's hot path reads the
daemon's shared listing as before so it adds nothing there. A picker in the
client pays the same small cost on its own listing, once per run, for the
panes running an interpreter or a version-named program.

`just bench-capped 0` idle daemon, before and after: see the commit message.

## Limits

- A wrapper that doesn't `exec`, `bash my-claude.sh` running `claude` as a
  child, is named for the script (`my-claude`), since the group's leader is
  the shell. Adding the name to `programs` works; finding the agent below it
  would need the group's members which is a scan.
- A program that rewrites its arguments (`process.title` in node) is named
  for what it wrote.
- A pipeline whose leader has exited has no leader to read; tmux has no name
  for it either and falls back to the pane's start command.
- Another user's process (`sudo claude`) can't be read on macOS; tmux's name
  and the fallback rules apply.
- Only macOS and Linux are read. Elsewhere it is tmux's name, as before.
