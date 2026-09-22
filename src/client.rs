//! The client half: find the socket, start a server if nothing answers, send
//! one JSON line and read one back.
use std::path::PathBuf;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::proto::{Request, Response};

/// Longest path a `sockaddr_un` can hold on macOS, minus the NUL terminator.
const SUN_PATH_MAX: usize = 103;

/// The socket path, or why the override cannot be used.
///
/// Separate from [`sock_path`] so the rule can be tested without a process
/// that exits.
pub fn resolve_sock_path(override_: Option<&std::ffi::OsStr>, uid: u32) -> Result<PathBuf, String> {
    // `TMUX_COMPANION_SOCK` puts a server beside the live one -- what the
    // measurements in BENCHMARKS.md use, so a benchmark run never touches the
    // status bar the user is actually looking at.
    if let Some(p) = override_ {
        if p.is_empty() {
            return Err("TMUX_COMPANION_SOCK is set but empty".to_string());
        }
        if p.len() > SUN_PATH_MAX {
            return Err(format!(
                "TMUX_COMPANION_SOCK is {} bytes and a unix socket address holds {SUN_PATH_MAX}.\n\
                 Refusing rather than falling back to the default socket: whoever set this \n\
                 variable wanted a server beside the live one, and quietly using the live one \n\
                 instead is the opposite of what they asked for.\n\
                 Try a shorter path, for example under /tmp.",
                p.len()
            ));
        }
        return Ok(PathBuf::from(p));
    }
    Ok(PathBuf::from(format!("/tmp/tmux-companion-{uid}.sock")))
}

/// The socket this process talks to, or would listen on.
///
/// Exits rather than falling back when the override is unusable. The fallback
/// this replaces printed a warning and then connected to the default socket,
/// which meant a test harness asking for an isolated server got the live one,
/// and every command it sent afterwards, including a shutdown, went there.
pub fn sock_path() -> PathBuf {
    match resolve_sock_path(
        std::env::var_os("TMUX_COMPANION_SOCK").as_deref(),
        nix::unistd::getuid().as_raw(),
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("tmux-companion: {e}");
            std::process::exit(2);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn no_override_is_the_default_socket_for_this_uid() {
        assert_eq!(
            resolve_sock_path(None, 501).unwrap(),
            PathBuf::from("/tmp/tmux-companion-501.sock")
        );
    }

    #[test]
    fn a_usable_override_is_used() {
        let p = OsStr::new("/tmp/mine.sock");
        assert_eq!(resolve_sock_path(Some(p), 501).unwrap(), PathBuf::from(p));
    }

    #[test]
    fn a_path_at_the_limit_is_still_usable() {
        let p: String = std::iter::repeat_n('a', SUN_PATH_MAX).collect();
        assert!(resolve_sock_path(Some(OsStr::new(&p)), 501).is_ok());
    }

    #[test]
    fn an_override_too_long_is_an_error_and_not_the_default_socket() {
        // This is the one that matters. The old behaviour warned and then
        // connected to the default socket, so a harness asking for an isolated
        // server silently got the live one and everything it sent afterwards,
        // including a shutdown, went there.
        let p: String = std::iter::repeat_n('a', SUN_PATH_MAX + 1).collect();
        let e = resolve_sock_path(Some(OsStr::new(&p)), 501).unwrap_err();
        assert!(e.contains("104 bytes"), "{e}");
        assert!(!e.contains("/tmp/tmux-companion-501.sock"), "{e}");
    }

    #[test]
    fn an_empty_override_is_an_error_rather_than_the_default() {
        // Empty usually means a variable that was meant to be set and was not,
        // and guessing the live socket from it is the same mistake.
        let e = resolve_sock_path(Some(OsStr::new("")), 501).unwrap_err();
        assert!(e.contains("empty"), "{e}");
    }
}
