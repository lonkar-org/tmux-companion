//! keys that tmux and the apps in its panes both want: what each layer binds, and where they collide
//!
//! The design, the routing rule and the pane-option contract are in
//! `docs/dev/design-key-routing.md`. Discovery, the report and the router all
//! run in the client; the daemon has no part in any of it.
//!
//! The library part is [`spell`]: key names from nvim, fzf, zsh, nano and
//! Claude Code, written the way tmux writes them, so two programs' bindings
//! can be compared at all.

#[doc(hidden)]
pub mod apps;
#[doc(hidden)]
pub mod collide;
#[doc(hidden)]
pub mod discover;
#[doc(hidden)]
pub mod route;
pub mod spell;
