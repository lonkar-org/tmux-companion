//! The client half: find the socket, start a server if nothing answers, send
//! one JSON line and read one back.
use std::path::PathBuf;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::proto::{Request, Response};

/// Longest path a `sockaddr_un` can hold on macOS, minus the NUL terminator.
const SUN_PATH_MAX: usize = 103;

/// The socket this process talks to, or would listen on.
pub fn sock_path() -> PathBuf {
    // `TMUX_COMPANION_SOCK` puts a server beside the live one -- what the
    // measurements in BENCHMARKS.md use, so a benchmark run never touches the
    // status bar the user is actually looking at.  Ignored, with a warning, if
    // it cannot fit in a unix socket address.
    if let Some(p) = std::env::var_os("TMUX_COMPANION_SOCK") {
        if !p.is_empty() && p.len() <= SUN_PATH_MAX {
            return PathBuf::from(p);
        }
        eprintln!(
            "tmux-companion: ignoring TMUX_COMPANION_SOCK ({} bytes; limit is {SUN_PATH_MAX})",
            p.len()
        );
    }
    let uid = nix::unistd::getuid();
    PathBuf::from(format!("/tmp/tmux-companion-{}.sock", uid))
}

/// One request, one response, no printing.
///
/// Split out of `send_and_print` so a test can assert on a `Response` instead
/// of scraping stdout, which is the whole reason an integration test over the
/// real socket is worth having.
pub async fn send(req: Request) -> anyhow::Result<Response> {
    let resp = send_once(&req).await?;

    // A daemon from an older build answers with an older build's behaviour,
    // and after the port it may not know the command at all. Replace it once
    // and retry; the retry is bounded because the new daemon reports the build
    // that just started it.
    if !resp.version.is_empty() && resp.version != crate::proto::build_id() {
        eprintln!(
            "tmux-companion: replacing daemon from build {} with {}",
            resp.version,
            crate::proto::build_id()
        );
        let _ = send_once(&Request::raw("__shutdown", serde_json::Value::Null)).await;
        wait_for_socket_to_go().await;
        return send_once(&req).await;
    }

    Ok(resp)
}

/// Wait for a shutting-down daemon to release the socket, briefly.
///
/// Bounded because the alternative is a status bar that hangs: if the old
/// daemon will not go, the retry connects to it and the caller gets its answer,
/// which is the same thing that happened before any of this existed.
async fn wait_for_socket_to_go() {
    for _ in 0..20 {
        if tokio::net::UnixStream::connect(sock_path()).await.is_err() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

/// One request, one response, no version check and no daemon replaced.
///
/// `doctor` uses this: a diagnostic that restarts the thing it is reporting on
/// is not a diagnostic.
pub async fn send_once(req: &Request) -> anyhow::Result<Response> {
    let stream = connect_with_retry().await?;
    let (reader, mut writer) = stream.into_split();

    let mut msg = serde_json::to_string(req)?;
    msg.push('\n');
    writer.write_all(msg.as_bytes()).await?;

    let mut lines = BufReader::new(reader).lines();
    match lines.next_line().await? {
        Some(line) => Ok(serde_json::from_str(&line)?),
        // The server closed without answering. Treated as an error rather than
        // an empty render, so a status bar shows something went wrong.
        None => anyhow::bail!("server closed the connection without a response"),
    }
}

/// Send one request and print the output field, or the error to stderr.
pub async fn send_and_print(req: Request) -> anyhow::Result<()> {
    let resp = send(req).await?;
    if let Some(err) = resp.error {
        eprintln!("tmux-companion error: {err}");
    } else {
        print!("{}", resp.output);
    }
    Ok(())
}

async fn connect_with_retry() -> anyhow::Result<tokio::net::UnixStream> {
    let sock = sock_path();
    check_socket_owner(&sock)?;

    for attempt in 0..=10u32 {
        match tokio::net::UnixStream::connect(&sock).await {
            Ok(s) => return Ok(s),
            Err(_) if attempt == 0 => {
                spawn_server()?;
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(50 * attempt as u64)).await;
            }
        }
    }
    // The server could not be reached and could not be started. The commonest
    // reason by far is a config file it refused to start on, and that error is
    // the only thing here worth putting in front of somebody.
    match last_config_error() {
        Some(e) => anyhow::bail!("config: {e}"),
        None => anyhow::bail!("server failed to start after retries"),
    }
}

/// The first line of the error the daemon last refused to start on.
///
/// One line, because the caller's stdout is a status bar with about 150
/// columns in it and a parse error does not fit. The whole thing is what
/// `tmux-companion config check` prints.
fn last_config_error() -> Option<String> {
    let path = crate::server::state_dir()?.join("last-error");
    let text = std::fs::read_to_string(path).ok()?;
    let first = text.lines().next()?.trim().to_string();
    if first.is_empty() {
        return None;
    }
    Some(format!("{first} — run tmux-companion config check"))
}

/// Refuse to talk to a socket somebody else owns.
///
/// The default path is under /tmp, which every user on the machine can write
/// to, so a socket at the expected name is not necessarily this user's daemon.
/// Connecting to somebody else's would hand them whatever a request carries and
/// take whatever they answered with straight to the status bar.
fn check_socket_owner(sock: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let meta = match std::fs::metadata(sock) {
        Ok(m) => m,
        // Absent is fine: the caller is about to start a server.
        Err(_) => return Ok(()),
    };
    let us = nix::unistd::getuid().as_raw();
    if meta.uid() != us {
        anyhow::bail!(
            "{} is owned by uid {}, not {}; refusing to use it",
            sock.display(),
            meta.uid(),
            us
        );
    }
    Ok(())
}

fn spawn_server() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .arg("server")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}
