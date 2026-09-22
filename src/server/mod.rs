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

/// Bind the socket and serve until killed.
///
/// Exits quietly, and successfully, if another server is already listening:
/// every client invocation tries to start one, so losing that race is the
/// ordinary case rather than an error.
pub async fn run() -> anyhow::Result<()> {
    let sock = sock_path();

    // If an existing server is accepting connections, exit quietly.
    if tokio::net::UnixStream::connect(&sock).await.is_ok() {
        return Ok(());
    }

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
            // Another server raced us to the bind — exit quietly.
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    // Parsed once, here, so a broken config stops the daemon starting rather
    // than being discovered segment by segment. The error goes to stderr and to
    // the state file, which is where a client looks when its own start attempt
    // produced no server.
    let config = match crate::config::load() {
        Ok((c, _)) => c,
        Err(e) => {
            record_config_error(&e);
            return Err(anyhow::anyhow!("{e}"));
        }
    };

    // Started before the accept loop, so it runs for as long as the daemon
    // does and stops when it stops. That is the whole of the lifetime
    // management the zsh version needed a PID lock file for.
    if config.autosave.enabled {
        let home = std::env::var("HOME").unwrap_or_default();
        let script = config.autosave.script_path(&home);
        let interval = std::time::Duration::from_secs(config.autosave.interval_secs.max(1));
        tokio::spawn(crate::tasks::autosave_loop(script, interval));
    }

    let state = Arc::new(Mutex::new(ServerState::with_config(config)));

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
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("last-error"), format!("{e}\n"));
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
