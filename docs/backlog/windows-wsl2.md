# Windows, through WSL2

Deferred 2026-10-07. When `screen-reader-mode` lands, this file gets its row in
that branch's `docs/backlog/README.md`.

**What.** tmux-companion tested and documented under WSL2, the way most
Windows developers who use tmux run it. Not a Windows build: tmux has no
native Windows port worth supporting (the Cygwin and MSYS2 ones are rough and
little used), and WSL2 runs a real Linux kernel, so tmux and the static musl
Linux binary run there unchanged.

**Why it waits.** Nobody here has a Windows machine with WSL2 to try it on,
and nobody has asked. `docs/how-to/install.md` says it should work, hasn't
been tested, and asks anybody who tries to report back; the README's "What it
doesn't do" points there.

**What's known.** The daemon's Unix socket, `/proc` and the tmux it drives
behave under WSL2 as on Linux, so the core should work. What is likely to need
care, from how WSL behaves rather than from a test:

| Part | Likely under WSL2 | Way out |
| --- | --- | --- |
| Clipboard: picker copies, `setup` snippets | no `pbcopy` or `xclip` by default | `[clipboard]` pointed at `clip.exe` or `win32yank` |
| `open` for URLs and `file:line` | no `xdg-open`, or one that opens inside Linux | `wslview` from wslu, or `explorer.exe` |
| Battery segment | reports the Linux VM, which often has no battery | empty or wrong; hide it when WSL is detected |
| Network bandwidth | counts the VM's virtual network card | still means something for traffic from inside WSL |
| Earcons | need audio inside Linux; WSLg provides it on current Windows 11 | off by default anyway |
| Prompt marks, focus events, Nerd Font glyphs | Windows Terminal supports them | a Nerd Font in Windows Terminal, or `[glyphs] preset = "ascii"` |

WSL1 has no real kernel and only partial Unix socket and `/proc` support. Rule
it out rather than support it.

The cost is small, since nothing new is built: one session on a Windows
machine running the tutorial and the e2e suite, a "Windows, through WSL2"
paragraph in `docs/reference/requirements.md` with whatever settings it
needed, the README's line changed from "may work" to what was found, and, if the battery segment
misbehaves, a check for WSL (`/proc/sys/kernel/osrelease` mentions
`microsoft`) that leaves it off.

**What brings it back.** Two steps, and only the first is worth doing
before anybody asks. First an investigation: does the binary run under WSL2 at
all, and which of the parts above actually break, by running the tutorial and
the e2e suite there once. Then the work that investigation turns up, the
requirements paragraph, the README line and any fixes, when somebody asks for
it. A report from somebody who tried, which the install page asks for, counts
as the first step done. Volunteers with a Windows machine are welcome to take
either.
