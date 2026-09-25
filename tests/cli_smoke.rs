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

// ── the sessions surface ─────────────────────────────────────────────────────
//
// Every flag here is one somebody types from a boot script or a key binding,
// where a rename or a changed default shows up as a command that silently does
// something else. Parsing is the cheapest place to pin that down.

#[test]
fn sessions_save_takes_its_two_flags() {
    use tmux_companion::cli::SessionsAction;
    let cli = Cli::parse_from([
        "tmux-companion",
        "sessions",
        "save",
        "--skip-pane-history",
        "--exclude",
        "y,scratch",
    ]);
    match cli.command {
        Cmd::Sessions {
            action:
                SessionsAction::Save {
                    skip_pane_history,
                    exclude,
                },
        } => {
            assert!(skip_pane_history);
            // Comma separated, because a binding writes one word and a shell
            // would otherwise need quoting somebody will get wrong.
            assert_eq!(exclude, vec!["y".to_string(), "scratch".to_string()]);
        }
        other => panic!("parsed as {other:?}"),
    }
}

#[test]
fn sessions_resurrect_takes_a_generation_and_every_flag() {
    use tmux_companion::cli::SessionsAction;
    let cli = Cli::parse_from([
        "tmux-companion",
        "sessions",
        "resurrect",
        "20260925T090000",
        "--only",
        "alpha,beta",
        "--exclude",
        "y",
        "--merge",
        "--dry-run",
        "--yes",
        "--detach",
    ]);
    match cli.command {
        Cmd::Sessions {
            action:
                SessionsAction::Resurrect {
                    stamp,
                    only,
                    exclude,
                    merge,
                    dry_run,
                    yes,
                    detach,
                },
        } => {
            assert_eq!(stamp.as_deref(), Some("20260925T090000"));
            assert_eq!(only, vec!["alpha".to_string(), "beta".to_string()]);
            assert_eq!(exclude, vec!["y".to_string()]);
            assert!(merge && dry_run && yes && detach);
        }
        other => panic!("parsed as {other:?}"),
    }
}

#[test]
fn a_resurrect_with_no_generation_means_the_newest() {
    use tmux_companion::cli::SessionsAction;
    let cli = Cli::parse_from(["tmux-companion", "sessions", "resurrect"]);
    match cli.command {
        Cmd::Sessions {
            action: SessionsAction::Resurrect { stamp, merge, .. },
        } => {
            assert_eq!(stamp, None);
            // Refusing a busy server is the default and merging is the flag.
            assert!(!merge);
        }
        other => panic!("parsed as {other:?}"),
    }
}

#[test]
fn shutdown_and_restart_default_their_daemon_flags_opposite_ways() {
    use tmux_companion::cli::SessionsAction;
    // The asymmetry is deliberate and easy to reverse by accident: a restart
    // that left the daemon running would hand back a new binary holding the
    // configuration it started with.
    let down = Cli::parse_from(["tmux-companion", "sessions", "shutdown"]);
    match down.command {
        Cmd::Sessions {
            action: SessionsAction::Shutdown { daemon_too, .. },
        } => assert!(!daemon_too, "shutdown should leave the daemon alone"),
        other => panic!("parsed as {other:?}"),
    }
    let up = Cli::parse_from(["tmux-companion", "sessions", "restart"]);
    match up.command {
        Cmd::Sessions {
            action: SessionsAction::Restart { keep_daemon, .. },
        } => assert!(!keep_daemon, "restart should bounce the daemon"),
        other => panic!("parsed as {other:?}"),
    }
}

#[test]
fn sessions_list_and_show_both_speak_json() {
    use tmux_companion::cli::SessionsAction;
    let list = Cli::parse_from(["tmux-companion", "sessions", "list", "--json"]);
    assert!(matches!(
        list.command,
        Cmd::Sessions {
            action: SessionsAction::List { json: true }
        }
    ));
    let show = Cli::parse_from(["tmux-companion", "sessions", "show", "20260925T090000"]);
    match show.command {
        Cmd::Sessions {
            action: SessionsAction::Show { stamp, json },
        } => {
            assert_eq!(stamp.as_deref(), Some("20260925T090000"));
            assert!(!json);
        }
        other => panic!("parsed as {other:?}"),
    }
}

#[test]
fn the_daemon_has_its_own_shutdown_and_restart() {
    assert!(matches!(
        Cli::parse_from(["tmux-companion", "shutdown"]).command,
        Cmd::Shutdown
    ));
    assert!(matches!(
        Cli::parse_from(["tmux-companion", "restart"]).command,
        Cmd::Restart
    ));
}

#[test]
fn project_close_is_where_closing_a_project_lives_now() {
    use tmux_companion::cli::ProjectAction;
    let cli = Cli::parse_from(["tmux-companion", "project", "close", "alpha", "--discard"]);
    match cli.command {
        Cmd::Project {
            action:
                Some(ProjectAction::Close {
                    session,
                    discard,
                    no_save,
                }),
            ..
        } => {
            assert_eq!(session.as_deref(), Some("alpha"));
            assert!(discard);
            assert!(!no_save);
        }
        other => panic!("parsed as {other:?}"),
    }
}

#[test]
fn the_old_close_project_name_still_parses() {
    // Hidden from help, and kept for one release because it is in at least one
    // tmux.conf. A rename that broke somebody's binding silently would be the
    // whole reason to have this test.
    assert!(matches!(
        Cli::parse_from(["tmux-companion", "close-project"]).command,
        Cmd::CloseProject { .. }
    ));
}
