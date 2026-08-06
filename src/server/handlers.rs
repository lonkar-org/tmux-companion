use std::{sync::Arc, time::Duration};

use tokio::sync::Mutex;

use crate::{
    proto::{Request, Response},
    segments,
    server::state::ServerState,
};

pub async fn dispatch(req: Request, state: Arc<Mutex<ServerState>>) -> Response {
    let result = match req.cmd.as_str() {
        "gst" => {
            let path = req.args["path"].as_str().map(std::path::PathBuf::from);
            let pid = req.args["pane_pid"].as_u64().map(|n| n as u32);
            let force = req.args["force"].as_bool().unwrap_or(false);
            let style = req.args["style"]
                .as_str()
                .and_then(crate::tmux::format::Style::parse)
                .unwrap_or_default();
            let no_cap = req.args["no_cap"].as_bool().unwrap_or(false);
            let branch_max_len = req.args["branch_max_len"].as_u64().map(|n| n as usize);
            segments::git::render(path, pid, force, style, no_cap, branch_max_len).await
        }
        "battery" => {
            const TTL: Duration = Duration::from_secs(30);
            async {
                let cached = {
                    let st = state.lock().await;
                    st.battery_cache
                        .as_ref()
                        .filter(|(_, t)| t.elapsed() < TTL)
                        .map(|(s, _)| s.clone())
                };
                if let Some(s) = cached {
                    Ok(s)
                } else {
                    let s = segments::battery::render().await?;
                    // Don't cache while charging — the percentage changes every second.
                    if !s.contains(segments::battery::CHARGING_ICON) {
                        state.lock().await.battery_cache =
                            Some((s.clone(), std::time::Instant::now()));
                    }
                    Ok(s)
                }
            }
            .await
        }
        "net" => {
            let mut st = state.lock().await;
            segments::network::render(&mut st.net_previous).await
        }
        "clients" => {
            let sa = req.args["session_attached"].as_u64().unwrap_or(0) as u32;
            let wac = req.args["window_active_clients"].as_u64().unwrap_or(0) as u32;
            segments::clients::render(sa, wac).await
        }
        "vim-bg" => {
            let pid = req.args["pane_pid"].as_u64().unwrap_or(0) as u32;
            segments::vim_bg::render(pid).await
        }
        "window" => {
            let dir_aliases = state.lock().await.dir_aliases.clone();
            match serde_json::from_value::<segments::window::WindowArgs>(req.args) {
                Ok(args) => Ok(segments::window::render(&args, &dir_aliases)),
                Err(e) => Err(anyhow::anyhow!("invalid window args: {}", e)),
            }
        }
        other => Err(anyhow::anyhow!("unknown command: {}", other)),
    };

    match result {
        Ok(output) => Response::ok(output),
        Err(e) => Response::err(e),
    }
}
