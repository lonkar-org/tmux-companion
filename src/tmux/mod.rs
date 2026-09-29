//! Everything that knows about tmux's own formatting language.
/// Colours, styles and the segment builder.
pub mod format;
/// Nerd Font codepoints.
pub mod icons;

/// The flag every tmux call here starts with.
///
/// tmux decides per client whether it may print UTF-8: yes when `$TMUX` is
/// set, otherwise only when `LC_ALL`, `LC_CTYPE` or `LANG` names UTF-8. Run
/// from outside tmux with no such locale, which is a plain container, a
/// daemon a service manager started, or `sessions shutdown` from a bare
/// terminal, tmux prints each tab in `-F` and `display -p` output as `_`, and
/// each byte of a multibyte name as `_` too. 3.4 and 3.7c both do it.
/// Everything here that splits a listing on `\t` then reads nothing:
/// `sessions save` refused with "could not be read ... alpha_/tmp". `-u`
/// tells that one client to assume UTF-8 and the tab comes back whatever the
/// locale. It is the client's flag, not the server's, so it goes on every
/// call rather than on the one that starts the server.
pub const UTF8_FLAG: &str = "-u";

/// A `tmux` command with [`UTF8_FLAG`] already on it, for anything whose
/// output gets read.
pub fn command() -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("tmux");
    cmd.arg(UTF8_FLAG);
    cmd
}

/// [`command`] for the places that run outside a runtime.
pub fn command_sync() -> std::process::Command {
    let mut cmd = std::process::Command::new("tmux");
    cmd.arg(UTF8_FLAG);
    cmd
}

/// A `tmux` command that attaches this terminal, without [`UTF8_FLAG`].
///
/// On `attach-session` the flag means more than the output of one command: it
/// tells tmux the terminal can draw UTF-8, whatever its locale says. That is
/// the user's decision, made by their locale or by `tmux -u` in their own
/// alias, so an attach runs tmux the way they would have.
pub fn attach_command() -> tokio::process::Command {
    tokio::process::Command::new("tmux")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(cmd: &std::process::Command) -> Vec<String> {
        std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn every_command_asks_tmux_for_utf8() {
        assert_eq!(argv(command().as_std()), ["tmux", "-u"]);
        assert_eq!(argv(&command_sync()), ["tmux", "-u"]);
    }

    #[test]
    fn an_attach_leaves_the_terminal_to_the_locale() {
        assert_eq!(argv(attach_command().as_std()), ["tmux"]);
    }

    /// A `Command::new("tmux")` anywhere else in `src/` skips the flag, and
    /// the tab it loses shows up only on a machine without a UTF-8 locale.
    #[test]
    fn no_call_site_builds_its_own_tmux_command() {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("read src") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && !path.ends_with("src/tmux/mod.rs")
                {
                    let text = std::fs::read_to_string(&path).expect("read file");
                    for (n, line) in text.lines().enumerate() {
                        if line.contains(concat!("Command::new(", "\"tmux\")")) {
                            out.push(format!("{}:{}", path.display(), n + 1));
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(
            found.is_empty(),
            "use crate::tmux::command() instead: {found:?}"
        );
    }
}
