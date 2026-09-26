//! One module per thing the status bar can draw.
/// How many coding agents are running, and how many are waiting.
pub mod agents;
/// Battery percentage and icon.
pub mod battery;
/// How many other clients are attached.
pub mod clients;
/// Git status.
pub mod git;
/// Bandwidth rate.
pub mod network;
/// Jobs stopped or running under a pane.
pub mod sh_jobs;
/// Suspended-editor marker. Superseded by [`sh_jobs`].
pub mod vim_bg;
/// Window status, with path abbreviation and per-directory icons.
pub mod window;
