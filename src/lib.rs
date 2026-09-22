//! tmux-companion: one binary in two modes.
//!
//! `server` binds a Unix socket and answers requests forever; every other
//! subcommand is a client that connects, writes one JSON line, reads one back
//! and exits. The library exists so `tests/` can link against the same code
//! the binary runs, which a `main.rs`-only crate cannot offer.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// Timestamped map with a reader-supplied TTL.
pub mod cache;
/// The command-line surface and its dispatch.
pub mod cli;
/// Talking to the server over the socket.
pub mod client;
/// The configuration file.
pub mod config;
/// What to ask somebody to run before they open an issue.
pub mod doctor;
/// Rendering samples of every style, for eyeballing.
pub mod preview;
/// The JSON request and response, and one args struct per command.
pub mod proto;
/// The things the status bar can draw.
pub mod segments;
/// The daemon.
pub mod server;
/// tmux's own formatting language.
pub mod tmux;
