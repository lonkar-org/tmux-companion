#!/usr/bin/env python3
"""What the status bar costs, per arm, in a real tmux with a real client.

usage:
  scripts/bench-bar.py --arm both --secs 45
  scripts/bench-bar.py --arm old --secs 20 --json out.json

The two arms are the two bars this repository has had:

  old   five #() calls, the shape before the port -- check-clients.zsh,
        `yrl gst`, check-vim-in-background.zsh, net-monitor.zsh and
        battery-life.zsh -- plus window-status.zsh, which tmux runs once per
        window on top of the five. The scripts are read out of the mysetup
        repository at 18e8db9^, the commit that replaced them, so this is the
        code that actually ran and not a reconstruction of it.

  new   one #() call: `tmux-companion status-right`.

Three things are added up, because no single counter sees all of it:

  the spawns        tmux forks `sh -c <string>` per distinct #() per client per
                    status-interval. Counted here rather than assumed, and
                    priced by running the same command under RUSAGE_CHILDREN.
  the tmux server   its own CPU, from ps, which is the drawing and the parsing.
  the daemon        the new arm only, from its own getrusage over the socket.

A reaped child's CPU is invisible from outside, which is why the spawn half is
priced separately and multiplied by a measured rate rather than read off a
process table.
"""
import argparse, json, os, resource, socket, statistics, subprocess, sys, time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BIN = os.environ.get("BENCH_BIN", str(REPO / "target/release/tmux-companion"))
MYSETUP = Path.home() / "git-repos/mysetup"
PORT_COMMIT = "18e8db9^"          # the commit that replaced the scripts
OLDBAR = Path("/tmp/tc-bench-oldbar")
SOCK = "/tmp/tc-bench-bar.sock"
COUNT = Path("/tmp/tc-bench-calls")
YRL = Path.home() / "go/bin/yrl"


def extract_old_scripts():
    """The pre-port bar, checked out of mysetup history into a temp directory.

    Not vendored into this repository: they are somebody's dotfiles, they are
    only needed to reproduce a number, and a copy here would rot against the
    original with nothing to notice it.
    """
    if not MYSETUP.exists():
        raise SystemExit(f"no {MYSETUP}; the old arm needs that repository for its scripts")
    OLDBAR.mkdir(exist_ok=True)
    names = ["check-clients", "check-vim-in-background", "net-monitor",
             "battery-life", "window-status"]
    for n in names:
        r = subprocess.run(["git", "-C", str(MYSETUP), "show",
                            f"{PORT_COMMIT}:home/.yrl/lib/{n}.zsh"],
                           capture_output=True, text=True)
        if r.returncode:
            raise SystemExit(f"cannot read {n}.zsh at {PORT_COMMIT}: {r.stderr.strip()}")
        # A wrapper counts the spawn and then becomes the script, so counting
        # costs a shell builtin rather than another fork.
        p = OLDBAR / f"{n}.zsh"
        p.write_text(r.stdout)
        p.chmod(0o755)
        w = OLDBAR / f"{n}-counted.zsh"
        w.write_text(f'#!/usr/bin/env zsh\nprint {n} >> {COUNT}\nexec {p} "$@"\n')
        w.chmod(0o755)
    y = OLDBAR / "yrl-gst-counted.zsh"
    y.write_text(f'#!/usr/bin/env zsh\nprint yrl-gst >> {COUNT}\nexec {YRL} gst "$@"\n')
    y.chmod(0o755)
    return OLDBAR


def req(cmd, args=None, sock=SOCK):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(sock)
    s.sendall((json.dumps({"cmd": cmd, "args": args or {}}) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    s.close()
    return json.loads(buf.decode())


def daemon_cpu_and_calls():
    o = req("__rusage")["output"].split()
    return sum(int(x) for x in o[:4]) / 1000.0, int(o[4])      # ms, count


def ps_cpu_ms(pid):
    """Cumulative CPU of one process, in ms, from ps."""
    r = subprocess.run(["ps", "-o", "time=", "-p", str(pid)], capture_output=True, text=True)
    t = r.stdout.strip()
    if not t:
        return None
    # mm:ss.ss, or hh:mm:ss
    parts = t.split(":")
    secs = 0.0
    for p in parts:
        secs = secs * 60 + float(p)
    return secs * 1000.0


def spawn_cpu(argv, n=25, blocks=3, env=None):
    """ms of CPU per fork/exec of this command, RUSAGE_CHILDREN over blocks."""
    out = []
    for _ in range(blocks):
        r0 = resource.getrusage(resource.RUSAGE_CHILDREN)
        for _ in range(n):
            subprocess.run(argv, shell=isinstance(argv, str), env=env or os.environ,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        r1 = resource.getrusage(resource.RUSAGE_CHILDREN)
        out.append(((r1.ru_utime - r0.ru_utime) + (r1.ru_stime - r0.ru_stime)) / n * 1000.0)
    return statistics.mean(out)


class Bar:
    def __init__(self, arm):
        self.arm = arm
        self.sockname = f"tcbar-{arm}"
        self.env = dict(os.environ, TMUX_COMPANION_SOCK=SOCK)
        self.client = None
        self.daemon = None

    def t(self, *a):
        return subprocess.run(["tmux", "-L", self.sockname, *a],
                              capture_output=True, text=True, env=self.env).stdout.strip()

    def start(self, secs):
        subprocess.run(["tmux", "-L", self.sockname, "kill-server"],
                       capture_output=True, env=self.env)
        COUNT.unlink(missing_ok=True)
        COUNT.touch()
        time.sleep(0.3)
        self.t("-f", "/dev/null", "new-session", "-d", "-c", str(REPO),
               "-x", "200", "-y", "50", "sleep", str(secs + 600))
        self.t("set", "-g", "status-interval", "1")
        self.t("set", "-g", "status-left-length", "200")
        self.t("set", "-g", "status-right-length", "200")
        if self.arm == "old":
            d = extract_old_scripts()
            self.t("set", "-g", "status-left",
                   f"#({d}/check-clients-counted.zsh #{{session_attached}} #{{window_active_clients}})")
            self.t("set", "-ga", "status-left",
                   f"#({d}/yrl-gst-counted.zsh #{{pane_current_path}})")
            self.t("set", "-ga", "status-left",
                   f"#({d}/check-vim-in-background-counted.zsh #{{pane_pid}})")
            self.t("set", "-g", "status-right", f"#({d}/net-monitor-counted.zsh)")
            self.t("set", "-ga", "status-right", f"#({d}/battery-life-counted.zsh)")
            self.t("set", "-g", "window-status-format",
                   f"#({d}/window-status-counted.zsh -i #I -n '#W' -w '#{{pane_current_path}}'"
                   f" -p '#{{pane_current_command}}' -I #{{window_id}} -f '#{{window_flags}}')")
        else:
            subprocess.run(["pkill", "-f", f"^{BIN} server"], capture_output=True)
            time.sleep(0.3)
            self.daemon = subprocess.Popen([BIN, "server"], env=self.env,
                                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(1.0)
            self.t("set", "-g", "status-left", " #S  %H:%M:%S ")
            self.t("set", "-g", "status-right",
                   f"#(env TMUX_COMPANION_SOCK={SOCK} {BIN} status-right "
                   f"--branch-max-len 40 #{{pane_current_path}})")
        # #() is gated per ATTACHED client, so an unattached server spawns
        # nothing at all and measures zero.
        self.client = subprocess.Popen(
            ["script", "-q", "/dev/null", "tmux", "-L", self.sockname, "attach"],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            env=self.env)
        time.sleep(2.5)
        self.pid = int(self.t("display-message", "-p", "#{pid}"))

    def run(self, secs):
        time.sleep(3)                       # let the first refresh settle
        COUNT.write_text("")
        c0 = ps_cpu_ms(self.pid)
        d0 = daemon_cpu_and_calls() if self.arm == "new" else (0.0, 0)
        t0 = time.time()
        time.sleep(secs)
        el = time.time() - t0
        c1 = ps_cpu_ms(self.pid)
        d1 = daemon_cpu_and_calls() if self.arm == "new" else (0.0, 0)
        if self.arm == "old":
            names = COUNT.read_text().split()
            spawns = {n: names.count(n) for n in set(names)}
        else:
            spawns = {"status-right": d1[1] - d0[1]}
        return {
            "seconds": el,
            "tmux_ms_per_s": (c1 - c0) / el,
            "daemon_ms_per_s": (d1[0] - d0[0]) / el,
            "spawns_per_s": {k: v / el for k, v in spawns.items()},
        }

    def stop(self):
        if self.client:
            self.client.terminate()
        subprocess.run(["tmux", "-L", self.sockname, "kill-server"],
                       capture_output=True, env=self.env)
        if self.daemon:
            subprocess.run(["pkill", "-f", f"^{BIN} server"], capture_output=True)


def price_spawns(arm):
    """ms of CPU per spawn, and how many distinct spawns a refresh makes."""
    if arm == "old":
        d = extract_old_scripts()
        env = os.environ
        calls = {
            "check-clients": f"{d}/check-clients.zsh 1 1",
            "yrl-gst": f"{YRL} gst {REPO}",
            "check-vim-in-background": f"{d}/check-vim-in-background.zsh {os.getpid()}",
            "net-monitor": f"{d}/net-monitor.zsh",
            "battery-life": f"{d}/battery-life.zsh",
            "window-status": (f"{d}/window-status.zsh -i 1 -n w -w {REPO} -p zsh "
                              f"-I 1 -f ''"),
        }
    else:
        env = dict(os.environ, TMUX_COMPANION_SOCK=SOCK)
        calls = {"status-right": f"{BIN} status-right --branch-max-len 40 {REPO}"}
    # `sh -c`, because that is how tmux runs a #() and the shell is part of the
    # bill whatever the command is.
    return {k: spawn_cpu(f"sh -c '{v}' >/dev/null 2>&1", env=env) for k, v in calls.items()}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--arm", choices=["old", "new", "both"], default="both")
    ap.add_argument("--secs", type=int, default=45)
    ap.add_argument("--json")
    args = ap.parse_args()

    results = {}
    for arm in (["old", "new"] if args.arm == "both" else [args.arm]):
        print(f"\n===== {arm} =====", flush=True)
        if arm == "new":
            b = Bar(arm)
            b.start(args.secs)
            per = price_spawns(arm)       # priced while the daemon is up
        else:
            per = price_spawns(arm)
            b = Bar(arm)
            b.start(args.secs)
        try:
            live = b.run(args.secs)
        finally:
            b.stop()

        # Each command's own rate against its own cost, then added up.
        rates = live["spawns_per_s"]
        spawn_ms_s = 0.0
        for k, ms in per.items():
            r = rates.get(k, 0.0)
            spawn_ms_s += r * ms
            print(f"  spawn  {k:32s} {ms:7.2f} ms x {r:4.2f}/s = {r*ms:7.2f} ms/s")
        total = live["tmux_ms_per_s"] + live["daemon_ms_per_s"] + spawn_ms_s
        live.update({"per_spawn_ms": per, "spawn_ms_per_s": spawn_ms_s,
                     "total_ms_per_s": total, "core_pct": total / 10.0})
        results[arm] = live
        print(f"  spawns/s        {sum(rates.values()):6.2f} total")
        print(f"  spawn CPU       {spawn_ms_s:6.2f} ms/s")
        print(f"  tmux server     {live['tmux_ms_per_s']:6.2f} ms/s")
        print(f"  daemon          {live['daemon_ms_per_s']:6.2f} ms/s")
        print(f"  TOTAL           {total:6.2f} ms/s = {total/10:5.2f}% of a core")

    if args.json:
        Path(args.json).write_text(json.dumps(results, indent=2))
        print(f"\nwrote {args.json}")


if __name__ == "__main__":
    main()
