//! The binary. Parses arguments, picks a runtime, and hands over to the
//! library; everything it used to hold lives in `cli.rs` so the tests can
//! reach it.

use clap::Parser;

use tmux_companion::cli::{Cli, Cmd, run};

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
