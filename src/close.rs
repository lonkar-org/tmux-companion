//! Closing a project session by letting every window exit on its own.
//!
//! Not `kill-session`. That sends `SIGHUP`, and nvim answers a hangup by
//! preserving its swap file and dying, which is what leaves a swap directory
//! full of `%Users%…swp` entries greeting you on the next open. Quitting nvim
//! the way a person would removes the swap file instead, and once the last
//! window is gone tmux drops the session by itself.

/// One pane, as tmux reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    /// tmux's pane id.
    pub id: String,
    /// The command running in it.
    pub command: String,
}

/// Parse `list-panes -F '#{pane_id} #{pane_current_command}'`.
pub fn parse_panes(listing: &str) -> Vec<Pane> {
    listing
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let (id, command) = l.trim().split_once(' ')?;
            Some(Pane {
                id: id.to_string(),
                command: command.trim().to_string(),
            })
        })
        .collect()
}

/// The panes running an editor, which have to be asked to quit first.
///
/// Only nvim, because it is the one that leaves state behind.
pub fn editors(panes: &[Pane]) -> Vec<&Pane> {
    panes.iter().filter(|p| p.command == "nvim").collect()
}

/// What to send to a pane that is not an editor.
///
/// A shell gets `exit`, and everything else gets ctrl-D. Matching on what a
/// pane is not rather than on what it is, because claude renames its own
/// process to its version string and there is no list of names to check
/// against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Farewell {
    /// Clear the line, then `exit`.
    ShellExit,
    /// Ctrl-D.
    EndOfFile,
    /// Nothing: an editor is handled separately.
    Skip,
}

/// How to say goodbye to a pane.
pub fn farewell(command: &str) -> Farewell {
    match command {
        "zsh" | "bash" | "sh" | "fish" => Farewell::ShellExit,
        "nvim" => Farewell::Skip,
        _ => Farewell::EndOfFile,
    }
}

/// The command that quits nvim.
///
/// `:xa` rather than `:qa!`: unsaved work written to a file is recoverable from
/// git and unsaved work discarded is not. If a buffer cannot be written the
/// quit fails, nvim stays up, and the caller stops without closing anything so
/// the reason is still on screen.
pub fn quit_command(discard: bool) -> &'static str {
    if discard { ":qa!" } else { ":xa" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panes() -> Vec<Pane> {
        parse_panes("%1 nvim\n%2 zsh\n%3 2.1.278\n%4 bash\n")
    }

    #[test]
    fn panes_are_parsed_into_ids_and_commands() {
        let p = panes();
        assert_eq!(p.len(), 4);
        assert_eq!(p[0].id, "%1");
        assert_eq!(p[0].command, "nvim");
    }

    #[test]
    fn a_blank_listing_is_no_panes_rather_than_an_error() {
        assert!(parse_panes("").is_empty());
        assert!(parse_panes("\n\n").is_empty());
    }

    #[test]
    fn only_the_editors_are_asked_to_quit_first() {
        let p = panes();
        let e = editors(&p);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].id, "%1");
    }

    #[test]
    fn a_shell_is_asked_to_exit_and_everything_else_gets_ctrl_d() {
        assert_eq!(farewell("zsh"), Farewell::ShellExit);
        assert_eq!(farewell("bash"), Farewell::ShellExit);
        assert_eq!(farewell("nvim"), Farewell::Skip);
        // claude renames itself to its version string, which is why this
        // matches on what a pane is not.
        assert_eq!(farewell("2.1.278"), Farewell::EndOfFile);
        assert_eq!(farewell("python"), Farewell::EndOfFile);
    }

    #[test]
    fn quitting_writes_unless_told_to_discard() {
        assert_eq!(quit_command(false), ":xa");
        assert_eq!(quit_command(true), ":qa!");
    }
}
