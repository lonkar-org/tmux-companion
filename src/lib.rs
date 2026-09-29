//! tmux-companion: one binary in two modes.
//!
//! `server` binds a Unix socket and answers requests forever; every other
//! subcommand is a client that connects, writes one JSON line, reads one back
//! and exits. The library exists so `tests/` can link against the same code
//! the binary runs, which a `main.rs`-only crate cannot offer.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// Fetching repositories in the background, so ahead and behind mean something.
/// What an agent says about itself, and the hooks that make it say so.
pub mod agent;
pub mod autofetch;
/// Sourcing tmux config when it changes on disk.
pub mod autoreload;
/// Timestamped map with a reader-supplied TTL.
pub mod brief;
pub mod cache;
/// The cheat sheet of hand-written bindings.
pub mod cheatsheet;
/// The command-line surface and its dispatch.
pub mod cli;
/// Closing a project session politely.
pub mod click;
/// Talking to the server over the socket.
pub mod client;
pub mod close;
/// The configuration file.
pub mod config;
/// Where the project picker's directory list comes from.
pub mod dirsource;
/// What to ask somebody to run before they open an issue.
pub mod doctor;
/// Key bindings, parsed out of tmux.
pub mod inbox;
pub mod journal;
pub mod keys;
/// Stopping what hangs in a pane: TERM, a wait, then KILL.
pub mod kill;

/// Running a segment in this process, with no daemon.
pub mod local;
/// Announcing a long command that finished out of sight.
pub mod note;
pub mod notify;
/// Whether the network is there, for the health mark.
pub mod online;
/// Opening a URL or file found in text.
pub mod open;
/// Every pane on the server, as a list to jump from.
pub mod panes;
pub mod picker;
/// The fuzzy picker and its state machine.
/// A pocket pane: a shell pulled out beside the editor and put away again.
pub mod pocket;
/// Who is listening on which port, and which pane started it.
pub mod ports;
/// Rendering samples of every style, for eyeballing.
pub mod preview;
/// Asking the terminal what it does.
pub mod probe;
/// Projects: one session each.
pub mod project;
/// Giving a pane a session of its own.
pub mod promote;
/// The JSON request and response, and one args struct per command.
pub mod proto;
/// Running a command from history in a side pane.
pub mod quiet;
/// The layout a checkout carries with it, and the trust it needs.
pub mod repofile;
/// What a restore will run in each pane, decided before anything runs.
pub mod restore;
pub mod run;
/// Per-project layouts captured from a live session.
pub mod saved;
/// Searching the scrollback of every pane at once.
pub mod search;
/// The things the status bar can draw.
pub mod segments;
/// The daemon.
pub mod server;
/// Snapshots of the whole server, in generations.
pub mod sessions;
/// The checklist of what the tool offers and a way to add what is missing.
pub mod setup;
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
