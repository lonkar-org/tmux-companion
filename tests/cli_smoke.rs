//! The first integration test, and mostly a proof that one is now possible:
//! before `src/lib.rs` existed nothing outside the binary could link to this
//! code at all. The socket round trip and the singleton belong in phase 0
//! step 5; this file only checks that the CLI surface the tests will drive is
//! reachable and that clap's own invariants hold.

use clap::{CommandFactory, Parser};
use tmux_companion::cli::{Cli, Cmd};

#[test]
fn clap_definition_is_internally_consistent() {
    // Catches duplicate flags, conflicting short options and malformed help
    // strings at test time rather than on somebody's first run.
    Cli::command().debug_assert();
}

#[test]
fn server_subcommand_parses() {
    let cli = Cli::parse_from(["tmux-companion", "server"]);
    assert!(matches!(cli.command, Cmd::Server));
}

#[test]
fn gst_takes_an_optional_path_and_a_ttl() {
    let cli = Cli::parse_from(["tmux-companion", "gst", "/tmp/repo", "--ttl", "0"]);
    match cli.command {
        Cmd::Gst { path, ttl, .. } => {
            assert_eq!(path.as_deref(), Some(std::path::Path::new("/tmp/repo")));
            // `--ttl 0` has to survive parsing as a real zero: it is the flag
            // that means "never serve this from cache".
            assert_eq!(ttl, 0.0);
        }
        other => panic!("expected Gst, got {other:?}"),
    }
}

#[test]
fn an_unknown_subcommand_is_an_error_not_a_default() {
    // Deliberately a name no subcommand will ever take. This test has gone
    // stale twice by naming something the port then implemented: first `keys`,
    // then `cheatsheet`.
    assert!(Cli::try_parse_from(["tmux-companion", "not-a-subcommand-and-never-will-be"]).is_err());
}
