//! tmux-companion: one daemon that draws the tmux status bar, and the tools
//! around a tmux server that a plugin manager used to provide.
//!
//! Most people want the binary:
//!
//! ```sh
//! cargo install --locked tmux-companion
//! tmux-companion doctor
//! ```
//!
//! and the manual, `man tmux-companion`, or the
//! [README](https://github.com/lonkar-org/tmux-companion#readme) for what it
//! does.
//!
//! # The library
//!
//! The same crate is a library, for a program that wants a part of what the
//! binary does without running the binary for it. Three parts are meant to be
//! used from outside, and the documentation shows only those:
//!
//! - **Asking the daemon.** [`daemon::Daemon`] sends one request to a running
//!   `tmux-companion server` and returns its answer, with the request and
//!   response types and each command's arguments in [`proto`]. It's
//!   blocking, needs no async runtime, and never starts or replaces a daemon.
//! - **Parsing and drawing.** [`segments::git`] parses `git status
//!   --porcelain=v2 --branch` into [`segments::git::GitStatus`] and renders it
//!   as a segment; [`tmux::format`] builds tmux style strings and turns them
//!   into ANSI for a terminal; [`segments::network`] is the arithmetic behind
//!   a transfer rate; [`keyroute::spell`] translates key names from nvim, fzf,
//!   zsh, nano and Claude Code into tmux's spelling. None of these touch tmux
//!   or a socket.
//! - **Session snapshots.** [`sessions::Snapshot`] is a whole tmux server as
//!   data: [`sessions::capture`] builds one from tmux's own listings,
//!   [`sessions::render`] and [`sessions::parse`] write and read the file
//!   format, [`sessions::store`] keeps generations of them,
//!   [`sessions::portable`] moves one between machines, and
//!   [`sessions::import`] reads what tmux-resurrect saved.
//!
//! ```
//! use tmux_companion::segments::git::GitStatus;
//!
//! let porcelain = "\
//! ## branch.oid 1f2e3d4c5b6a
//! ## branch.head main
//! ## branch.upstream origin/main
//! ## branch.ab +2 -0
//! 1 .M N... 100644 100644 100644 aaaa bbbb src/lib.rs
//! ? notes.txt
//! ";
//! let status = GitStatus::parse_porcelain_v2(porcelain);
//! assert_eq!(status.branch, "main");
//! assert_eq!(status.ahead, 2);
//! assert_eq!(status.unstaged.modified, 1);
//! assert_eq!(status.untracked, 1);
//! ```
//!
//! # Stability
//!
//! The parts above follow semver: before 1.0 a breaking change to them bumps
//! the minor version and has a line in the changelog. Everything else in the
//! crate is the binary's own code. It's public so the integration tests can
//! link against it, it's hidden from these docs and it changes whenever the
//! binary needs it to, in any release.
//!
//! The library pulls in everything the binary uses, tokio and ratatui
//! included; there are no feature flags to trim it yet.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

// ── The library ─────────────────────────────────────────────────────────────

pub mod daemon;
pub mod keyroute;
pub mod proto;
pub mod segments;
pub mod sessions;
pub mod tmux;

// ── The binary's own code: public for tests/, hidden, no semver promise ────

#[doc(hidden)]
pub mod agent;
#[doc(hidden)]
pub mod autofetch;
#[doc(hidden)]
pub mod autoreload;
#[doc(hidden)]
pub mod brief;
#[doc(hidden)]
pub mod cache;
#[doc(hidden)]
pub mod cheatsheet;
#[doc(hidden)]
pub mod chunk;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod click;
#[doc(hidden)]
pub mod client;
#[doc(hidden)]
pub mod close;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod dirsource;
#[doc(hidden)]
pub mod doctor;
#[doc(hidden)]
pub mod earcons;
#[doc(hidden)]
pub mod inbox;
#[doc(hidden)]
pub mod journal;
#[doc(hidden)]
pub mod jump;
#[doc(hidden)]
pub mod keys;
#[doc(hidden)]
pub mod kill;
#[doc(hidden)]
pub mod local;
#[doc(hidden)]
pub mod note;
#[doc(hidden)]
pub mod notify;
#[doc(hidden)]
pub mod online;
#[doc(hidden)]
pub mod open;
#[doc(hidden)]
pub mod panes;
#[doc(hidden)]
pub mod picker;
#[doc(hidden)]
pub mod pocket;
#[doc(hidden)]
pub mod ports;
#[doc(hidden)]
pub mod preview;
#[doc(hidden)]
pub mod probe;
#[doc(hidden)]
pub mod project;
#[doc(hidden)]
pub mod promote;
#[doc(hidden)]
pub mod quiet;
#[doc(hidden)]
pub mod repofile;
#[doc(hidden)]
pub mod restore;
#[doc(hidden)]
pub mod run;
#[doc(hidden)]
pub mod saved;
#[doc(hidden)]
pub mod search;
#[doc(hidden)]
pub mod server;
#[doc(hidden)]
pub mod setup;
#[doc(hidden)]
pub mod shell;
#[doc(hidden)]
pub mod tasks;
#[doc(hidden)]
pub mod theme;
#[doc(hidden)]
pub mod window_names;
