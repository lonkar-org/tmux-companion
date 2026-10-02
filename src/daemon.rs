//! Asking a running tmux-companion daemon for a segment, from another program.
//!
//! The daemon serves one JSON line in and one JSON line out over a Unix
//! socket, which is the whole protocol: [`Request`] and [`Response`] in
//! [`crate::proto`] are the two lines. This module is the client for anything
//! that is not the `tmux-companion` binary: a status bar of your own, an
//! editor plugin, a script in Rust.
//!
//! It differs from what the binary does in three ways, on purpose:
//!
//! - **It never starts a daemon.** The binary spawns `<itself> server` when
//!   nothing answers, and from inside another program "itself" is that
//!   program. A missing daemon is [`Error::NotRunning`], and starting one is
//!   `tmux-companion server`'s job.
//! - **It never replaces one.** The binary restarts a daemon from an older
//!   build; a library linked into something else has no business stopping the
//!   user's daemon. The build that answered is in [`Response::version`] and
//!   [`Daemon::build`], to compare if it matters.
//! - **It is blocking**, over `std::os::unix::net`, so it needs no async
//!   runtime. One request is one connect, one write and one read, a few
//!   hundred microseconds against a warm daemon; call it from a thread or
//!   `spawn_blocking` if that is too long to hold.
//!
//! ```no_run
//! use tmux_companion::daemon::Daemon;
//! use tmux_companion::proto::GstArgs;
//!
//! let daemon = Daemon::new()?;
//! let segment = daemon.git_status(&GstArgs {
//!     path: Some("/path/to/repo".into()),
//!     ..GstArgs::default()
//! })?;
//! // tmux markup: `#[fg=...,bg=...]` runs and Nerd Font glyphs.
//! print!("{segment}");
//! # Ok::<(), tmux_companion::daemon::Error>(())
//! ```
//!
//! The argument structs reject fields they do not know, so a request from a
//! newer library to an older daemon fails with the field's name in
//! [`Error::Daemon`] rather than being answered wrongly.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::proto::{GstArgs, Request, Response, StatusRightArgs};

/// What can go wrong asking the daemon.
#[derive(Debug)]
pub enum Error {
    /// Nothing is listening on the socket: no daemon, or one that exited.
    NotRunning {
        /// The socket that was tried.
        socket: PathBuf,
        /// What connecting said.
        source: std::io::Error,
    },
    /// `TMUX_COMPANION_SOCK` is set to something that cannot be a socket.
    Socket(String),
    /// Connected, and then reading or writing failed.
    Io(std::io::Error),
    /// The daemon closed the connection without answering.
    NoAnswer,
    /// A line that is not the protocol: the socket belongs to something else,
    /// or the request could not be encoded.
    Protocol(serde_json::Error),
    /// The daemon answered with an error: an unknown command, arguments it
    /// would not take, or a segment that failed.
    Daemon(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotRunning { socket, source } => write!(
                f,
                "no tmux-companion daemon at {} ({source}); `tmux-companion server` starts one",
                socket.display()
            ),
            Error::Socket(why) => f.write_str(why),
            Error::Io(e) => write!(f, "talking to the daemon: {e}"),
            Error::NoAnswer => f.write_str("the daemon closed the connection without answering"),
            Error::Protocol(e) => write!(f, "not a tmux-companion answer: {e}"),
            Error::Daemon(e) => write!(f, "the daemon said: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::NotRunning { source, .. } => Some(source),
            Error::Io(e) => Some(e),
            Error::Protocol(e) => Some(e),
            _ => None,
        }
    }
}

/// A daemon's socket, and how long to wait on it.
#[derive(Debug, Clone)]
pub struct Daemon {
    socket: PathBuf,
    timeout: Option<Duration>,
}

/// How long a request may take before it is given up on, unless
/// [`Daemon::timeout`] says otherwise. A cold `git status` in a large
/// repository is the slowest thing the daemon does, and it is well under this.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

impl Daemon {
    /// The daemon the `tmux-companion` binary would talk to: the socket
    /// `TMUX_COMPANION_SOCK` names, or `/tmp/tmux-companion-<uid>.sock`.
    pub fn new() -> Result<Self, Error> {
        let socket = crate::client::resolve_sock_path(
            std::env::var_os("TMUX_COMPANION_SOCK").as_deref(),
            nix::unistd::getuid().as_raw(),
        )
        .map_err(Error::Socket)?;
        Ok(Self::at(socket))
    }

    /// The daemon listening on this socket.
    pub fn at(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
            timeout: Some(DEFAULT_TIMEOUT),
        }
    }

    /// Wait this long for an answer, or forever with `None`.
    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// The socket this talks to.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Send one request and return the response as it came, an error field
    /// included. The lowest level there is; the methods below are this plus
    /// turning [`Response::error`] into [`Error::Daemon`].
    pub fn send(&self, req: &Request) -> Result<Response, Error> {
        let stream = UnixStream::connect(&self.socket).map_err(|source| Error::NotRunning {
            socket: self.socket.clone(),
            source,
        })?;
        stream.set_read_timeout(self.timeout).map_err(Error::Io)?;
        stream.set_write_timeout(self.timeout).map_err(Error::Io)?;

        let mut line = serde_json::to_string(req).map_err(Error::Protocol)?;
        line.push('\n');
        (&stream).write_all(line.as_bytes()).map_err(Error::Io)?;

        let mut answer = String::new();
        let read = BufReader::new(&stream)
            .read_line(&mut answer)
            .map_err(Error::Io)?;
        if read == 0 {
            return Err(Error::NoAnswer);
        }
        serde_json::from_str(&answer).map_err(Error::Protocol)
    }

    /// Run a command by name with its arguments, and return what it printed.
    ///
    /// `args` is the command's args struct from [`crate::proto`], or anything
    /// that serialises to the same JSON object.
    pub fn request<T: Serialize>(&self, cmd: &str, args: &T) -> Result<String, Error> {
        let value = serde_json::to_value(args).map_err(Error::Protocol)?;
        self.answer(&Request::raw(cmd, value))
    }

    /// The git status segment for a repository, as tmux markup.
    pub fn git_status(&self, args: &GstArgs) -> Result<String, Error> {
        self.request("gst", args)
    }

    /// The whole right-hand side of the status bar in one call: git, network
    /// rate and battery, as tmux markup.
    pub fn status_right(&self, args: &StatusRightArgs) -> Result<String, Error> {
        self.request("status-right", args)
    }

    /// Download and upload rate since the last time anything asked.
    pub fn network(&self) -> Result<String, Error> {
        self.answer(&Request::raw("net", serde_json::Value::Null))
    }

    /// Battery charge and icon, empty on a machine without one.
    pub fn battery(&self) -> Result<String, Error> {
        self.answer(&Request::raw("battery", serde_json::Value::Null))
    }

    /// The build id of the daemon that answers, `0.7.0+1790917509.bb588af`,
    /// for comparing against [`crate::proto::build_id`].
    pub fn build(&self) -> Result<String, Error> {
        // Every answer carries the build. This is the cheapest question: one
        // getrusage and no state.
        let resp = self.send(&Request::raw("__rusage", serde_json::Value::Null))?;
        Ok(resp.version)
    }

    fn answer(&self, req: &Request) -> Result<String, Error> {
        let resp = self.send(req)?;
        match resp.error {
            Some(e) => Err(Error::Daemon(e)),
            None => Ok(resp.output),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A one-shot daemon: answers the first line with `reply` and hands back
    /// what it was sent.
    fn fake(reply: &'static str) -> (Daemon, std::thread::JoinHandle<String>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            (&stream).write_all(reply.as_bytes()).unwrap();
            line
        });
        (Daemon::at(path), handle, dir)
    }

    #[test]
    fn a_request_is_one_line_and_the_output_comes_back() {
        let (d, sent, _dir) = fake("{\"output\":\"main +1\",\"error\":null,\"version\":\"x\"}\n");
        let out = d
            .git_status(&GstArgs {
                path: Some("/r".into()),
                ..GstArgs::default()
            })
            .unwrap();
        assert_eq!(out, "main +1");
        let sent: serde_json::Value = serde_json::from_str(&sent.join().unwrap()).unwrap();
        assert_eq!(sent["cmd"], "gst");
        assert_eq!(sent["args"]["path"], "/r");
    }

    #[test]
    fn the_daemon_s_error_is_an_error_not_an_empty_segment() {
        let (d, _sent, _dir) =
            fake("{\"output\":\"\",\"error\":\"unknown field `x`\",\"version\":\"x\"}\n");
        match d.battery() {
            Err(Error::Daemon(e)) => assert!(e.contains("unknown field")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nothing_listening_is_not_running_and_nothing_is_started() {
        let dir = tempfile::tempdir().unwrap();
        let d = Daemon::at(dir.path().join("none.sock"));
        assert!(matches!(d.network(), Err(Error::NotRunning { .. })));
        assert!(!dir.path().join("none.sock").exists());
    }

    #[test]
    fn a_closed_connection_is_no_answer() {
        let (d, _sent, _dir) = fake("");
        assert!(matches!(d.battery(), Err(Error::NoAnswer)));
    }

    #[test]
    fn the_args_defaults_are_the_wire_defaults() {
        let a = GstArgs::default();
        assert_eq!(a.ttl_secs, 5.0);
        assert!(a.path.is_none() && !a.force);
        assert_eq!(StatusRightArgs::default().ttl_secs, 5.0);
    }
}
