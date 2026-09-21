//! The client and the server, started together, talking over a real unix
//! socket. Before `src/lib.rs` existed this could not be written at all, and
//! the one path every subcommand depends on was covered by running the binary
//! by hand and looking at the bar.
//!
//! Every test here points `TMUX_COMPANION_SOCK` at its own socket under
//! `/tmp`, so none of this can reach the daemon serving somebody's status bar.
//! `/tmp` rather than the system temp directory because a `sockaddr_un` holds
//! 103 bytes on macOS and `$TMPDIR` there is already about half of that.

use std::{
    process::{Child, Command},
    time::{Duration, Instant},
};

use tmux_companion::proto::{ClientsArgs, GstArgs, Request, Response};

/// A server process on a socket of its own, killed when the test ends.
struct TestServer {
    child: Child,
    sock: std::path::PathBuf,
}

impl TestServer {
    fn start(tag: &str) -> Self {
        let sock = std::path::PathBuf::from(format!(
            "/tmp/tc-test-{}-{}-{}.sock",
            tag,
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        let _ = std::fs::remove_file(&sock);
        let child = Command::new(env!("CARGO_BIN_EXE_tmux-companion"))
            .arg("server")
            .env("TMUX_COMPANION_SOCK", &sock)
            .spawn()
            .expect("server spawns");
        let server = Self { child, sock };
        server.wait_until_listening();
        server
    }

    fn wait_until_listening(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if std::os::unix::net::UnixStream::connect(&self.sock).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("server never started listening on {}", self.sock.display());
    }

    /// Run one request against this server's socket.
    ///
    /// Deliberately not `client::send`: that reads `TMUX_COMPANION_SOCK`, and
    /// the environment belongs to the whole test binary, so two tests running
    /// in parallel would overwrite each other's socket and one of them would
    /// talk to a server that had already been dropped. Client-side behaviour
    /// is covered by running the binary itself, further down, where the
    /// environment is that process's own.
    fn send(&self, req: Request) -> Response {
        send_to(&self.sock, req)
    }
}

/// One request and one response over a socket, with no environment involved.
fn send_to(sock: &std::path::Path, req: Request) -> Response {
    use std::io::{BufRead, BufReader, Write};

    let mut stream = std::os::unix::net::UnixStream::connect(sock).expect("connect");
    let mut msg = serde_json::to_string(&req).expect("serialise");
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("write");

    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .expect("read a response line");
    assert!(!line.is_empty(), "server closed without answering");
    serde_json::from_str(&line).expect("parse response")
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.sock);
    }
}

#[test]
fn a_noop_round_trips_over_the_socket() {
    let server = TestServer::start("noop");
    let resp = server.send(Request::build("noop", &()));
    assert_eq!(resp.error, None);
    assert_eq!(resp.output, "");
}

#[test]
fn gst_renders_this_repository_over_the_socket() {
    let server = TestServer::start("gst");
    let args = GstArgs {
        path: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
        pane_pid: None,
        force: false,
        style: Default::default(),
        no_cap: false,
        branch_max_len: None,
        branch_icon: false,
        ttl_secs: 5.0,
    };
    let resp = server.send(Request::build("gst", &args));
    assert_eq!(resp.error, None, "gst errored: {:?}", resp.error);
    assert!(
        !resp.output.is_empty(),
        "a git repository should render something"
    );
    // The crate's own checkout is a work tree, so the branch glyph block is
    // there whatever branch the test happens to run on.
    assert!(
        resp.output.contains("#["),
        "expected tmux markup: {:?}",
        resp.output
    );
}

#[test]
fn an_unknown_command_comes_back_as_an_error_not_an_empty_render() {
    let server = TestServer::start("unknown");
    let resp = server.send(Request {
        cmd: "keys".into(),
        args: serde_json::Value::Null,
    });
    assert_eq!(resp.output, "");
    let err = resp.error.expect("unknown command must error");
    assert!(err.contains("keys"), "{err}");
}

#[test]
fn a_misspelled_argument_crosses_the_socket_as_a_named_error() {
    // The typed args structs are only worth anything if the error survives the
    // wire with the field name still in it.
    let server = TestServer::start("badarg");
    let resp = server.send(Request {
        cmd: "clients".into(),
        args: serde_json::json!({"session_atached": 2}),
    });
    let err = resp.error.expect("unknown key must error");
    assert!(err.contains("invalid clients args"), "{err}");
    assert!(err.contains("session_atached"), "{err}");
}

#[test]
fn typed_args_survive_the_round_trip() {
    let server = TestServer::start("clients");
    let resp = server.send(Request::build(
        "clients",
        &ClientsArgs {
            session_attached: 2,
            window_active_clients: 1,
        },
    ));
    assert_eq!(resp.error, None);
}

#[test]
fn a_second_server_exits_quietly_and_leaves_the_first_serving() {
    // The singleton guarantee. Every client invocation tries to start a server
    // when it cannot connect, so two starting at once is the normal case, not
    // the exceptional one.
    let server = TestServer::start("singleton");

    let second = Command::new(env!("CARGO_BIN_EXE_tmux-companion"))
        .arg("server")
        .env("TMUX_COMPANION_SOCK", &server.sock)
        .output()
        .expect("second server runs");
    assert!(
        second.status.success(),
        "second server should exit 0, got {:?}: {}",
        second.status.code(),
        String::from_utf8_lossy(&second.stderr)
    );

    let resp = server.send(Request::build("noop", &()));
    assert_eq!(resp.error, None, "the first server must still be serving");
}

#[test]
fn a_stale_socket_file_does_not_stop_a_server_starting() {
    // What is left behind when a server is killed with SIGKILL: a socket file
    // with nothing listening on it.
    let sock = std::path::PathBuf::from(format!("/tmp/tc-test-stale-{}.sock", std::process::id()));
    std::fs::write(&sock, b"not a socket").expect("write stale file");

    let mut child = Command::new(env!("CARGO_BIN_EXE_tmux-companion"))
        .arg("server")
        .env("TMUX_COMPANION_SOCK", &sock)
        .spawn()
        .expect("server spawns");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut connected = false;
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
            connected = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&sock);
    assert!(
        connected,
        "a stale socket file should be replaced, not fatal"
    );
}

#[test]
fn the_binary_client_prints_what_the_server_returned() {
    // The other tests speak to the socket directly. This one covers the client
    // half: `connect_with_retry`, the response parse and the print.
    let server = TestServer::start("clientbin");
    let out = Command::new(env!("CARGO_BIN_EXE_tmux-companion"))
        .args(["gst", env!("CARGO_MANIFEST_DIR")])
        .env("TMUX_COMPANION_SOCK", &server.sock)
        .output()
        .expect("client runs");

    assert!(
        out.status.success(),
        "client exited {:?}",
        out.status.code()
    );
    assert!(
        !out.stdout.is_empty(),
        "client printed nothing; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_client_with_no_server_starts_one() {
    // What every tmux refresh does after a reboot, and the reason `gst .` works
    // without anybody running `server` first.
    let sock = std::path::PathBuf::from(format!(
        "/tmp/tc-test-autostart-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&sock);

    let out = Command::new(env!("CARGO_BIN_EXE_tmux-companion"))
        .args(["gst", env!("CARGO_MANIFEST_DIR")])
        .env("TMUX_COMPANION_SOCK", &sock)
        .output()
        .expect("client runs");

    // Kill the server this test caused to exist, by the exact path of the test
    // binary under target/, which cannot match an installed daemon.
    let _ = Command::new("pkill")
        .args([
            "-f",
            &format!("{} server", env!("CARGO_BIN_EXE_tmux-companion")),
        ])
        .status();
    let _ = std::fs::remove_file(&sock);

    assert!(
        out.status.success(),
        "client exited {:?}",
        out.status.code()
    );
    assert!(
        !out.stdout.is_empty(),
        "auto-started server rendered nothing; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
