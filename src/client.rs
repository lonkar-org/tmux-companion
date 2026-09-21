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
    let stream = connect_with_retry().await?;
    let (reader, mut writer) = stream.into_split();

    let mut msg = serde_json::to_string(&req)?;
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
    anyhow::bail!("server failed to start after retries")
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
