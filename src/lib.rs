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
/// The cheat sheet of hand-written bindings.
pub mod cheatsheet;
/// The command-line surface and its dispatch.
pub mod cli;
/// Talking to the server over the socket.
pub mod client;
/// The configuration file.
pub mod config;
/// What to ask somebody to run before they open an issue.
pub mod doctor;
/// Key bindings, parsed out of tmux.
pub mod keys;
/// The fuzzy picker and its state machine.
pub mod picker;
/// Rendering samples of every style, for eyeballing.
pub mod preview;
/// Projects: one session each.
pub mod project;
/// The JSON request and response, and one args struct per command.
pub mod proto;
/// Running a command from history in a side pane.
pub mod run;
/// The things the status bar can draw.
pub mod segments;
/// The daemon.
pub mod server;
/// Timed work and small tmux commands.
pub mod tasks;
/// Theme colour arithmetic and the files it writes.
pub mod theme;
/// tmux's own formatting language.
pub mod tmux;
