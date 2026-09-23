#!/usr/bin/env python3
"""What a picker costs from the moment the key goes down.

usage:
  scripts/bench-keys.py --arm new --runs 15
  scripts/bench-keys.py --arm old --runs 15
  scripts/bench-keys.py --arm both --json out.json

Both arms are driven the same way: a private tmux server, a real client on a
pty, and the real binding fired with `send-keys -K -c <client>` so tmux runs
the popup it would run for a person. The clock starts when send-keys returns
and stops when the bytes for the first result row reach the client.

Two things about the method are worth knowing before trusting a number.

`capture-pane` cannot see a popup. Verified on tmux 3.7c: while a popup is
printing, the active pane and every pane on the server come back empty. The
pty is the only place the popup's bytes appear, which is why this reads the
client's terminal rather than asking tmux what is on screen. It is also the
cheaper end: each capture-pane poll is a fork of its own, and at a 2 ms poll
interval the measurement costs more than the thing measured.

The popup is measured separately and subtracted. `display-popup -E true` is
tmux's own cost -- allocating the overlay, drawing its border, forking the
command -- and it is the same for both arms. Reporting it inside the total
would be charging this tool for tmux's work; hiding it entirely would flatter
both arms against the number a person actually waits through. So: total, the
popup constant, and the difference.
"""
import argparse, fcntl, json, os, pty, re, statistics, struct, subprocess, sys, termios, time
from pathlib import Path

HOME = Path.home()
COMRADES = HOME / ".config/tmux/comrades"
REPO = Path(__file__).resolve().parent.parent
BIN = os.environ.get("BENCH_BIN", str(REPO / "target/release/tmux-companion"))

# A private zoxide database, so the benchmark neither reads the author's real
# directory history nor writes a visit into it. Both arms read zoxide through
# this variable, which is the only reason the two lists are comparable at all.
ZO = "/tmp/tc-bench-zoxide"
ROOT = "/private/tmp/tc-bench"          # /private, because macOS resolves /tmp to it
DIRS = [f"{ROOT}/dir-{i:02d}" for i in range(1, 21)]
CWD = f"{ROOT}/dir-07"                  # where the bench session sits
# The first row naming a seeded directory, whichever it turns out to be. Not a
# named row: the two pickers order and scroll their lists differently, so
# `dir-07` is on screen in one and scrolled out of the other, and waiting for a
# particular row would time the scroll position rather than the draw.
DIR_MARKER = b"dir-"
SESSION = "tc-bench-session"            # a second live session, so the list is not one row


ESCAPES = re.compile(rb"\x1b\[[0-9;?]*[a-zA-Z]|\x1b[\]\(][^\x07\x1b]*(?:\x07|\x1b\\)?")


def flat(raw):
    """The bytes as text, with the escapes and the spacing taken out.

    tmux does not send a row as a row. It sends runs of characters with cursor
    moves between them, so a name on screen can arrive as two writes with a
    position escape in the middle, and a plain `marker in buffer` misses it.
    Stripping the escapes and squashing the whitespace is the same trick
    demo/add-markers.py uses on a cast, for the same reason.
    """
    return re.sub(rb"\s+", b"", ESCAPES.sub(b"", raw))



CPU_LOG = Path("/tmp/tc-bench-cpu.txt")
WRAPPER = Path("/tmp/tc-bench-wrap.zsh")


def wrap_cpu(cmd):
    """Run the picker untouched, then report what it and its children cost.

    `times` is a shell builtin, so this costs no extra process and, more to the
    point, does not go near the picker's file descriptors. The first attempt
    here wrapped the command in `/usr/bin/time -l ... 2>>log`, which put the
    report in the log and the picker's own drawing in there with it: the escape
    sequences went to the file instead of the terminal, nothing ever appeared
    on screen, and every run sat out its ten second timeout before failing.

    The second line of `times` is the children's user and system time, which
    for these two arms is the whole picker: fzf, the preview forks and the zsh
    that drives them, or one client round trip.
    """
    WRAPPER.write_text(
        "#!/usr/bin/env zsh\n"
        f"{cmd}\n"
        f"times >> {CPU_LOG}\n"
    )
    WRAPPER.chmod(0o755)
    return str(WRAPPER)


TIMES = re.compile(r"(\d+)m([\d.]+)s\s+(\d+)m([\d.]+)s")


def read_cpu():
    """ms of CPU per invocation, from the children's line of each `times`."""
    try:
        lines = CPU_LOG.read_text().splitlines()
    except FileNotFoundError:
        return []
    out = []
    for i, line in enumerate(lines):
        if i % 2 == 0:            # the shell's own line; the children are next
            continue
        m = TIMES.match(line.strip())
        if m:
            um, us, sm, ss = m.groups()
            out.append(((int(um) * 60 + float(us)) + (int(sm) * 60 + float(ss))) * 1000.0)
    return out


class Tmux:
    """A private tmux server with one real client on a pty."""

    def __init__(self, sock, env=None, width=160, height=44):
        self.sock = sock
        self.env = dict(os.environ, **(env or {}))
        self.width, self.height = width, height
        self.fd = self.pid = None

    def __call__(self, *a, check=False):
        r = subprocess.run(["tmux", "-L", self.sock, *a],
                           capture_output=True, text=True, env=self.env)
        if check and r.returncode:
            raise SystemExit(f"tmux {' '.join(a)}: {r.stderr.strip()}")
        return r.stdout.strip()

    def start(self, cwd=CWD):
        subprocess.run(["tmux", "-L", self.sock, "kill-server"],
                       capture_output=True, env=self.env)
        time.sleep(0.2)
        self("-f", "/dev/null", "new-session", "-d", "-c", cwd,
             "-x", str(self.width), "-y", str(self.height), "sleep", "100000", check=True)
        self.pid, self.fd = pty.fork()
        if self.pid == 0:                       # the child is the client
            os.environ.update(self.env)
            os.execvp("tmux", ["tmux", "-L", self.sock, "attach"])
        # The pty defaults to 80x24 whatever the session was created at, and a
        # picker draws as many rows as the client is tall, so an unset size
        # measures a different amount of work than a person sees.
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ,
                    struct.pack("HHHH", self.height, self.width, 0, 0))
        os.set_blocking(self.fd, False)
        for _ in range(100):
            if self("list-clients", "-F", "#{client_name}"):
                break
            time.sleep(0.05)
        else:
            raise SystemExit(f"no client attached to {self.sock}")
        self.client = self("list-clients", "-F", "#{client_name}").splitlines()[0]
        time.sleep(0.5)

    def drain(self, quiet=0.15, limit=3.0):
        """Read until the client has been silent for `quiet` seconds.

        A single non-blocking read is not enough. The popup that just closed is
        still repainting the session under it, and those bytes arrive after the
        read returns empty, land in the next run's buffer, and match the marker
        before the next popup has drawn anything. That is what a 15.8 ms
        reading of a picker is: the previous picture.
        """
        t0 = time.perf_counter()
        last = time.perf_counter()
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

    def press(self, key, marker, timeout=10.0):
        """Milliseconds from the key going down to `marker` reaching the client."""
        self.drain()
        seen = b""
        t0 = time.perf_counter()
        subprocess.run(["tmux", "-L", self.sock, "send-keys", "-K", "-c", self.client, key],
                       capture_output=True, env=self.env)
        while time.perf_counter() - t0 < timeout:
            try:
                chunk = os.read(self.fd, 65536)
            except (BlockingIOError, OSError):
                time.sleep(0.0005)
                continue
            if not chunk:
                break
            seen += chunk
            if flat(marker) in flat(seen):
                return (time.perf_counter() - t0) * 1000.0
        return None

    def dismiss(self):
        # Escape closes both pickers. The wait is for the popup to be gone
        # before the next run starts, not for politeness: a second popup over
        # the first measures the redraw of two overlays.
        subprocess.run(["tmux", "-L", self.sock, "send-keys", "-K", "-c", self.client, "Escape"],
                       capture_output=True, env=self.env)
        self.drain()

    def stop(self):
        subprocess.run(["tmux", "-L", self.sock, "kill-server"],
                       capture_output=True, env=self.env)
        if self.pid:
            try:
                os.close(self.fd)
                os.waitpid(self.pid, os.WNOHANG)
            except OSError:
                pass


def seed_zoxide():
    """Twenty directories in a private zoxide database.

    Twenty and not three because both pickers filter the list by the pane's own
    directory the moment they open, so a three-entry database measures a picker
    with nothing to do. Twenty is about what a working database holds and is
    enough for the fuzzy matcher to be doing real work when the first row
    lands.
    """
    subprocess.run(["rm", "-rf", ZO], capture_output=True)
    os.makedirs(ZO, exist_ok=True)
    for d in DIRS:
        os.makedirs(d, exist_ok=True)
        subprocess.run(["zoxide", "add", d], env=dict(os.environ, _ZO_DATA_DIR=ZO),
                       capture_output=True)


def stats(runs):
    runs = sorted(r for r in runs if r is not None)
    if not runs:
        return None
    return {"n": len(runs), "min": runs[0], "p50": statistics.median(runs),
            "max": runs[-1]}


def measure(arm, runs):
    """Both pickers and the popup constant, for one arm."""
    env = {"_ZO_DATA_DIR": ZO}
    if arm == "new":
        env["TMUX_COMPANION_SOCK"] = "/tmp/tc-bench.sock"
        subprocess.run(["pkill", "-f", f"^{BIN} server"], capture_output=True)
        time.sleep(0.2)
        subprocess.Popen([BIN, "server"], env=dict(os.environ, **env),
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1.0)                        # let the battery pre-warm finish
        project = f"{BIN} project"
        window = f"{BIN} new-window"
    else:
        project = f"zsh {COMRADES}/project-session.zsh"
        window = f"zsh {COMRADES}/zoxide-window.zsh"

    t = Tmux(f"tcbench-{arm}", env=env)
    t.start()
    # A session whose name is the marker, so the project picker's first
    # screenful contains it in both arms: one lists live sessions, the other
    # lists live sessions, and this is in both lists.
    t("new-session", "-d", "-s", SESSION, "-c", CWD, "sleep", "100000")
    t("bind", "-n", "M-s", "display-popup", "-h", "75%", "-w", "95%", "-B", "-E", project)
    t("bind", "-n", "M-c", "display-popup", "-h", "65%", "-w", "65%", "-B", "-E", window)
    t("bind", "-n", "M-e", "display-popup", "-h", "65%", "-w", "65%", "-B", "-E",
      "sh", "-c", "printf POPUPREADY")

    out = {}
    try:
        # Warm both paths once; the first call of either arm pays for a cold
        # zoxide read and, on the new arm, a cold daemon connection.
        for key, marker in (("M-s", DIR_MARKER), ("M-c", DIR_MARKER)):
            t.press(key, marker, timeout=15)
            t.dismiss()

        for name, key in [("project picker", "M-s"), ("new-window picker", "M-c"),
                          ("empty popup", "M-e")]:
            marker = b"POPUPREADY" if key == "M-e" else DIR_MARKER
            got = []
            # The CPU run is separate from the latency run: the wrapper writes
            # its report when the picker exits, which is after the escape, and
            # a file appended to mid-measurement is one more thing happening
            # while the clock is running.
            CPU_LOG.unlink(missing_ok=True)
            for _ in range(runs):
                got.append(t.press(key, marker))
                t.dismiss()
            out[name] = stats(got)
            if out[name] and key != "M-e":
                cmd = {"M-s": project, "M-c": window}[key]
                t(("bind"), "-n", key, "display-popup",
                  "-h", "75%" if key == "M-s" else "65%",
                  "-w", "95%" if key == "M-s" else "65%", "-B", "-E", wrap_cpu(cmd))
                for _ in range(runs):
                    t.press(key, marker)
                    t.dismiss()
                time.sleep(0.5)
                cpu = read_cpu()
                if cpu:
                    # The mean and not the median: `times` reports
                    # centiseconds, so one reading is quantised to 10 ms and
                    # only the average over the block carries finer detail
                    # than that.
                    out[name]["cpu_mean"] = statistics.mean(cpu)
                    out[name]["cpu_n"] = len(cpu)
                t("bind", "-n", key, "display-popup",
                  "-h", "75%" if key == "M-s" else "65%",
                  "-w", "95%" if key == "M-s" else "65%", "-B", "-E", cmd)
    finally:
        t.stop()
        if arm == "new":
            subprocess.run(["pkill", "-f", f"^{BIN} server"], capture_output=True)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--arm", choices=["new", "old", "both"], default="both")
    ap.add_argument("--runs", type=int, default=15)
    ap.add_argument("--json")
    args = ap.parse_args()

    if not os.path.exists(BIN):
        raise SystemExit(f"no binary at {BIN}; run `just build` first")
    if args.arm in ("old", "both") and not COMRADES.exists():
        raise SystemExit(f"no {COMRADES}; the old arm needs the comrades scripts on disk")

    seed_zoxide()
    arms = ["old", "new"] if args.arm == "both" else [args.arm]
    results = {}
    for arm in arms:
        print(f"\n===== {arm} =====", flush=True)
        results[arm] = measure(arm, args.runs)
        for name, s in results[arm].items():
            if s is None:
                print(f"  {name:20s} never drew the marker")
            else:
                cpu = (f"   cpu mean {s['cpu_mean']:6.1f} ms (n={s['cpu_n']})"
                       if s.get("cpu_mean") else "")
                print(f"  {name:20s} n={s['n']:2d}  min {s['min']:6.1f}  "
                      f"p50 {s['p50']:6.1f}  max {s['max']:6.1f} ms{cpu}")

    if "old" in results and "new" in results:
        print("\n-- keypress to first row, popup cost split out --")
        for name in ("project picker", "new-window picker"):
            o, n = results["old"].get(name), results["new"].get(name)
            po, pn = results["old"]["empty popup"], results["new"]["empty popup"]
            if o and n:
                print(f"  {name:20s} old {o['p50']:6.1f} ms (ours {o['p50']-po['p50']:6.1f})"
                      f"   new {n['p50']:6.1f} ms (ours {n['p50']-pn['p50']:6.1f})")

    if args.json:
        Path(args.json).write_text(json.dumps(results, indent=2))
        print(f"\nwrote {args.json}")


if __name__ == "__main__":
    main()
