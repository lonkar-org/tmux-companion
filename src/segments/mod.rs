//! One module per thing the status bar can draw.
/// Battery percentage and icon.
pub mod battery;
/// How many other clients are attached.
pub mod clients;
/// Git status.
pub mod git;
/// Bandwidth rate.
pub mod network;
/// Suspended-editor marker.
pub mod vim_bg;
/// Window status, with path abbreviation and per-directory icons.
pub mod window;
