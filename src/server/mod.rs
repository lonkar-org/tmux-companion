//! The server half: bind the socket, accept forever, hand each line to a
//! handler.
mod handlers;
/// Server state and its caches.
pub mod state;

use std::sync::Arc;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
    sync::Mutex,
};

use crate::{client::sock_path, proto::Request, server::state::ServerState};

/// How often the daemon checks that the socket file it bound is still the one
/// clients reach.
///
/// A stat of one path.  Thirty seconds is slow enough to cost nothing and
/// quick enough that a sandbox teardown does not leave a daemon standing for
/// the rest of the day.
const SOCKET_WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// Bind the socket and serve until killed.
///
/// Exits quietly, and successfully, if another server is already listening:
/// every client invocation tries to start one, so losing that race is the
/// ordinary case rather than an error.
pub async fn run() -> anyhow::Result<()> {
    let sock = sock_path();

    // Everything from here to the bind is one critical section, and it has to
    // be one.  A daemon unlinks the socket file before it binds, because that
    // is the only way to clear the file a crashed predecessor left behind, and
    // unlinking is exactly what makes `AddrInUse` unreachable: two starts that
    // race each other both pass the connect check, both unlink, and both bind.
    // The second unlink takes the first one's socket file away.  What is left
    // is a daemon nobody can connect to, holding an unlinked inode, with
    // nothing in the process that would ever tell it to stop.
    //
    // That is not a theoretical race.  It is where the orphaned daemons come
    // from, and they arrive in pairs: two starts in the same second, one of
    // them unreachable from birth.
    //
    // Losing the lock is the ordinary case, the same way losing the bind was:
    // the client that spawned this process is already retrying its connect,
    // and whoever holds the lock is the daemon it will reach.
    let _lock = match start_lock(&lock_path(&sock)) {
        StartLock::Held(l) => Some(l),
        StartLock::Busy => return Ok(()),
        // No lock is worse than a lock and better than no daemon.  A /tmp this
        // process cannot write to is a broken machine, not a race to lose.
        StartLock::Unavailable(e) => {
            eprintln!("tmux-companion: starting without the start lock: {e}");
            None
        }
    };

    // If an existing server is accepting connections, exit quietly.
    if tokio::net::UnixStream::connect(&sock).await.is_ok() {
        return Ok(());
    }

    // Parsed before the socket exists, and that order is the whole point. A
    // daemon that binds first and validates second is reachable for as long as
    // the parse takes, so a client can connect, be accepted, and then have the
    // connection reset when the daemon gives up on the config. What reaches
    // the person is `Connection reset by peer (os error 104)` rather than the
    // name of the key they got wrong, and which of the two they see depends on
    // how loaded the machine is.
    //
    // The error goes to stderr and to the state file, which is where a client
    // looks when its own start attempt produced no server.
    let config = match crate::config::load() {
        Ok((c, _)) => {
            // The record of the last refusal goes when a daemon gets past the
            // config, because from here on it is not true any more. Without
            // this it was written on a bad start and never taken away, so
            // `doctor` kept reporting a parse error somebody had already
            // fixed, and a later client that failed to connect for some
            // unrelated reason blamed the config for it.
            clear_config_error();
            c
        }
        Err(e) => {
            record_config_error(&e);
            return Err(anyhow::anyhow!("{e}"));
        }
    };

    // Remove stale socket file from a previous crashed run.
    let _ = std::fs::remove_file(&sock);

    let listener = match UnixListener::bind(&sock) {
        Ok(l) => {
            // The socket is an execution surface: a request makes this process
            // read git state today and spawn commands once `run` and `open`
            // land, and /tmp is a directory every user on the machine can write
            // to. 0600 means only its owner can connect.
            //
            // There is a window between bind and chmod. Closing it properly
            // needs the socket created inside a 0700 directory, which is what
            // moving to $XDG_RUNTIME_DIR would give; until then this narrows it
            // from forever to microseconds.
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))
            {
                eprintln!("tmux-companion: could not restrict socket permissions: {e}");
            }
            l
        }
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Another server raced us to the bind — exit quietly.  With the
            // start lock held this is unreachable; it stays because a daemon
            // from a build without the lock can still be mid-start beside us.
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    // One line per start, so the log says which build has been answering
    // since when. Everything else the daemon writes is a failure.
    eprintln!(
        "tmux-companion: daemon {} started, pid {}",
        crate::proto::build_id(),
        std::process::id()
    );

    // The one thing that bounds this process's life.  Everything else here
    // runs forever on purpose.
    match socket_identity(&sock) {
        Some(mine) => {
            tokio::spawn(watch_socket(sock.clone(), mine));
        }
        None => eprintln!(
            "tmux-companion: cannot stat the socket just bound; \
             the unreachable-daemon watchdog is off for this process"
        ),
    }

    // The state comes first so the timers can note a failure on it; before
    // the health mark existed a failed timer had only the log to go to.
    let state = Arc::new(Mutex::new(ServerState::with_config(config.clone())));

    // Started before the accept loop, so it runs for as long as the daemon
    // does and stops when it stops. That is the whole of the lifetime
    // management the zsh version needed a PID lock file for.
    if config.autosave.enabled {
        let home = std::env::var("HOME").unwrap_or_default();
        let script = config.autosave.script_path(&home);
        let interval = std::time::Duration::from_secs(config.autosave.interval_secs.max(1));
        tokio::spawn(crate::tasks::autosave_loop(
            script,
            interval,
            Arc::clone(&state),
        ));
    }

    // A snapshot of the whole server on the schedule the config asks for, and
    // a marker saying this daemon is alive, so a restore can tell a crash from
    // a clean stop. The marker is written whether or not the timer runs: it is
    // about the daemon, not about the snapshots.
    if let Some(dir) = state_dir() {
        if let Err(e) = crate::sessions::timer::mark_running_in(&dir, std::process::id()) {
            eprintln!("tmux-companion: could not record that this daemon is running: {e}");
        }
        // Which build last wrote here, for the reader of a state directory
        // that a later build cannot make sense of.
        if let Err(e) = record_build_in(&dir, &crate::proto::build_id()) {
            eprintln!("tmux-companion: could not record the build in the state directory: {e}");
        }
    }

    // `pkill tmux-companion` and a terminal's ^C are how most daemons stop,
    // and until this ran neither took the marker off, so every restore after
    // an upgrade thought the machine had gone down badly.
    tokio::spawn(stop_on_signal(sock.clone()));
    if config.sessions.autosave != crate::config::SessionsAutosave::Off {
        tokio::spawn(crate::sessions::timer::sessions_autosave_loop(
            config.sessions.clone(),
            Arc::clone(&state),
        ));
    }

    // The inbox: which agents have stopped and what each one asked, kept by
    // the daemon so the question is on record for a window nobody looked at.
    if config.agents.inbox && !config.agents.programs.is_empty() {
        tokio::spawn(crate::inbox::inbox_loop(
            config.agents.clone(),
            Arc::clone(&state),
        ));
    }

    if config.autoreload.enabled {
        tokio::spawn(crate::autoreload::autoreload_loop(
            config.autoreload.clone(),
            crate::keys::tmux_conf_path(),
        ));
    }

    if config.window_names.enabled {
        tokio::spawn(crate::window_names::window_names_loop(
            config.window_names.clone(),
            config.sh_jobs.clone(),
        ));
    }

    if config.notify.enabled {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        tokio::spawn(crate::notify::notify_loop(
            config.notify.clone(),
            shell,
            Arc::clone(&state),
        ));
    }

    let autofetch = config.git.autofetch.clone();

    // Off unless asked for: this is the only part of the tool that talks to a
    // network, and a daemon quietly reaching a remote is not a surprise
    // anybody should get from a status bar.
    if autofetch.enabled {
        tokio::spawn(crate::autofetch::autofetch_loop(
            Arc::clone(&state),
            autofetch,
        ));
    }

    // Pre-warm battery cache so the first tmux refresh doesn't hit the ~600ms
    // cold-start cost of IOKit initialization in the battery crate.
    {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Ok(s) = crate::segments::battery::render().await {
                state.lock().await.battery_cache = Some((s, std::time::Instant::now()));
            }
        });
    }

    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, state).await {
                eprintln!("tmux-companion handler error: {e}");
            }
        });
    }
}

/// The lock file beside a socket: the same path with `.lock` on the end.
///
/// Beside the socket rather than in a fixed directory, because the socket path
/// is what `TMUX_COMPANION_SOCK` moves.  A test harness that gives itself its
/// own socket gets its own lock for free, and two harnesses running at once do
/// not serialise against each other.
fn lock_path(sock: &std::path::Path) -> std::path::PathBuf {
    let mut p = sock.as_os_str().to_os_string();
    p.push(".lock");
    std::path::PathBuf::from(p)
}

/// What a start got when it asked for the lock.
enum StartLock {
    /// Ours until this process exits.
    Held(nix::fcntl::Flock<std::fs::File>),
    /// Another start is in flight, or a daemon is running.
    Busy,
    /// The lock file could not be opened at all.
    Unavailable(std::io::Error),
}

/// Take the exclusive lock a daemon holds from before it unlinks the socket
/// until it exits.
///
/// `flock` and not a PID file: the kernel drops it when the process goes,
/// however it goes, so there is no stale lock to reason about and no second
/// liveness check to get wrong.  A daemon killed with SIGKILL leaves the lock
/// file on disk and the lock itself released, which is the state the next
/// start wants.
fn start_lock(path: &std::path::Path) -> StartLock {
    use nix::fcntl::{Flock, FlockArg};
    use std::os::unix::fs::OpenOptionsExt;

    let file = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)
    {
        Ok(f) => f,
        Err(e) => return StartLock::Unavailable(e),
    };

    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(l) => StartLock::Held(l),
        Err((_, _)) => StartLock::Busy,
    }
}

/// What identifies the socket file this daemon bound.
///
/// Device, inode, and the inode's change time. Not the path: a replacement
/// socket at the same path is a different file, and that is exactly the case a
/// daemon has to notice.
///
/// The change time is in there because device and inode are not enough on
/// Linux, where an inode number is handed straight back out after the file
/// using it is unlinked. A harness that deletes a socket and starts a daemon
/// that binds a new one at the same path gets the same inode often enough to
/// matter, and the old daemon then decides it is still looking at its own
/// socket. macOS does not reuse them that eagerly, which is why this passed
/// here and failed on the Linux runner.
///
/// Nothing touches the socket file after the `chmod` that follows the bind, so
/// the change time is stable for the life of the daemon. The identity is taken
/// after that `chmod` for the same reason.
fn socket_identity(sock: &std::path::Path) -> Option<(u64, u64, i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(sock).ok()?;
    Some((m.dev(), m.ino(), m.ctime(), m.ctime_nsec()))
}

/// Exit once the socket file this daemon bound is gone or has been replaced.
///
/// Until this existed nothing bounded a daemon's life but a signal.  A test
/// harness or a demo script that pointed `TMUX_COMPANION_SOCK` at a sandbox,
/// ran, and then deleted the sandbox left a daemon holding an unlinked inode
/// forever: unreachable, idle, and invisible to every `pkill` pattern aimed at
/// the installed binary.  They accumulate one per run, and nothing in the
/// process ever notices.
///
/// Exiting on a missing socket is right for the live daemon too.  A socket
/// file that has gone is a daemon no client can reach, and the next client
/// starts a fresh one in a few milliseconds.
async fn watch_socket(sock: std::path::PathBuf, mine: (u64, u64, i64, i64)) {
    loop {
        tokio::time::sleep(SOCKET_WATCH_INTERVAL).await;
        if socket_identity(&sock) != Some(mine) {
            eprintln!(
                "tmux-companion: {} is no longer this daemon's socket; exiting.",
                sock.display()
            );
            std::process::exit(0);
        }
    }
}

/// Stop on SIGTERM or SIGINT the way `__shutdown` stops: marker off, socket
/// file gone, exit status zero.
///
/// The lock file stays.  The kernel releases the lock itself when this
/// process exits, and unlinking the file first would hand a start that had
/// already opened it a lock nobody else can see, which is the pair of
/// daemons the lock exists to prevent.
async fn stop_on_signal(sock: std::path::PathBuf) {
    use tokio::signal::unix::{SignalKind, signal};

    let (mut term, mut int) = match (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) {
        (Ok(t), Ok(i)) => (t, i),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!(
                "tmux-companion: cannot listen for signals, a kill will leave the marker: {e}"
            );
            return;
        }
    };
    let which = tokio::select! {
        _ = term.recv() => "SIGTERM",
        _ = int.recv() => "SIGINT",
    };
    eprintln!("tmux-companion: {which}, stopping");
    if let Some(dir) = state_dir() {
        crate::sessions::timer::clear_marker_in(&dir);
    }
    let _ = std::fs::remove_file(&sock);
    std::process::exit(0);
}

/// Record which build last wrote the state directory.
///
/// One line, the build id, replaced on every start.  A state directory is
/// read by whichever build is installed, and when that build cannot make
/// sense of a file the first question is which one wrote it.
fn record_build_in(dir: &std::path::Path, build_id: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("VERSION"), format!("{build_id}\n"))
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    state: Arc<Mutex<ServerState>>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    if let Some(line) = lines.next_line().await? {
        let req: Request = serde_json::from_str(&line)?;
        let resp = handlers::dispatch(req, state).await;
        let mut out = serde_json::to_string(&resp)?;
        out.push('\n');
        writer.write_all(out.as_bytes()).await?;
    }
    Ok(())
}

/// Write a config error where a client can find it.
///
/// A daemon that refuses to start says why on stderr, and nobody sees stderr:
/// the client that spawned it redirects all three streams to /dev/null so a
/// status bar is not corrupted by a stray line. The file is the copy somebody
/// can actually read, and `tmux-companion config check` prints it in full.
fn record_config_error(e: &crate::config::ConfigError) {
    eprintln!("tmux-companion: {e}");
    let Some(dir) = state_dir() else { return };
    write_config_error(&dir, &e.to_string());
}

/// Forget the last refusal, because a daemon has just started without one.
fn clear_config_error() {
    let Some(dir) = state_dir() else { return };
    forget_config_error(&dir);
}

/// The file half of [`record_config_error`], against a directory it is given.
///
/// Split out so both halves are tested against a temporary directory rather
/// than against whatever `$XDG_STATE_HOME` happens to be on the machine
/// running the suite.
fn write_config_error(dir: &std::path::Path, message: &str) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join("last-error"), format!("{message}\n"));
}

/// The file half of [`clear_config_error`].
fn forget_config_error(dir: &std::path::Path) {
    let _ = std::fs::remove_file(dir.join("last-error"));
}

/// Where a daemon's stderr goes, from the config when it parses and the state
/// directory when it does not.
///
/// Read by the client that starts the daemon, because a process has no say
/// over where its own stderr was pointed. The broken-config case still gets a
/// log, since that is the case where the daemon has the most to say.
pub fn daemon_log_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let general = crate::config::load()
        .map(|(c, _)| c.general)
        .unwrap_or_default();
    general.log_path(&home, state_dir())
}

/// Keep the daemon log from growing without bound: past `LOG_LIMIT` bytes the
/// file is moved aside to `<name>.1`, replacing the previous one.
pub const LOG_LIMIT: u64 = 1 << 20;

/// Move a log aside once it is over [`LOG_LIMIT`], so a client opening it for
/// append starts a fresh one.
pub fn rotate_log(path: &std::path::Path) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() > LOG_LIMIT {
        let mut aside = path.as_os_str().to_owned();
        aside.push(".1");
        let _ = std::fs::rename(path, aside);
    }
}

/// `$XDG_STATE_HOME/tmux-companion`, or `~/.local/state/tmux-companion`.
pub fn state_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })
        .map(|d| d.join("tmux-companion"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemon_that_starts_cleanly_takes_the_last_refusal_away() {
        // It used to be written on a bad start and never removed, so `doctor`
        // went on reporting a parse error somebody had already fixed, and a
        // client that failed to connect for an unrelated reason blamed the
        // config for it.
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("last-error");

        write_config_error(dir.path(), "config.toml: unknown field `bogus`");
        let written = std::fs::read_to_string(&file).expect("written");
        assert!(written.contains("unknown field"), "{written}");
        assert!(
            written.ends_with('\n'),
            "a line, not a fragment: {written:?}"
        );

        forget_config_error(dir.path());
        assert!(!file.exists(), "the refusal should be gone");
    }

    #[test]
    fn forgetting_a_refusal_nobody_recorded_is_not_an_error() {
        // The ordinary case: every daemon that has ever started cleanly on a
        // machine with a good config takes this path.
        let dir = tempfile::tempdir().expect("tempdir");
        forget_config_error(dir.path());
        forget_config_error(dir.path());
        assert!(!dir.path().join("last-error").exists());

        // And a directory that is not there either.
        forget_config_error(&dir.path().join("no-such-directory"));
    }

    #[test]
    fn the_state_directory_names_the_build_that_last_wrote_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = dir.path().join("tmux-companion");

        record_build_in(&state, "0.2.0+1790400979").expect("record");
        let text = std::fs::read_to_string(state.join("VERSION")).expect("written");
        assert_eq!(text, "0.2.0+1790400979\n");

        // Replaced, not appended: the question is which build wrote last.
        record_build_in(&state, "0.3.0+1790500000").expect("record again");
        let text = std::fs::read_to_string(state.join("VERSION")).expect("written");
        assert_eq!(text, "0.3.0+1790500000\n");
    }

    #[test]
    fn the_lock_sits_beside_the_socket_it_guards() {
        assert_eq!(
            lock_path(std::path::Path::new("/tmp/tmux-companion-501.sock")),
            std::path::PathBuf::from("/tmp/tmux-companion-501.sock.lock")
        );
    }

    #[test]
    fn a_second_start_finds_the_lock_busy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.sock.lock");

        let first = start_lock(&path);
        assert!(matches!(first, StartLock::Held(_)));
        // Same process, so this is flock's own semantics rather than a
        // cross-process assertion: flock is per open file description, and two
        // opens of one path are two descriptions.
        assert!(matches!(start_lock(&path), StartLock::Busy));

        drop(first);
        assert!(matches!(start_lock(&path), StartLock::Held(_)));
    }

    #[test]
    fn a_lock_file_that_cannot_be_opened_is_unavailable_not_busy() {
        // The difference decides whether a daemon starts: busy means somebody
        // else is doing it, unavailable means nobody will.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no-such-directory").join("s.lock");
        assert!(matches!(start_lock(&path), StartLock::Unavailable(_)));
    }

    #[test]
    fn a_replaced_file_is_not_the_file_that_was_there() {
        // The version of this that compared device and inode alone passed on
        // macOS and failed on the Linux runner, because Linux hands an inode
        // number straight back out after the file using it is unlinked: the
        // replacement came back with the same pair. That was a real hole in
        // the watchdog and not only in the test.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("s.sock");

        std::fs::write(&path, b"").expect("write");
        let first = socket_identity(&path).expect("stat");
        assert_eq!(socket_identity(&path), Some(first));

        std::fs::remove_file(&path).expect("remove");
        assert_eq!(socket_identity(&path), None);

        // Slept on purpose: the two files are distinguished by the inode's
        // change time when the number is reused, and a filesystem whose
        // timestamps have coarse resolution needs a moment between them.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, b"").expect("write again");
        assert_ne!(
            socket_identity(&path),
            Some(first),
            "a new file at the same path must not pass for the old one"
        );
    }
}
