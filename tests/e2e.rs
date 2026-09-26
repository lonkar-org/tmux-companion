//! End-to-end tests against a real tmux.
//!
//! The unit tests assert values. These assert behaviour, which is a different
//! question and the one that kept being answered wrong: every bug found by
//! using this tool rather than testing it was invisible to 639 unit tests.
//!
//! A command that was never wired to a subcommand. A picker footer promising a
//! key nobody implemented. A line in the shipped example config that made the
//! whole right-hand side render empty. A dialog that answered itself because
//! raw mode failed. All of them pass every unit test in the repository, because
//! none of them is a wrong value.
//!
//! Each test gets its own tmux server on its own socket, its own config, state
//! and daemon socket, and tears the lot down afterwards. Nothing here can see
//! the tmux the developer is sitting in, and the sockets live directly in
//! `/tmp` because a unix socket address holds 103 bytes and a path under a long
//! temporary directory silently falls back to the live daemon.
//!
//! They skip rather than fail when tmux is not installed, so `cargo test` still
//! works on a machine without it, and `TC_SKIP_E2E=1` skips them on a machine
//! that has one. Neither holds when `CI` is set: there a skip is a failure,
//! because a suite that passes by not running is the thing CI exists to notice.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// A private tmux server, a private daemon socket and a sandbox to run in.
struct Tmux {
    socket: String,
    daemon_sock: PathBuf,
    sandbox: PathBuf,
    binary: PathBuf,
}

/// Serial numbers for the daemon sockets, so each test gets its own.
///
/// One socket for the whole binary was the obvious thing and it leaked a
/// daemon per run. Tests run in parallel, so one test's teardown sent
/// `__shutdown` while another test's client was still calling, and that client
/// started a replacement daemon a moment after the shutdown that was supposed
/// to be the last one. Nothing was left to stop the replacement.
static DAEMON_SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl Tmux {
    /// Start a server with the config this repository ships.
    ///
    /// The shipped example rather than a minimal one on purpose: it is the
    /// file people copy, and nothing else in the repository ever ran it.
    fn start(name: &str) -> Option<Self> {
        if std::env::var_os("TC_SKIP_E2E").is_some_and(|v| v == "1") {
            skipping(name, "TC_SKIP_E2E=1");
            return None;
        }
        if Command::new("tmux").arg("-V").output().is_err() {
            skipping(name, "no tmux on this machine");
            return None;
        }
        let binary = target_binary();
        let sandbox = std::env::temp_dir().join(format!("tce2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&sandbox);
        // A sandbox that cannot be made is a failure, not a reason to skip:
        // `.ok()?` here used to turn a full /tmp into a green test.
        for dir in ["bin", "state", "config/tmux-companion"] {
            let path = sandbox.join(dir);
            std::fs::create_dir_all(&path)
                .unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));
        }
        // On PATH under its own name, because the example config calls
        // `tmux-companion` and that is the thing under test.
        let _ = std::os::unix::fs::symlink(&binary, sandbox.join("bin/tmux-companion"));

        // Short, and not built from `name`: a socket address holds about a
        // hundred bytes and these test names run to sixty.
        let serial = DAEMON_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let t = Tmux {
            socket: format!("tce2e{name}{}", std::process::id()),
            daemon_sock: PathBuf::from(format!("/tmp/tce2e{}-{serial}.sock", std::process::id())),
            sandbox,
            binary,
        };
        t.write_tmux_shim();
        Some(t)
    }

    /// A `tmux` on PATH that always talks to this test's own server.
    ///
    /// `env` points the tool at the right server with `$TMUX`, which works for
    /// everything that reads it. `sessions shutdown` and `sessions restart`
    /// refuse to run with `$TMUX` set, because stopping a server from inside it
    /// takes the pane with it, so testing those means running with `$TMUX`
    /// unset, and with it unset a bare `tmux` finds the developer's own server
    /// and kills that instead. It has happened once in this repository's
    /// history and the shim is what makes it impossible.
    ///
    /// `-L` is added only when the caller did not pass one, so [`Tmux::tmux`],
    /// which passes its own, goes through unchanged.
    fn write_tmux_shim(&self) {
        let Ok(real) = Command::new("sh").args(["-c", "command -v tmux"]).output() else {
            return;
        };
        let real = String::from_utf8_lossy(&real.stdout).trim().to_string();
        if real.is_empty() {
            return;
        }
        let shim = self.sandbox.join("bin/tmux");
        let script = format!(
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = -L ] && exec {real} \"$@\"; done\nexec {real} -L {} \"$@\"\n",
            self.socket
        );
        if std::fs::write(&shim, script).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755));
        }
    }

    /// The binary, as if run from a terminal that is not inside tmux.
    ///
    /// The shim keeps its tmux calls on this test's server even with `$TMUX`
    /// unset, which is the only safe way to exercise the commands that stop a
    /// server.
    fn run_outside(&self, args: &[&str]) -> (String, String, bool) {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args);
        self.env(&mut cmd);
        cmd.env_remove("TMUX");
        match cmd.output() {
            Ok(o) => (
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string(),
                o.status.success(),
            ),
            Err(e) => (String::new(), e.to_string(), false),
        }
    }

    /// Every session on this test's server, sorted.
    fn sessions(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .tmux(&["list-sessions", "-F", "#{session_name}"])
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        out.sort();
        out
    }

    /// Every pane on this test's server as `session:window.pane command`.
    fn panes(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .tmux(&[
                "list-panes",
                "-a",
                "-F",
                "#{session_name}:#{window_index}.#{pane_index} #{pane_current_command}",
            ])
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        out.sort();
        out
    }

    /// The environment every command here runs in.
    fn env(&self, cmd: &mut Command) {
        let path = format!(
            "{}:{}",
            self.sandbox.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        // Which tmux the tool talks to. Without this the binary shells out to
        // plain `tmux`, which finds a server through $TMUX and otherwise falls
        // back to the default socket, so every tmux command the tool ran went
        // to the developer's own server or, on a machine with no tmux running,
        // to nothing at all: `error connecting to /tmp/tmux-0/default`.
        //
        // That is not a detail. `a_new_session_is_painted_by_the_hook_the_
        // example_config_sets` exists to prove that `theme apply -t ""`
        // resolves an empty target to the session it names, and that call has
        // never once reached the server the test set up. It passed anyway,
        // because the session-created hook runs inside the server, inherits
        // $TMUX, and repainted the session a moment after the test had unset
        // the option. Whether that repaint landed before or after the unset is
        // what decided the result, which is why it passed here and failed on
        // CI, and why three separate theories about tmux targets went nowhere.
        //
        // The path is where tmux puts a socket named with -L. The uid comes
        // off a directory this process just made rather than from another
        // dependency.
        let uid = std::fs::metadata(&self.sandbox)
            .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
            .unwrap_or(0);
        let tmux_sock = format!("/tmp/tmux-{uid}/{}", self.socket);

        cmd.env("TMUX", format!("{tmux_sock},0,0"))
            .env("PATH", path)
            // HOME as well as XDG_CONFIG_HOME, because the themes directory is
            // resolved from both: with no `tmux.conf` under XDG_CONFIG_HOME,
            // `themes_dir` looks for `$HOME/.tmux.conf` and, finding one,
            // answers `$HOME/.tmux/themes`. Leaving HOME alone therefore let
            // the machine running the suite decide where a test's themes went,
            // and the ubuntu CI runner has a `~/.tmux.conf` where this laptop
            // and act's container do not. That is the whole of the failure
            // that looked like a tmux 3.4 difference for two pushes.
            .env("HOME", &self.sandbox)
            .env("XDG_CONFIG_HOME", self.sandbox.join("config"))
            .env("XDG_STATE_HOME", self.sandbox.join("state"))
            .env("_ZO_DATA_DIR", self.sandbox.join("zoxide"))
            .env("TMUX_COMPANION_SOCK", &self.daemon_sock)
            .env("PS1", "demo %# ")
            .env("PROMPT", "demo %# ");
    }

    /// One tmux command against this server.
    fn tmux(&self, args: &[&str]) -> String {
        let mut cmd = Command::new("tmux");
        cmd.arg("-L").arg(&self.socket).args(args);
        self.env(&mut cmd);
        match cmd.output() {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim_end().to_string(),
            Err(_) => String::new(),
        }
    }

    /// The binary itself, outside tmux.
    fn run(&self, args: &[&str]) -> (String, String, bool) {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args);
        self.env(&mut cmd);
        match cmd.output() {
            Ok(o) => (
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string(),
                o.status.success(),
            ),
            Err(e) => (String::new(), e.to_string(), false),
        }
    }

    /// Create a session running the shipped example config.
    fn session(&self, name: &str, dir: &Path) {
        let conf = repo_root().join("docs/tmux.conf.full.example");
        self.tmux(&[
            "-f",
            &conf.display().to_string(),
            "new-session",
            "-d",
            "-s",
            name,
            "-c",
            &dir.display().to_string(),
            "-x",
            "120",
            "-y",
            "32",
        ]);
    }

    /// What a pane currently shows.
    fn capture(&self, target: &str) -> String {
        self.tmux(&["capture-pane", "-p", "-t", target])
    }

    /// Wait for something to become true, so a slow machine does not turn a
    /// working feature into a failing test.
    fn until(&self, secs: u64, mut f: impl FnMut(&Self) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if f(self) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        self.tmux(&["kill-server"]);
        let mut cmd = Command::new(&self.binary);
        cmd.arg("__shutdown");
        self.env(&mut cmd);
        let _ = cmd.output();
        // The daemon unlinks this itself on the way out. Removed again here
        // because a daemon that never started still leaves the client's own
        // failed bind behind, and a socket file with nothing behind it reads
        // as a live daemon to anything that stats the path.
        let _ = std::fs::remove_file(&self.daemon_sock);
        let _ = std::fs::remove_dir_all(&self.sandbox);
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The binary under test.
///
/// Cargo builds it for every integration test run and hands the path over in
/// `CARGO_BIN_EXE_<name>`, so there is nothing to look for. This used to guess
/// from `current_exe`, two directories up, and a guess that missed came back
/// as `None`, which every caller read as "skip": the same silence the
/// missing-tmux case had, with no line printed to say so.
fn target_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tmux-companion"))
}

/// A test is about to skip. Say so once, or refuse if this is CI.
///
/// Every tmux test in this file skips by returning early from
/// [`Tmux::start`], and a skipped test is a passed test as far as cargo can
/// tell. On a laptop that is the right trade: no tmux, no e2e, the rest of
/// the suite still runs. On a runner it is the wrong one, and the comment in
/// ci.yml that said so enforced nothing. `CI` is set on every GitHub runner,
/// so with it set a skip panics and the job goes red instead of quiet.
fn skipping(name: &str, why: &str) {
    assert!(
        std::env::var_os("CI").is_none(),
        "{name} would skip ({why}), and CI is set, so it fails instead. \
         Install tmux on the runner, or unset TC_SKIP_E2E."
    );
    static ANNOUNCED: std::sync::Once = std::sync::Once::new();
    ANNOUNCED.call_once(|| eprintln!("skipping the tmux tests in tests/e2e.rs: {why}"));
}

/// A git repository with something for the bar to draw.
fn repo_with_changes(root: &Path) -> PathBuf {
    let dir = root.join("acme-api");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let git = |args: &[&str]| {
        let _ = Command::new("git").args(args).current_dir(&dir).output();
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "demo@example.com"]);
    git(&["config", "user.name", "Demo"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "first"]);
    std::fs::write(dir.join("src/main.rs"), "fn main() {}\n// changed\n").unwrap();
    dir
}

// ── the tests ────────────────────────────────────────────────────────────────

/// Every `#(...)` the example config puts on the bar, with tmux's format
/// specifiers replaced by something real.
///
/// `#{E:status-right}` is not a way to check this: it expands formats and does
/// not run `#()` at all, so a bar that draws nothing looks identical to one
/// that draws perfectly.
fn bar_commands(conf: &str, repo: &Path) -> Vec<Vec<String>> {
    let pid = std::process::id().to_string();
    let repo = repo.display().to_string();
    let mut out = Vec::new();
    for line in conf.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find("#(") {
            let after = &rest[start + 2..];
            let Some(end) = after.find(')') else { break };
            let call = &after[..end];
            rest = &after[end..];
            if !call.contains("tmux-companion") {
                continue;
            }
            let call = call
                .replace("#{pane_current_path}", &repo)
                .replace("#{pane_pid}", &pid)
                .replace("#{session_attached}", "1")
                .replace("#{window_active_clients}", "1");
            // Anything still in #{...} is a format this test has no value for,
            // and inventing one would exercise a command nobody runs.
            if call.contains("#{") {
                continue;
            }
            let args: Vec<String> = call
                .split_whitespace()
                .skip(1)
                .map(str::to_string)
                .collect();
            if !args.is_empty() {
                out.push(args);
            }
        }
    }
    out
}

#[test]
fn every_bar_command_in_the_example_config_actually_runs() {
    // `docs/tmux.conf.full.example` once passed #{pane_pid} to `status-right`,
    // which takes a path and nothing else, so clap rejected the extra argument
    // and everybody who copied that file got a blank right-hand side. Nothing
    // in the repository ran it until a recording did.
    let Some(t) = Tmux::start("bar") else { return };
    let dir = repo_with_changes(&t.sandbox);
    let conf = std::fs::read_to_string(repo_root().join("docs/tmux.conf.full.example")).unwrap();

    let calls = bar_commands(&conf, &dir);
    assert!(calls.len() >= 3, "only found {} #() calls", calls.len());

    for call in calls {
        let args: Vec<&str> = call.iter().map(String::as_str).collect();
        let (out, err, ok) = t.run(&args);
        assert!(
            ok,
            "the bar runs `tmux-companion {}`, which failed:\n{err}",
            call.join(" ")
        );
        if call[0] == "status-right" {
            assert!(
                !out.trim().is_empty(),
                "`status-right` succeeded and drew nothing"
            );
        }
    }
}

#[test]
fn both_example_configs_set_the_background_the_segments_draw_against() {
    // `segments/window.rs` hardcodes BG_BAR as colour233 and lifts the current
    // window to 236 on top of it. Neither example set `status-style`, so
    // tmux's default green showed through everywhere a segment did not reach,
    // which is most of the bar.
    let Some(t) = Tmux::start("style") else {
        return;
    };
    for name in [
        "docs/tmux.conf.starter.example",
        "docs/tmux.conf.example",
        "docs/tmux.conf.full.example",
    ] {
        let conf = repo_root().join(name);
        t.tmux(&[
            "-f",
            &conf.display().to_string(),
            "new-session",
            "-d",
            "-s",
            "style",
        ]);
        let style = t.tmux(&["display-message", "-p", "-t", "style", "#{status-style}"]);
        assert!(
            style.contains("233"),
            "{name} leaves the bar at {style:?}, and the segments draw against colour233"
        );
        t.tmux(&["kill-session", "-t", "style"]);
    }
}

#[test]
fn every_binding_in_the_example_names_a_subcommand_that_exists() {
    // `zoxide-window.zsh` was bound to prefix+c and the port recorded it as
    // done without ever building the command, so the key had nothing to call
    // and the feature was lost rather than ported. A checklist cannot catch
    // that; asking the binary can.
    let Some(t) = Tmux::start("binds") else {
        return;
    };
    for name in [
        "docs/tmux.conf.starter.example",
        "docs/tmux.conf.example",
        "docs/tmux.conf.full.example",
    ] {
        let conf = std::fs::read_to_string(repo_root().join(name)).unwrap();
        let mut checked = 0;
        // Comments mention the binary in prose, and prose is not a call.
        let code: String = conf
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        for word in code.split_whitespace().collect::<Vec<_>>().windows(2) {
            if !word[0].ends_with("tmux-companion") {
                continue;
            }
            let sub = word[1].trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
            if sub.is_empty() || sub.starts_with('-') {
                continue;
            }
            let (_, err, ok) = t.run(&[sub, "--help"]);
            assert!(
                ok,
                "{name} calls `tmux-companion {sub}`, which is not a subcommand:\n{err}"
            );
            checked += 1;
        }
        // The bar alone makes one call; the other two make several.
        let floor = if name == "docs/tmux.conf.example" {
            1
        } else {
            8
        };
        assert!(
            checked >= floor,
            "only {checked} calls found in {name}, the parser is wrong"
        );
    }
}

#[test]
fn the_run_pane_waits_for_an_answer_instead_of_closing_itself() {
    // The command runs under an interactive shell, which hands the terminal to
    // its own process group and does not hand it back, leaving this process in
    // the background where enabling raw mode is an error. Every failure path
    // in the dialog takes the default, which is Close, so the pane shut about
    // a second after it opened. It only bit commands that fork, so a builtin
    // looked fine and `seq` did not.
    let Some(t) = Tmux::start("dialog") else {
        return;
    };
    let dir = repo_with_changes(&t.sandbox);
    t.session("dialog", &dir);

    let exe = t.binary.display().to_string();
    t.tmux(&[
        "split-window",
        "-d",
        "-t",
        "dialog:0",
        "-c",
        &dir.display().to_string(),
        &format!("{exe} run --exec 'seq 1 5'"),
    ]);

    let panes = |t: &Tmux| t.tmux(&["list-panes", "-t", "dialog:0"]).lines().count();
    assert!(t.until(10, |t| panes(t) == 2), "the run pane never opened");
    assert!(
        t.until(5, |t| t
            .tmux(&["list-panes", "-t", "dialog:0", "-F", "#{pane_id}"])
            .lines()
            .any(|p| t.capture(p).contains("[C]lose"))),
        "the dialog never drew"
    );

    // The point of the test: still there after the moment it used to vanish in.
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        panes(&t),
        2,
        "the run pane closed on its own; the dialog answered itself"
    );
}

#[test]
fn a_new_window_opens_where_the_pane_was() {
    // prefix+c with nothing typed is "another window here", which is what
    // tmux's own binding meant before a picker replaced it.
    let Some(t) = Tmux::start("newwin") else {
        return;
    };
    let dir = repo_with_changes(&t.sandbox);
    t.session("newwin", &dir);

    let exe = t.binary.display().to_string();
    t.tmux(&[
        "split-window",
        "-d",
        "-t",
        "newwin:0",
        "-c",
        &dir.display().to_string(),
        &format!("{exe} new-window"),
    ]);
    assert!(
        t.until(10, |t| t
            .tmux(&["list-panes", "-t", "newwin:0", "-F", "#{pane_id}"])
            .lines()
            .any(|p| t.capture(p).contains("New window at"))),
        "the picker never drew"
    );

    let pane = t
        .tmux(&["list-panes", "-t", "newwin:0", "-F", "#{pane_id}"])
        .lines()
        .last()
        .unwrap_or_default()
        .to_string();
    t.tmux(&["send-keys", "-t", &pane, "Enter"]);

    assert!(
        t.until(10, |t| t
            .tmux(&["list-windows", "-t", "newwin"])
            .lines()
            .count()
            == 2),
        "enter on the prefilled query opened no window"
    );
    let dirs = t.tmux(&[
        "list-panes",
        "-s",
        "-t",
        "newwin",
        "-F",
        "#{pane_current_path}",
    ]);
    assert!(
        dirs.lines().any(|d| d.ends_with("acme-api")),
        "the new window did not open in the pane's directory: {dirs}"
    );
}

#[test]
fn project_builds_the_windows_the_layout_asks_for() {
    let Some(t) = Tmux::start("layout") else {
        return;
    };
    std::fs::write(
        t.sandbox.join("config/tmux-companion/config.toml"),
        "[project]\nzoxide = false\n\n[[layout]]\nname = \"default\"\n\n  [[layout.window]]\n  name = \"edit\"\n\n  [[layout.window]]\n  name = \"tests\"\n",
    )
    .unwrap();
    let dir = repo_with_changes(&t.sandbox);
    t.session("layout", &dir);

    let other = t.sandbox.join("payments-api");
    std::fs::create_dir_all(&other).unwrap();
    let exe = t.binary.display().to_string();
    t.tmux(&[
        "send-keys",
        "-t",
        "layout:0",
        &format!("{exe} project {}", other.display()),
        "Enter",
    ]);

    // Wait for both windows, not for the session: a layout builds its windows
    // one `new-window` at a time, so a check that only waits for the session
    // to exist reads the list halfway through building it and fails on a busy
    // machine while passing on a quiet one.
    assert!(
        t.until(15, |t| t
            .tmux(&["has-session", "-t", "=payments-api"])
            .is_empty()
            && t.tmux(&[
                "list-windows",
                "-t",
                "=payments-api",
                "-F",
                "#{window_name}"
            ])
            .lines()
            .count()
                == 2),
        "the session never got both of the layout's windows"
    );
    let windows = t.tmux(&[
        "list-windows",
        "-t",
        "=payments-api",
        "-F",
        "#{window_name}",
    ]);
    let names: Vec<&str> = windows.lines().collect();
    assert_eq!(names, vec!["edit", "tests"], "got {names:?}");
}

/// The `session-created` hook in the example config has to paint a session
/// somebody has never picked a theme for, because that is every session until
/// they do.
///
/// It did not. The fallback named `blue`, `magenta`, `orange` and `grey`, none
/// of which `theme init` writes, so a fresh install got an unpainted bar and
/// "No such file or directory" once per session.
#[test]
fn a_new_session_is_painted_by_the_hook_the_example_config_sets() {
    let Some(t) = Tmux::start("themehook") else {
        return;
    };
    let dir = repo_with_changes(&t.sandbox);

    // What a new user runs, in the order the docs give it.
    let (_, err, ok) = t.run(&["theme", "init"]);
    assert!(ok, "theme init failed: {err}");
    let (_, err, ok) = t.run(&[
        "theme",
        "gen",
        "--apply",
        "--shades",
        "--background",
        "#121212",
    ]);
    assert!(ok, "theme gen failed: {err}");

    t.session("painted", &dir);

    // The hook is a `run-shell`, which tmux runs without waiting for it, so
    // the option appears a moment after the session does.
    t.until(5, |t| {
        !t.tmux(&["show", "-t", "painted", "-v", "@theme-session-name-bg"])
            .is_empty()
    });

    let bg = t.tmux(&["show", "-t", "painted", "-v", "@theme-session-name-bg"]);
    assert!(
        bg.starts_with("colour"),
        "the hook left the session unpainted: @theme-session-name-bg was {bg:?}"
    );

    // Session scope, not server scope: a global value here is the bug where
    // the last session created repaints every other one.
    let global = t.tmux(&["show", "-gv", "@theme-session-name-bg"]);
    assert!(
        global.is_empty() || global != bg,
        "the theme was set globally, so every session shares it: {global:?}"
    );

    // An empty target, which is what `#{session_id}` expands to under tmux
    // 3.5, has to mean the session that was named rather than whichever one
    // happens to be current. Under 3.5 the hook painted the wrong session and
    // left the new one bare.
    t.tmux(&[
        "new-session",
        "-d",
        "-s",
        "second",
        "-c",
        &dir.display().to_string(),
    ]);
    // `-t second` is a target-PANE, so the session has to have one before the
    // theme can be pointed at it.
    assert!(
        t.until(5, |t| !t
            .tmux(&["list-panes", "-t", "second", "-F", "#{pane_id}"])
            .is_empty()),
        "the second session never grew a pane"
    );
    t.tmux(&["set", "-t", "second", "-u", "@theme-session-name-bg"]);
    let (out, err, ok) = t.run(&["theme", "apply", "second", "-t", ""]);
    assert!(ok, "theme apply with an empty target failed: {err}");
    // The example config's session-created hook is a `run-shell`, which tmux
    // does not wait for, so this reads the same option the hook writes and has
    // to allow for the hook still being in flight.
    t.until(5, |t| {
        t.tmux(&["show", "-t", "second", "-v", "@theme-session-name-bg"])
            .starts_with("colour")
    });
    let second = t.tmux(&["show", "-t", "second", "-v", "@theme-session-name-bg"]);
    assert!(
        second.starts_with("colour"),
        "an empty target painted something other than the named session: {second:?}\n{}",
        // This assertion has failed on the ubuntu job and nowhere else, and it
        // has survived two fixes aimed at guesses about why: the pane not
        // being ready, and the suite reading the runner's own home. Neither was
        // it. It does not reproduce here in twenty-two runs, idle or under
        // load, and a tmux 3.4 container does everything this needs, a nested
        // source-file carrying its target included. So the next failure brings
        // its evidence rather than another theory.
        theme_forensics(&t, "second", &out, &err)
    );
}

/// What the theme test needs to say when it fails on a machine nobody can
/// reach: which tmux, which themes are on disk, what the tool printed, and
/// what the sessions actually hold.
fn theme_forensics(t: &Tmux, session: &str, out: &str, err: &str) -> String {
    let dir = t.sandbox.join("config/tmux/themes");
    let mut themes: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    themes.sort();
    format!(
        "  tmux:         {}\n             themes dir:   {} ({} files)\n             ink.tmux:     {}\n             _apply.tmux:  {}\n             first ten:    {:?}\n             apply stdout: {out:?}\n             apply stderr: {err:?}\n             sessions:     {:?}\n             {session} options: {:?}\n             global:       {:?}",
        t.tmux(&["-V"]),
        dir.display(),
        themes.len(),
        dir.join("ink.tmux").is_file(),
        dir.join("_apply.tmux").is_file(),
        themes.iter().take(10).collect::<Vec<_>>(),
        t.tmux(&["list-sessions", "-F", "#{session_name}"]),
        t.tmux(&["show", "-t", session]),
        t.tmux(&["show", "-g", "@theme-session-name-bg"]),
    )
}

/// With no themes on disk at all there is nothing to source, and that is a
/// normal state rather than an error to put on somebody's terminal.
#[test]
fn a_session_created_before_any_theme_exists_says_nothing() {
    let Some(t) = Tmux::start("themenone") else {
        return;
    };
    let dir = repo_with_changes(&t.sandbox);
    t.session("quiet", &dir);

    let messages = t.tmux(&["show-messages"]);
    assert!(
        !messages.contains("No such file"),
        "creating a session complained about a missing theme: {messages}"
    );
}

/// The left side has to be set, not appended to.
///
/// `docs/tmux.conf.full.example` only ever ran `set -ga status-left`, so it
/// appended to tmux's default `[#S] ` under the default status-left-length of
/// 10. The session name came out cut in half as `[playgroun`, and everything
/// appended after it -- the clock, the clients segment -- was past the limit
/// and never drawn at all.
#[test]
fn the_left_side_shows_a_whole_session_name_and_what_follows_it() {
    let Some(t) = Tmux::start("leftside") else {
        return;
    };
    let dir = repo_with_changes(&t.sandbox);
    t.session("leftside", &dir);

    let left = t.tmux(&["show", "-gv", "status-left"]);
    assert!(
        !left.starts_with("[#S]"),
        "the left side is still tmux's default with things appended: {left:?}"
    );
    assert!(
        left.contains("#S"),
        "the left side does not show the session name: {left:?}"
    );

    let length: usize = t
        .tmux(&["show", "-gv", "status-left-length"])
        .parse()
        .unwrap_or(0);
    assert!(
        length >= left.len().min(40),
        "status-left-length is {length}, which truncates what the config draws"
    );

    // Long enough to be cut by the default of 10, so the test fails on the
    // actual symptom rather than on the setting behind it.
    t.tmux(&["rename-session", "-t", "leftside", "a-long-session-name"]);
    let rendered = t.tmux(&[
        "display-message",
        "-t",
        "a-long-session-name",
        "-p",
        "#{T:status-left}",
    ]);
    assert!(
        rendered.contains("a-long-session-name"),
        "the session name is truncated on the bar: {rendered:?}"
    );
}

/// The `client-attached` hook has to fire for a session tmux named itself and
/// stay quiet for one somebody asked for by name.
///
/// This was a tmux format condition in the config first:
///
///   if-shell -F "#{?#{==:#{session_name},#{s|[0-9]||:#{session_name}}},0,1}"
///
/// which `display-message -p` evaluates correctly and which, from inside a
/// hook, ran the command for no session at all. The decision moved into the
/// binary, and this is the test that would have caught the format version.
#[test]
fn the_attach_hook_offers_the_picker_only_for_a_session_tmux_named_itself() {
    let Some(t) = Tmux::start("attachhook") else {
        return;
    };
    let dir = repo_with_changes(&t.sandbox);

    // A marker instead of a popup: a popup needs a client and cannot be
    // captured, and what is being tested is which sessions reach it.
    let marker = t.sandbox.join("fired");
    t.tmux(&[
        "new-session",
        "-d",
        "-s",
        "0",
        "-c",
        &dir.display().to_string(),
    ]);
    t.tmux(&[
        "new-session",
        "-d",
        "-s",
        "named",
        "-c",
        &dir.display().to_string(),
    ]);
    t.tmux(&[
        "new-session",
        "-d",
        "-s",
        "1",
        "-c",
        &dir.display().to_string(),
    ]);
    t.tmux(&["split-window", "-t", "=1:"]);

    // `=name:` and not a bare name. Two of these sessions are called "0" and
    // "1", and a bare number as a target is ambiguous: tmux can read it as a
    // session name or as a window index in whichever session is current. tmux
    // 3.7 reads it as the session and 3.4 does not, so this passed on a laptop
    // and failed on CI, reporting session "1" when it asked for "0". The `=`
    // asks for an exact session name and the colon says the rest is a window.
    for (session, expected) in [("=0:", true), ("=named:", false), ("=1:", false)] {
        let answer = t.tmux(&[
            "display-message",
            "-t",
            session,
            "-p",
            "#{session_name}\t#{session_windows}\t#{window_panes}\t#{pane_current_command}",
        ]);
        assert_eq!(
            tmux_companion::project::is_an_untouched_default_session(&answer),
            expected,
            "session {session:?} answered {answer:?}"
        );
    }
    let _ = std::fs::remove_file(marker);
}

/// `start --last` goes back to the session attached most recently.
#[test]
fn start_last_picks_the_session_used_most_recently() {
    let Some(t) = Tmux::start("startlast") else {
        return;
    };
    let dir = repo_with_changes(&t.sandbox);
    for name in ["first", "second", "third"] {
        t.tmux(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-c",
            &dir.display().to_string(),
        ]);
    }
    let listing = t.tmux(&[
        "list-sessions",
        "-F",
        "#{session_last_attached} #{session_name}",
    ]);
    // Never attached, so every stamp is 0 and the answer is whichever tmux
    // lists first rather than an error.
    assert!(
        tmux_companion::project::most_recent_session(&listing).is_some(),
        "nothing chosen from {listing:?}"
    );
}

/// Every subcommand has a paragraph in the manual.
///
/// A rule that says "keep the man page in sync" is one somebody forgets on the
/// day they are busy. This is the same rule with a build failure attached: add
/// a command, and the page has to name it before the suite goes green.
#[test]
fn the_manual_documents_every_subcommand() {
    let manual = include_str!("../docs/tmux-companion.1");
    let binary = target_binary();

    let help = Command::new(&binary)
        .arg("--help")
        .output()
        .expect("--help runs");
    let help = String::from_utf8_lossy(&help.stdout).into_owned();

    // The subcommand names clap prints, which is the list the manual has to
    // cover. `help` is clap's own and documents itself.
    let commands: Vec<&str> = help
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .filter(|name| *name != "help")
        .collect();

    assert!(
        commands.len() > 20,
        "did not read the subcommand list: {commands:?}"
    );

    let missing: Vec<&str> = commands
        .iter()
        .filter(|name| !manual.contains(&format!("Ic {name}")))
        .copied()
        .collect();
    assert!(
        missing.is_empty(),
        "docs/tmux-companion.1 does not document: {missing:?}"
    );
}

/// The skill names no command that has gone away.
///
/// `skills/tmux-companion/SKILL.md` tells an agent which commands exist, and an
/// agent acts on it without checking. The version of that file this replaced
/// offered `project autosave` and `sessions resurrect --attach NAME`, neither
/// of which was ever built, so this reads every command and flag the file spells
/// out and holds them against `--help`.
///
/// Only spans the file wrote as code are read. Prose is where a command gets
/// mentioned in passing, and a test that failed on prose would be one people
/// work around by writing worse documentation.
#[test]
fn the_skill_names_no_command_that_went_away() {
    let skill = include_str!("../skills/tmux-companion/SKILL.md");
    let binary = target_binary();

    let help_for = |path: &[&str]| -> String {
        let out = Command::new(&binary)
            .args(path)
            .arg("--help")
            .output()
            .expect("--help runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // The names clap prints under `Commands:`, for whichever level it was asked
    // about. A command with no subcommands has no such block and answers empty,
    // which is how the caller below knows not to grade a second word.
    let subcommands = |help: &str| -> Vec<String> {
        help.lines()
            .skip_while(|l| !l.starts_with("Commands:"))
            .skip(1)
            .take_while(|l| !l.trim().is_empty())
            .filter_map(|l| l.split_whitespace().next())
            .filter(|n| *n != "help")
            .map(str::to_string)
            .collect()
    };

    let top = help_for(&[]);
    let commands = subcommands(&top);
    assert!(
        commands.len() > 20,
        "did not read the subcommand list: {commands:?}"
    );

    let mut problems: Vec<String> = Vec::new();
    // A command name, not a flag and not a placeholder: `sessions resurrect`
    // reads as one, `project --print` and `sessions show STAMP` do not.
    let bare_word = |w: &str| {
        w.starts_with(|c: char| c.is_ascii_lowercase())
            && w.chars().all(|c| c.is_ascii_lowercase() || c == '-')
    };

    for span in code_spans(skill) {
        let mut words = span.split_whitespace().peekable();
        if words.peek() == Some(&"tmux-companion") {
            words.next();
            match words.peek() {
                // `tmux-companion <thing>` is a claim that <thing> is a command,
                // so an unknown word here is the failure this test exists for.
                Some(w) if bare_word(w) && !commands.iter().any(|c| c == *w) => {
                    problems.push(format!("no such command: tmux-companion {w}"));
                    continue;
                }
                _ => {}
            }
        }
        let Some(first) = words.next() else { continue };
        if !commands.iter().any(|c| c == first) {
            continue;
        }

        let mut path = vec![first];
        let deeper = subcommands(&help_for(&path));
        if let Some(second) = words.peek()
            && bare_word(second)
            && !deeper.is_empty()
        {
            if deeper.iter().any(|c| c == *second) {
                path.push(words.next().expect("peeked"));
            } else {
                problems.push(format!("no such subcommand: {first} {second}"));
                continue;
            }
        }

        let help = help_for(&path);
        for word in words {
            let flag = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
            if !flag.starts_with("--") || flag.len() < 4 {
                continue;
            }
            if !help.contains(flag) {
                problems.push(format!("no such flag: {} {flag}", path.join(" ")));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "skills/tmux-companion/SKILL.md is stale:\n  {}",
        problems.join("\n  ")
    );
}

/// Every `backticked span` and every line inside a fence, from a Markdown file.
///
/// Fenced lines come through whole because a shell line is one command; inline
/// spans come through whole for the same reason, and both are split by the
/// caller.
fn code_spans(markdown: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut fenced = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            // A comment after a command is prose again.
            spans.push(line.split('#').next().unwrap_or(line).trim().to_string());
            continue;
        }
        let mut rest = line;
        while let Some(open) = rest.find('`') {
            rest = &rest[open + 1..];
            let Some(close) = rest.find('`') else { break };
            spans.push(rest[..close].to_string());
            rest = &rest[close + 1..];
        }
    }
    spans
}

/// The manual's configuration section names every section of the config.
#[test]
fn the_manual_names_every_configuration_section() {
    let manual = include_str!("../docs/tmux-companion.1");
    let example = include_str!("../docs/config.example.toml");

    let sections: Vec<String> = example
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('[') && l.ends_with(']') && !l.starts_with("[["))
        .map(|l| l.trim_start_matches('[').trim_end_matches(']').to_string())
        .filter(|s| !s.contains('.'))
        .collect();

    let missing: Vec<&String> = sections
        .iter()
        .filter(|s| !manual.contains(&format!("[{s}]")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/tmux-companion.1 does not name these config sections: {missing:?}"
    );
}

/// `panes --print` lists a pane with what it runs, and the bar counts it as an
/// agent when `[agents] programs` says so.
///
/// `sleep` stands in for an agent because it is on every machine and stays
/// put, which `claude` does neither of on a CI runner. The state is `busy` when
/// the row is read a moment after the command started, and `waiting` if the
/// runner took longer than `waiting_secs` to get here; both are the feature
/// working, so either is accepted.
#[test]
fn panes_lists_what_runs_where_and_the_bar_counts_the_agents() {
    let Some(t) = Tmux::start("panes") else {
        return;
    };
    std::fs::write(
        t.sandbox.join("config/tmux-companion/config.toml"),
        "[agents]\nprograms = [\"sleep\"]\n\n\
         [[status.right.segments]]\nname = \"git\"\n\n\
         [[status.right.segments]]\nname = \"agents\"\nseparator_before = \" \"\n",
    )
    .unwrap();
    let dir = repo_with_changes(&t.sandbox);
    t.session("alpha", &dir);
    send_when_ready(&t, "=alpha:", "sleep 300");
    assert!(
        t.until(10, |t| t.panes().iter().any(|p| p.contains("sleep"))),
        "the fixture never started: {:?}",
        t.panes()
    );

    let (out, err, ok) = t.run(&["panes", "--print"]);
    assert!(ok, "panes --print failed:\n{err}");
    let row = out
        .lines()
        .find(|l| l.starts_with("alpha:"))
        .unwrap_or_else(|| panic!("no row for alpha in {out:?}"));
    let cols: Vec<&str> = row.split('\t').collect();
    assert_eq!(cols.len(), 5, "at, program, state, cwd, id: {row:?}");
    assert_eq!(cols[1], "sleep", "{row:?}");
    assert!(
        cols[2] == "busy" || cols[2].starts_with("waiting "),
        "an agent is busy or waiting, never idle: {row:?}"
    );
    assert!(cols[4].starts_with('%'), "the id is last: {row:?}");

    // The agent filter keeps it, and a session nobody has is nothing to show
    // rather than an error.
    let (out, _, ok) = t.run(&["panes", "--agents", "--print"]);
    assert!(ok && out.contains("\tsleep\t"), "{out:?}");
    let (out, err, ok) = t.run(&["panes", "-t", "nowhere", "--print"]);
    assert!(ok, "{err}");
    assert!(out.trim().is_empty(), "{out:?}");
    assert!(err.contains("no panes to show"), "{err:?}");

    // And the bar, through the daemon this test's config started.
    let (out, err, ok) = t.run(&["status-right", &dir.display().to_string()]);
    assert!(ok, "status-right failed:\n{err}");
    assert!(
        out.contains("1 agent"),
        "the bar did not count the sleeping agent: {out:?}"
    );
}

#[test]
fn the_inbox_holds_an_agent_that_stopped_with_what_its_screen_said() {
    // A sleeping "agent" with a one-second waiting threshold: the daemon's
    // loop sees it stop, captures the pane, and `inbox` lists it with the
    // last line on screen, which here is the shell's own prompt line.
    let Some(t) = Tmux::start("inbox") else {
        return;
    };
    std::fs::write(
        t.sandbox.join("config/tmux-companion/config.toml"),
        "[agents]\nprograms = [\"sleep\"]\nwaiting_secs = 1\ninterval_secs = 1\n",
    )
    .unwrap();
    let dir = repo_with_changes(&t.sandbox);
    t.session("alpha", &dir);
    send_when_ready(&t, "=alpha:", "echo QUESTION-MARKER; sleep 300");
    assert!(
        t.until(10, |t| t.panes().iter().any(|p| p.contains("sleep"))),
        "the fixture never started: {:?}",
        t.panes()
    );
    // Any client call starts the daemon, and with it the inbox loop.
    let _ = t.run(&["doctor"]);
    assert!(
        t.until(15, |t| {
            let (out, _, _) = t.run(&["inbox", "--print"]);
            out.lines()
                .any(|l| l.starts_with("alpha:") && l.contains("\tsleep\t"))
        }),
        "the inbox never listed the stopped agent"
    );
    let (out, _, _) = t.run(&["inbox", "--print"]);
    let row = out.lines().find(|l| l.starts_with("alpha:")).unwrap();
    let cols: Vec<&str> = row.split('\t').collect();
    assert_eq!(cols.len(), 5, "at, program, waited, question, id: {row:?}");
    assert!(cols[2].ends_with('s') || cols[2].ends_with('m'), "{row:?}");
    assert!(
        cols[3].contains("QUESTION-MARKER"),
        "the question is the last line drawn: {row:?}"
    );
    assert!(cols[4].starts_with('%'), "{row:?}");
}

#[test]
fn the_brief_says_what_is_waiting_and_counts_the_server() {
    let Some(t) = Tmux::start("brief") else {
        return;
    };
    let dir = repo_with_changes(&t.sandbox);
    t.session("alpha", &dir);
    let (out, err, ok) = t.run(&["brief", "--print"]);
    assert!(ok, "brief --print failed:\n{err}");
    assert!(out.starts_with("Nothing is waiting on you."), "{out:?}");
    assert!(out.contains("Health: ok"), "{out:?}");
    assert!(out.contains("1 session, 0 agents (0 waiting)"), "{out:?}");
    // The hook form on a quiet server does nothing and says nothing.
    let (out, err, ok) = t.run(&["brief", "--hook"]);
    assert!(
        ok && out.trim().is_empty() && err.trim().is_empty(),
        "{out:?} {err:?}"
    );
}

#[test]
fn quiet_hours_take_the_agent_count_off_the_bar_and_say_so() {
    let Some(t) = Tmux::start("quiet") else {
        return;
    };
    std::fs::write(
        t.sandbox.join("config/tmux-companion/config.toml"),
        "[agents]\nprograms = [\"sleep\"]\n\n\
         [[status.right.segments]]\nname = \"git\"\n\n\
         [[status.right.segments]]\nname = \"agents\"\nseparator_before = \" \"\n\n\
         [[status.right.segments]]\nname = \"health\"\nseparator_before = \" \"\n",
    )
    .unwrap();
    let dir = repo_with_changes(&t.sandbox);
    t.session("alpha", &dir);
    send_when_ready(&t, "=alpha:", "sleep 300");
    assert!(
        t.until(10, |t| t.panes().iter().any(|p| p.contains("sleep"))),
        "the fixture never started: {:?}",
        t.panes()
    );
    let dir_s = dir.display().to_string();
    let (out, err, ok) = t.run(&["status-right", &dir_s]);
    assert!(ok && out.contains("1 agent"), "before quiet: {out:?} {err}");

    let (out, err, ok) = t.run(&["quiet", "5m"]);
    assert!(ok, "quiet failed:\n{err}");
    assert!(out.starts_with("quiet for"), "{out:?}");
    // The agents cache is two seconds old at most; the health check five.
    assert!(
        t.until(10, |t| {
            let (out, _, _) = t.run(&["status-right", &dir_s]);
            !out.contains("agent") && out.contains("quiet")
        }),
        "the bar kept counting, or never said quiet"
    );
    let (out, _, _) = t.run(&["quiet"]);
    assert!(out.starts_with("quiet for"), "{out:?}");
    let (out, _, _) = t.run(&["quiet", "off"]);
    assert_eq!(out.trim(), "not quiet");
    assert!(
        t.until(10, |t| {
            let (out, _, _) = t.run(&["status-right", &dir_s]);
            out.contains("1 agent")
        }),
        "the count did not come back"
    );
}

#[test]
fn a_note_on_a_pane_is_what_panes_shows_beside_the_program() {
    let Some(t) = Tmux::start("note") else {
        return;
    };
    let dir = repo_with_changes(&t.sandbox);
    t.session("alpha", &dir);
    send_when_ready(&t, "=alpha:", "sleep 300");
    assert!(
        t.until(10, |t| t.panes().iter().any(|p| p.contains("sleep"))),
        "the fixture never started: {:?}",
        t.panes()
    );
    let id = t.tmux(&["display-message", "-p", "-t", "=alpha:", "#{pane_id}"]);
    let id = id.trim();

    let (_, err, ok) = t.run(&["note", "--pane", id, "claude: cache"]);
    assert!(ok, "note failed:\n{err}");
    let (out, _, ok) = t.run(&["note", "--pane", id]);
    assert!(ok);
    assert_eq!(out.trim(), "claude: cache");

    let (out, err, ok) = t.run(&["panes", "--print"]);
    assert!(ok, "panes --print failed:\n{err}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("alpha:") && l.contains("claude: cache")),
        "the note is not beside the program: {out:?}"
    );

    // A snapshot taken while the note is on carries it, so a restore can put
    // it back; the screen capture is skipped since the words are the point.
    let (_, err, ok) = t.run(&["sessions", "save", "--skip-pane-history"]);
    assert!(ok, "sessions save failed:\n{err}");
    let dir = t.sandbox.join("state/tmux-companion/sessions");
    let newest = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .max()
        .expect("a snapshot file");
    let text = std::fs::read_to_string(&newest).unwrap();
    assert!(text.contains("title = \"claude: cache\""), "{text}");

    let (_, _, ok) = t.run(&["note", "--pane", id, "--clear"]);
    assert!(ok);
    let (out, _, _) = t.run(&["panes", "--print"]);
    assert!(!out.contains("claude: cache"), "{out:?}");
}

#[test]
fn the_health_mark_appears_when_the_config_changes_under_a_running_daemon() {
    // The [autosave] timer failed for a day with only the log to show for it.
    // This drives the one case a test can make happen on purpose: the daemon
    // starts, config.toml is written after it, and the bar says `config`.
    let Some(t) = Tmux::start("health") else {
        return;
    };
    let config = t.sandbox.join("config/tmux-companion/config.toml");
    let text = "[[status.right.segments]]\nname = \"git\"\n\n\
                [[status.right.segments]]\nname = \"health\"\nseparator_before = \" \"\n";
    std::fs::write(&config, text).unwrap();
    let dir = repo_with_changes(&t.sandbox);

    // A healthy daemon draws no mark at all.
    let (out, err, ok) = t.run(&["status-right", &dir.display().to_string()]);
    assert!(ok, "status-right failed:\n{err}");
    assert!(!out.contains("config"), "nothing changed yet: {out:?}");

    // mtime resolution is a second on some filesystems, so the edit lands a
    // clear second after the daemon read the file.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&config, format!("{text}\n# edited\n")).unwrap();

    // The check is cached for five seconds; the mark shows on the read after.
    assert!(
        t.until(10, |t| {
            let (out, _, _) = t.run(&["status-right", &dir.display().to_string()]);
            out.contains("config")
        }),
        "the bar never showed the config mark"
    );

    let (out, _, ok) = t.run(&["doctor"]);
    assert!(ok);
    let health = out
        .lines()
        .find(|l| l.trim_start().starts_with("health"))
        .unwrap_or_else(|| panic!("no health line in {out:?}"));
    assert!(health.contains("config.toml changed"), "{health:?}");
}

// ── the sessions store ───────────────────────────────────────────────────────
//
// The unit tests under `src/sessions` assert values, and there are 126 of them.
// None of them runs a restore. Every one of the three bugs found in this
// feature was found by running it: a base-index read off a server that did not
// exist yet, a window opened at its session's directory instead of its own, and
// an import that filed no panes because the file lists them before the windows
// they belong to. All three pass every unit test in the repository.

/// Two sessions with something running in them, ready to be saved.
///
/// Windows are addressed by name throughout. The shipped example config sets
/// no `base-index`, so this server numbers its first window 0 where the laptop
/// this was written on numbers it 1, and every target written as `:1` or `:2`
/// silently hit nothing.
fn a_server_worth_saving(t: &Tmux, dir: &Path) {
    t.session("alpha", dir);
    t.tmux(&["rename-window", "-t", "=alpha:", "edit"]);
    t.tmux(&["new-window", "-d", "-t", "=alpha:", "-n", "watch"]);
    // `tail` is in the shipped restore table, and it stays running, so the
    // pane's command is the same before and after.
    send_when_ready(t, "=alpha:watch", "tail -f /dev/null");
    t.session("beta", dir);
    assert!(
        t.until(10, |t| t.panes().iter().any(|p| p.contains("tail"))),
        "the fixture never started: {:?}",
        t.panes()
    );
}

/// Type a command into a pane once its shell is there to receive it.
///
/// A shell that has not drawn its prompt drops the keys, and these sandboxes
/// have no `shell-init` line, so there is no prompt mark to wait for. Sending
/// again is cheaper than guessing how long a cold zsh takes on a loaded CI box.
fn send_when_ready(t: &Tmux, target: &str, command: &str) {
    let first = command.split_whitespace().next().unwrap_or(command);
    for _ in 0..40 {
        t.tmux(&["send-keys", "-t", target, command, "C-m"]);
        if t.until(1, |t| {
            t.tmux(&[
                "display-message",
                "-p",
                "-t",
                target,
                "#{pane_current_command}",
            ])
            .contains(first)
        }) {
            return;
        }
    }
}

#[test]
fn a_saved_server_comes_back_with_its_windows_and_what_they_were_running() {
    let Some(t) = Tmux::start("sessrestore") else {
        return;
    };
    let dir = t.sandbox.clone();
    a_server_worth_saving(&t, &dir);

    let before = t.panes();
    assert!(before.iter().any(|p| p.contains("tail")), "{before:?}");

    let (out, err, ok) = t.run(&["sessions", "save"]);
    assert!(ok, "save failed: {out} {err}");
    assert!(out.contains("2 sessions"), "{out}");

    // Take the server down the way a reboot would, leaving the snapshot.
    t.tmux(&["kill-server"]);
    assert!(t.sessions().is_empty());

    let (out, err, ok) = t.run_outside(&["sessions", "resurrect"]);
    assert!(ok, "restore failed: {out} {err}");
    assert!(
        t.until(10, |t| t.sessions() == vec!["alpha", "beta"]),
        "sessions after restore: {:?} ({out})",
        t.sessions()
    );
    assert!(
        t.until(10, |t| t.panes().iter().any(|p| p.contains("tail"))),
        "the command never came back: {:?}",
        t.panes()
    );
    assert_eq!(
        t.tmux(&["list-windows", "-t", "=alpha", "-F", "#{window_name}"]),
        "edit\nwatch"
    );
}

#[test]
fn a_restore_refuses_a_server_that_is_already_busy_and_changes_nothing() {
    let Some(t) = Tmux::start("sessrefuse") else {
        return;
    };
    let dir = t.sandbox.clone();
    a_server_worth_saving(&t, &dir);
    let (_, _, ok) = t.run(&["sessions", "save"]);
    assert!(ok);

    let before = t.panes();
    let (out, err, ok) = t.run_outside(&["sessions", "resurrect"]);
    assert!(!ok, "a busy server should refuse: {out}");
    assert!(err.contains("already running"), "{err}");
    assert_eq!(t.panes(), before, "a refused restore changed the server");
}

#[test]
fn a_merge_brings_back_only_what_is_missing() {
    let Some(t) = Tmux::start("sessmerge") else {
        return;
    };
    let dir = t.sandbox.clone();
    a_server_worth_saving(&t, &dir);
    let (_, _, ok) = t.run(&["sessions", "save"]);
    assert!(ok);

    let beta_created = t.tmux(&["display-message", "-p", "-t", "=beta", "#{session_created}"]);
    t.tmux(&["kill-session", "-t", "=alpha"]);
    assert_eq!(t.sessions(), vec!["beta"]);

    let (out, err, ok) = t.run_outside(&["sessions", "resurrect", "--merge"]);
    assert!(ok, "merge failed: {out} {err}");
    assert!(
        t.until(10, |t| t.sessions() == vec!["alpha", "beta"]),
        "{:?}",
        t.sessions()
    );
    // The session that was already there is the same one, not a rebuilt copy.
    assert_eq!(
        t.tmux(&["display-message", "-p", "-t", "=beta", "#{session_created}"]),
        beta_created
    );
}

#[test]
fn a_dry_run_prints_the_commands_and_touches_nothing() {
    let Some(t) = Tmux::start("sessdry") else {
        return;
    };
    let dir = t.sandbox.clone();
    a_server_worth_saving(&t, &dir);
    let (_, _, ok) = t.run(&["sessions", "save"]);
    assert!(ok);

    // Something to restore, so the listing has content, and something live, so
    // there is state a dry run could damage.
    t.tmux(&["kill-session", "-t", "=alpha"]);
    let before = t.panes();
    let (out, err, ok) = t.run_outside(&["sessions", "resurrect", "--merge", "--dry-run"]);
    assert!(ok, "{out} {err}");
    assert!(out.contains("new-session -d -s alpha"), "{out}");
    assert!(out.contains("tail -f /dev/null"), "{out}");
    assert_eq!(t.sessions(), vec!["beta"], "a dry run built something");
    assert_eq!(t.panes(), before);
}

#[test]
fn shutdown_saves_before_it_stops_the_server() {
    let Some(t) = Tmux::start("sessdown") else {
        return;
    };
    let dir = t.sandbox.clone();
    a_server_worth_saving(&t, &dir);

    let (out, err, ok) = t.run_outside(&["sessions", "shutdown"]);
    assert!(ok, "shutdown failed: {out} {err}");
    assert!(
        t.until(10, |t| t.sessions().is_empty()),
        "the server is still up"
    );

    // The save happened before the stop, so the snapshot holds what was there.
    let (list, _, ok) = t.run(&["sessions", "list"]);
    assert!(ok);
    assert!(list.contains("2 sessions"), "{list}");
}

#[test]
fn shutdown_refuses_from_inside_tmux_and_leaves_the_server_alone() {
    let Some(t) = Tmux::start("sessinside") else {
        return;
    };
    let dir = t.sandbox.clone();
    a_server_worth_saving(&t, &dir);

    // `run` leaves $TMUX set, which is what a pane inside the server looks like.
    let (out, err, ok) = t.run(&["sessions", "shutdown"]);
    assert!(!ok, "should refuse from inside tmux: {out}");
    assert!(err.contains("outside tmux"), "{err}");
    assert_eq!(t.sessions(), vec!["alpha", "beta"]);
}

#[test]
fn restart_puts_the_server_back_the_way_it_was() {
    let Some(t) = Tmux::start("sessrestart") else {
        return;
    };
    let dir = t.sandbox.clone();
    a_server_worth_saving(&t, &dir);

    let (out, err, ok) = t.run_outside(&["sessions", "restart"]);
    assert!(ok, "restart failed: {out} {err}");
    assert!(
        t.until(15, |t| t.sessions() == vec!["alpha", "beta"]),
        "sessions after restart: {:?} ({out} {err})",
        t.sessions()
    );
    assert!(
        t.until(10, |t| t.panes().iter().any(|p| p.contains("tail"))),
        "{:?}",
        t.panes()
    );
}

#[test]
fn an_excluded_session_is_named_before_it_is_dropped() {
    let Some(t) = Tmux::start("sessexcl") else {
        return;
    };
    let dir = t.sandbox.clone();
    a_server_worth_saving(&t, &dir);

    let (out, _, ok) = t.run(&["sessions", "shutdown", "--dry-run", "--exclude", "beta"]);
    assert!(ok, "{out}");
    assert!(out.contains("not saving: beta"), "{out}");
    assert!(out.contains("will not come back"), "{out}");
}

#[test]
fn a_command_nothing_claims_opens_its_pane_and_is_not_run() {
    let Some(t) = Tmux::start("sessdeny") else {
        return;
    };
    let dir = t.sandbox.clone();
    t.session("gamma", &dir);
    // Nothing in the shipped restore table claims this, so the restore must
    // open the pane and leave it at a prompt rather than running it again.
    let flag = t.sandbox.join("it-ran");
    send_when_ready(
        &t,
        "=gamma:",
        &format!("touch {} && sleep 300", flag.display()),
    );
    assert!(
        t.until(10, |_| flag.exists()),
        "the command never ran at all"
    );

    let (_, _, ok) = t.run(&["sessions", "save"]);
    assert!(ok);
    t.tmux(&["kill-server"]);
    let _ = std::fs::remove_file(&flag);

    let (out, err, ok) = t.run_outside(&["sessions", "resurrect", "--yes"]);
    assert!(ok, "{out} {err}");
    assert!(t.until(10, |t| !t.sessions().is_empty()));
    // Give it longer than the prompt wait, so a command that was going to run
    // has had every chance to.
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        !flag.exists(),
        "a command no restore.program row claims was run anyway"
    );
}

#[test]
fn the_daemon_leaves_a_marker_that_says_it_did_not_stop_cleanly() {
    let Some(t) = Tmux::start("sessmark") else {
        return;
    };
    let marker = t.sandbox.join("state/tmux-companion/sessions/running");

    // Any client starts a daemon, and the daemon writes the marker.
    let (_, _, ok) = t.run(&["noop"]);
    assert!(ok);
    assert!(t.until(10, |_| marker.exists()), "no marker after a start");

    let (out, _, ok) = t.run(&["shutdown"]);
    assert!(ok, "{out}");
    assert!(
        t.until(10, |_| !marker.exists()),
        "a clean stop left the marker behind"
    );
}

#[test]
fn an_imported_resurrect_save_is_read_when_there_is_nothing_of_our_own() {
    let Some(t) = Tmux::start("sessimport") else {
        return;
    };
    let old = t.sandbox.join(".local/share/tmux/resurrect");
    std::fs::create_dir_all(&old).unwrap();
    let file = old.join("tmux_resurrect_20260925T102324.txt");
    std::fs::write(
        &file,
        "pane\tdelta\t1\t1\t:*\t1\ttitle\t:/tmp\t1\tnvim\t:nvim\n\
         window\tdelta\t1\t:edit\t1\t:*\tb644,80x24,0,0,1\toff\n\
         state\tdelta\tdelta\n",
    )
    .unwrap();
    let _ = std::os::unix::fs::symlink("tmux_resurrect_20260925T102324.txt", old.join("last"));

    let (out, err, ok) = t.run_outside(&["sessions", "resurrect", "--dry-run"]);
    assert!(ok, "{out} {err}");
    assert!(out.contains("no snapshot of our own"), "{out}");
    assert!(out.contains("new-session -d -s delta"), "{out}");
    assert!(out.contains("send-keys -t =delta:1.1 nvim"), "{out}");
}
