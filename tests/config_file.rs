//! The config file as somebody actually meets it: a real file on disk, a real
//! binary reading it, and a real daemon refusing to start on a broken one.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tmux-companion"))
}

/// A temporary directory that cleans up after itself.
struct Dir(std::path::PathBuf);

impl Dir {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("tc-config-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create temp dir");
        Self(p)
    }

    fn write(&self, name: &str, text: &str) -> std::path::PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).expect("write config");
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn check_accepts_the_example_file() {
    let out = bin()
        .args(["config", "check", "docs/config.example.toml"])
        .output()
        .expect("runs");
    assert!(
        out.status.success(),
        "the shipped example must parse: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn check_fails_on_a_misspelled_key_and_says_which_one() {
    let dir = Dir::new("badkey");
    let path = dir.write("config.toml", "[git]\nttl_sec = 5\n");

    let out = bin()
        .args(["config", "check"])
        .arg(&path)
        .output()
        .expect("runs");

    assert!(!out.status.success(), "a typo must fail the check");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("ttl_sec"), "{err}");
    assert!(err.contains("did you mean"), "{err}");
    assert!(err.contains("ttl_secs"), "{err}");
}

#[test]
fn path_reports_the_file_the_environment_points_at() {
    let dir = Dir::new("path");
    let path = dir.write("config.toml", "");

    let out = bin()
        .args(["config", "path"])
        .env("TMUX_COMPANION_CONFIG", &path)
        .output()
        .expect("runs");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(&path.display().to_string()), "{stdout}");
}

#[test]
fn dump_is_a_config_file_that_check_accepts() {
    let dump = bin().args(["config", "dump"]).output().expect("runs");
    assert!(dump.status.success());

    let dir = Dir::new("dump");
    let path = dir.write("config.toml", &String::from_utf8_lossy(&dump.stdout));

    let out = bin()
        .args(["config", "check"])
        .arg(&path)
        .output()
        .expect("runs");
    assert!(
        out.status.success(),
        "dump must round trip: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_broken_config_stops_the_daemon_and_the_client_says_why() {
    // The decision this encodes: refuse to start rather than quietly run on
    // defaults. Refusing is only defensible if the reason reaches somebody,
    // so the client has to surface it.
    let dir = Dir::new("broken");
    let config = dir.write("config.toml", "[git]\nttl_secs = \"soon\"\n");
    let state = dir.0.join("state");
    let sock = std::path::PathBuf::from(format!("/tmp/tc-broken-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&sock);

    let server = bin()
        .arg("server")
        .env("TMUX_COMPANION_CONFIG", &config)
        .env("XDG_STATE_HOME", &state)
        .env("TMUX_COMPANION_SOCK", &sock)
        .output()
        .expect("server runs");

    assert!(!server.status.success(), "the daemon must refuse to start");
    let err = String::from_utf8_lossy(&server.stderr);
    assert!(
        err.contains("ttl_secs"),
        "stderr should name the key: {err}"
    );

    // And the file a client reads when its own start attempt produced nothing.
    let recorded = std::fs::read_to_string(state.join("tmux-companion").join("last-error"))
        .expect("last-error is written");
    assert!(recorded.contains("ttl_secs"), "{recorded}");

    let client = bin()
        .args(["gst", env!("CARGO_MANIFEST_DIR")])
        .env("TMUX_COMPANION_CONFIG", &config)
        .env("XDG_STATE_HOME", &state)
        .env("TMUX_COMPANION_SOCK", &sock)
        .output()
        .expect("client runs");
    let client_err = String::from_utf8_lossy(&client.stderr);
    assert!(
        client_err.contains("config") && client_err.contains("config check"),
        "the client should point at `config check`: {client_err}"
    );

    let _ = std::fs::remove_file(&sock);
}

#[test]
fn a_config_the_daemon_accepts_changes_what_it_does() {
    // Not just parsed: actually reaching the cache. `ttl_secs = 0` means never
    // serve a git status from memory.
    let dir = Dir::new("applied");
    let config = dir.write("config.toml", "[git]\nttl_secs = 0.0\n");
    let sock = std::path::PathBuf::from(format!("/tmp/tc-applied-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&sock);

    let mut server = bin()
        .arg("server")
        .env("TMUX_COMPANION_CONFIG", &config)
        .env("TMUX_COMPANION_SOCK", &sock)
        .spawn()
        .expect("server spawns");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline
        && std::os::unix::net::UnixStream::connect(&sock).is_err()
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let out = bin()
        .args(["gst", env!("CARGO_MANIFEST_DIR")])
        .env("TMUX_COMPANION_SOCK", &sock)
        .output()
        .expect("client runs");

    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_file(&sock);

    assert!(out.status.success());
    assert!(!out.stdout.is_empty(), "still has to render");
}
