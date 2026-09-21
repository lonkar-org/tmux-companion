mod cache;
mod client;
mod preview;
mod proto;
mod segments;
mod server;
mod tmux;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use proto::Request;

#[derive(Parser)]
#[command(name = "tmux-companion", about = "Singleton tmux status server")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
#[command(rename_all = "kebab-case")]
enum Cmd {
    /// Run as persistent background server
    Server,

    /// Git status segment
    Gst {
        /// Path to git repository (defaults to current directory)
        path: Option<PathBuf>,
        pane_pid: Option<u32>,
        /// Bypass the cache and force a fresh git status fetch
        #[arg(short = 'f', long, action = clap::ArgAction::SetTrue)]
        force: bool,
        /// Color style: fill (solid background), outline, outline-bright
        #[arg(short = 's', long, default_value = "outline-bright")]
        style: String,
        /// Omit the trailing end-cap glyph (for use at the start of status-right)
        #[arg(long, action = clap::ArgAction::SetTrue)]
        no_cap: bool,
        /// Middle-ellipsize the branch name when longer than this (default 20)
        #[arg(long)]
        branch_max_len: Option<usize>,
        /// Show the git glyph before the branch name (off by default)
        #[arg(long, action = clap::ArgAction::SetTrue)]
        branch_icon: bool,
        /// Seconds a cached status stays fresh (0 disables the cache)
        #[arg(long, default_value = "5")]
        ttl: f64,
    },

    /// Whole right-hand status side in one call: git status, bandwidth and
    /// battery, computed concurrently and returned with the tmux literals that
    /// used to sit between them in the config.
    StatusRight {
        /// Path to the current pane's directory (defaults to current directory)
        path: Option<PathBuf>,
        /// Color style: fill (solid background), outline, outline-bright
        #[arg(short = 's', long, default_value = "outline-bright")]
        style: String,
        /// Middle-ellipsize the branch name when longer than this (default 20)
        #[arg(long)]
        branch_max_len: Option<usize>,
        /// Show the git glyph before the branch name (off by default)
        #[arg(long, action = clap::ArgAction::SetTrue)]
        branch_icon: bool,
        /// Bypass the git cache and force a fresh git status fetch
        #[arg(short = 'f', long, action = clap::ArgAction::SetTrue)]
        force: bool,
        /// Seconds a cached git status stays fresh (0 disables the cache).
        /// Applies to the git segment only -- bandwidth is always live.
        #[arg(long, default_value = "5")]
        ttl: f64,
    },

    /// Print sample git segments in every color style (local, no server)
    Preview,

    /// Battery status segment
    Battery,

    /// Network bandwidth segment
    Net,

    /// Multi-client indicator segment
    Clients {
        session_attached: u32,
        window_active_clients: u32,
    },

    /// Background nvim indicator segment
    VimBg { pane_pid: u32 },

    /// Window status segment
    Window {
        #[arg(short = 'c', action = clap::ArgAction::SetTrue)]
        current: bool,
        #[arg(short = 'i')]
        index: u32,
        #[arg(short = 'I')]
        window_id: Option<String>,
        #[arg(short = 'n', default_value = "")]
        name: String,
        #[arg(short = 'w')]
        path: Option<PathBuf>,
        #[arg(short = 'p', default_value = "")]
        process: String,
        #[arg(short = 's')]
        start_path: Option<PathBuf>,
        #[arg(short = 'f', default_value = "")]
        flags: String,
        #[arg(short = 'l', default_value = "0")]
        last: u32,
        /// Number of panes in the window (#{window_panes})
        #[arg(short = 'P', default_value = "1")]
        pane_count: u32,
        /// Index of the window's active pane (#{pane_index})
        #[arg(short = 'A', default_value = "0")]
        pane_index: u32,
    },

    /// Bare client round trip with no server work, for benchmarking.
    #[command(hide = true)]
    Noop,
}

/// Parse first, then build the smallest runtime the chosen subcommand needs.
///
/// `#[tokio::main]` used to stand up a multi-threaded runtime -- one worker
/// thread per core, sixteen on this machine -- before clap had even looked at
/// the arguments, on every one of the client invocations tmux makes every
/// second.  A client does one connect, one write and one read; a
/// `current_thread` runtime serves that exactly as well for 3.5 ms less CPU per
/// spawn.  The server keeps the multi-threaded runtime: it fans segments out
/// across `spawn_blocking` and `tokio::join!`.
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let runtime = if matches!(cli.command, Cmd::Server) {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
    };

    runtime.block_on(run(cli.command))
}

async fn run(command: Cmd) -> anyhow::Result<()> {
    match command {
        Cmd::Server => {
            server::run().await?;
        }
        Cmd::Gst {
            path,
            pane_pid,
            force,
            style,
            no_cap,
            branch_max_len,
            branch_icon,
            ttl,
        } => {
            let req = Request {
                cmd: "gst".into(),
                args: serde_json::json!({
                    "path": path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    "pane_pid": pane_pid,
                    "force": force,
                    "style": style,
                    "no_cap": no_cap,
                    "branch_max_len": branch_max_len,
                    "branch_icon": branch_icon,
                    "ttl_secs": ttl,
                }),
            };
            client::send_and_print(req).await?;
        }
        Cmd::StatusRight {
            path,
            style,
            branch_max_len,
            branch_icon,
            force,
            ttl,
        } => {
            let req = Request {
                cmd: "status-right".into(),
                args: serde_json::json!({
                    "path": path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    "style": style,
                    "branch_max_len": branch_max_len,
                    "branch_icon": branch_icon,
                    "force": force,
                    "ttl_secs": ttl,
                }),
            };
            client::send_and_print(req).await?;
        }
        Cmd::Preview => {
            print!("{}", preview::render());
        }
        Cmd::Battery => {
            client::send_and_print(Request {
                cmd: "battery".into(),
                args: serde_json::Value::Object(Default::default()),
            })
            .await?;
        }
        Cmd::Net => {
            client::send_and_print(Request {
                cmd: "net".into(),
                args: serde_json::Value::Object(Default::default()),
            })
            .await?;
        }
        Cmd::Clients {
            session_attached,
            window_active_clients,
        } => {
            client::send_and_print(Request {
                cmd: "clients".into(),
                args: serde_json::json!({
                    "session_attached": session_attached,
                    "window_active_clients": window_active_clients,
                }),
            })
            .await?;
        }
        Cmd::VimBg { pane_pid } => {
            client::send_and_print(Request {
                cmd: "vim-bg".into(),
                args: serde_json::json!({ "pane_pid": pane_pid }),
            })
            .await?;
        }
        Cmd::Window {
            current,
            index,
            window_id,
            name,
            path,
            process,
            start_path,
            flags,
            last,
            pane_count,
            pane_index,
        } => {
            let name_opt = if name.is_empty() { None } else { Some(name) };
            let proc_opt = if process.is_empty() {
                None
            } else {
                Some(process)
            };
            client::send_and_print(Request {
                cmd: "window".into(),
                args: serde_json::json!({
                    "current": current,
                    "index": index,
                    "window_id": window_id,
                    "name": name_opt,
                    "path": path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    "process": proc_opt,
                    "start_path": start_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    "flags": flags,
                    "last": last,
                    "pane_count": pane_count,
                    "pane_index": pane_index,
                }),
            })
            .await?;
        }
        Cmd::Noop => {
            client::send_and_print(Request {
                cmd: "noop".into(),
                args: serde_json::Value::Object(Default::default()),
            })
            .await?;
        }
    }

    Ok(())
}
