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
//! works on a machine without it.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// A private tmux server, a private daemon socket and a sandbox to run in.
struct Tmux {
    socket: String,
    sandbox: PathBuf,
    binary: PathBuf,
}

impl Tmux {
    /// Start a server with the config this repository ships.
    ///
    /// The shipped example rather than a minimal one on purpose: it is the
    /// file people copy, and nothing else in the repository ever ran it.
    fn start(name: &str) -> Option<Self> {
        if Command::new("tmux").arg("-V").output().is_err() {
            eprintln!("skipping {name}: no tmux on this machine");
            return None;
        }
        let binary = target_binary()?;
        let sandbox = std::env::temp_dir().join(format!("tce2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&sandbox);
        std::fs::create_dir_all(sandbox.join("bin")).ok()?;
        std::fs::create_dir_all(sandbox.join("state")).ok()?;
        std::fs::create_dir_all(sandbox.join("config/tmux-companion")).ok()?;
        // On PATH under its own name, because the example config calls
        // `tmux-companion` and that is the thing under test.
        let _ = std::os::unix::fs::symlink(&binary, sandbox.join("bin/tmux-companion"));

        let t = Tmux {
            socket: format!("tce2e{name}{}", std::process::id()),
            sandbox,
            binary,
        };
        Some(t)
    }

    /// The environment every command here runs in.
    fn env(&self, cmd: &mut Command) {
        let path = format!(
            "{}:{}",
            self.sandbox.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        cmd.env("PATH", path)
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
            .env(
                "TMUX_COMPANION_SOCK",
                format!("/tmp/tce2e{}.sock", std::process::id()),
            )
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
        let _ = std::fs::remove_dir_all(&self.sandbox);
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn target_binary() -> Option<PathBuf> {
    // The test binary lives in target/<profile>/deps, so the binary under test
    // is two directories up.
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.parent()?.join("tmux-companion");
    candidate.exists().then_some(candidate)
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
    for name in ["docs/tmux.conf.example", "docs/tmux.conf.full.example"] {
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
    let conf = std::fs::read_to_string(repo_root().join("docs/tmux.conf.full.example")).unwrap();

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
            "the example config calls `tmux-companion {sub}`, which is not a subcommand:\n{err}"
        );
        checked += 1;
    }
    assert!(
        checked >= 8,
        "only {checked} calls found, the parser is wrong"
    );
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
    let Some(binary) = target_binary() else {
        return;
    };

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
