//! Two hosts on one machine: the link between two daemons, each with its own
//! tmux server, daemon socket, HOME and XDG directories.
//!
//! Host B is reached with `env` instead of ssh. Its host command sets B's
//! environment and ends in `sh -c`, so the bridge line the link appends runs
//! against B exactly as ssh would run it on a VM, with no ssh in the path:
//!
//! ```text
//! env -u TMUX TMUX_COMPANION_SOCK=<b> TMUX_TMPDIR=<b-tmux> HOME=<b-home> ... sh -c 'tmux-companion bridge'
//! ```
//!
//! The design names `TMUX_COMPANION_SOCK`, `TMUX_TMPDIR` and `HOME`; `-u TMUX`
//! and the XDG directories are added because A's daemon runs with A's, and a
//! bridge that inherited them would read A's config and state.
//!
//! Nothing here reaches the developer's own tmux server or daemon: every tmux
//! call the test makes names its server with `-S` and runs with `TMUX`
//! unset, and the binary runs with `TMUX` unset and `TMUX_TMPDIR` pointing at
//! a directory of its own, so a bare `tmux` inside it finds that one.
//!
//! Real ssh stays a manual check, against `localhost` with Remote Login on.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tmux-companion"))
}

/// Skip, or fail when CI is set: the same rule as tests/e2e.rs.
fn skipping(name: &str, why: &str) {
    assert!(
        std::env::var_os("CI").is_none(),
        "{name} would skip ({why}), and CI is set, so it fails instead"
    );
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| eprintln!("skipping the tmux tests in tests/remote.rs: {why}"));
}

/// Write an executable through `sh`, so no handle of this process holds it
/// open for writing when it runs ("Text file busy").
fn write_executable(path: &Path, content: &str) {
    use std::io::Write;
    let mut child = Command::new("sh")
        .args([
            "-c",
            "cat > \"$1.tmp\" && chmod 755 \"$1.tmp\" && mv \"$1.tmp\" \"$1\"",
            "sh",
        ])
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("sh runs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(content.as_bytes())
        .expect("writes");
    assert!(child.wait().expect("sh ends").success());
}

/// One machine: a tmux server and a daemon of its own.
struct Side {
    root: PathBuf,
    uid: u32,
}

impl Side {
    fn new(root: PathBuf) -> Self {
        for d in [
            "home",
            "config/tmux-companion",
            "state",
            "data",
            "cache",
            "tmux",
        ] {
            std::fs::create_dir_all(root.join(d)).expect("side dirs");
        }
        let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&root).expect("stat"));
        let tdir = root.join(format!("tmux/tmux-{uid}"));
        std::fs::create_dir_all(&tdir).expect("tmux dir");
        std::fs::set_permissions(&tdir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .expect("chmod");
        Side { root, uid }
    }

    fn sock(&self) -> PathBuf {
        self.root.join("d.sock")
    }

    fn tmux_tmpdir(&self) -> PathBuf {
        self.root.join("tmux")
    }

    fn tmux_sock(&self) -> PathBuf {
        self.root.join(format!("tmux/tmux-{}/default", self.uid))
    }

    fn config(&self) -> PathBuf {
        self.root.join("config/tmux-companion/config.toml")
    }

    fn state(&self) -> PathBuf {
        self.root.join("state/tmux-companion")
    }

    /// The variables that make a process this side's.
    fn vars(&self) -> Vec<(String, String)> {
        let s = |p: PathBuf| p.display().to_string();
        vec![
            ("TMUX_COMPANION_SOCK".into(), s(self.sock())),
            ("TMUX_TMPDIR".into(), s(self.tmux_tmpdir())),
            ("HOME".into(), s(self.root.join("home"))),
            ("XDG_CONFIG_HOME".into(), s(self.root.join("config"))),
            ("XDG_STATE_HOME".into(), s(self.root.join("state"))),
            ("XDG_DATA_HOME".into(), s(self.root.join("data"))),
            ("XDG_CACHE_HOME".into(), s(self.root.join("cache"))),
        ]
    }

    fn env(&self, cmd: &mut Command, bin: &Path) {
        // Inside the side's own directory: a session the binary makes starts
        // there, so anything a name ran would land where the test looks.
        cmd.current_dir(&self.root);
        cmd.env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("TMUX_COMPANION_CONFIG")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            );
        for (k, v) in self.vars() {
            cmd.env(k, v);
        }
    }

    /// One tmux command against this side's server, by its socket.
    fn tmux(&self, bin: &Path, args: &[&str]) -> (String, String, bool) {
        let mut cmd = Command::new("tmux");
        cmd.arg("-S")
            .arg(self.tmux_sock())
            .arg("-f")
            .arg("/dev/null");
        cmd.args(args);
        self.env(&mut cmd, bin);
        let o = cmd.output().expect("tmux runs");
        (
            String::from_utf8_lossy(&o.stdout).trim_end().to_string(),
            String::from_utf8_lossy(&o.stderr).trim_end().to_string(),
            o.status.success(),
        )
    }

    fn must(&self, bin: &Path, args: &[&str]) -> String {
        let (out, err, ok) = self.tmux(bin, args);
        assert!(ok, "tmux {} failed: {err}", args.join(" "));
        out
    }

    /// The binary as this side, from outside tmux.
    fn run(&self, bin: &Path, args: &[&str]) -> (String, String, bool) {
        let mut cmd = Command::new(binary());
        cmd.args(args);
        self.env(&mut cmd, bin);
        let o = cmd.output().expect("binary runs");
        (
            String::from_utf8_lossy(&o.stdout).to_string(),
            String::from_utf8_lossy(&o.stderr).to_string(),
            o.status.success(),
        )
    }

    fn daemon_running(&self) -> bool {
        std::os::unix::net::UnixStream::connect(self.sock()).is_ok()
    }
}

/// Two sides, and a `bin` directory with the binary under its own name.
struct Pair {
    dir: PathBuf,
    bin: PathBuf,
    a: Side,
    b: Side,
}

impl Pair {
    fn start(name: &str) -> Option<Self> {
        if std::env::var_os("TC_SKIP_E2E").is_some_and(|v| v == "1") {
            skipping(name, "TC_SKIP_E2E=1");
            return None;
        }
        if Command::new("tmux").arg("-V").output().is_err() {
            skipping(name, "no tmux on this machine");
            return None;
        }
        // Short: three socket paths live under it and a socket address holds
        // about a hundred bytes.
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = PathBuf::from(format!("/tmp/tcr{}-{serial}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        std::os::unix::fs::symlink(binary(), bin.join("tmux-companion")).expect("link");
        let pair = Pair {
            a: Side::new(dir.join("a")),
            b: Side::new(dir.join("b")),
            bin,
            dir,
        };
        Some(pair)
    }

    /// B's host command: B's environment, then `sh -c`, with `extra` before
    /// the `sh` for a test that needs more (a PATH, say).
    fn b_command(&self, extra: &[String]) -> Vec<String> {
        let mut argv = vec!["env".to_string(), "-u".into(), "TMUX".into()];
        for (k, v) in self.b.vars() {
            argv.push(format!("{k}={v}"));
        }
        argv.extend(extra.iter().cloned());
        argv.push("sh".into());
        argv.push("-c".into());
        argv
    }

    /// Write A's config with one host `b` reached by `command`, and `keys`
    /// as its other lines.
    fn a_config(&self, command: &[String], keys: &str) {
        let argv: Vec<String> = command.iter().map(|w| toml_string(w)).collect();
        let text = format!(
            "[[hosts]]\nname = \"b\"\ncommand = [{}]\n{keys}\n",
            argv.join(", ")
        );
        std::fs::write(self.a.config(), text).expect("config");
    }

    fn a(&self, args: &[&str]) -> (String, String, bool) {
        self.a.run(&self.bin, args)
    }

    /// A fake ssh for B: takes `-t` the way ssh does, then runs its last
    /// argument with B's environment through `sh -c`, as ssh would on the
    /// host. TERM is one every terminfo has, as ssh would pass the
    /// terminal's own.
    fn fake_ssh(&self) -> Vec<String> {
        let script = self.dir.join("fakessh");
        let vars: Vec<String> = self
            .b
            .vars()
            .into_iter()
            .map(|(k, v)| format!("{k}='{v}'"))
            .collect();
        write_executable(
            &script,
            &format!(
                "#!/bin/sh\n[ \"$1\" = -t ] && shift\nexec env -u TMUX -u TMUX_PANE TERM=xterm-256color {} sh -c \"$1\"\n",
                vars.join(" ")
            ),
        );
        vec![script.display().to_string()]
    }

    fn b(&self, args: &[&str]) -> (String, String, bool) {
        self.b.run(&self.bin, args)
    }

    /// A control-mode client attached to `session` on A: an attached client
    /// that needs no terminal, which `switch-client` and
    /// `detach-on-destroy` act on like any other.
    fn a_client(&self, session: &str) -> std::process::Child {
        let mut client = Command::new("tmux");
        client
            .arg("-S")
            .arg(self.a.tmux_sock())
            .args(["-C", "attach", "-t", &format!("={session}")])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        self.a.env(&mut client, &self.bin);
        let mut client = client.spawn().expect("control client");
        // A size, as a terminal has: a control client on 3.4 and 3.5a
        // reports no height until it is told one.
        use std::io::Write;
        client
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(b"refresh-client -C 120x40\n")
            .expect("control client takes commands");
        client
    }

    /// Poll `hosts` on A until `want` holds, or fail with what it said last.
    fn hosts_until(&self, secs: u64, want: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let (out, err, _) = self.a(&["hosts"]);
            if want(&out) {
                return out;
            }
            assert!(
                Instant::now() < deadline,
                "hosts never got there:\n{out}{err}"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        for side in [&self.a, &self.b] {
            if side.daemon_running() {
                let _ = side.run(&self.bin, &["shutdown"]);
            }
            let _ = side.tmux(&self.bin, &["kill-server"]);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Wait for `cond`, or fail saying what was waited for.
fn wait_for(secs: u64, what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !cond() {
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// [`wait_for`], saying what `why` says when it never comes.
fn wait_until(secs: u64, mut cond: impl FnMut() -> bool, why: impl Fn() -> String) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !cond() {
        assert!(Instant::now() < deadline, "never: {}", why());
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn toml_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

const NASTY: &str = "x'; touch pwned; '";

/// The whole read side of phase 1: no link until asked, the hello, B's
/// sessions on A without the mirror one, the name with a quote and a
/// semicolon arriving as a name and running nothing, the cache on disk, the
/// line in doctor.
#[test]
fn a_host_links_on_demand_and_lists_its_sessions() {
    let Some(p) = Pair::start("links") else {
        return;
    };
    let home_b = p.b.root.join("home");
    p.b.must(
        &p.bin,
        &[
            "new-session",
            "-d",
            "-s",
            "api",
            "-c",
            &home_b.display().to_string(),
        ],
    );
    p.b.must(
        &p.bin,
        &[
            "new-session",
            "-d",
            "-s",
            NASTY,
            "-c",
            &home_b.display().to_string(),
        ],
    );
    p.b.must(
        &p.bin,
        &["new-session", "-d", "-s", "api~mac", "-t", "=api"],
    );
    p.b.must(
        &p.bin,
        &["set-option", "-t", "api~mac", "@tc-mirror", "mac"],
    );
    p.a_config(&p.b_command(&[]), "poll_secs = 1");

    // A's daemon is up and has read its hosts; on-demand starts nothing.
    let (out, _, ok) = p.a(&["hosts"]);
    assert!(ok);
    assert!(
        out.starts_with("b: on-demand, no link, never heard from"),
        "{out}"
    );
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!p.b.daemon_running(), "nothing asked, so no bridge ran");

    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    assert!(out.starts_with("b answered: tmux-companion "), "{out}");
    assert!(out.contains(", tmux "), "the host's tmux: {out}");
    assert!(p.b.daemon_running(), "the bridge started B's daemon");

    let out = p.hosts_until(15, |o| o.contains(&format!("{NASTY}@b")));
    let mut lines = out.lines();
    let head = lines.next().expect("a host line");
    assert!(head.starts_with("b: on-demand, link up, "), "{head}");
    assert!(head.contains(", last heard "), "{head}");
    assert!(head.contains("2 sessions"), "{head}");
    assert!(head.contains("the bridge started its daemon"), "{head}");
    let rows: Vec<&str> = lines.collect();
    assert!(rows.contains(&"api@b, 1 window"), "{rows:?}");
    assert!(
        rows.contains(&format!("{NASTY}@b, 1 window").as_str()),
        "{rows:?}"
    );
    assert!(
        !out.contains("api~mac"),
        "a mirror session isn't listed: {out}"
    );
    for dir in [&p.dir, &home_b] {
        assert!(!dir.join("pwned").exists(), "a session name ran something");
    }
    assert!(
        p.a.state().join("hosts/b.bin").exists(),
        "the answer is kept on disk"
    );

    let (doctor, _, _) = p.a(&["doctor"]);
    let line = doctor
        .lines()
        .find(|l| l.trim_start().starts_with("hosts"))
        .expect("a hosts line in doctor");
    assert!(line.contains("b: on-demand, link up"), "{line}");
}

/// An on-demand link goes once nothing has asked for `idle_secs`, and the
/// rows stay, with when they were heard.
#[test]
fn an_on_demand_link_closes_when_nothing_needs_it() {
    let Some(p) = Pair::start("idle") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    p.a_config(&p.b_command(&[]), "poll_secs = 1\nidle_secs = 2");
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    let out = p.hosts_until(15, |o| o.contains("no link"));
    assert!(out.contains("last heard "), "{out}");
    assert!(out.contains("api@b"), "the rows outlive the link: {out}");
}

/// `off` starts nothing, even when asked.
#[test]
fn an_off_host_never_links() {
    let Some(p) = Pair::start("off") else {
        return;
    };
    p.a_config(&p.b_command(&[]), "link = \"off\"");
    let (out, _, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(!ok);
    assert_eq!(out.trim(), "b: its link is off");
    let (out, _, _) = p.a(&["hosts"]);
    assert_eq!(out.lines().next(), Some("b: off, never heard from"));
    std::thread::sleep(Duration::from_millis(500));
    assert!(!p.b.daemon_running());
}

/// `while-attached` comes up with nobody asking once a client is attached to
/// A's server.
#[test]
fn a_while_attached_link_comes_up_with_a_client() {
    let Some(p) = Pair::start("attached") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    p.a.must(&p.bin, &["new-session", "-d", "-s", "here"]);
    p.a_config(
        &p.b_command(&[]),
        "link = \"while-attached\"\npoll_secs = 1",
    );
    let (out, _, _) = p.a(&["hosts"]);
    assert!(out.contains("no link"), "{out}");

    // A control-mode client is an attached client and needs no terminal.
    let mut client = Command::new("tmux");
    client
        .arg("-S")
        .arg(p.a.tmux_sock())
        .args(["-C", "attach", "-t", "=here"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    p.a.env(&mut client, &p.bin);
    let mut client = client.spawn().expect("control client");
    let out = p.hosts_until(20, |o| o.contains("api@b"));
    assert!(out.contains("link up"), "{out}");
    let _ = client.kill();
    let _ = client.wait();
}

/// A host command that ends at once says what it said.
#[test]
fn a_host_that_exits_at_once_says_why() {
    let Some(p) = Pair::start("exits") else {
        return;
    };
    let cmd: Vec<String> = [
        "sh",
        "-c",
        "echo 'ssh: connect to host b port 22: Connection refused' >&2; exit 255",
    ]
    .map(String::from)
    .to_vec();
    p.a_config(&cmd, "");
    let (out, _, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(!ok);
    assert_eq!(
        out.trim(),
        "b: ssh: connect to host b port 22: Connection refused"
    );
    let (out, _, _) = p.a(&["hosts"]);
    assert!(
        out.contains("link down: ssh: connect to host b port 22: Connection refused, retrying in "),
        "{out}"
    );
}

/// A host that never answers is called down after `connect_secs`.
#[test]
fn a_host_that_hangs_is_called_down_after_connect_secs() {
    let Some(p) = Pair::start("hangs") else {
        return;
    };
    let cmd: Vec<String> = ["sh", "-c", "sleep 30"].map(String::from).to_vec();
    p.a_config(&cmd, "connect_secs = 1");
    let started = Instant::now();
    let (out, _, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(!ok);
    assert_eq!(out.trim(), "b: no answer in 1s");
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
}

/// A host that writes something other than Cap'n Proto is told apart from
/// one that is slow.
#[test]
fn a_host_that_sends_something_else_is_not_the_protocol() {
    let Some(p) = Pair::start("garbage") else {
        return;
    };
    let cmd: Vec<String> = [
        "sh",
        "-c",
        "printf 'Welcome to the host! Last login: yesterday\\n'; sleep 30",
    ]
    .map(String::from)
    .to_vec();
    p.a_config(&cmd, "connect_secs = 2");
    let (out, _, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(!ok);
    assert_eq!(out.trim(), "b sent something that isn't the protocol");
}

/// A bridge with fewer methods than this build uses: the hello says so, the
/// link asks nothing it didn't list, and `hosts` says what is left out.
#[test]
fn an_older_bridge_is_asked_only_what_it_serves() {
    let Some(p) = Pair::start("older") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    let old = p.dir.join("old");
    std::fs::create_dir_all(&old).expect("dir");
    write_executable(
        &old.join("tmux-companion"),
        &format!(
            "#!/bin/sh\nexec {} \"$@\" --methods hello\n",
            binary().display()
        ),
    );
    let path = format!(
        "PATH={}:{}",
        old.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    p.a_config(&p.b_command(&[path]), "poll_secs = 1");
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    let out = p.hosts_until(10, |o| o.contains("link up"));
    assert!(
        out.contains("its bridge lacks sessions and segments and inbox"),
        "{out}"
    );
    assert!(!out.contains("api@b"), "sessions was never asked: {out}");
}

// ── Phase 4: push ───────────────────────────────────────────────────────────

/// A `tmux-companion` for B's PATH that runs the real one with `extra` after
/// its arguments: the bridge of an older build, say.
fn wrapped_bridge(p: &Pair, dir: &str, body: &str) -> String {
    let old = p.dir.join(dir);
    std::fs::create_dir_all(&old).expect("dir");
    write_executable(&old.join("tmux-companion"), body);
    format!(
        "PATH={}:{}",
        old.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

/// With a bridge that serves `watch`, a session made on B reaches A's list
/// with no poll: the poll is ten minutes away, and the row comes within
/// seconds, pushed when B's own listing saw it.
#[test]
fn a_watching_host_pushes_a_new_session_without_a_poll() {
    let Some(p) = Pair::start("watch") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    p.a_config(&p.b_command(&[]), "poll_secs = 600");
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    let out = p.hosts_until(15, |o| o.contains("api@b") && o.contains(", pushing,"));
    assert!(!out.contains("lacks"), "{out}");
    assert!(!out.contains("polled"), "{out}");

    p.b.must(&p.bin, &["new-session", "-d", "-s", "web"]);
    let started = Instant::now();
    let out = p.hosts_until(10, |o| o.contains("web@b"));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert!(out.contains("2 sessions"), "{out}");

    p.b.must(&p.bin, &["kill-session", "-t", "=api"]);
    p.hosts_until(10, |o| !o.contains("api@b") && o.contains("web@b"));
}

/// A bridge from before `watch`: the hello doesn't list it, so A polls as
/// it always did, and `hosts` says what the bridge lacks.
#[test]
fn an_older_bridge_without_watch_keeps_polling() {
    let Some(p) = Pair::start("nowatch") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    let path = wrapped_bridge(
        &p,
        "old",
        &format!(
            "#!/bin/sh\nexec {} \"$@\" --methods sessions,segments,inbox,dirs\n",
            binary().display()
        ),
    );
    p.a_config(&p.b_command(&[path]), "poll_secs = 1");
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    let out = p.hosts_until(15, |o| o.contains("api@b"));
    assert!(out.contains("its bridge lacks watch and ping"), "{out}");
    assert!(out.contains("link up, polled every 1s,"), "{out}");
    p.b.must(&p.bin, &["new-session", "-d", "-s", "web"]);
    p.hosts_until(10, |o| o.contains("web@b"));
}

/// A host that stops answering while A watches it: nothing is polled, so the
/// pings are what notice. Three go unanswered and the link is restarted,
/// saying why; the next try brings it back.
#[test]
fn a_watching_host_that_stops_answering_pings_is_restarted() {
    let Some(p) = Pair::start("pings") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    let pid = p.dir.join("bridge.pid");
    // The bridge in the background with its stdin kept, so the test can
    // stop it by its pid: a host that is there and says nothing. Through
    // fd 3: dash, Ubuntu's sh, points a background job's stdin at /dev/null
    // before its own redirects, so `<&0` there hands the bridge nothing.
    let path = wrapped_bridge(
        &p,
        "stoppable",
        &format!(
            "#!/bin/sh\nexec 3<&0\n{} \"$@\" <&3 &\necho $! > '{}'\nwait\n",
            binary().display(),
            pid.display()
        ),
    );
    p.a_config(
        &p.b_command(&[path]),
        "poll_secs = 600
ping_secs = 1",
    );
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    p.hosts_until(15, |o| o.contains("api@b") && o.contains("link up"));
    let first = std::fs::read_to_string(&pid)
        .expect("pid")
        .trim()
        .to_string();
    let signal = |sig: &str, who: &str| {
        let _ = Command::new("kill").args([sig, who]).status();
    };
    signal("-STOP", &first);

    let out = p.hosts_until(15, |o| o.contains("no answer to 3 pings in 3s"));
    assert!(out.contains("link down: "), "{out}");
    let out = p.hosts_until(20, |o| o.contains("link up"));
    assert!(out.contains("api@b"), "{out}");
    let second = std::fs::read_to_string(&pid)
        .expect("pid")
        .trim()
        .to_string();
    assert_ne!(first, second, "a new bridge");
    // The stopped one went with its link: TERM, then CONT so it takes it.
    signal("-KILL", &first);
}

/// A host whose daemon refuses its config: the bridge says so, and the
/// reason is the daemon's.
#[test]
fn a_host_whose_daemon_refuses_its_config_says_why() {
    let Some(p) = Pair::start("refuses") else {
        return;
    };
    std::fs::write(p.b.config(), "bogus_key = 1\n").expect("config");
    p.a_config(&p.b_command(&[]), "connect_secs = 5");
    let (out, _, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(!ok);
    assert!(out.starts_with("b: config: "), "{out}");
    assert!(
        out.contains("config check"),
        "the daemon's own words: {out}"
    );
}

/// `hosts add`: kept on the tmux server, or in config.toml with --save; a
/// taken name, a bad name and a command a shell would run are refused.
#[test]
fn hosts_add_keeps_a_host_on_the_server_or_in_the_config() {
    let Some(p) = Pair::start("add") else {
        return;
    };
    p.a.must(&p.bin, &["new-session", "-d", "-s", "here"]);
    let (out, err, ok) = p.a(&["hosts", "add", "vm", "--", "ssh", "dev@vm"]);
    assert!(ok, "{out}{err}");
    assert!(
        out.starts_with("vm kept until this tmux server exits"),
        "{out}"
    );
    let (out, _, _) = p.a(&["hosts"]);
    assert_eq!(
        out.lines().next(),
        Some("vm: on-demand, no link, never heard from, kept until this tmux server exits")
    );
    let opt = p.a.must(&p.bin, &["show-options", "-sv", "@tc-hosts"]);
    assert!(opt.contains("dev@vm"), "{opt}");

    let (_, err, ok) = p.a(&["hosts", "add", "vm", "--", "ssh", "other"]);
    assert!(!ok);
    assert!(
        err.contains("vm is already a host, reached with ssh dev@vm"),
        "{err}"
    );
    let (_, err, ok) = p.a(&["hosts", "add", "a.b", "--", "ssh", "x"]);
    assert!(!ok);
    assert!(err.contains("letters, digits"), "{err}");
    let (_, err, ok) = p.a(&["hosts", "add", "v2", "--", "ssh $HOST"]);
    assert!(!ok);
    assert!(err.contains("`$` isn't allowed"), "{err}");

    let (out, err, ok) = p.a(&["hosts", "add", "saved", "--save", "--", "ssh 'saved box'"]);
    assert!(ok, "{out}{err}");
    assert!(out.starts_with("saved saved to "), "{out}");
    let text = std::fs::read_to_string(p.a.config()).expect("config");
    assert!(text.contains("[[hosts]]"), "{text}");
    assert!(text.contains("\"saved box\""), "{text}");
    let (out, _, _) = p.a(&["hosts"]);
    let names: Vec<&str> = out
        .lines()
        .map(|l| l.split(':').next().unwrap_or(""))
        .collect();
    assert_eq!(names, ["saved", "vm"], "config first, then the server's");
}

/// The session on B the mirror of `remote` attached to: the one carrying
/// `@tc-mirror`, grouped with `remote`, named `remote~<machine>`.
fn grouped_on_b(p: &Pair, remote: &str) -> Option<(String, String)> {
    let (out, _, _) = p.b.tmux(
        &p.bin,
        &["list-sessions", "-F", "#{session_name}\t#{@tc-mirror}"],
    );
    out.lines()
        .filter_map(|l| l.split_once('\t'))
        .find(|(name, mirror)| !mirror.is_empty() && name.starts_with(&format!("{remote}~")))
        .map(|(n, m)| (n.to_string(), m.to_string()))
}

/// The whole of phase 2's opening path, through the switcher itself: B's
/// group and sessions in `--print`; the picker started in a pane on A opens
/// on B's session, the second session, and Enter makes `api@b` with its
/// options and switches A's client there; B gains a grouped session with
/// `@tc-mirror` and its bar off, which B's own switcher doesn't list; the
/// mirror bar carries B's git branch; `M-a` in `api@b` moves B's grouped
/// session to its other window; and a detach on B closes `api@b` and puts A's
/// client back where it was.
#[test]
fn a_remote_session_opens_as_a_mirror_session_and_closes_with_a_detach() {
    let Some(p) = Pair::start("mirror") else {
        return;
    };
    let repo = p.b.root.join("home/repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "trunk"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "first",
    ]);
    let repo_s = repo.display().to_string();
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api", "-c", &repo_s]);
    p.b.must(&p.bin, &["new-window", "-d", "-t", "=api:", "-c", &repo_s]);
    // B's own M-a, the way the example configs bind it.
    p.b.must(
        &p.bin,
        &[
            "bind",
            "-n",
            "M-a",
            "run-shell",
            "tmux-companion switch windows '#{session_id}' --client '#{client_name}'",
        ],
    );
    p.a_config(
        &p.fake_ssh(),
        "poll_secs = 1\n[project]\ndirs_source = \"none\"\n[switcher]\nannounce = false",
    );

    p.a.must(&p.bin, &["new-session", "-d", "-s", "here"]);
    let mut client = p.a_client("here");
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    p.hosts_until(15, |o| o.contains("api@b"));

    let (out, err, ok) = p.a(&["switch", "sessions", "--print"]);
    assert!(ok, "{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "here");
    assert!(lines[1].starts_with("b, 1 session, "), "{out}");
    assert_eq!(lines[2], "api@b", "{out}");

    // The picker in a pane of `here`, as M-s would open it in a popup: the
    // second session is B's, so Enter goes there.
    // By its full path and with the test's PATH: a login shell's PATH can
    // put an installed binary first, and an installed tmux, which on the
    // pinned legs is a newer client than the server it talks to.
    let picker = format!(
        "env PATH='{}:{}' '{}' switch sessions",
        p.bin.display(),
        std::env::var("PATH").unwrap_or_default(),
        binary().display()
    );
    p.a.must(&p.bin, &["send-keys", "-t", "=here:", &picker, "Enter"]);
    let screen = || p.a.tmux(&p.bin, &["capture-pane", "-p", "-t", "=here:"]).0;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !screen().contains("@b") {
        assert!(
            Instant::now() < deadline,
            "the picker never drew:\n{}",
            screen()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    p.a.must(&p.bin, &["send-keys", "-t", "=here:", "Enter"]);
    let client_session = || {
        p.a.tmux(&p.bin, &["list-clients", "-F", "#{client_session}"])
            .0
    };
    wait_for(10, "A's client in api@b", || client_session() == "api@b");
    let opt = |name: &str| {
        p.a.tmux(&p.bin, &["show-options", "-v", "-t", "=api@b:", name])
            .0
    };
    assert_eq!(opt("@tc-host"), "b");
    assert_eq!(opt("@tc-remote"), "api");
    assert_eq!(opt("prefix"), "None");
    assert_eq!(opt("detach-on-destroy"), "previous");

    let deadline = Instant::now() + Duration::from_secs(15);
    while grouped_on_b(&p, "api").is_none() {
        assert!(
            Instant::now() < deadline,
            "never: the grouped session on B; the mirror pane says:\n{}\nB has:\n{}\nA's window {} clients {}",
            p.a.tmux(&p.bin, &["capture-pane", "-p", "-t", "=api@b:"]).0,
            p.b.tmux(&p.bin, &["list-sessions"]).0,
            p.a.tmux(
                &p.bin,
                &[
                    "display-message",
                    "-p",
                    "-t",
                    "=api@b:",
                    "#{window_width}x#{window_height} #{pane_tty}"
                ]
            )
            .0,
            p.a.tmux(
                &p.bin,
                &[
                    "list-clients",
                    "-F",
                    "#{client_name} #{client_width}x#{client_height} #{client_flags}"
                ]
            )
            .0,
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let (grouped, _) = grouped_on_b(&p, "api").expect("grouped");
    let b_opt = |name: &str| {
        p.b.tmux(
            &p.bin,
            &["show-options", "-v", "-t", &format!("={grouped}:"), name],
        )
        .0
    };
    assert_eq!(b_opt("status"), "off");
    assert_eq!(b_opt("destroy-unattached"), "on");
    let (b_list, _, _) = p.b(&["switch", "sessions", "--print"]);
    assert_eq!(b_list.trim(), "api", "B's own list leaves the view out");

    // The bar: B's branch, rendered on A, in A's option.
    wait_for(15, "the mirror bar", || opt("@tc-right").contains("trunk"));
    let bar = opt("@tc-right");
    assert!(!bar.contains("#("), "{bar}");

    // M-a in api@b: the key goes on to B, whose switcher moves the grouped
    // session to its other window.
    let b_window = || {
        p.b.tmux(
            &p.bin,
            &[
                "display-message",
                "-p",
                "-t",
                &format!("={grouped}:"),
                "#{window_index}",
            ],
        )
        .0
    };
    let before = b_window();
    let id = p.a.must(
        &p.bin,
        &["display-message", "-p", "-t", "=api@b:", "#{session_id}"],
    );
    let (out, err, ok) = p.a(&["switch", "windows", &id]);
    assert!(ok, "{out}{err}");
    wait_for(10, "B's grouped session on its other window", || {
        b_window() != before
    });
    assert_eq!(
        p.b.tmux(
            &p.bin,
            &["display-message", "-p", "-t", "=api:", "#{window_index}"]
        )
        .0,
        before,
        "B's own session stays where it was"
    );

    // A detach on B: the shim ends cleanly, api@b closes, and A's client is
    // back where it came from.
    p.b.must(&p.bin, &["detach-client", "-s", &format!("={grouped}")]);
    wait_for(10, "A's client back in here", || client_session() == "here");
    let (_, _, still) = p.a.tmux(&p.bin, &["has-session", "-t", "=api@b"]);
    assert!(!still, "api@b closed");
    wait_for(5, "the grouped session gone", || {
        grouped_on_b(&p, "api").is_none()
    });
    let _ = client.kill();
    let _ = client.wait();
}

/// `sessions start NAME@HOST` with the name holding a quote and a semicolon:
/// the mirror session opens and the grouped session on B is made, every name
/// arriving as a name and nothing in it run. A typed name B has no session
/// by is made there.
#[test]
fn a_session_named_like_a_command_opens_as_a_name() {
    let Some(p) = Pair::start("nastymirror") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", NASTY]);
    p.a_config(&p.fake_ssh(), "poll_secs = 1");
    p.a.must(&p.bin, &["new-session", "-d", "-s", "here"]);
    let spec = format!("{NASTY}@b");
    let (out, err, ok) = p.a(&["sessions", "start", &spec, "--detach"]);
    assert!(ok, "{out}{err}");
    assert_eq!(out.trim(), format!("{NASTY}@b is open"));
    wait_for(15, "the grouped session on B, attached", || {
        grouped_on_b(&p, NASTY).is_some_and(|(g, _)| {
            p.b.tmux(
                &p.bin,
                &[
                    "display-message",
                    "-p",
                    "-t",
                    &format!("={g}:"),
                    "#{session_attached}",
                ],
            )
            .0 == "1"
        })
    });
    let mirror = p.a.must(
        &p.bin,
        &[
            "show-options",
            "-v",
            "-t",
            &format!("={NASTY}@b:"),
            "@tc-remote",
        ],
    );
    assert_eq!(mirror, NASTY);

    let (out, err, ok) = p.a(&["sessions", "start", "fresh@b", "--detach"]);
    assert!(ok, "{out}{err}");
    wait_for(15, "fresh made on B", || {
        p.b.tmux(&p.bin, &["has-session", "-t", "=fresh"]).2
    });
    wait_for(15, "and its grouped session", || {
        grouped_on_b(&p, "fresh").is_some()
    });

    for dir in [&p.dir, &p.a.root, &p.b.root] {
        assert!(!dir.join("pwned").exists(), "a session name ran something");
    }
}

/// A session that went from B: the shim says so in one line and waits.
#[test]
fn a_session_gone_from_the_host_is_said_in_one_line() {
    let Some(p) = Pair::start("gone") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "keep"]);
    p.a_config(&p.fake_ssh(), "poll_secs = 1");
    p.a.must(&p.bin, &["new-session", "-d", "-s", "here"]);
    // In a pane, for a terminal: the shim waits for a key there.
    let exe = binary().display().to_string();
    p.a.must(
        &p.bin,
        &[
            "new-window",
            "-d",
            "-t",
            "=here:",
            "-n",
            "shim",
            &format!("'{exe}' host attach b missing"),
        ],
    );
    wait_for(15, "the shim's line", || {
        p.a.tmux(&p.bin, &["capture-pane", "-p", "-t", "=here:shim"])
            .0
            .contains("missing no longer exists on b")
    });
    let screen =
        p.a.must(&p.bin, &["capture-pane", "-p", "-t", "=here:shim"]);
    assert!(screen.contains("r reconnect   q close"), "{screen}");
    p.a.must(&p.bin, &["send-keys", "-t", "=here:shim", "q"]);
    wait_for(10, "the shim closed on q", || {
        !p.a.tmux(
            &p.bin,
            &["list-windows", "-t", "=here", "-F", "#{window_name}"],
        )
        .0
        .contains("shim")
    });
}

// ── Phase 3: the rest of the read side ──────────────────────────────────────

/// The pane id of the first pane in `target` on a side.
fn pane_id(side: &Side, bin: &Path, target: &str) -> String {
    side.must(bin, &["display-message", "-p", "-t", target, "#{pane_id}"])
}

/// `brief` says each host in one line from what A already knows, and starts
/// no link for it: with B's daemon gone, a brief that linked would start it
/// again through the bridge.
#[test]
fn the_brief_says_each_host_from_the_cache_and_starts_no_link() {
    let Some(p) = Pair::start("brief") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    p.a_config(&p.b_command(&[]), "poll_secs = 1\nidle_secs = 1");
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    p.hosts_until(15, |o| o.contains("api@b"));
    // The link closes once nothing wants it, then B's daemon goes.
    p.hosts_until(15, |o| o.starts_with("b: on-demand, no link"));
    let _ = p.b(&["shutdown"]);
    wait_for(10, "B's daemon gone", || !p.b.daemon_running());

    let (out, err, ok) = p.a(&["brief", "--print"]);
    assert!(ok, "{out}{err}");
    let line = out
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("b: "))
        .unwrap_or_else(|| panic!("no host line in the brief:\n{out}"));
    assert!(line.starts_with("b: 1 session, last heard "), "{line}");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!p.b.daemon_running(), "the brief started a link");
    let (hosts, _, _) = p.a(&["hosts"]);
    assert!(hosts.starts_with("b: on-demand, no link"), "{hosts}");
}

/// B's waiting agent arrives in A's inbox with `on b` where its place is,
/// sounds A's earcon and is counted in A's brief. Picking it opens `api@b` with the
/// pane chosen on B; picking it again with the mirror open respawns the
/// attach with the same pane.
#[test]
fn a_hosts_agent_is_in_the_inbox_and_a_pick_goes_to_its_pane() {
    let Some(p) = Pair::start("inbox") else {
        return;
    };
    std::fs::write(
        p.b.config(),
        // Long enough that the panes going quiet is no news: only the
        // hook's `asked` puts the agent in the inbox.
        "[agents]\nprograms = [\"sleep\"]\nwaiting_secs = 3600\ninterval_secs = 1\n",
    )
    .expect("B config");
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api", "sleep 600"]);
    p.b.must(&p.bin, &["new-window", "-d", "-t", "=api:", "sleep 600"]);
    let windows = p.b.must(
        &p.bin,
        &["list-windows", "-t", "=api", "-F", "#{window_index}"],
    );
    let second = windows.lines().nth(1).expect("two windows").to_string();
    let agent = pane_id(&p.b, &p.bin, &format!("=api:{second}"));
    let earcon = p.dir.join("earcon");
    p.a_config(
        &p.fake_ssh(),
        &format!(
            "poll_secs = 1\n[earcons]\nenabled = true\nasked = [\"touch\", {}]\n",
            toml_string(&earcon.display().to_string())
        ),
    );
    p.a.must(&p.bin, &["new-session", "-d", "-s", "here"]);
    let mut client = p.a_client("here");
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    p.hosts_until(15, |o| o.contains("api@b"));
    // A poll or two with nothing waiting, so the asking is news.
    std::thread::sleep(Duration::from_millis(2500));
    assert!(!earcon.exists(), "nothing has asked yet");

    let (out, err, ok) = p.b(&["agent", "asked", "--pane", &agent]);
    assert!(ok, "{out}{err}");
    let mut row = String::new();
    wait_for(20, "B's agent in A's inbox", || {
        let (out, _, _) = p.a(&["inbox", "--print"]);
        row = out
            .lines()
            .find(|l| l.contains(" on b\t"))
            .unwrap_or_default()
            .to_string();
        !row.is_empty()
    });
    assert!(
        row.starts_with(&format!("api:{second}.0 on b\t")),
        "{row:?}"
    );
    assert!(row.contains("asked"), "{row:?}");
    assert!(row.ends_with(&format!("\t{agent}")), "{row:?}");
    wait_for(10, "A's earcon for B's agent", || earcon.exists());
    let (brief, _, _) = p.a(&["brief", "--print"]);
    assert!(brief.contains("1 agent asked"), "{brief}");

    // The pick, through the inbox picker in a pane of `here`.
    let picker = format!(
        "env PATH='{}:{}' '{}' inbox",
        p.bin.display(),
        std::env::var("PATH").unwrap_or_default(),
        binary().display()
    );
    let pick = || {
        p.a.must(&p.bin, &["send-keys", "-t", "=here:", &picker, "Enter"]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !p
            .a
            .tmux(&p.bin, &["capture-pane", "-p", "-t", "=here:"])
            .0
            .contains(" on b")
        {
            assert!(Instant::now() < deadline, "the inbox never drew");
            std::thread::sleep(Duration::from_millis(100));
        }
        p.a.must(&p.bin, &["send-keys", "-t", "=here:", "Enter"]);
    };
    pick();
    let client_session = || {
        p.a.tmux(&p.bin, &["list-clients", "-F", "#{client_session}"])
            .0
    };
    wait_for(10, "A's client in api@b", || client_session() == "api@b");
    let on_b = || {
        grouped_on_b(&p, "api").map(|(g, _)| {
            p.b.tmux(
                &p.bin,
                &[
                    "display-message",
                    "-p",
                    "-t",
                    &format!("={g}:"),
                    "#{pane_id}",
                ],
            )
            .0
        })
    };
    wait_for(15, "the grouped session on B at the agent's pane", || {
        on_b().as_deref() == Some(agent.as_str())
    });
    let (grouped, _) = grouped_on_b(&p, "api").expect("grouped");
    assert_eq!(
        p.b.must(
            &p.bin,
            &["display-message", "-p", "-t", "=api:", "#{window_index}"]
        ),
        windows.lines().next().expect("first"),
        "B's own session stays where it was"
    );

    // Elsewhere on B, then the pick again with api@b open: the attach is
    // respawned with the pane chained, and lands there.
    let first = windows.lines().next().expect("first").to_string();
    p.b.must(
        &p.bin,
        &["select-window", "-t", &format!("={grouped}:{first}")],
    );
    wait_for(5, "the grouped session moved", || {
        on_b().as_deref() != Some(agent.as_str())
    });
    p.a.must(&p.bin, &["switch-client", "-t", "=here"]);
    pick();
    wait_for(10, "A's client in api@b again", || {
        client_session() == "api@b"
    });
    wait_for(15, "the grouped session back at the agent's pane", || {
        on_b().as_deref() == Some(agent.as_str())
    });
    let (sessions, _, _) =
        p.a.tmux(&p.bin, &["list-sessions", "-F", "#{session_name}"]);
    assert_eq!(
        sessions.lines().filter(|l| *l == "api@b").count(),
        1,
        "one mirror session: {sessions}"
    );
    let _ = client.kill();
    let _ = client.wait();
}

/// Directories come from one place: inside a mirror session, the host's own
/// configured source over the link, listed on A as `name@b`. The shim with
/// `--dir` starts the project on B with `project --detached` and attaches to
/// the session it names; `project --detached` alone makes the session and
/// prints its name, switching nobody.
#[test]
fn a_hosts_directories_come_over_the_link_and_open_there() {
    let Some(p) = Pair::start("dirs") else {
        return;
    };
    // Canonical, since /tmp is a link on macOS and a session's path is
    // where tmux resolved it.
    let home_b = p.b.root.join("home").canonicalize().expect("home");
    let (w1, w2) = (home_b.join("w1"), home_b.join("w2"));
    std::fs::create_dir_all(&w1).expect("w1");
    std::fs::create_dir_all(&w2).expect("w2");
    std::fs::write(
        p.b.config(),
        format!(
            "[project]\ndirs_command = [\"printf\", \"%s\\n\", {}, {}]\n",
            toml_string(&w1.display().to_string()),
            toml_string(&w2.display().to_string())
        ),
    )
    .expect("B config");
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    p.a_config(
        &p.fake_ssh(),
        "poll_secs = 1\n[project]\ndirs_source = \"none\"",
    );

    // project --detached on B: the session, its name, no client moved.
    let w2s = w2.display().to_string();
    let (out, err, ok) = p.b(&["project", "--detached", &w2s]);
    assert!(ok, "{out}{err}");
    assert_eq!(out.trim(), "w2");
    assert!(p.b.tmux(&p.bin, &["has-session", "-t", "=w2"]).2);
    let (out, err, ok) = p.b(&["project", "--detached", &w2s]);
    assert!(ok && out.trim() == "w2", "again is the same: {out}{err}");

    // A in a mirror session: the picker's rows, the host's directories.
    let (out, err, ok) = p.a(&["sessions", "start", "api@b", "--detach"]);
    assert!(ok, "{out}{err}");
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    p.hosts_until(15, |o| o.contains("w2@b"));
    let tmux_env = format!("{},1,0", p.a.tmux_sock().display());
    let mut cmd = Command::new(binary());
    cmd.args(["project", "--print"]);
    p.a.env(&mut cmd, &p.bin);
    cmd.env("TMUX", &tmux_env);
    let o = cmd.output().expect("project runs");
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(
        o.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("session\tapi@b\t"), "{out}");
    assert!(
        lines.iter().any(|l| l.starts_with("host\tb, ")),
        "B's group line: {out}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("session\tw2@b")),
        "B's other session: {out}"
    );
    assert!(
        lines.contains(&format!("dir\tw1@b\t{}\t", w1.display()).as_str()),
        "B's directory, over the link: {out}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("dir\tw2@b")),
        "a directory one of B's sessions stands for is left out: {out}"
    );

    // Outside a mirror session the same picker lists A's own source, here
    // none, and still every session on every host.
    let (out, _, _) = p.a(&["project", "--print"]);
    assert!(!out.contains("dir\t"), "{out}");
    assert!(out.contains("session\tw2@b"), "{out}");

    // The shim with --dir, in a pane of A: B gains the project and a grouped
    // session on it.
    p.a.must(&p.bin, &["new-session", "-d", "-s", "here"]);
    let exe = binary().display().to_string();
    p.a.must(
        &p.bin,
        &[
            "new-window",
            "-d",
            "-t",
            "=here:",
            "-n",
            "shim",
            &format!("'{exe}' host attach b w1 --dir '{}'", w1.display()),
        ],
    );
    wait_for(15, "w1 made on B", || {
        p.b.tmux(&p.bin, &["has-session", "-t", "=w1"]).2
    });
    wait_for(15, "and its grouped session", || {
        grouped_on_b(&p, "w1").is_some()
    });
}

/// A mirror session is kept in a snapshot with its host and its session on
/// the host, and a resurrect reopens it with the shim waiting for `r`: no
/// ssh until the key, then the attach.
#[test]
fn a_mirror_session_survives_a_snapshot_and_comes_back_waiting_for_r() {
    let Some(p) = Pair::start("resurrect") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    p.a_config(&p.fake_ssh(), "poll_secs = 1");
    p.a.must(&p.bin, &["new-session", "-d", "-s", "here"]);
    let (out, err, ok) = p.a(&["sessions", "start", "api@b", "--detach"]);
    assert!(ok, "{out}{err}");
    wait_for(15, "the grouped session on B", || {
        grouped_on_b(&p, "api").is_some()
    });
    let (out, err, ok) = p.a(&["sessions", "save", "--skip-pane-history"]);
    assert!(ok, "{out}{err}");
    let snaps = p.a.state().join("sessions");
    let newest = std::fs::read_dir(&snaps)
        .unwrap_or_else(|e| panic!("{}: {e}", snaps.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .max()
        .expect("a snapshot");
    let text = std::fs::read_to_string(&newest).expect("snapshot");
    assert!(text.contains("host = \"b\""), "{text}");
    assert!(text.contains("remote = \"api\""), "{text}");
    assert!(
        text.contains("command = \"tmux-companion host attach b api\""),
        "{text}"
    );

    let _ = p.a.tmux(&p.bin, &["kill-server"]);
    wait_for(15, "the grouped session gone with A's server", || {
        grouped_on_b(&p, "api").is_none()
    });
    let (out, err, ok) = p.a(&["sessions", "resurrect", "--yes", "--detach"]);
    assert!(ok, "{out}{err}");
    let opt = |name: &str| {
        p.a.tmux(&p.bin, &["show-options", "-v", "-t", "=api@b:", name])
            .0
    };
    assert_eq!(opt("@tc-host"), "b");
    assert_eq!(opt("@tc-remote"), "api");
    assert_eq!(opt("prefix"), "None");
    let screen = || p.a.tmux(&p.bin, &["capture-pane", "-p", "-t", "=api@b:"]).0;
    wait_for(10, "the shim waiting", || {
        screen().contains("b: press r to connect")
    });
    std::thread::sleep(Duration::from_millis(1000));
    assert!(grouped_on_b(&p, "api").is_none(), "no ssh before r");
    p.a.must(&p.bin, &["send-keys", "-t", "=api@b:", "r"]);
    wait_for(15, "the attach after r", || {
        grouped_on_b(&p, "api").is_some()
    });
}

/// A daemon on B that answers what a bridge asks with whatever the test
/// says, the way a broken or hostile host could: an inbox entry whose words
/// hold control characters and `#(`. A keeps them as text, without the
/// control characters, and runs nothing.
#[test]
fn a_hosts_inbox_arrives_as_text_whatever_it_holds() {
    let Some(p) = Pair::start("dirty") else {
        return;
    };
    if Command::new("python3").arg("-V").output().is_err() {
        skipping("dirty", "no python3 for the fake daemon");
        return;
    }
    let script = p.dir.join("fake-daemon.py");
    std::fs::write(
        &script,
        r#"import json, socket, sys, time
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.bind(sys.argv[1])
s.listen(8)
sessions = {"polled_at": 1, "sessions": [{
    "name": "api", "id": "$1", "windows": 1, "attached": 0,
    "last_attached": 0, "activity": 0, "used": 0, "path": "/"}]}
inbox = [{
    "id": "%9", "at": "api:0.0", "program": "cl\u0007aude #(touch pwned)",
    "path": "/", "since": 1, "lines": "", "state": "asked",
    "question": "ok\u001b[31m? #(touch pwned)\nnext"}]
answers = {
    "noop": "",
    "__link_sessions": json.dumps(sessions),
    "__inbox": json.dumps(inbox),
    "__link_watch": json.dumps({"generation": 1, "sessions": sessions,
                                "inbox": inbox, "segments": []}),
}
while True:
    c, _ = s.accept()
    line = c.makefile().readline()
    if not line:
        c.close()
        continue
    req = json.loads(line)
    cmd = req.get("cmd", "")
    # A watch that already has generation 1 waits, as a daemon with no
    # change would, rather than answer the same thing in a loop.
    if cmd == "__link_watch" and (req.get("args") or {}).get("since") is not None:
        time.sleep(1)
    out = answers.get(cmd)
    resp = {"output": out or "", "error": None if out is not None else "no",
            "version": "9.9.9+9999999999"}
    c.sendall((json.dumps(resp) + "\n").encode())
    c.close()
"#,
    )
    .expect("script");
    let mut fake = Command::new("python3")
        .arg("-I")
        .arg(&script)
        .arg(p.b.sock())
        .spawn()
        .expect("fake daemon");
    wait_for(10, "the fake daemon listening", || p.b.daemon_running());
    p.a_config(&p.b_command(&[]), "poll_secs = 1");
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    let mut row = String::new();
    wait_for(15, "B's entry in A's inbox", || {
        let (out, _, _) = p.a(&["inbox", "--print"]);
        row = out
            .lines()
            .find(|l| l.contains(" on b"))
            .unwrap_or_default()
            .to_string();
        !row.is_empty()
    });
    assert!(
        !row.chars().any(|c| c.is_control() && c != '\t'),
        "a control character arrived: {row:?}"
    );
    assert!(row.contains("claude #(touch pwned)"), "{row:?}");
    assert!(row.contains("ok[31m? #(touch pwned)next"), "{row:?}");
    let (brief, _, _) = p.a(&["brief", "--print"]);
    assert!(
        !brief.chars().any(|c| c.is_control() && c != '\n'),
        "{brief:?}"
    );
    assert!(brief.contains("b: 1 session, 1 agent asked"), "{brief}");
    for dir in [&p.dir, &p.a.root, &p.b.root] {
        assert!(!dir.join("pwned").exists(), "an entry ran something");
    }
    let _ = fake.kill();
    let _ = fake.wait();
}

// ── Phase 6: handover ───────────────────────────────────────────────────────

/// A third tmux server whose one pane is the terminal A's client runs in:
/// a handover gives that terminal to B's tmux, which needs a real one. Its
/// server goes when this does.
struct Outer {
    sock: PathBuf,
}

impl Outer {
    fn tmux(&self, p: &Pair, args: &[&str]) -> (String, bool) {
        let mut cmd = Command::new("tmux");
        cmd.arg("-S").arg(&self.sock).arg("-f").arg("/dev/null");
        cmd.args(args);
        p.a.env(&mut cmd, &p.bin);
        let o = cmd.output().expect("tmux runs");
        (
            String::from_utf8_lossy(&o.stdout).trim_end().to_string(),
            o.status.success(),
        )
    }
}

impl Drop for Outer {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .arg("-S")
            .arg(&self.sock)
            .arg("kill-server")
            .env_remove("TMUX")
            .output();
    }
}

/// alt-enter on B's session in `M-s`, for a host that mirrors: the message
/// line says so, A's client is replaced by the host command, B's own tmux
/// has the terminal on `api` itself (no mirror session here, no grouped
/// session there), and a detach on B comes back to the session A's client
/// left.
#[test]
fn a_handover_gives_the_terminal_to_the_host_and_a_detach_comes_back() {
    let Some(p) = Pair::start("handover") else {
        return;
    };
    p.b.must(&p.bin, &["new-session", "-d", "-s", "api"]);
    p.a_config(
        &p.fake_ssh(),
        "poll_secs = 1\n[project]\ndirs_source = \"none\"\n[switcher]\nannounce = false",
    );
    p.a.must(&p.bin, &["new-session", "-d", "-s", "here"]);
    let outer = Outer {
        sock: p.dir.join("c.sock"),
    };
    let attach = format!(
        // `=here` quoted: the pane's shell may be zsh, which reads a
        // leading `=` as a command's path.
        "env -u TMUX tmux -S '{}' attach-session -t '=here'",
        p.a.tmux_sock().display()
    );
    // The pane stays when its client ends, so a failure can say what the
    // terminal showed.
    let (_, ok) = outer.tmux(
        &p,
        &[
            "start-server",
            ";",
            "set-option",
            "-g",
            "remain-on-exit",
            "on",
            ";",
            "new-session",
            "-d",
            "-x",
            "120",
            "-y",
            "40",
            "-s",
            "outer",
            &attach,
        ],
    );
    assert!(ok, "the outer terminal");
    let terminal = || {
        format!(
            "{}\n{}",
            outer
                .tmux(
                    &p,
                    &[
                        "display-message",
                        "-p",
                        "-t",
                        "=outer:",
                        "#{pane_dead} #{pane_dead_status} #{pane_current_command} #{pane_start_command}"
                    ]
                )
                .0,
            outer.tmux(&p, &["capture-pane", "-p", "-t", "=outer:"]).0
        )
    };
    let a_clients = || {
        p.a.tmux(&p.bin, &["list-clients", "-F", "#{client_session}"])
            .0
    };
    let b_clients = || {
        p.b.tmux(&p.bin, &["list-clients", "-F", "#{client_session}"])
            .0
    };
    wait_until(
        10,
        || a_clients() == "here",
        || {
            format!(
                "A's client in here, not {:?}; the terminal:\n{}",
                a_clients(),
                terminal()
            )
        },
    );
    let (out, err, ok) = p.a(&["hosts", "connect", "b"]);
    assert!(ok, "{out}{err}");
    p.hosts_until(15, |o| o.contains("api@b"));

    let picker = format!(
        "env PATH='{}:{}' '{}' switch sessions",
        p.bin.display(),
        std::env::var("PATH").unwrap_or_default(),
        binary().display()
    );
    p.a.must(&p.bin, &["send-keys", "-t", "=here:", &picker, "Enter"]);
    let screen = || p.a.tmux(&p.bin, &["capture-pane", "-p", "-t", "=here:"]).0;
    wait_for(10, "the picker", || screen().contains("@b"));
    p.a.must(&p.bin, &["send-keys", "-t", "=here:", "M-Enter"]);

    wait_until(
        15,
        || b_clients() == "api",
        || {
            format!(
                "B's tmux with a client on api; the terminal:\n{}",
                terminal()
            )
        },
    );
    wait_for(5, "A's client gone", || a_clients().is_empty());
    let (_, _, mirror) = p.a.tmux(&p.bin, &["has-session", "-t", "=api@b"]);
    assert!(!mirror, "a handover makes no mirror session");
    assert!(grouped_on_b(&p, "api").is_none(), "nor a grouped session");

    p.b.must(&p.bin, &["detach-client", "-s", "=api"]);
    wait_until(
        10,
        || a_clients() == "here",
        || format!("A's client back in here; the terminal:\n{}", terminal()),
    );
    assert!(b_clients().is_empty(), "{}", b_clients());
}
