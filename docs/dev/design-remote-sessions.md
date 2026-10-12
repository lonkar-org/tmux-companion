# Sessions across hosts

Status: draft, every question answered 2026-10-12, nothing built
Owner: tmux-companion
Author: claude
Problem: a VM reached over ssh runs its own tmux and its own tmux-companion. Its sessions don't show in the Mac's session list and getting to one is `ssh`, then `tmux attach`, then remembering which prefix belongs to which tmux. The two daemons can talk to each other so the Mac's pickers can list the VM's sessions beside its own and open one the way they open a local one.

This is the design for that: where the list comes from and how it travels, how a remote session opens, what each picker, command and bar shows, what it costs on a capped CPU, what fails and how it says so and what a remote daemon may be asked.

## Terms

- Local: the machine whose tmux client you're looking at. Remote, or host: another machine with tmux and tmux-companion, reached by a command that opens ssh to it.
- Host command: one argv per host that opens ssh to it, `ssh vm` or a wrapper that ends up in ssh. tmux-companion appends `-t` when it wants a terminal and the remote command last.
- Link: one long-lived host command per host, owned by the local daemon, carrying Cap'n Proto messages to and from the remote daemon over the ssh session's stdin and stdout.
- Bridge: `tmux-companion bridge`, what the link runs on the host. It serves one Cap'n Proto interface on its stdio and answers from its own daemon.
- Mirror session: the local session `api@vm` that stands for the remote session `api`, holding one window whose pane runs the attach.
- Grouped session: the session the attach creates on the VM, grouped with `api`, one per mirror session, so the Mac gets its own current window and its own options there.

## Where sessions are listed today

Every one of these asks the local tmux and nothing else. Each is a place a host's rows appear.

| Where | Code | What it lists | With hosts |
| --- | --- | --- | --- |
| `switch sessions` (`M-s`) | `cli.rs` `run_switch_sessions`, `project::sessions` | `list-sessions -F SESSIONS_FORMAT` with the switcher hooks chained in front; directories later through `picker::Later` | host groups after local sessions, phase 2 |
| `project` picker | `cli.rs`, `project::sessions` and `project::directories` | sessions then directories from `dirsource` | every session; directories of one place only, phase 3 |
| `brief` | `brief.rs`, `tmux_capture(["list-sessions", ...])` | a session count, idle sessions, the inbox, health | one line per host, phase 3 |
| `inbox` | `inbox.rs`, the daemon's `__inbox` | agents waiting, from the daemon's pane poll | VM agents while the link is up, phase 3 |
| `panes` picker | `panes.rs` `list` and `__reports` | every pane on this server | not planned |
| `sessions idle` | `sessions/idle.rs` | sessions nobody looked at for days | no: a remote's idle sessions are its own business |
| `sessions save/resurrect` | `sessions/timer.rs`, `capture.rs` | the whole server, for snapshots | mirror sessions recorded and reopened, phase 3 |
| `theme` and `sessions` CLI | `theme/cli.rs`, `sessions/cli.rs` | names, to validate | no |
| push loop | `push.rs` | sessions with a client, to set `@tc-right` | the mirror session's bar from the link, phase 2 |

A pick today runs `focus_session`: `switch-client -t =NAME` inside tmux and `attach-session` outside it.

The daemon has no session listing of its own. It has the shared `list-panes -a` poll (`panes::shared`) that the inbox, notify, journal and the agents segment read and the remote daemon's answer comes from that poll, not from a new loop (rule 3).

On the VM the same places have to leave grouped sessions out, since a grouped session is the Mac's view and not a session anybody there made: the switcher, the project picker, `brief`, `sessions idle`, snapshots and the push loop all skip a session carrying `@tc-mirror`.

## What the protocol allows today

The socket is one JSON line in and one out per connection (`proto::Request`, `proto::Response`), mode 0600 and an execution surface: `dirs` runs whatever `command` the request names with the environment the request gives and `__shutdown` stops the daemon. `client::send` also replaces a daemon from an older build by sending it `__shutdown` which is right on one machine and wrong across two where the other end's build is somebody else's upgrade schedule. `daemon::Daemon` already has the other behaviour: it never starts or replaces a daemon. The link is that behaviour over a pipe, in Cap'n Proto rather than JSON.

## Prior art

Read on 2026-10-11. Anything not confirmed in a primary source says so.

- **Nesting.** `send-prefix` sends the prefix on to the inner tmux and `prefix` and `prefix2` both take `None` ([tmux(1)](https://man.openbsd.org/tmux), [FAQ](https://github.com/tmux/tmux/wiki/FAQ)). The usual way of living with a nested tmux is an F12 toggle that sets `prefix None` and `key-table off` on the outer one and greys its bar, then F12 again to undo it ([samoshkin/tmux-config](https://github.com/samoshkin/tmux-config)). `new-session -A` attaches when the session exists which is the `ssh -t host tmux new -A -s NAME` people put in a window (tmux(1)). `prefix` being a session option is what option (d) uses: the toggle, set once per mirror session.
- **`detach-client -E`** runs a shell command to replace the client (tmux(1)) which is option (c) in one tmux command. `destroy-unattached` takes `keep-last` and `keep-group` in the 3.4, 3.5a and 3.7c manuals; this design needs only `on`.
- **Control mode.** `-CC` frames each reply in `%begin`/`%end`, streams `%output` with control bytes octal-escaped and reports `%window-add`, `%layout-change`, `%session-changed` and the rest ([tmux(1)](https://man.openbsd.org/tmux), [Control Mode wiki](https://github.com/tmux/tmux/wiki/Control-Mode)). 3.2 added flow control (`pause-after`, `%pause`, `refresh-client -A`) and format subscriptions (`refresh-client -B`, at most one `%subscription-changed` a second) ([CHANGES](https://raw.githubusercontent.com/tmux/tmux/master/CHANGES)). iTerm2 maps windows to native tabs and sizes every window to the smallest client ([iTerm2](https://iterm2.com/documentation-tmux-integration.html)); WezTerm's support is still an open issue ([#336](https://github.com/wezterm/wezterm/issues/336)); ET's page names iTerm2 and a Terminator fork as the only clients ([ET](https://eternalterminal.dev/howitworks/)). No published latency figures were found.
- **ssh.** `ControlMaster auto` with `ControlPersist` shares one connection, `ssh -O check|forward|exit` drives it, `-L /local.sock:/remote.sock` forwards a socket, `StreamLocalBindMask` defaults to 0177 though "some operating systems ignore file modes on sockets", `StreamLocalBindUnlink` defaults to `no` and a stale file then blocks the forward, `BatchMode yes` turns off prompts and host-key questions, `ConnectTimeout` covers the handshake and `ServerAliveInterval` is off by default ([ssh_config(5)](https://man.openbsd.org/ssh_config), [ssh(1)](https://man.openbsd.org/ssh)).
- **mosh** survives roaming and sleep over UDP, syncs only the visible screen and has no port forwarding and no non-interactive use ([mosh.org](https://mosh.org/), [a maintainer on the git list](https://lore.kernel.ime.usp.br/git/552E732E.20107@debian.org/T/)). **Eternal Terminal** bootstraps over ssh, then replays lost bytes on reconnect and does forward ports ([EternalTerminal](https://github.com/MisterTea/EternalTerminal)). Either could carry the attach; neither is needed for the link.
- **WezTerm** SSH domains start a mux server on the remote and talk to it over a Unix socket through ssh, with the remote's binary path configurable and `multiplexing = "None"` is a plain ssh that loses panes on a drop ([multiplexing](https://wezterm.org/multiplexing.html), [SshDomain](https://wezterm.org/config/lua/SshDomain.html)). **VS Code Remote-SSH** installs a server on the remote, runs most of the work there and keeps the UI local, all through the ssh connection ([VS Code](https://code.visualstudio.com/docs/remote/ssh)). Both are the shape of T2 below: the same program on both ends, one ssh between them and the remote half doing the remote's work.
- **Session managers.** sesh and tmux-sessionx list local sessions, zoxide and tmuxinator projects and nothing remote ([sesh](https://github.com/joshmedeski/sesh), [tmux-sessionx](https://github.com/omerxx/tmux-sessionx)); smug's only remote touch is a pane running ssh ([smug](https://github.com/ivaaaan/smug)); zellij has a web client with tokens and nothing ssh-specific ([zellij](https://zellij.dev/documentation/web-client.html)). tmuxinator wasn't checked.
- **The nearest thing to this design**, [tmux-smart-pane](https://github.com/msc5/tmux-smart-pane), lists remote sessions in an fzf picker with an ssh call per host, names them `remote:<host>:<session>` and connects by detaching the local client and running ssh from the bare terminal, with a variable in the remote session so leaving it brings the picker back: option (c), with a process per host per open. Others (mtmux, emux, gotomux) were seen in search results only.
- **kitty's ssh kitten** copies terminfo and shares connections per kitty instance ([kitty](https://sw.kovidgoyal.net/kitty/kittens/ssh/)). tmate's hosted servers shut down in July 2025 ([tmate](https://github.com/tmate-io/tmate)). Neither changes the design.

## Options

Every option shares the list half: the local daemon keeps the remote's session list in a cache and a picker draws from the cache. They differ in how the list travels and in what opening a remote session does.

### Transport for the list

**T1, a forwarded socket.** One `ssh -N -L /tmp/tc-vm.sock:/tmp/tmux-companion-<uid>.sock vm`, best under a ControlMaster. The local daemon then talks to the remote daemon as if it were local. It needs nothing new on the remote. It needs `-L` to reach ssh, `StreamLocalBindUnlink yes` or a stale socket file blocks the next forward, a path under the 103 bytes `client.rs` already enforces and the remote uid in the path. The local socket then reaches the whole remote daemon, `dirs` and `__shutdown` included, for any local process of the same user.

**T2, a stdio bridge.** The link runs `<host command> 'tmux-companion bridge'` and speaks over the ssh session's stdin and stdout, the shape VS Code's remote server and `ssh host nc -U` use. Nothing listens anywhere new, any wrapper that can run one remote command works, the remote decides what it answers and the test transport is the same code with `sh -c` in place of ssh. It needs the bridge on the remote so both ends must run a build that has it.

T2 is the transport. T1 is in the comparison only.

### Opening a remote session

**(a) Nested, one local window.** Picking `api` on `vm` adds a window to the session you're in, running `ssh -t vm tmux attach -t =api`. Cheapest to build. The window is a stranger in a local session: `M-a` mixes local and remote windows, the window switcher can't see inside it, both prefixes are the same key and two bars stack.

**(b) Control mode.** The local daemon holds `ssh vm tmux -CC attach -t api` and maps each remote window to a local window whose pane runs a small renderer fed by `%output`, with keys going back as `send-keys`. It's what iTerm2 does with native tabs. tmux-companion would become a terminal renderer for each remote pane: every key through a `send-keys`, layouts and resizes kept in step, copy mode on a replay. That's a second multiplexer inside the first and a process per remote pane. Researched, not proposed.

**(c) Hand the client over.** Picking a remote session runs `detach-client -E "<host command> -t 'tmux attach -t =api'; exec tmux attach"`: tmux replaces this terminal's client with the ssh and the remote tmux owns the whole terminal, its bar and its keys. When you detach from the remote the `exec` puts you back on the local server. The local bar, local `M-s` and the local inbox's sounds are out of reach until you come back.

**(d) Nested, one mirror session.** Picking `api` on `vm` switches to a local session `api@vm`, with the prefix handed to the VM, the VM's bar off for this view and the local bar showing the VM's segments.

### Comparison

| | (a) nested window | (b) control mode | (c) handover | (d) mirror session |
| --- | --- | --- | --- | --- |
| Keys | outer catches the prefix and root keys; `C-b C-b` or an F12 toggle | all local, no inner tmux | all remote | prefix remote, `M-s` and `M-a` routed by the companion |
| Bars | two | one, local | one, remote | one, local, with the VM's segments |
| Local events while on the remote | outer bar and sounds | yes | sounds only | local bar and sounds |
| Cost per open | one ssh | one ssh, then a process per remote pane and a `send-keys` per key | one ssh | one ssh and one shim process |
| Back to local | change window | change window | detach the remote | `M-s`, or detach the remote |
| Screen reader | two bars of chrome | depends on the renderer | one tmux, the remote's | one bar; host said in words on the row and in the announcement |

## Decision

Option (d) over transport T2, with the link in Cap'n Proto. The local daemon brings a host's link up when something needs it, the remote daemon answers from its own pane poll and the pickers draw remote rows from a cache kept on disk, with each host's last contact said in words. Opening a remote session switches to a local mirror session `api@vm`. The VM gives the Mac a grouped session with its bar off, the prefix goes to the VM and the local bar carries the VM pane's git, agents and health segments, fetched over the link. Everything that changes the VM happens in the ssh you'd type yourself, run in a pane you can see. Handover (c) comes in phase 6, per host or for one pick.

## The link

### Cap'n Proto

The link is Cap'n Proto RPC over the ssh session's stdio, two-party, with the schema in `schema/link.capnp` and the Rust generated from it committed beside it. What it gives that JSON lines don't:

- The allowlist is the interface. The bridge's bootstrap object has a method per question and nothing else, so `dirs`, `__shutdown` and the rest aren't refused, they don't exist on the link. A newer Mac can't ask for more than the VM's schema has.
- Schema evolution is the format's own: a field or a method added later is ignored or answered `unimplemented` by an older end, and the hello says which.
- Phase 4's push stream is a capability the Mac hands the VM, which the VM can call with one method and no other.

The hello is the first call: the build id, a protocol number and the methods the bridge serves. The Mac uses nothing the hello didn't list and never asks a link to stop a daemon, whichever build is newer.

No build needs the `capnp` tool. `just capnp` regenerates the Rust after a schema change and CI fails when the committed file is stale against the schema, so a source install, the brew formula and crates.io build with cargo alone.

The local socket stays JSON lines until its own phase. The `daemon` and `proto` modules are on docs.rs, so moving the socket to Cap'n Proto is a breaking change to the library surface: a minor version bump before 1.0, a CHANGELOG line under Changed, with the Rust API kept and the daemon answering JSON for good beside Cap'n Proto, told apart by the first byte (`{` is JSON, anything else is a Cap'n Proto segment table).

### The interface

```capnp
interface Bridge {
  hello    @0 (build :Text, protocol :UInt16) -> (build :Text, protocol :UInt16, methods :List(Text), startedDaemon :Bool);
  sessions @1 () -> (polledAt :UInt64, sessions :List(Session));
  segments @2 (session :Text) -> (polledAt :UInt64, segments :List(Segment));
  inbox    @3 () -> (entries :List(Entry));
  dirs     @4 () -> (paths :List(Text));        # phase 3, the VM's own configured source
  watch    @5 (sink :SessionSink) -> ();         # phase 4
  ping     @6 () -> ();                         # phase 4, every 15 s while watching
}
```

`Segment` is structured: text, a role (`git`, `agents`, `health`), a foreground and a background colour as names and a glyph by its `icons.rs` name. The VM never sends tmux markup. A `#(` in a status line runs a command on the Mac, so the Mac renders the segment itself and treats every string as text.

### Modes, per host

- `on-demand`, the default. No link until a picker needs one. Opening `M-s` asks the daemon to start the link in the background and the picker draws from the cache at once; the frame never waits. The link stays up while a mirror session to that host is open or a picker is showing, and closes after `idle_secs` (600, ten minutes) with neither.
- `while-attached`. Up while any client is attached to the local server. It's the mode for a VM where agents run long: on-demand hears a VM's agents only while a picker or a mirror session holds the link up, so a question asked while you work on the Mac waits until you look. The choice is per host, and the manual says this in those words.
- `off`. No link. The host is still in `hosts`, and `sessions start api@vm` opens a mirror session by name, the `sessions start NAME` that exists already taking a host after the `@`.

While a link is up the Mac asks `sessions` every `poll_secs` (5): one message each way, no process at either end. Phase 4's `watch` replaces the poll.

A failed link is retried at 5 s, then doubling to 5 minutes and reset when a picker asks. Each retry is one ssh process.

### The cache

The last `sessions` answer for each host is written to the state directory, `hosts/<name>.bin`, with the time it was read. It survives a daemon restart, so the first `M-s` of the day lists the VM before any ssh has run. Each host group in the picker says when the Mac last heard from it, `15m`, `5h` or `3d`, the same short ages `idle` uses.

### The remote side

`sessions` answers from the VM daemon's shared pane poll with `list-sessions -F` chained into the same `tmux` call, so the VM starts no process for the link that it wasn't already starting. Grouped sessions carrying `@tc-mirror` are left out.

The bridge starts the VM's daemon when none runs, the way any client does, and says so in the hello (`startedDaemon`), which `hosts` reports.

## Configuration

```toml
[[hosts]]
name = "vm"                   # the word on every row and in every announcement
command = ["ssh", "vm"]       # one argv; `-t` and the remote command are appended
link = "on-demand"            # or "while-attached", or "off"
poll_secs = 5
idle_secs = 600
connect_secs = 10
open = "mirror"               # or "handover"; alt-enter in M-s hands over once
```

The remote command is appended as one argument, a shell line, because ssh hands its arguments to the remote shell joined into one; in tests the argv ends in `sh -c` and the same line runs locally. The wrapper the VM is reached with passes `-t` and a trailing command through to ssh, which this relies on.

### Adding and forgetting a host

The static `[[hosts]]` stays. A host can also be added from a popup:

- `hosts add NAME -- COMMAND...` is the command, with `--save` to write it to `config.toml` and without it until the local tmux server exits. It prints one line saying which.
- The popup is a view of it: two fields, name and command, one per line with a plain label, Tab between them, then `s` save to config or `t` keep it until the local tmux server exits, `Esc` to leave. The cursor sits in the field being typed into and every state is one line a screen reader reads where the cursor is. It has no key by default: `setup` offers one in its catalogue, and in `M-s` ctrl-n on a host's group line opens it.

Forgetting is ctrl-x in the session picker, which no picker in the shared screen binds today (`setup` uses it through its own key list):

- On a remote session row it forgets that cached row. The next poll brings it back if the session still exists.
- On a host's group line the picker closes, tmux's message line asks `forget vm? y/n` and `M-s` opens again on the same row after the answer; `y` removes the host's cache and its `[[hosts]]` entry. A host added for this tmux server only goes from the server option `@tc-hosts`.

Writing `config.toml` uses the same path `setup` already writes it with.

## Opening, step by step

1. The picker exits on Enter. Nothing it does waits on the network.
2. `switch-client -t '=api@vm'` if the mirror session exists.
3. Otherwise one tmux call: `new-session -d -s 'api@vm' 'tmux-companion host attach vm api'`, the session options (`@tc-host vm`, `@tc-remote api`, `prefix None`, `detach-on-destroy previous`), then `switch-client`.
4. The shim prints one plain line, `connecting to vm for api` and runs `command` with `-t` and `tmux new-session -t =api -s 'api~<mac>' \; set @tc-mirror <mac> \; set status off \; set destroy-unattached on`. That's a grouped session for this mirror session, its bar off and gone when the Mac detaches. ssh's own prompts, if any, are in a pane you're looking at.
5. When the attach ends: a clean detach closes the mirror session and `detach-on-destroy previous` puts the client back where it came from. Anything else leaves one line, `the connection to vm ended: <reason>` and waits for `r` to reconnect or `q` to close. No timer.

tmux's message line says `api on vm` on the switch, the way the switcher announces a local one.

### Keys in a mirror session

- `prefix None` on the mirror session, so the prefix goes to the VM. `prefix` is a session option, so the local prefix still works in every other session.
- `M-s` is the local picker with every host in it, the one key that crosses hosts.
- `M-a` and `M-A` run `switch windows` as today. It reads `@tc-host` in the one tmux call it already makes and, in a mirror session, hands the key to the pane with `send-keys`, so the VM's own switcher moves through the VM's windows in the grouped session. The binding doesn't change.

### The bar in a mirror session

One bar, the Mac's. The VM's is off for the grouped session only, so a person at the VM sees theirs as before.

- Left: `api@vm`, with `@vm` and the nf-md-ssh glyph (U+F08C0) in the host colour. The host colour is two tmux options, `@tc-host-fg` and `@tc-host-bg`, defined once: the example configs set them and the bar's ssh hostname segment reads them too instead of its own `color203` on `color233`, so the host looks the same wherever it's drawn and nothing in the code holds a colour.
- Right: the VM pane's segments, git, agents and health, from `segments` over the link for the grouped session's active pane, rendered by the Mac. The daemon sets them on the mirror session as `@tc-right` in every bar mode, call mode included, followed by this machine's own health as `here: <reason>` when it has one. When the link is down the right side says `vm: no link` in words, not an empty space.

## What each surface shows

- `switch sessions` (`M-s`): local sessions as today, then one group per host in config order. The group line is a row: `vm, 4 sessions, last heard 15m`. Under it each session as `api@vm` with `@vm` in the host colour; the ssh glyph is on the group line. When the host can't be reached its cached rows stay in a disabled tone with the word `unreachable` on each, and the reason is in the preview.
- `switch sessions --print`: the same lines in the same order, with no glyph and no padding. It's the source of truth the popup is a view of.
- `project`: every session, local and remote, as `M-s` shows them. Directories come from one place only: opened from a local session, the local source; opened inside a mirror session, that VM's source through `dirs` on the link. Picking a VM directory opens a mirror session whose shim runs `tmux-companion project --detached DIR` on the VM before the attach.
- `brief`: one line per host, `vm: 4 sessions, 1 agent asked, last heard 12s` or `vm: 4 sessions, last heard 3d, unreachable: Connection timed out`.
- `inbox`: the VM's agents with `on vm` in the place column while its link is up, and their earcons with them. Picking one opens the mirror session with `select-pane -t %N` chained after the attach, and when that mirror session is already open its shim respawns the attach with the same chain.
- `hosts`: each host in plain lines, its mode, link up or down and why, when it was last heard from, the VM's build and whether the bridge started its daemon. `doctor` gets a `hosts` line from the same function.
- Snapshots record mirror sessions with `@tc-host` and `@tc-remote`; `sessions resurrect` reopens each through its host with the shim waiting for `r`, the shim's command being a row the restore table runs.

Rows come from the daemon's cache with the first frame. A picker never swaps a refreshed list in under the cursor; the next open shows it.

## Failure modes

None of these is a health reason. A VM that's off is usually off on purpose.

| What happens | What it looks like | What a person is told |
| --- | --- | --- |
| VM off | ssh fails at `ConnectTimeout` | `unreachable` on the group and its rows, the reason in their preview, in `hosts` and `brief` |
| ssh hangs | no hello within `connect_secs` | `unreachable` as above, the reason `no answer in 10s`; the link is killed and retried |
| Auth wants a prompt | BatchMode refuses | `vm needs a login: run ssh vm once in a terminal` |
| Unknown or changed host key | BatchMode refuses | ssh's line, then `connect once with ssh vm in a terminal to accept the key` |
| Old remote build, no `bridge` | the remote shell says command not found | `vm runs tmux-companion 0.9.x, which has no bridge` |
| Older bridge, fewer methods | the hello doesn't list them | the feature is left out for that host, `hosts` says which |
| Remote daemon refuses its config | the bridge says so | the reason, as `doctor` says it locally |
| Slow network | answers late | rows say when the host was last heard; nothing in a picker waits |
| Connection drops while attached | the shim's ssh exits non-zero | the shim's one line, `r` reconnects |
| Remote sends something that isn't the schema | decoding fails | `vm sent something that isn't the protocol`, link restarted |

Host keys stay strict: `BatchMode yes` and no `accept-new`. The link's environment has `SSH_ASKPASS_REQUIRE=never` and no controlling terminal, and the docs say to set `BatchMode yes` and `ConnectTimeout` for the host in `~/.ssh/config` as well, for the attach.

## Security

- One direction. The Mac starts every link and asks every question. The VM can answer and, from phase 4, call the one sink object the Mac handed it. Nothing on the Mac listens, there's no `-R` and no new socket on either end.
- The VM decides what it answers. The interface has no method that runs a command or changes state, and `dirs` on the link runs the VM's own configured source, never a command the Mac names.
- No markup crosses. Segments arrive as text and colour names and the Mac builds the tmux string, so nothing the VM sends can reach `#(`.
- Remote data is untrusted on arrival. Lengths are capped (500 sessions, 4 KiB a string), control characters go, `#` is doubled before any tmux format and every name is shell-quoted into the attach. ssh joins its arguments into one remote shell line, so a session named `x'; touch pwned; '` is a real case. A test holds each of these.
- Actions happen in the open: opening, reconnecting, starting a project on the VM and jumping to a pane are all the attach in a pane, the same ssh you'd type.
- No credentials pass through tmux-companion. ssh's agent, keys and config do what they do today.

## Accessibility

The job: going to a session on another machine and knowing what's waiting there.

- The commands that do it in plain lines are `switch sessions --print`, `hosts` and `hosts add`. Lines in a stable order: local first, then hosts in config order, each group line before its sessions.
- The host is said in words on every row and on the bar, `api@vm`, never by the glyph or the colour alone. Ages are words too.
- The mirror session's switch is announced on the message line, `api on vm`, because focus moves to a session that didn't exist a moment ago. `forget vm? y/n` is asked on the message line, which is what a screen reader hears.
- The shim's states and the add-host popup's states are single plain lines, replaced once. Nothing animates and nothing is timed: reconnecting is a key, never a countdown.
- One bar in a mirror session. `read-bar` reads the VM's segments from the same `segments` call, the session and host first, then the Mac.
- `prefix None` makes the prefix do something different in one session, which is exactly the kind of thing nobody can see, so the manual says it in a sentence and `api@vm` on the bar says where you are.
- Screen-reader mode, when it lands, reads group lines and remote rows the way it reads local ones: no glyph, no padding.

## Cost under a capped CPU

- Before the first frame: one more socket question to the local daemon, which answers from memory or the disk cache. No process per host, none per row, nothing over the network. Rules 1 and 4 hold. Starting an on-demand link is the daemon's work after the question is answered.
- Chrome first doesn't change: the remote rows come with the local ones.
- Link up: one idle ssh process per host and one message each way every 5 s. The VM answers from its existing pane poll.
- The bar in a mirror session: one `segments` call per tick while the session has a client, no process, set as the mirror session's `@tc-right` in every bar mode.
- Opening: the shim and one ssh. A ControlMaster in `~/.ssh/config` makes later opens skip the handshake; the design doesn't manage one.
- Measure, `just bench-capped 0 32` with a fake host: `switch sessions` frame and first row with 0 and 20 remote rows (target: no change), the daemon's idle ms/s with a link up against none (target: under 0.5 ms/s more), the bar after a pane switch in a mirror session, and pick to the VM's first byte in the mirror pane with and without a ControlMaster.

## Testing

Two hosts on one machine, as `just bench-capped` already isolates one:

- Two tmux servers on their own sockets, `TMUX_TMPDIR` each, two daemons with their own `TMUX_COMPANION_SOCK`, `HOME` and XDG directories.
- The host command in the test config is `["env", "TMUX_COMPANION_SOCK=<b>", "TMUX_TMPDIR=<b-tmux>", "HOME=<b-home>", "sh", "-c"]`, so the bridge and the attach run against server B with no ssh in the path.
- Unit tests: shell quoting (every quote, `$`, backtick, newline), the hello and method gating, the default host name, segment rendering from structured fields with `#(` in every string, ages, row order, the disk cache round trip.
- e2e: `switch sessions --print` lists B's group and sessions; a pick creates `X@b` with `@tc-host`, `prefix None` and `detach-on-destroy previous`; server B gains a grouped session with `@tc-mirror`, `status off`, and B's own `switch sessions --print` doesn't list it; detaching closes `X@b` and the A client lands where it came from; `M-a` in `X@b` moves B's grouped session to its next window.
- Link modes: `on-demand` starts nothing until the picker opens and stops after `idle`; `off` starts nothing ever.
- Fault tests with fake hosts: one that exits at once, one that sleeps, one that writes bytes that aren't the schema, an older bridge with fewer methods, a session named with a quote and a semicolon.
- Real ssh stays a manual check against `localhost` with Remote Login on, written in the how-to.
- Every pinned tmux, 3.4, 3.5a and 3.7c, for `prefix None` per session, `detach-on-destroy previous`, `destroy-unattached on` on a grouped session and `status off` on one session of a group.

## Phases

1. **The link.** `schema/link.capnp` with its generated Rust committed, `just capnp` and the CI staleness check, `tmux-companion bridge` with the versioned hello, `[[hosts]]` with one argv, the three link modes and the poll, the disk cache, `hosts`, `hosts add` and its `doctor` line, the two-server harness and every fault test. Nothing in a picker yet.
2. **Remote sessions in `M-s`, opened as mirror sessions.** Host groups with last-heard ages, unreachable rows in the `offline` tone with the reason in the preview, `--print`, ctrl-x forget for rows and hosts with the message-line confirm and `--at`, the add-host popup on ctrl-n with its default name and test, the shim with the grouped session on the VM and `@tc-mirror` filtered out of the VM's own pickers, `prefix None`, `M-a` routed by `switch windows`, the mirror session's bar with `segments`, the host colour options and the hostname segment reading them, `sessions start NAME@HOST` and the announcement.
3. **The rest of the read side.** `brief` lines from the cache, the project picker's sessions and one-place directories through `dirs` with `project --detached` on the VM, the VM's agents in the inbox with their earcons, snapshots that record mirror sessions, a resurrect that reopens them waiting for `r` and the shipped restore row for the shim.
4. **Push.** `watch` with a sink capability replaces the poll for sessions, the inbox and mirror segments, `ping` every 15 s keeps the link honest, and `read-bar` reads the VM's segments.
5. **The local socket in Cap'n Proto.** Its own release: the binary's client moves to Cap'n Proto, the daemon keeps answering JSON indefinitely by the first byte, `daemon` and `proto` keep their Rust API with the new wire and a JSON fallback for an old daemon, and the release takes a minor bump and a CHANGELOG line under Changed.
6. **Handover.** `open = "handover"` per host and alt-enter on a remote row in `M-s` for a one-off, coming back with `exec tmux attach -t =<the session you left>`.

## Phase details

What each phase settles before any code is written, every point decided 2026-10-12. Facts checked on the way are said where they're used.

### Phase 2: remote sessions in `M-s`

#### Rows and wording

**Group line.** One row per host before its sessions: `vm, 4 sessions, last heard 15m` and `vm, 4 sessions, live` while the link is up. Searchable by the host's name so typing `vm` keeps the group and its rows.

**Session row.** Label `api@vm` so the host is in the words a screen reader reads and a search for `@vm` works, then the same `idle 3d` column local rows carry and `attached on vm` when somebody at the VM is in it.

**Order inside a group.** The VM's own recency, `@tc_ses_used` then `session_last_attached`, the same rule as local rows.

**A mirror session that's open** is a local session too, with its own `@tc_ses_used` stamp so it would be listed twice. It stays in the local part where recency puts it and where `M-s` once takes you back to it and its row in the host group is left out. The group line's count still counts it.

**Unreachable.** The cached rows stay, drawn in a disabled tone, and each says so in words with a column `unreachable`, as does the group line (`vm, 4 sessions, last heard 3d, unreachable`), so the state never rests on the tone alone. The reason, `Connection timed out` with when it was last tried, is in the preview pane of the group line and of every one of its rows. Enter on any of them retries the link instead of opening, and says `vm: connecting` on the message line. `--print` puts the reason on the group line, `vm, 4 sessions, last heard 3d, unreachable: Connection timed out`.

**Enter on a group line** starts or retries that host's link and says `vm: connecting` on the message line. It goes nowhere, since a group isn't a place.

**The disabled tone** is a new picker tone, `offline`. `Dim` already means detail or a repeat of the row above and one style carries one meaning.

**Unreachable rows** are skipped by `M-s` and `M-S` presses: Enter on one retries instead of going anywhere so a step onto it would be a step to nowhere.

**Steps are over sessions only.** `M-s` and `M-S` step over group lines and land only on sessions, local or remote, and the cursor's start rule (second row) counts sessions only.

**Announcement.** `api on vm, 5 of 9`, the switcher's `ai, 2 of 4` with the host in words.

**Colour.** The picker colours by meaning, one tone per meaning (`paint.rs`). A host is a meaning so it's a new tone, `host`, painted from `@tc-host-fg`, and it paints only the `@vm` part. A row's label can't be half one colour so the row is `api` plus a column `@vm` in the host tone; the label the matcher and a screen reader get is still `api@vm`.

**Glyph.** nf-md-ssh on the group line only and the session glyph on each row as local rows have it.

**A typed `name@vm`** with no row for it creates the session on the VM (`tmux new-session -d -s name` before the grouped attach) and opens it, the way a typed path opens a project.

**Directories in `M-s`** follow the project picker's rule from phase 3: inside a mirror session a typed path lists and opens the VM's directories. Until phase 3 they stay local.

#### Forgetting with ctrl-x

The shared picker ends on a ctrl key from a picker's own list (`Ended::Key`) which is how `setup` uses ctrl-x and ctrl-e today; a key can't hold the picker open and ask.

**Confirm.** ctrl-x on a group line closes the picker and asks on tmux's message line, `forget vm? y/n`, with `command-prompt -1` so one key answers and every key reaches the command, not only `y` as `confirm-before` would. Any key but `y` is no. Either way the command then opens `M-s` again with the cursor on the row it was on, found by identity through a hidden `--at` flag on `switch sessions`. The message line is what a screen reader hears, and nothing in the popup has to learn to ask.

**A remote session row** is forgotten without a question. It's a cache entry and the next poll brings it back if the session is still there.

**ctrl-x on a local row** says why on the message line, `api is on this Mac; only remote rows can be forgotten`, and changes nothing.

**Forgetting a host with a mirror session open** is refused with the reason on the message line, `api@vm is open; close it first`, rather than closing a session somebody may be in.

**Writing config.toml.** `setup::merge` adds a `[[hosts]]` entry only when no entry of that name exists which is `hosts add --save`. Removing one is new, beside it, with the same comment-keeping `toml_edit` document and the same write through a symlink to the real file.

#### Adding a host

**Opening it.** No default binding; `setup` offers one in its catalogue. In `M-s` ctrl-n on a host's group line opens the popup and since a server with no hosts has no group line, `hosts add` and the `setup` key are the ways to the first one.

**Fields.** Name and command, each a labelled line, Tab between them. The link mode is `on-demand` and changes in the config file.

**Name.** Letters, digits, `-` and `_`, up to 32 because it ends up in session names, file names under `hosts/` and shell lines. The field comes pre-filled with a name that can't collide, taken from the command once it's typed: its last word that doesn't start with `-`, with any `user@` dropped, cut at the first `.`, every other character turned into `-` and cut to 32, so `ssh dev@vm.local` gives `vm`. When `vm` is taken it's `vm-2`, then `vm-3`.

**Command.** Split into argv by shell-word rules without running a shell: quotes group and `$`, `|`, `;` and backticks are refused with a line saying so.

**Test.** Enter on the command runs the hello once and shows one line, `vm answered: tmux-companion 0.10.0, tmux 3.4` or the reason. Then `s` saves to config, `t` keeps it for this tmux server only, Esc leaves.

**This session only** means until the local tmux server exits. The host goes in a server option, `@tc-hosts`, so it survives a daemon restart and dies with the server it was added for.

**A name already in use**, typed over the default, is refused with one line naming the existing host's command.

#### The mirror session

**One grouped session per mirror session.** A mirror session's pane is one VM client however many Mac clients look at it so the grouped session is one per mirror session rather than one per Mac client. Its name is `api~<Mac short hostname>` so two Macs on one VM don't collide.

**tmux on the VM.** Everything the attach uses, grouped sessions, `status off` on one session of a group, `destroy-unattached on`, is in 3.4's manual and `detach-on-destroy previous` and `prefix None` on the Mac are too (read in the pinned 3.4, 3.5a and 3.7c pages). A VM on an older distro may have less. The hello reports the VM's tmux and the floor is 3.4 there as well; `hosts` says so in a line when a VM's tmux is older.

**Window size.** A grouped session shares its windows and `window-size` is a window option so the Mac and anybody at the VM resize the same windows. The VM's setting is left alone and the manual says so.

**The failure screen.** The shim's whole screen is plain lines, replaced once:

```text
connecting to vm for api
```

then, on anything but a clean detach:

```text
the connection to vm ended: Connection timed out
r reconnect   q close
```

Reconnecting is `r` only. A link coming back doesn't restart an attach by itself, since that would put a VM screen where a person may be reading the failure.

If the VM no longer has the session the line says `api no longer exists on vm` and waits for `q`, the same as any other ending.

Endings are told apart by ssh's exit status (255 is ssh's own failure), tmux's exit after a detach (0) and the last stderr line which is what the reason shows.

#### Keys and the bar

**Root keys.** `M-s`, `M-S`, `M-g` and `M-z` stay local, since each is about everything or about you. `M-a` and `M-A` go to the VM: `switch windows` adds `#{@tc-host}` to the one call it already makes and, in a mirror session, sends the key to the mirror pane. tmux sends it as `ESC a` in one write so the VM's `escape-time` reads it as `M-a`.

**The local prefix** isn't reachable in a mirror session; `M-s` leaves. A `prefix2` the person set stays local, since `prefix None` touches only `prefix`.

**The VM's announcements** still show. With `status off`, tmux gives a message or a prompt one line at the bottom anyway (`lines = (lines == 0) ? 1 : lines` in `screen_redraw_set_context`, read in the 3.4 and 3.7c source) so the VM's switcher speaks inside the mirror pane.

**Call mode.** `#(tmux-companion status-right ... #{pane_current_path})` would render the shim's local directory in a mirror session. The daemon sets `@tc-right` on mirror sessions in every bar mode and the example `status-right` reads it first when `@tc-host` is set.

**Segment refresh.** One `segments` call per bar tick, every second, while a mirror session has a client attached.

**The Mac's health** stays on the bar in a mirror session, after the VM's segments, as `here: <reason>`, so a local problem isn't hidden by being elsewhere.

**The whole bar** keeps its colours in a mirror session and only `@vm` and the glyph take the host colour. A styled thing means one thing and a bar recoloured by place would say "remote" with every segment while the words `api@vm` already say it.

**Left side.** ` api@vm 󰣀 `, the glyph after the name: `api` in the session colours, then `@vm` and the glyph in the host colours, written in the example `status-left` with `#{?@tc-host,...}` so no process runs.

**Clicks.** `MouseDown1StatusRight` opens what a local segment's click opens. On a mirror bar the segments are the VM's and a click on them does nothing.

### Phase 3: the rest of the read side

**`brief`.** One line per host, `vm: 4 sessions, 1 agent asked, last heard 12s`, or `vm: 4 sessions, last heard 3d, unreachable: Connection timed out`.

**`brief` reads the cache only and never starts a link.** It runs from the `client-attached` hook so starting links there would make every host `while-attached` in effect.

**A VM project.** `project DIR` on the VM switches or attaches which would put the Mac on the real session rather than a grouped one. It gains `--detached`, create the session and print its name and the shim runs that, then the grouped attach.

**Sessions and projects in the project picker.** Inside a mirror session it shows that VM's projects and every session on every host; opened from a local session, the Mac's projects and every session on every host.

**Inbox pick** on a VM agent whose mirror session is open: the pane has to move on the VM and the link is read-only. The shim respawns its attach with `select-pane -t %N` chained, one reconnect which a ControlMaster makes quick. The link stays read-only.

**While the Mac is quiet** the VM's earcons are silent as local ones are, whatever makes the Mac quiet: the quiet state is being reworked on its own branch to follow macOS Focus, and this reads that state rather than a schedule. The VM's entry text is untrusted and goes through the same sanitising as names.

**Resurrect** brings mirror sessions back with the shim showing `vm: press r to connect`, rather than a dozen ssh handshakes at once after a reboot.

**The restore table** gets a shipped row for the shim so a resurrect doesn't ask about it.

### Phase 4: push

**Liveness.** With nothing to poll, a dead link would go unnoticed. A Cap'n Proto `ping` every 15 s, three missed and the link is restarted, in the link itself so it doesn't depend on `~/.ssh/config`.

**Pushed.** Sessions, the inbox and the segments of any grouped session with a Mac client, each when it changes on the VM's poll.

**`read-bar`** in a mirror session: `api on vm. vm: branch main, 1 agent waiting. This Mac: nothing wrong.` The session and host first, then the VM's segments, then the Mac's health in words. `read-bar` is on the screen-reader branch, not main so this lands with whichever of the two comes second.

### Phase 5: the local socket in Cap'n Proto

**Why.** A status-right round trip is a few hundred microseconds today so Cap'n Proto won't move a measured moment by the 8 ms design-perceived.md asks. Phase 5 is one protocol in the code instead of two and that's the reason it carries.

**The JSON window.** `daemon.rs` documents the JSON line as the whole protocol for other programs so the daemon keeps answering JSON indefinitely, told apart by the first byte, and only the binary's own client moves.

**The library.** `daemon::Daemon`'s methods and the `proto` argument structs stay the Rust API and the library speaks Cap'n Proto to a daemon whose hello says it can and JSON otherwise. The minor bump and the CHANGELOG line still go in, since `Request` and `Response` change.

### Phase 6: handover

**Per host or per pick.** `open = "handover"` on a host and alt-enter on a remote row in `M-s` to hand over once, since alt-enter already means "do the other thing" in the picker.

**Coming back** is `exec tmux attach -t =<the session you left>` so a detach from the VM lands where you were.

### Configuration names and defaults

Per-host keys only, each with a built-in default, because `[hosts]` can't be both a table of defaults and an array of tables.

### Measured, with budgets

Each phase lands with `just bench-capped 0 32` and a fake host.

**Phase 1.** The daemon's idle cost with one link up against none: at most 0.5 ms/s more at load 0. The hello over the fake host: 50 ms p50 at load 32.

**Phase 2.** `M-s` frame and first row within 8 ms of today's with 20 remote rows, at load 32. Pick to the VM's first byte in the mirror pane over the fake host, 150 ms p50 at load 32. The mirror bar after a pane switch on the VM, 1.5 s p50, since it rides the bar's one-second tick. The real VM is measured and written down, not budgeted, as its network isn't ours.

**Phase 3.** A VM agent asking to the Mac's earcon, 8 s p95 while polling: the VM inbox's 2 s plus the 5 s poll and a second of slack.

**Phase 4.** The same earcon within 3 s p95 with `watch` and the idle cost no higher than phase 1's poll.

**Phase 5.** No measured moment worse than today's, the status-right round trip included.

## Out of scope

- Control mode as the way to open a remote session, option (b), for the reasons under it. Control mode as the daemon's own link to its own tmux is a separate idea in design-capped-cpu.md.
- mosh and Eternal Terminal as the transport. They keep an interactive session alive across a network change, which suits the attach, but the link needs a plain pipe and gets one from ssh.
- The VM reaching back: the VM's `M-s` listing the Mac's sessions is the same design pointed the other way.
- Managing ssh: keys, agents, ControlMaster and known hosts stay in `~/.ssh/config`.
