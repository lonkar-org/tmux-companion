//! tmux-companion: one binary in two modes.
//!
//! `server` binds a Unix socket and answers requests forever; every other
//! subcommand is a client that connects, writes one JSON line, reads one back
//! and exits. The library exists so `tests/` can link against the same code
//! the binary runs, which a `main.rs`-only crate cannot offer.

pub mod cache;
pub mod cli;
pub mod client;
pub mod preview;
pub mod proto;
pub mod segments;
pub mod server;
pub mod tmux;
