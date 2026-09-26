//! A one-line note on a pane, for the human who reads the bar and the picker.
//!
//! tmux already has the field: a pane's title, which `select-pane -T` sets and
//! `#{pane_title}` reads, drawn in the pane border when `pane-border-status`
//! is on and shown by `panes` beside the program. This is the command that
//! writes it, so an agent working beside a human can say what it is doing in
//! four words without either of them remembering the tmux spelling.

use crate::cli::{pane_target, tmux, tmux_display_at};

/// The `select-pane` that sets a title, with the target when there is one.
///
/// An empty title is how a note is cleared: tmux then shows the pane's own
/// default again, which is the hostname, and `panes` reads that as no note.
pub fn select_pane_args(pane: Option<&str>, title: &str) -> Vec<String> {
    let mut args = vec!["select-pane".to_string()];
    if let Some(p) = pane {
        args.push("-t".to_string());
        args.push(p.to_string());
    }
    args.push("-T".to_string());
    args.push(title.to_string());
    args
}

/// `note`: set, clear or print the note on a pane.
///
/// With no text and no `--clear` it prints the current title, which is what a
/// script or an agent checks before writing over somebody else's note. The
/// pane defaults the way `zen` and `open` default, because a binding runs this
/// through `run-shell`, where `$TMUX_PANE` is the pane the server touched last
/// rather than the one the key was pressed in.
pub async fn run(text: Option<String>, pane: Option<String>, clear: bool) -> anyhow::Result<()> {
    let pane = pane.filter(|p| !p.trim().is_empty()).or_else(pane_target);
    match (text, clear) {
        (Some(_), true) => anyhow::bail!("--clear takes no text"),
        (None, false) => {
            let title = tmux_display_at(pane.as_deref(), "#{pane_title}").await;
            if !title.is_empty() {
                println!("{title}");
            }
        }
        (text, _) => {
            let title = text.unwrap_or_default();
            let args = select_pane_args(pane.as_deref(), &title);
            let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
            tmux(&borrowed).await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_goes_to_the_named_pane_or_the_default_one() {
        assert_eq!(
            select_pane_args(Some("%3"), "claude: cache"),
            ["select-pane", "-t", "%3", "-T", "claude: cache"]
        );
        assert_eq!(select_pane_args(None, "x"), ["select-pane", "-T", "x"]);
    }

    #[test]
    fn clearing_is_an_empty_title_rather_than_a_different_command() {
        // tmux has no "unset title"; the empty string is what brings the
        // default back, and `panes` reads the default as no note.
        assert_eq!(
            select_pane_args(Some("%1"), ""),
            ["select-pane", "-t", "%1", "-T", ""]
        );
    }
}
