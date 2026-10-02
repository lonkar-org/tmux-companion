//! One module per thing the status bar can draw.
//!
//! Two are library: [`git`], which parses and renders git status, and
//! [`network`], the arithmetic behind a transfer rate. The rest read the
//! daemon's own state and are the binary's.
#[doc(hidden)]
/// How many coding agents are running, and how many are waiting.
pub mod agents;
#[doc(hidden)]
/// Battery percentage and icon.
pub mod battery;
#[doc(hidden)]
/// How many other clients are attached.
pub mod clients;
pub mod git;
#[doc(hidden)]
pub mod health;
pub mod network;
#[doc(hidden)]
/// Jobs stopped or running under a pane.
pub mod sh_jobs;
#[doc(hidden)]
/// Suspended-editor marker. Superseded by [`sh_jobs`].
pub mod vim_bg;
#[doc(hidden)]
/// Window status, with path abbreviation and per-directory icons.
pub mod window;
