//! One link, from the local side: run a host's command with the bridge as its
//! remote command, say hello, ask it things, and when anything goes wrong say
//! why in the words a person can act on.
//!
//! Everything here runs on a `LocalSet`: Cap'n Proto's RPC system is not
//! `Send`. `manager` gives each host a thread of its own for that.

use std::{
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use capnp_rpc::{RpcSystem, rpc_twoparty_capnp, twoparty};
use tokio::io::{AsyncBufReadExt, AsyncRead, ReadBuf};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::{
    BRIDGE_LINE, Hello, PROTOCOL, Segment, Session, Window, clean, read_dirs, read_entries,
    read_segments, read_sessions, read_windows, reader_options,
};
use crate::{
    config::Host,
    link_capnp::{bridge, session_sink},
};

/// The last lines a host command wrote to stderr, newest last.
type Tail = Arc<Mutex<Vec<String>>>;

/// How many stderr lines are kept for the reason.
const TAIL_LINES: usize = 8;

/// A reader that notes whether anything at all came through: a host that
/// sent bytes and no hello sent something that isn't the protocol, and one
/// that sent nothing is slow or gone.
struct Watched<R> {
    inner: R,
    saw: Arc<AtomicBool>,
}

impl<R: AsyncRead + Unpin> AsyncRead for Watched<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            self.saw.store(true, Ordering::Relaxed);
        }
        r
    }
}

/// One link that said hello.
pub struct Link {
    name: String,
    command: Vec<String>,
    connect: Duration,
    bridge: bridge::Client,
    child: tokio::process::Child,
    tail: Tail,
    saw: Arc<AtomicBool>,
    rpc: tokio::task::JoinHandle<Result<(), capnp::Error>>,
}

/// The argv the link runs: the host's command and the bridge line after it.
pub fn argv(host: &Host) -> Vec<String> {
    let mut v = host.command.clone();
    v.push(BRIDGE_LINE.to_string());
    v
}

/// The host command as a person would type it, for "run ssh vm once".
fn typed(command: &[String]) -> String {
    command
        .iter()
        .map(|w| super::quote(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What went wrong, as one line naming the host, from everything known about
/// a link that stopped: what its command wrote to stderr, how it exited,
/// whether any bytes came back, and whether it was a wait that ran out.
///
/// The order is the order of usefulness: ssh refusing for want of a login or
/// a key says what to do about it, a missing bridge says which end to
/// upgrade, and the rest says what happened.
pub fn reason(
    name: &str,
    command: &[String],
    stderr: &[String],
    exit: Option<i32>,
    saw_bytes: bool,
    waited: Option<Duration>,
) -> String {
    let all = stderr.join("\n");
    let last = stderr
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| clean(l.trim()))
        .unwrap_or_default();
    let typed = typed(command);
    if all.contains("Host key verification failed")
        || all.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
        || all.contains("No ED25519 host key is known")
        || all.contains("host key for")
    {
        let line = stderr
            .iter()
            .find(|l| l.contains("Host key") || l.contains("host key") || l.contains("HOST"))
            .map(|l| clean(l.trim()))
            .unwrap_or(last);
        return format!("{line}; connect once with {typed} in a terminal to accept the key");
    }
    if all.contains("Permission denied")
        || all.contains("password:")
        || all.contains("passphrase")
        || all.contains("Too many authentication failures")
    {
        return format!("{name} needs a login: run {typed} once in a terminal");
    }
    if exit == Some(127) || (all.contains("not found") && all.contains("tmux-companion")) {
        return format!("{name} has no tmux-companion on its PATH");
    }
    if all.contains("unrecognized subcommand") && all.contains("bridge") {
        return format!("{name} runs a tmux-companion with no bridge: upgrade it there");
    }
    if saw_bytes {
        return format!("{name} sent something that isn't the protocol");
    }
    if let Some(w) = waited {
        return format!("no answer in {}s", w.as_secs().max(1));
    }
    if !last.is_empty() {
        return last;
    }
    match exit {
        Some(0) => format!("{name}'s host command ended before the bridge said hello"),
        Some(code) => format!("{name}'s host command exited with status {code}"),
        None => format!("{name}'s host command ended"),
    }
}

/// Start the host command and say hello, within `connect_secs`.
///
/// The command runs in a session of its own with no controlling terminal and
/// `SSH_ASKPASS_REQUIRE=never`, so ssh can't stop to ask for a password or a
/// key it would accept: it fails at once, with a line saying why, instead of
/// waiting on a prompt nobody can see. Host keys stay strict.
pub async fn open(host: &Host) -> Result<(Link, Hello), String> {
    let argv = argv(host);
    let connect = Duration::from_secs(host.connect_secs.max(1));
    let mut cmd = tokio::process::Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env_remove("SSH_ASKPASS")
        .env_remove("DISPLAY")
        .kill_on_drop(true);
    // SAFETY: setsid is async-signal-safe and touches nothing in this
    // process's memory; it only detaches the child from any terminal.
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid()
                .map(|_| ())
                .map_err(std::io::Error::from)
        });
    }
    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "{}: cannot run {}: {e}",
            host.name,
            argv.first().map(String::as_str).unwrap_or("")
        )
    })?;

    let tail: Tail = Arc::new(Mutex::new(Vec::new()));
    if let Some(stderr) = child.stderr.take() {
        let tail = Arc::clone(&tail);
        tokio::task::spawn_local(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                t.push(clean(&line));
                if t.len() > TAIL_LINES {
                    t.remove(0);
                }
            }
        });
    }

    let saw = Arc::new(AtomicBool::new(false));
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let network = twoparty::VatNetwork::new(
        futures::io::BufReader::new(
            Watched {
                inner: stdout,
                saw: Arc::clone(&saw),
            }
            .compat(),
        ),
        futures::io::BufWriter::new(stdin.compat_write()),
        rpc_twoparty_capnp::Side::Client,
        reader_options(),
    );
    let mut system = RpcSystem::new(Box::new(network), None);
    let bridge: bridge::Client = system.bootstrap(rpc_twoparty_capnp::Side::Server);
    let rpc = tokio::task::spawn_local(system);

    let mut link = Link {
        name: host.name.clone(),
        command: host.command.clone(),
        connect,
        bridge,
        child,
        tail,
        saw,
        rpc,
    };
    let mut req = link.bridge.hello_request();
    req.get().set_build(crate::proto::build_id());
    req.get().set_protocol(PROTOCOL);
    let answer = match tokio::time::timeout(connect, req.send().promise).await {
        Err(_) => return Err(link.fail(Some(connect)).await),
        Ok(Err(e)) => return Err(link.fail_with(e).await),
        Ok(Ok(a)) => a,
    };
    let hello = match read_hello(&answer) {
        Ok(h) => h,
        Err(e) => return Err(link.fail_with(e).await),
    };
    Ok((link, hello))
}

/// The hello's results, cleaned.
fn read_hello(
    answer: &capnp::capability::Response<bridge::hello_results::Owned>,
) -> capnp::Result<Hello> {
    let r = answer.get()?;
    let text = |t: capnp::Result<capnp::text::Reader<'_>>| -> capnp::Result<String> {
        Ok(clean(&String::from_utf8_lossy(t?.as_bytes())))
    };
    let methods = r
        .get_methods()?
        .iter()
        .take(64)
        .map(|m| Ok(clean(&String::from_utf8_lossy(m?.as_bytes()))))
        .collect::<capnp::Result<Vec<_>>>()?;
    Ok(Hello {
        build: text(r.get_build())?,
        protocol: r.get_protocol(),
        methods,
        started_daemon: r.get_started_daemon(),
        tmux: text(r.get_tmux())?,
    })
}

impl Link {
    /// The host's sessions, within the connect time.
    pub async fn sessions(&mut self) -> Result<(u64, Vec<Session>), String> {
        let req = self.bridge.sessions_request();
        let answer = match tokio::time::timeout(self.connect, req.send().promise).await {
            Err(_) => return Err(self.fail(Some(self.connect)).await),
            Ok(Err(e)) => return Err(self.fail_with(e).await),
            Ok(Ok(a)) => a,
        };
        let read = answer
            .get()
            .and_then(|r| Ok((r.get_polled_at(), read_sessions(r.get_sessions()?)?)));
        match read {
            Ok(v) => Ok(v),
            Err(e) => Err(self.fail_with(e).await),
        }
    }

    /// One session's segments and windows, within the connect time; a
    /// bridge from before windows answers none. The outer error is the link
    /// breaking; the inner one is the host saying no, which leaves the link
    /// up: a grouped session not there yet is an answer, not a failure.
    pub async fn segments(
        &mut self,
        session: &str,
    ) -> Result<Result<(Vec<Segment>, Vec<Window>), String>, String> {
        let mut req = self.bridge.segments_request();
        req.get().set_session(session);
        let answer = match tokio::time::timeout(self.connect, req.send().promise).await {
            Err(_) => return Err(self.fail(Some(self.connect)).await),
            Ok(Err(e)) if e.extra.starts_with("remote exception: ") => {
                return Ok(Err(clean(&e.extra["remote exception: ".len()..])));
            }
            Ok(Err(e)) => return Err(self.fail_with(e).await),
            Ok(Ok(a)) => a,
        };
        match answer.get().and_then(|r| {
            Ok((
                read_segments(r.get_segments()?)?,
                read_windows(r.get_windows()?)?,
            ))
        }) {
            Ok(v) => Ok(Ok(v)),
            Err(e) => Err(self.fail_with(e).await),
        }
    }

    /// The agents waiting on the host, within the connect time; the two
    /// errors as for [`Link::segments`].
    pub async fn inbox(&mut self) -> Result<Result<Vec<crate::inbox::Entry>, String>, String> {
        let req = self.bridge.inbox_request();
        let answer = match tokio::time::timeout(self.connect, req.send().promise).await {
            Err(_) => return Err(self.fail(Some(self.connect)).await),
            Ok(Err(e)) if e.extra.starts_with("remote exception: ") => {
                return Ok(Err(clean(&e.extra["remote exception: ".len()..])));
            }
            Ok(Err(e)) => return Err(self.fail_with(e).await),
            Ok(Ok(a)) => a,
        };
        match answer.get().and_then(|r| read_entries(r.get_entries()?)) {
            Ok(v) => Ok(Ok(v)),
            Err(e) => Err(self.fail_with(e).await),
        }
    }

    /// The directories the host's own configured source lists, within the
    /// connect time; the two errors as for [`Link::segments`].
    pub async fn dirs(&mut self) -> Result<Result<Vec<String>, String>, String> {
        let req = self.bridge.dirs_request();
        let answer = match tokio::time::timeout(self.connect, req.send().promise).await {
            Err(_) => return Err(self.fail(Some(self.connect)).await),
            Ok(Err(e)) if e.extra.starts_with("remote exception: ") => {
                return Ok(Err(clean(&e.extra["remote exception: ".len()..])));
            }
            Ok(Err(e)) => return Err(self.fail_with(e).await),
            Ok(Ok(a)) => a,
        };
        match answer.get().and_then(|r| read_dirs(r.get_paths()?)) {
            Ok(v) => Ok(Ok(v)),
            Err(e) => Err(self.fail_with(e).await),
        }
    }

    /// Start watching: the host calls `sink` from now on, first with
    /// everything and then with each change. The call returned stays
    /// unanswered while the link lives, so when it ends the link went.
    pub fn watch(
        &self,
        sink: session_sink::Client,
        machine: &str,
    ) -> capnp::capability::Promise<
        capnp::capability::Response<bridge::watch_results::Owned>,
        capnp::Error,
    > {
        let mut req = self.bridge.watch_request();
        req.get().set_sink(sink);
        req.get().set_machine(machine);
        req.send().promise
    }

    /// One ping. Whatever comes back, an answer or the host's refusal, says
    /// the host is there.
    pub fn ping(&self) -> impl std::future::Future<Output = ()> + use<> {
        let promise = self.bridge.ping_request().send().promise;
        async move {
            let _ = promise.await;
        }
    }

    /// The reason for a watch that ended in `e`, everything stopped.
    pub async fn broke(&mut self, e: capnp::Error) -> String {
        self.fail_with(e).await
    }

    /// Close the link: the RPC system and the host command both go.
    pub async fn close(mut self) {
        self.stop().await;
    }

    /// [`Link::close`] for a link still borrowed: what a link called down
    /// for not answering does.
    pub async fn shut(&mut self) {
        self.stop().await;
    }

    async fn stop(&mut self) {
        self.rpc.abort();
        if let Some(pid) = self.child.id() {
            // The whole session the command started: `sh -c` and whatever
            // it ran, not only the first process.
            let group = nix::unistd::Pid::from_raw(pid as i32);
            let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGTERM);
            // A stopped process holds a TERM until it runs again: a link
            // called down for not answering pings may be one.
            let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGCONT);
        }
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await;
    }

    /// The reason for a call that ended in `e`.
    ///
    /// An exception the bridge raised is the host saying why, its daemon
    /// refusing its config say, and its words are the reason. Anything else
    /// is the link breaking, and the command's own end says more.
    async fn fail_with(&mut self, e: capnp::Error) -> String {
        if let Some(said) = e.extra.strip_prefix("remote exception: ") {
            let said = clean(said);
            self.stop().await;
            return format!("{}: {said}", self.name);
        }
        self.fail(None).await
    }

    /// Stop everything and say why, `waited` set when a wait ran out.
    async fn fail(&mut self, waited: Option<Duration>) -> String {
        // The RPC system ends a moment after the call that saw it break.
        let rpc_error = match tokio::time::timeout(Duration::from_millis(200), &mut self.rpc).await
        {
            Ok(Ok(Err(e))) => Some(e),
            _ => None,
        };
        let exited = if waited.is_some() {
            None
        } else {
            tokio::time::timeout(Duration::from_secs(1), self.child.wait())
                .await
                .ok()
                .and_then(Result::ok)
        };
        self.stop().await;
        // stderr arrives on its own task; let it catch up with the exit.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let tail = self.tail.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let saw = self.saw.load(Ordering::Relaxed);
        let protocol = saw
            && rpc_error
                .as_ref()
                .is_some_and(|e| e.kind != capnp::ErrorKind::Disconnected);
        reason(
            &self.name,
            &self.command,
            &tail,
            exited.and_then(|s| s.code()),
            saw && (protocol || waited.is_some() || rpc_error.is_some()),
            waited,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> Vec<String> {
        vec!["ssh".into(), "vm".into()]
    }

    #[test]
    fn the_bridge_line_goes_last_as_one_argument() {
        let h = Host::new("vm", cmd());
        assert_eq!(argv(&h), ["ssh", "vm", "tmux-companion bridge"]);
    }

    #[test]
    fn every_failure_says_what_to_do_about_it() {
        let r = |stderr: &[&str], exit, saw, waited| {
            let lines: Vec<String> = stderr.iter().map(|s| s.to_string()).collect();
            reason("vm", &cmd(), &lines, exit, saw, waited)
        };
        assert_eq!(
            r(
                &["user@vm: Permission denied (publickey)."],
                Some(255),
                false,
                None
            ),
            "vm needs a login: run ssh vm once in a terminal"
        );
        assert_eq!(
            r(
                &[
                    "No ED25519 host key is known for vm and you have requested strict checking.",
                    "Host key verification failed."
                ],
                Some(255),
                false,
                None
            ),
            "No ED25519 host key is known for vm and you have requested strict checking.; connect once with ssh vm in a terminal to accept the key"
        );
        assert_eq!(
            r(
                &["sh: tmux-companion: command not found"],
                Some(127),
                false,
                None
            ),
            "vm has no tmux-companion on its PATH"
        );
        assert_eq!(
            r(
                &["error: unrecognized subcommand 'bridge'"],
                Some(2),
                false,
                None
            ),
            "vm runs a tmux-companion with no bridge: upgrade it there"
        );
        assert_eq!(
            r(&[], None, true, Some(Duration::from_secs(10))),
            "vm sent something that isn't the protocol"
        );
        assert_eq!(
            r(&[], None, false, Some(Duration::from_secs(10))),
            "no answer in 10s"
        );
        assert_eq!(
            r(
                &["ssh: connect to host vm port 22: Connection timed out"],
                Some(255),
                false,
                None
            ),
            "ssh: connect to host vm port 22: Connection timed out"
        );
        assert_eq!(
            r(&[], Some(3), false, None),
            "vm's host command exited with status 3"
        );
    }

    #[test]
    fn a_reason_from_the_host_is_cleaned_before_it_is_shown() {
        let lines = vec!["\x1b[2Jboom\x07".to_string()];
        assert_eq!(
            reason("vm", &cmd(), &lines, Some(1), false, None),
            "[2Jboom"
        );
    }
}
