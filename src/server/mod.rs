mod handlers;
pub mod state;

use std::sync::Arc;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
    sync::Mutex,
};

use crate::{client::sock_path, proto::Request, server::state::ServerState};

pub async fn run() -> anyhow::Result<()> {
    let sock = sock_path();

    // If an existing server is accepting connections, exit quietly.
    if tokio::net::UnixStream::connect(&sock).await.is_ok() {
        return Ok(());
    }

    // Remove stale socket file from a previous crashed run.
    let _ = std::fs::remove_file(&sock);

    let listener = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Another server raced us to the bind — exit quietly.
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    let state = Arc::new(Mutex::new(ServerState::new()));

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
