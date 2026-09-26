//! tmux-companion: one binary in two modes.
//!
//! `server` binds a Unix socket and answers requests forever; every other
//! subcommand is a client that connects, writes one JSON line, reads one back
//! and exits. The library exists so `tests/` can link against the same code
//! the binary runs, which a `main.rs`-only crate cannot offer.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// Fetching repositories in the background, so ahead and behind mean something.
pub mod autofetch;
/// Sourcing tmux config when it changes on disk.
pub mod autoreload;
/// Timestamped map with a reader-supplied TTL.
pub mod cache;
/// The cheat sheet of hand-written bindings.
pub mod cheatsheet;
/// The command-line surface and its dispatch.
pub mod cli;
/// Talking to the server over the socket.
pub mod client;
/// Closing a project session politely.
pub mod close;
/// The configuration file.
pub mod config;
/// Where the project picker's directory list comes from.
pub mod dirsource;
/// What to ask somebody to run before they open an issue.
pub mod doctor;
/// Key bindings, parsed out of tmux.
pub mod keys;

/// Running a segment in this process, with no daemon.
pub mod local;
/// Announcing a long command that finished out of sight.
pub mod notify;
/// Opening a URL or file found in text.
pub mod open;
/// Every pane on the server, as a list to jump from.
pub mod panes;
/// The fuzzy picker and its state machine.
pub mod picker;
/// Rendering samples of every style, for eyeballing.
pub mod preview;
/// Asking the terminal what it does.
pub mod probe;
/// Projects: one session each.
pub mod project;
/// The JSON request and response, and one args struct per command.
pub mod proto;
/// What a restore will run in each pane, decided before anything runs.
pub mod restore;
/// Running a command from history in a side pane.
pub mod run;
/// Per-project layouts captured from a live session.
pub mod saved;
/// The things the status bar can draw.
pub mod segments;
/// The daemon.
pub mod server;
/// Snapshots of the whole server, in generations.
pub mod sessions;
/// The OSC 133 prompt marks and the shell code that emits them.
pub mod shell;
/// Timed work and small tmux commands.
pub mod tasks;
/// Theme colour arithmetic and the files it writes.
pub mod theme;
/// tmux's own formatting language.
pub mod tmux;
/// Naming windows after what is running in them.
pub mod window_names;
