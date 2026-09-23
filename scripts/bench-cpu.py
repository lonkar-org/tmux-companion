#!/usr/bin/env python3
"""Before/after harness for the tmux-companion status-bar rework.

Two independent measurements, matching the evaluation's method:

  * server CPU  -- read from the server's own getrusage(2) via the `__rusage`
    probe (self + reaped children, microsecond resolution), differenced across
    a block of N requests sent straight down the unix socket, so no process
    spawn is included.

  * spawn CPU   -- RUSAGE_CHILDREN deltas around blocks of real fork/exec, run
    as interleaved blocks so drift and thermal state hit both arms equally.

Usage:  python3 bench-cpu.py <binary> <label> [--tmux]
The server is always started on a private socket and a private cache path, so
the user's live server and status bar are never touched.
"""
import json, os, resource, socket, statistics, subprocess, sys, time

BIN = sys.argv[1]
LABEL = sys.argv[2]
WITH_TMUX = "--tmux" in sys.argv

SOCK = "/tmp/tc-measure.sock"
REPO = os.environ.get("BENCH_REPO") or os.path.dirname(os.path.abspath(__file__))
ENV = dict(os.environ, TMUX_COMPANION_SOCK=SOCK)


# ── socket plumbing ──────────────────────────────────────────────────────────

def req(cmd, args=None):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(SOCK)
    s.sendall((json.dumps({"cmd": cmd, "args": args or {}}) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    s.close()
    return json.loads(buf.decode())


def probe():
    """(total_cpu_us, request_count) from the server's own getrusage."""
    o = req("__rusage")["output"].split()
    su, ss, cu, cs = (int(x) for x in o[:4])
    return su + ss + cu + cs, int(o[4])


def start_server():
    stop_server()
    subprocess.Popen([BIN, "server"], env=ENV, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL)
    for _ in range(100):
        try:
            req("noop")
            time.sleep(0.8)   # let the battery pre-warm task finish
            return
        except OSError:
            time.sleep(0.05)
    raise SystemExit("server did not start")


def stop_server():
    subprocess.run(["pkill", "-f", f"^{BIN} server"], capture_output=True)
    for p in (SOCK,):
        try:
            os.unlink(p)
        except OSError:
            pass
    time.sleep(0.3)


# ── measurement A: server CPU per request ────────────────────────────────────

def server_cpu(cmd, args, n=60, warm=5, pace=0.0):
    first = req(cmd, args)
    if first.get("error"):
        raise SystemExit(f"{cmd} failed: {first['error']} (args={args})")
    for _ in range(warm):
        req(cmd, args)
        if pace:
            time.sleep(pace)
    c0, _ = probe()
    for _ in range(n):
        req(cmd, args)
        if pace:
            time.sleep(pace)
    c1, _ = probe()
    return (c1 - c0) / n / 1000.0     # ms per call


# ── measurement B: spawn CPU per fork/exec ───────────────────────────────────

def spawn_cpu(argv, n=40, blocks=4, shell=False):
    out = []
    for _ in range(blocks):
        r0 = resource.getrusage(resource.RUSAGE_CHILDREN)
        for _ in range(n):
            subprocess.run(argv, shell=shell, env=ENV,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        r1 = resource.getrusage(resource.RUSAGE_CHILDREN)
        d = (r1.ru_utime - r0.ru_utime) + (r1.ru_stime - r0.ru_stime)
        out.append(d / n * 1000.0)
    return statistics.mean(out), (statistics.stdev(out) if len(out) > 1 else 0.0)


# ── measurement C: real tmux, real status bar ────────────────────────────────

L = ["tmux", "-L", "tcmeasure"]


def tm(*a):
    return subprocess.run(L + list(a), capture_output=True, text=True).stdout.strip()


def tmux_endtoend(setup, secs=45):
    envpfx = f"TMUX_COMPANION_SOCK={SOCK} "
    setup(envpfx + BIN)
    time.sleep(3)
    c0, n0 = probe()
    t0 = time.time()
    time.sleep(secs)
    el = time.time() - t0
    c1, n1 = probe()
    return (n1 - n0) / el, (c1 - c0) / 1000.0 / el


def setup_five(B):
    tm("set", "-g", "status-left-length", "80")
    tm("set", "-g", "status-right-length", "150")
    tm("set", "-g", "status-left", " #S ")
    tm("set", "-ga", "status-left",
       f"#({B} clients #{{session_attached}} #{{window_active_clients}})")
    tm("set", "-ga", "status-left", " %H:%M:%S")
    tm("set", "-ga", "status-left", f"#({B} vim-bg #{{pane_pid}})")
    tm("set", "-g", "status-right",
       f"#({B} gst --no-cap --branch-max-len 40 #{{pane_current_path}} #{{pane_pid}})")
    tm("set", "-ga", "status-right", f"#({B} net)")
    tm("set", "-ga", "status-right",
       f"#[reverse,fg=color237]#[bg=color237,none]#({B} battery) ")


def setup_one(B):
    tm("set", "-g", "status-left", " #S  %H:%M:%S ")
    tm("set", "-g", "status-right",
       f"#({B} status-right --branch-max-len 40 #{{pane_current_path}})")


# ── run ──────────────────────────────────────────────────────────────────────

print(f"\n===== {LABEL} =====")
print(f"binary: {BIN}")
start_server()

print("\n-- server CPU per request (ms, via getrusage, socket-direct) --")
seg = {}
combined = req("status-right", {"path": REPO, "branch_max_len": 40}).get("error") is None
if combined:
    seg["status-right (combined)"] = server_cpu(
        "status-right", {"path": REPO, "branch_max_len": 40}, pace=0.02)
seg["gst  (with pane_pid, as today)"] = server_cpu(
    "gst", {"path": REPO, "pane_pid": os.getpid(), "no_cap": True, "branch_max_len": 40})
seg["gst  (no pane_pid)"] = server_cpu(
    "gst", {"path": REPO, "no_cap": True, "branch_max_len": 40})
seg["net"] = server_cpu("net", {})
seg["battery (30s cache)"] = server_cpu("battery", {})
seg["clients"] = server_cpu("clients", {"session_attached": 1, "window_active_clients": 1})
seg["vim-bg"] = server_cpu("vim-bg", {"pane_pid": os.getpid()})
for k, v in seg.items():
    print(f"  {k:36s} {v:7.2f} ms")

print("\n-- spawn CPU per fork/exec (ms, RUSAGE_CHILDREN, 4x40 interleaved) --")
sp = {}
m, s = spawn_cpu(["/bin/echo"])
sp["/bin/echo (floor)"] = m
print(f"  {'/bin/echo (floor)':36s} {m:7.2f} +/-{s:4.2f} ms")
m, s = spawn_cpu([BIN, "noop"])
sp["client round trip"] = m
print(f"  {'client round trip':36s} {m:7.2f} +/-{s:4.2f} ms")
m, s = spawn_cpu(f"{BIN} noop", shell=True)
sp["client via sh -c (as tmux runs it)"] = m
print(f"  {'client via sh -c (as tmux runs)':36s} {m:7.2f} +/-{s:4.2f} ms")

if WITH_TMUX:
    print("\n-- real tmux, 1 attached client, status-interval 1 --")
    tm("kill-server")
    time.sleep(0.5)
    subprocess.Popen(L + ["new-session", "-d", "-s", "t", "-c", REPO, "sleep 4000"],
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    # an attached client is required: #() is gated per attached client
    client = subprocess.Popen(
        ["script", "-q", "/dev/null"] + L + ["attach", "-t", "t"],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(2)
    print(f"  clients attached: {len(tm('list-clients').splitlines())}")
    for name, fn in ([("one #() call", setup_one)] if combined
                     else [("five #() calls", setup_five)]):
        cps, ms = tmux_endtoend(fn)
        spawn = sp["client via sh -c (as tmux runs it)"]
        print(f"  {name:28s} {cps:5.2f} calls/s   server {ms:6.2f} ms/s")
        print(f"  {'':28s} spawn {cps*spawn:6.2f} ms/s   TOTAL {ms + cps*spawn:6.2f} ms/s "
              f"= {(ms + cps*spawn)/10:5.2f}% of a core")
    client.terminate()
    tm("kill-server")

stop_server()
print()
