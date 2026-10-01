//! keys that tmux and the apps in its panes both want: what each layer binds, and where they collide
//!
//! The design, the routing rule and the pane-option contract are in
//! `docs/dev/design-key-routing.md`. Discovery, the report and the router all
//! run in the client; the daemon has no part in any of it.

pub mod apps;
pub mod collide;
pub mod discover;
pub mod route;
pub mod spell;
