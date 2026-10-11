#!/usr/bin/env python3
"""Pickers and the daemon's idle cost, phase by phase, for one CPU condition.

usage (through the wrapper, which is what isolates it):
  scripts/bench-capped.sh                     loads 0, 16 and 32
  scripts/bench-capped.py --label load16 --json out.json

Run it through `scripts/bench-capped.sh`. That wrapper is what makes it safe: it
points HOME, every XDG directory and TMUX_TMPDIR at a temporary directory, so
the daemon started here cannot read the real config's state directory, write
the live daemon's crash marker, or find the live tmux server with a bare
`tmux`. Run on its own, this script refuses to start unless BENCH_TMP is set.

What one press measures, in wall-clock seconds shared with the binary through
TMUX_COMPANION_TRACE (src/trace.rs):

  spawn   key sent      -> the picker's main()   tmux, the shell, exec, dyld
  rows    main()        -> every row and preview exists
  paint   rows          -> Paint::detect done    a tmux call and a config read
  draw    paint         -> first frame drawn
  seen    key sent      -> first row's bytes at the client, the number a person
                           waits through

and then a burst of five keys typed in one send-keys, timed from the send to
the frame drawn after the last of them, which is what typing a query feels
like when frames are slower than fingers.

Idle cost is the daemon's and the tmux server's CPU over a quiet window with
the status bar ticking every second, read from `ps` before and after.
"""
import argparse, fcntl, json, os, pty, re, statistics, struct, subprocess, sys, termios, time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BIN = os.environ.get("BENCH_BIN", str(REPO / "target/release/tmux-companion"))
TMP = os.environ.get("BENCH_TMP", "")
if not TMP or not os.environ.get("TMUX_TMPDIR", "").startswith(TMP):
    raise SystemExit("run this through scripts/bench-capped.sh; it isolates HOME, XDG and TMUX_TMPDIR")
TMP = Path(os.path.realpath(TMP))   # macOS: /var -> /private/var, and pickers print the real path
SOCK = str(TMP / "tc.sock")
# The private tmux server, named outright. tmux reads $TMUX before
# TMUX_TMPDIR, so a bare `tmux` run from a pane talks to that pane's server;
# see the note at the top of bench-capped.sh for what that cost once.
TSOCK = str(TMP / "tmux" / f"tmux-{os.getuid()}" / "default")
Path(TSOCK).parent.mkdir(mode=0o700, parents=True, exist_ok=True)
os.environ.pop("TMUX", None)
os.environ.pop("TMUX_PANE", None)
TRACE = TMP / "trace.tsv"
ZO = TMP / "zoxide"
DIRS = TMP / "dirs"
WIDTH, HEIGHT = 160, 44
QUIET = "stty -echo; exec sleep 100000"
# How the bar reaches tmux: `call` (the call-only line), `auto` (the fallback
# line, config left alone, so the default mode decides) or `push` (the
# fallback line and `[bar] mode = "push"`).
BAR = os.environ.get("BAR", "call")

ESCAPES = re.compile(rb"\x1b\[[0-9;?]*[a-zA-Z]|\x1b[\]\(][^\x07\x1b]*(?:\x07|\x1b\\)?")


def flat(raw):
    """The bytes with escapes and whitespace gone; see bench-keys.py for why."""
    return re.sub(rb"\s+", b"", ESCAPES.sub(b"", raw))


def stats(xs):
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return None
    def pct(q):
        return xs[min(len(xs) - 1, round(q * (len(xs) - 1)))]
    # p99 of 15 presses is the slowest of them; it says something only over
    # RUNS=100 or more, which is what a p99 budget wants checked with.
    return {"n": len(xs), "min": xs[0], "p50": statistics.median(xs), "p95": pct(0.95),
            "p99": pct(0.99),
            "mean": statistics.mean(xs), "max": xs[-1]}


def trace_lines(since):
    """(pid, what, t) for every mark at or after `since`."""
    out = []
    try:
        text = TRACE.read_text()
    except FileNotFoundError:
        return out
    for line in text.splitlines():
        parts = line.split("\t")
        if len(parts) == 3:
            t = float(parts[2])
            if t >= since:
                out.append((parts[0], parts[1], t))
    return out


class Tmux:
    """A private tmux server, a generated config, one real client on a pty."""

    def __init__(self, env):
        self.env = env
        self.fd = self.pid = None

    def __call__(self, *a, check=False):
        r = subprocess.run(["tmux", "-S", TSOCK, *a], capture_output=True, text=True,
                           env=self.env)
        if check and r.returncode:
            raise SystemExit(f"tmux {' '.join(a)}: {r.stderr.strip()}")
        return r.stdout.strip()

    def start(self, conf, cwd):
        # The pane under the popups runs nothing that echoes: a key that went
        # to it instead of to the picker must not show up on the pty and be
        # counted as a key the picker drew.
        self("-f", str(conf), "new-session", "-d", "-s", "bench", "-c", cwd,
             "-x", str(WIDTH), "-y", str(HEIGHT), "sh", "-c", QUIET, check=True)
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ.clear()
            os.environ.update(self.env)
            os.execvp("tmux", ["tmux", "-S", TSOCK, "attach", "-t", "bench"])
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", HEIGHT, WIDTH, 0, 0))
        os.set_blocking(self.fd, False)
        for _ in range(200):
            if self("list-clients", "-F", "#{client_name}"):
                break
            time.sleep(0.05)
        else:
            try:
                said = os.read(self.fd, 65536)
            except OSError:
                said = b""
            gone = os.waitpid(self.pid, os.WNOHANG)
            raise SystemExit(f"no client attached; the client said: {flat(said)[-300:]!r}; "
                             f"exited {gone}; sessions: {self('list-sessions')!r}")
        self.client = self("list-clients", "-F", "#{client_name}").splitlines()[0]
        self.drain(quiet=0.5)

    def drain(self, quiet=0.15, limit=5.0):
        t0 = last = time.perf_counter()
        while time.perf_counter() - t0 < limit:
            try:
                chunk = os.read(self.fd, 65536)
            except (BlockingIOError, OSError):
                chunk = b""
            if chunk:
                last = time.perf_counter()
            elif time.perf_counter() - last >= quiet:
                return
            else:
                time.sleep(0.005)

    def bind(self, key):
        """A binding key (M-s and the rest), handled by tmux as if typed."""
        subprocess.run(["tmux", "-S", TSOCK, "send-keys", "-K", "-c", self.client, key],
                       capture_output=True, env=self.env)

    def keys(self, *keys):
        """Keys for the program in the popup, written to the client's pty
        the way a person's keyboard writes them, in one write.

        Not `send-keys -K`: tmux 3.4 runs root-table bindings for a key sent
        that way but hands nothing to a popup, so on the floor version every
        Escape was dropped, the first picker never closed, and each later
        press landed inside it. Found on the first run with a pinned 3.4;
        3.5a and later deliver both ways.
        """
        os.write(self.fd, b"".join(KEYBYTES.get(k, k.encode()) for k in keys))

    def press(self, key, marker, timeout=20.0, frame=None, then=()):
        """(wall time of the send, ms until `marker` reached the client or
        None, ms until `frame` did or None).

        `then` is sent the moment the key has been: keys typed before the
        picker can have drawn anything, the way a fast typist types.
        """
        self.drain()
        seen = b""
        wall = time.time()
        t0 = time.perf_counter()
        self.bind(key)
        if then:
            self.keys(*then)
        framed = None
        while time.perf_counter() - t0 < timeout:
            try:
                chunk = os.read(self.fd, 65536)
            except (BlockingIOError, OSError):
                time.sleep(0.0005)
                continue
            if not chunk:
                break
            seen += chunk
            flat_seen = flat(seen)
            if frame is not None and framed is None and flat(frame) in flat_seen:
                framed = (time.perf_counter() - t0) * 1000.0
            if flat(marker) in flat_seen:
                # A picker that draws its rows before its title (the title
                # sits on a border that is written after the list) is read on
                # until the title has come too.
                if frame is not None and framed is None:
                    marked = getattr(self, "_marked", None) or (time.perf_counter() - t0) * 1000.0
                    self._marked = marked
                    continue
                marked = getattr(self, "_marked", None)
                self._marked = None
                return wall, marked or (time.perf_counter() - t0) * 1000.0, framed
        marked = getattr(self, "_marked", None)
        self._marked = None
        if marked is not None:
            return wall, marked, framed
        self.missed = flat(seen)[-600:]
        return wall, None, framed

    def stop(self):
        self("kill-server")
        if self.pid:
            try:
                os.close(self.fd)
                os.waitpid(self.pid, 0)
            except OSError:
                pass


def seed(n_dirs):
    """Directories with files in them, in a private zoxide database.

    The project picker previews a directory by listing it, so empty ones would
    measure a picker with nothing to do.
    """
    DIRS.mkdir(parents=True, exist_ok=True)
    ZO.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, _ZO_DATA_DIR=str(ZO))
    paths = []
    for i in range(1, n_dirs + 1):
        d = DIRS / f"dir-{i:03d}"
        d.mkdir(exist_ok=True)
        for j in range(20):
            (d / f"file-{j:02d}.txt").touch()
        paths.append(str(d))
    subprocess.run(["zoxide", "add", *paths], env=env, check=True, capture_output=True)


# A program that prints a file and then waits, compiled rather than copied: a
# copy of a system binary is killed on sight by code signing on macOS, and a
# script reports its interpreter's name to tmux rather than its own. The name
# is what matters. claude sets its process title to its version, which is how
# the daemon tells an agent pane from a shell (panes.rs, is_version_name), so
# the agent stub is called `2.1.0`; the editor stub is called `nvim`.
# With a second argument it prints a line every that many seconds, which is
# what a working agent looks like to the inbox loop.
STUB_C = r"""
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
int main(int c, char **v) {
    FILE *f = c > 1 ? fopen(v[1], "r") : NULL;
    if (f) { int ch; while ((ch = fgetc(f)) != EOF) putchar(ch); fclose(f); }
    fflush(stdout);
    int every = c > 2 ? atoi(v[2]) : 0;
    for (long n = 0;; n++) {
        if (every > 0) { sleep(every); printf("\x1b[2m  working, step %ld\x1b[0m\n", n); fflush(stdout); }
        else pause();
    }
}
"""
STUBS = TMP / "bin"
AGENT, EDITOR = "2.1.0", "nvim"


def stubs():
    STUBS.mkdir(exist_ok=True)
    src = TMP / "stub.c"
    src.write_text(STUB_C)
    for name in (AGENT, EDITOR):
        subprocess.run(["cc", "-O1", "-o", str(STUBS / name), str(src)], check=True)


def screens():
    """What each kind of pane has printed: an agent's transcript ending in a
    question, an editor's screenful, a shell's scrollback. Colour and box
    drawing included, since the previews parse ANSI and that is part of the
    cost being measured."""
    out = TMP / "screens"
    out.mkdir(exist_ok=True)
    c = lambda n, t: f"\x1b[38;5;{n}m{t}\x1b[0m"
    agent = []
    for i in range(300):
        agent += [c(214, "\u23fa") + f" Read(src/module_{i % 40}.rs)",
                  c(244, "  \u23bf  Read 120 lines"),
                  f"  fn step_{i}(x: usize) -> usize {{ x * {i} + 1 }}",
                  c(70, "\u23fa") + f" Update(src/module_{i % 40}.rs) with {i % 7 + 1} additions",
                  ""]
    box = "\u256d" + "\u2500" * 70 + "\u256e"
    agent += [box, "\u2502 Do you want to make this edit to module_7.rs?" + " " * 24 + "\u2502",
              "\u2502 \u276f 1. Yes" + " " * 59 + "\u2502",
              "\u2502   2. No, and tell Claude what to do differently" + " " * 22 + "\u2502",
              "\u2570" + "\u2500" * 70 + "\u256f"]
    (out / "agent.txt").write_text("\n".join(agent) + "\n")
    editor = [c(240, f"{n:4d} ") + c(75, "fn") + f" handler_{n}(req: Request) -> Response {{"
              for n in range(1, 42)]
    editor.append(c(16, "\x1b[48;5;150m NORMAL ") + " src/server/handlers.rs   rust  utf-8  42:1 ")
    (out / "editor.txt").write_text("\n".join(editor) + "\n")
    shell = []
    for n in range(400):
        shell.append(c(39, "~/dir-007") + c(70, " \u276f ") + f"cargo test --lib case_{n}")
        shell.append(f"test case_{n} ... " + c(70, "ok"))
    (out / "shell.txt").write_text("\n".join(shell) + "\n")
    return out


def push_config():
    """Turn on `[bar] mode = "push"` in the bench home's config, adding the
    line to a copied config's `[bar]` table or writing one."""
    import re
    path = Path(os.environ.get("XDG_CONFIG_HOME", Path(os.environ["HOME"]) / ".config"))
    path = path / "tmux-companion" / "config.toml"
    path.parent.mkdir(parents=True, exist_ok=True)
    text = path.read_text() if path.exists() else ""
    if re.search(r"(?m)^\[bar\]\s*$", text):
        text = re.sub(r"(?m)^(\[bar\]\s*)$", "\\1\nmode = \"push\"", text, count=1)
    else:
        text += '\n[bar]\nmode = "push"\n'
    path.write_text(text)


def spawn_price(n=20):
    """ms of CPU one `#()` call costs: the shell and the client, as tmux
    runs them, timed here the same way."""
    import resource
    env = dict(os.environ, TMUX_COMPANION_SOCK=SOCK)
    env.pop("TMUX_COMPANION_TRACE", None)
    cmd = f"{BIN} status-right --branch-max-len 40 {REPO}"
    b = resource.getrusage(resource.RUSAGE_CHILDREN)
    for _ in range(n):
        subprocess.run(["sh", "-c", cmd], env=env, capture_output=True)
    a = resource.getrusage(resource.RUSAGE_CHILDREN)
    return ((a.ru_utime - b.ru_utime) + (a.ru_stime - b.ru_stime)) * 1000 / n


def switch_config():
    """A config for the window switcher alone: Enter commits, no clock."""
    path = TMP / "switch.toml"
    path.write_text('[switcher]\ncommit = "enter"\n')
    return path


def conf_file():
    """The tmux.conf the bench server starts from.

    With a copy of your own tmux.conf in the bench HOME (CONFIG=mine in the
    wrapper), that is loaded first, so the keys picker has your bindings to
    list and the hooks you run are running. The attach hooks are then taken
    off: a popup opened by an attach hook would take the keys the bench sends.
    The bar and the bindings below come last, so they hold either way.

    Without one (CONFIG=default, and any host that is not yours) the keys
    picker lists only bindings with a `companion:` note, of which tmux has
    none, so it drew its empty message and never its marker. The starter
    config is loaded in its place, the 27 noted bindings a new user starts
    from, with four noted select-pane bindings for the marker to find.
    """
    conf = TMP / "tmux.conf"
    mine = Path(os.environ["HOME"]) / ".config/tmux/tmux.conf"
    bar = f"#({BIN} status-right --branch-max-len 40 #{{pane_current_path}})"
    if BAR in ("push", "auto"):
        # What the docs recommend: the pushed value, the call while it is unset.
        bar = ("#{?#{&&:#{@tc-right},#{e|<:%s,#{e|+:#{@tc-alive},30}}},#{@tc-right},"
               + bar + "}")
    lines = []
    if mine.exists():
        lines += [f'source-file "{mine}"', "set-hook -gu client-attached"]
    else:
        lines += [f'source-file "{REPO / "docs/tmux.conf.starter.example"}"']
        lines += [f'bind -N "companion: pane {name}" {k} select-pane -{d}'
                  for k, d, name in (("h", "L", "left"), ("j", "D", "down"),
                                     ("k", "U", "up"), ("l", "R", "right"))]
    lines += [
        # A lone ESC on the pty is held this long before tmux calls it
        # Escape. 10 ms is the default from 3.5 on; 3.4's is 500, which would
        # hold every closing Escape for half a second on the floor version.
        "set -s escape-time 10",
        "set -g status-interval 1",
        f'set -g status-right "{bar}"',
        "set -g status-right-length 150",
        "set -g history-limit 5000",
        f'bind -n M-s display-popup -B -E -w 80% -h 70% "{BIN} project"',
        f'bind -n M-c display-popup -B -E -w 65% -h 65% "{BIN} new-window"',
        f'bind -n M-p display-popup -B -E -w 80% -h 70% "{BIN} panes"',
        f'bind -n M-k display-popup -B -E -w 80% -h 60% "{BIN} keys"',
        'bind -n M-e display-popup -B -E -w 65% -h 65% "printf POPUPREADY; sleep 0.2"',
        f'bind -n M-o display-popup -B -E -w 80% -h 70% "{BIN} switch sessions"',
        # The window list with its clock off, so the burst typed after the
        # first frame is measured rather than racing a commit on the pause.
        f'bind -n M-w display-popup -B -E -w 50 -h 12 '
        f'"TMUX_COMPANION_CONFIG={switch_config()} {BIN} switch windows fx-01"',
    ]
    conf.write_text("\n".join(lines) + "\n")
    return conf


def fill(t, args):
    """A working day's server: sessions of single-pane windows, the panes a
    mix of agents (some working, the rest waiting on a question), editors and
    shells, each with a screen and scrollback of plausible output.

    The defaults are one real setup on a weekday, counted from its own
    snapshots: 8 sessions, 16 windows, 7 agents, 6 editors, 3 shells and 166
    directories in zoxide. They are flags, so a heavier day is one argument.
    """
    stubs()
    scr = screens()
    kinds = (["busy"] * args.busy + ["agent"] * (args.agents - args.busy)
             + ["editor"] * args.editors + ["shell"] * args.shells)
    cmd = {
        "busy": [str(STUBS / AGENT), str(scr / "agent.txt"), "3"],
        "agent": [str(STUBS / AGENT), str(scr / "agent.txt")],
        "editor": [str(STUBS / EDITOR), str(scr / "editor.txt")],
        "shell": ["sh", "-c", f"cat {scr / 'shell.txt'}; exec sleep 100000"],
    }
    made = set()
    for i, kind in enumerate(kinds):
        s = i % args.sessions + 1
        name = f"fx-{s:02d}"
        cwd = str(DIRS / f"dir-{s:03d}")
        if name in made:
            t("new-window", "-d", "-t", name, "-c", cwd, *cmd[kind], check=True)
        else:
            t("new-session", "-d", "-s", name, "-c", cwd, *cmd[kind], check=True)
            made.add(name)


PICKERS = [
    # name, key, marker for the first row, marker for the frame (its title)
    ("project", "M-s", b"fx-", b"Project ]"),
    # The row for the pane's own directory: the title is on the loading
    # frame now, which is drawn before any row exists.
    ("new-window", "M-c", b"here", b"New window at ]"),
    ("panes", "M-p", b"fx-", b"Panes ]"),
    ("keys", "M-k", b"select-pane", b"Keys ]"),
    ("switch sessions", "M-o", b"fx-", b"Projects ]"),
    # fx-01's windows, under the list's title; no loading frame comes first,
    # so the title is both markers.
    ("switch windows", "M-w", b"Windows", b"Windows ]"),
    ("empty popup", "M-e", b"POPUPREADY", None),
]
# Typed straight after the binding key: no row contains it, so it shows on
# the pty only as the picker's query, and only if every key reached it.
TYPEAHEAD = ["z", "z", "q", "x"]
# Typed into an open picker before one more key is timed: input lag is
# measured with a long query, the way zsh-bench measures it with a long
# command line.
LONG = list("abcdefghijklmnopqrst")
BURST = ["d", "i", "r", "-", "0"]
# The bytes a terminal sends for the named keys the bench types; anything else
# is its own character.
KEYBYTES = {"Escape": b"\x1b", "Enter": b"\r", "BSpace": b"\x7f", "C-c": b"\x03"}


def phases(wall, marks):
    """The phase split for the first picker process that started after `wall`."""
    # The bar's own `status-right` clients mark `main` too, once a second, so
    # the picker is the first process after the press that got as far as rows.
    pickers = {pid for pid, what, _ in marks if what == "rows"}
    mains = [(t, pid) for pid, what, t in marks
             if what == "main" and t >= wall and pid in pickers]
    if not mains:
        return None
    t_main, pid = min(mains)
    mine = [(what, t) for p, what, t in marks if p == pid]

    def first(what):
        ts = [t for w, t in mine if w == what]
        return min(ts) if ts else None

    rows, paint, drawn, chrome = first("rows"), first("paint"), first("drawn"), first("chrome")
    if None in (rows, paint, drawn):
        return {"pid": pid, "spawn": (t_main - wall) * 1000}
    out = {"pid": pid, "spawn": (t_main - wall) * 1000, "rows": (rows - t_main) * 1000,
           "paint": (paint - rows) * 1000, "draw": (drawn - paint) * 1000}
    # A picker that draws its frame before its rows: main to that frame.
    if chrome is not None:
        out["chrome"] = (chrome - t_main) * 1000
    return out


def burst(t, pid, timeout=20.0):
    """ms from sending five keys to the frame drawn after the fifth, and the
    mean per-key handling time (key read to its frame)."""
    wall = time.time()
    t.keys(*BURST)
    deadline = time.time() + timeout
    while time.time() < deadline:
        mine = [(w, ts) for p, w, ts in trace_lines(wall) if p == pid]
        keys = [ts for w, ts in mine if w == "key"]
        if len(keys) >= len(BURST):
            after = [ts for w, ts in mine if w == "drawn" and ts > keys[len(BURST) - 1]]
            if after:
                drawn = sorted(ts for w, ts in mine if w == "drawn")
                per = []
                for k in keys[: len(BURST)]:
                    nxt = [d for d in drawn if d > k]
                    if nxt:
                        per.append((nxt[0] - k) * 1000)
                return (min(after) - wall) * 1000, statistics.mean(per) if per else None
        time.sleep(0.01)
    return None, None


def input_lag(t, pid, timeout=20.0):
    """ms from one more key to the frame drawn after it, with a 20-character
    query already typed and drawn, from the picker's own key and drawn marks:
    the pty cannot say which frame carried which key."""
    def settle(since, n):
        deadline = time.time() + timeout
        while time.time() < deadline:
            mine = [(w, ts) for p, w, ts in trace_lines(since) if p == pid]
            keys = [ts for w, ts in mine if w == "key"]
            if len(keys) >= n:
                after = [ts for w, ts in mine if w == "drawn" and ts > keys[n - 1]]
                if after:
                    return keys[n - 1], min(after)
            time.sleep(0.01)
        return None, None
    w = time.time()
    t.keys(*LONG)
    if settle(w, len(LONG))[0] is None:
        return None
    w = time.time()
    t.keys("u")
    key, drawn = settle(w, 1)
    return None if key is None else (drawn - key) * 1000


def bar_after_switch(t, runs=5):
    """ms from selecting the window in the repository to the branch name on
    the pty, coming from a window that is not in one."""
    branch = subprocess.run(["git", "-C", str(REPO), "branch", "--show-current"],
                            capture_output=True, text=True).stdout.strip()
    if not branch:
        return None
    # The part after the last slash: a branch type's prefix is drawn as its
    # icon, so with CONFIG=mine `release/next` reaches the bar as the icon and
    # `next`, the full name never appears, and every run read "never showed
    # the branch" (2026-10-10).
    branch = branch.rsplit("/", 1)[-1]
    # Window ids, not indexes: with CONFIG=mine the tmux.conf may set
    # base-index 1, and then `bench:0` named no window, the select failed
    # without a word, the repository's window never left the screen, and
    # what was timed was the clock's next redraw, about a second in every
    # mode (2026-10-09). check=True so a switch that did not happen stops the
    # bench rather than timing something else.
    home = t("display-message", "-p", "-t", "bench:", "#{window_id}", check=True)
    away = t("new-window", "-d", "-P", "-F", "#{window_id}", "-t", "bench:",
             "-c", str(DIRS / "dir-001"), "sh", "-c", QUIET, check=True)
    out = []
    for _ in range(runs):
        t("select-window", "-t", away, check=True)
        t.drain(quiet=2.5, limit=6.0)
        seen = b""
        t0 = time.perf_counter()
        t("select-window", "-t", home, check=True)
        while time.perf_counter() - t0 < 6.0:
            try:
                chunk = os.read(t.fd, 65536)
            except (BlockingIOError, OSError):
                time.sleep(0.0005)
                continue
            seen += chunk
            if flat(branch.encode()) in flat(seen):
                out.append((time.perf_counter() - t0) * 1000.0)
                break
        else:
            out.append(None)
    return stats(out)


def cpu_seconds(pid):
    """Total CPU time of one pid from ps, in seconds.

    On Linux from /proc/<pid>/stat instead: procps prints cputime in whole
    seconds, which over a 30-second idle window reads as 0 or 33 ms/s, while
    utime and stime there are clock ticks (10 ms)."""
    stat = Path(f"/proc/{pid}/stat")
    if sys.platform.startswith("linux") and stat.exists():
        try:
            # The command name is in parentheses and may hold spaces, so the
            # fields are counted from the last ')'; utime and stime are 14, 15.
            rest = stat.read_text().rsplit(")", 1)[1].split()
        except (OSError, IndexError):
            return None
        return (int(rest[11]) + int(rest[12])) / os.sysconf("SC_CLK_TCK")
    out = subprocess.run(["ps", "-o", "cputime=", "-p", str(pid)], capture_output=True,
                         text=True).stdout.strip()
    if not out:
        return None
    parts = [float(p) for p in out.replace("-", ":").split(":")]
    secs = 0.0
    for p in parts:
        secs = secs * 60 + p
    return secs


def rusage():
    """(self_us, children_us) from the bench daemon's own getrusage, over its
    socket: microsecond resolution, and the children are the tmux and git
    processes it started, which `ps` on the daemon does not count."""
    import socket
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(SOCK)
    s.sendall((json.dumps({"cmd": "__rusage", "args": {}}) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    s.close()
    su, ss, cu, cs = (int(x) for x in json.loads(buf)["output"].split()[:4])
    return su + ss, cu + cs


def idle_cost(t, daemon_pid, secs):
    """ms of CPU per second for the daemon and the tmux server, bar ticking."""
    server = t("display-message", "-p", "#{pid}")
    t.drain(quiet=1.0, limit=3.0)
    r0 = rusage()
    d0, s0, w0 = cpu_seconds(daemon_pid), cpu_seconds(server), time.time()
    end = time.time() + secs
    while time.time() < end:
        t.drain(quiet=secs, limit=min(1.0, end - time.time()))
    d1, s1, w1 = cpu_seconds(daemon_pid), cpu_seconds(server), time.time()
    r1 = rusage()
    span = w1 - w0
    marks = [(pid, what, t) for pid, what, t in trace_lines(w0) if t <= w1]
    calls = sum(1 for pid, what, _ in marks if what == "tmux" and pid == str(daemon_pid))
    # Every `#()` client marks `main`; nothing else starts one while idle.
    bar_spawns = sum(1 for pid, what, _ in marks if what == "main" and pid != str(daemon_pid))
    pushed = t("show-option", "-qv", "-t", "bench", "@tc-right")
    price = spawn_price()
    return {"secs": span,
            "daemon_ms_per_s": None if None in (d0, d1) else (d1 - d0) * 1000 / span,
            "tmux_ms_per_s": None if None in (s0, s1) else (s1 - s0) * 1000 / span,
            "daemon_self_ms_per_s": (r1[0] - r0[0]) / 1000 / span,
            "daemon_children_ms_per_s": (r1[1] - r0[1]) / 1000 / span,
            "daemon_tmux_calls_per_s": calls / span,
            "bar_spawns_per_s": bar_spawns / span,
            "spawn_price_ms": price,
            "pushed": bool(pushed)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--label", default="load0")
    ap.add_argument("--runs", type=int, default=15)
    ap.add_argument("--dirs", type=int, default=166)
    ap.add_argument("--sessions", type=int, default=8)
    ap.add_argument("--agents", type=int, default=7)
    ap.add_argument("--busy", type=int, default=2, help="agents printing as they work")
    ap.add_argument("--editors", type=int, default=6)
    ap.add_argument("--shells", type=int, default=3)
    ap.add_argument("--idle-secs", type=int, default=30)
    ap.add_argument("--only", help="comma-separated picker names")
    ap.add_argument("--json")
    args = ap.parse_args()

    if not os.path.exists(BIN):
        raise SystemExit(f"no binary at {BIN}; run `just build` first")
    seed(args.dirs)
    if BAR == "push":
        push_config()
    env = dict(os.environ, TMUX_COMPANION_SOCK=SOCK, TMUX_COMPANION_TRACE=str(TRACE),
               _ZO_DATA_DIR=str(ZO))
    env.pop("TMUX", None)
    daemon = subprocess.Popen([BIN, "server"], env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, start_new_session=True)
    t = Tmux(env)
    out = {"label": args.label, "pickers": {}, "fixture": {
        "dirs": args.dirs, "sessions": args.sessions, "agents": args.agents,
        "busy": args.busy, "editors": args.editors, "shells": args.shells,
        "config": "mine" if (Path(os.environ["HOME"]) / ".config/tmux-companion").exists()
        else "default"}}
    try:
        time.sleep(1.0)
        t.start(conf_file(), str(REPO))
        fill(t, args)
        t.drain(quiet=1.0)
        wanted = set(args.only.split(",")) if args.only else None
        for name, key, marker, frame in PICKERS:
            if wanted and name not in wanted:
                continue
            # One warm press: the first of anything pays for cold caches.
            t.press(key, marker)
            t.keys("Escape")
            seen, split, typed, per_key, frames, ahead, lost, longq = [], [], [], [], [], [], 0, []
            for _ in range(args.runs):
                wall, ms, framed = t.press(key, marker, frame=frame)
                seen.append(ms)
                frames.append(framed if frame is not None else ms)
                time.sleep(0.3)   # let the trace file catch the marks
                ph = phases(wall, trace_lines(wall)) if key != "M-e" else None
                if ph:
                    split.append(ph)
                    if key != "M-e" and ms is not None:
                        b, k = burst(t, ph["pid"])
                        typed.append(b)
                        per_key.append(k)
                        longq.append(input_lag(t, ph["pid"]))
                t.keys("Escape")
                if key != "M-e":
                    # Type-ahead: the query sent the moment the key has been.
                    # The query with the picker's cursor block after it: the
                    # pty's own echo shows the bare letters before the picker
                    # has read them, and only the picker draws the block.
                    _, typed_ms, _ = t.press(key, ("".join(TYPEAHEAD) + "\u2588").encode(),
                                             timeout=10.0, then=TYPEAHEAD)
                    ahead.append(typed_ms)
                    lost += typed_ms is None
                    t.keys("Escape")
            res = {"seen": stats(seen), "frame": stats(frames)}
            if key != "M-e":
                res["typeahead"] = stats(ahead)
                res["typeahead_lost"] = lost
                res["input20"] = stats(longq)
            for p in ("spawn", "chrome", "rows", "paint", "draw"):
                res[p] = stats([s.get(p) for s in split])
            if key != "M-e":
                res["burst5"] = stats(typed)
                res["per_key"] = stats(per_key)
            out["pickers"][name] = res
            s = res["seen"]
            line = (f"  {name:12s} seen p50 {s['p50']:7.1f} p95 {s['p95']:7.1f}" if s
                    else f"  {name:12s} never drew its marker")
            for p in ("spawn", "chrome", "rows", "paint", "draw"):
                if res[p]:
                    line += f"  {p} {res[p]['p50']:6.1f}"
            if res.get("burst5"):
                line += f"  burst5 {res['burst5']['p50']:6.1f}"
            print(line, flush=True)
            if res.get("frame") or res.get("typeahead") is not None:
                f, a, i = res.get("frame"), res.get("typeahead"), res.get("input20")
                print(f"  {'':12s} frame p50 {f['p50'] if f else float('nan'):7.1f}"
                      + (f"  type-ahead p50 {a['p50']:6.1f} p95 {a['p95']:6.1f} lost {res['typeahead_lost']}"
                         if a else (f"  type-ahead lost {res['typeahead_lost']}" if 'typeahead_lost' in res else ""))
                      + (f"  input20 p50 {i['p50']:5.1f} p95 {i['p95']:5.1f}" if i else ""), flush=True)
            if not s and getattr(t, "missed", None):
                print(f"    last bytes: {t.missed!r}", flush=True)
        # BAR_RUNS: how many window switches the bar is timed over (default 5).
        out["bar_switch"] = bar_after_switch(t, runs=int(os.environ.get("BAR_RUNS", "5")))
        b = out["bar_switch"]
        print(f"  bar after a window switch ({BAR}): "
              + (f"p50 {b['p50']:.0f} p95 {b['p95']:.0f} ms (n={b['n']})" if b else "never showed the branch"),
              flush=True)
        out["idle"] = idle_cost(t, daemon.pid, args.idle_secs)
        i = out["idle"]
        total = (i["daemon_self_ms_per_s"] + i["daemon_children_ms_per_s"]
                 + (i["tmux_ms_per_s"] or 0) + i["bar_spawns_per_s"] * i["spawn_price_ms"])
        print(f"  bar {BAR}: {i['bar_spawns_per_s']:.2f} #() calls/s at {i['spawn_price_ms']:.1f} ms"
              f", @tc-right {'set' if i['pushed'] else 'unset'}"
              f"; all told {total:.2f} ms/s", flush=True)
        print(f"  idle {i['secs']:.0f}s  daemon {i['daemon_self_ms_per_s']:.2f} ms/s"
              f" + its children {i['daemon_children_ms_per_s']:.2f} ms/s"
              f" ({i['daemon_tmux_calls_per_s']:.2f} tmux calls/s)"
              f"  tmux server {i['tmux_ms_per_s'] or 0:.2f} ms/s", flush=True)
    finally:
        t.stop()
        daemon.terminate()
        try:
            daemon.wait(timeout=5)
        except subprocess.TimeoutExpired:
            daemon.kill()
    if args.json:
        Path(args.json).write_text(json.dumps(out, indent=2))


if __name__ == "__main__":
    main()
