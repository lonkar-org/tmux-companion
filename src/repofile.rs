//! The layout a checkout carries with it: `.tmux-companion.toml` in the
//! project root.
//!
//! A `[[project.override]]` matches a path in one person's `config.toml`, so
//! the same repository on a second machine, or in a colleague's home, opens as
//! whatever that machine's default is. This file travels with the checkout:
//! `[[window]]` rows in the same shape as `[[layout.window]]`, or `layout =
//! "name"` to name a layout the reader's own config defines, or both, with
//! the windows winning.
//!
//! A file in a checkout that can run commands is a file anybody who can push
//! to the repository can use to run commands on your machine, so the commands
//! are honoured only under a path `[project] trusted` lists. Anywhere else
//! the names still count, since a plain shell called `edit` beside one called
//! `ai` is already what `toggle` and the bar need, and every `command` is
//! blanked. `project show` says which happened.
//!
//! Precedence, from `saved::resolve`: a saved layout, because it was captured
//! on this machine by a key somebody pressed; then this file; then the
//! override table; then the default.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::LayoutWindow;

/// The file's name, at the project root.
pub const FILE: &str = ".tmux-companion.toml";

/// What the file may say.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct RepoLayout {
    /// A `[[layout]]` in the reader's config, by name. Used when there are no
    /// windows here, so a checkout can say "the work layout" without saying
    /// what that is.
    pub layout: String,
    /// The windows, same shape as `[[layout.window]]`.
    pub window: Vec<LayoutWindow>,
}

/// What reading the file found.
#[derive(Debug, Clone, PartialEq)]
pub enum RepoFile {
    /// A file to open with.
    Layout {
        /// The file, so `project show` can name it.
        file: PathBuf,
        /// What it says.
        layout: RepoLayout,
    },
    /// A file that is there and not used, with the reason in words.
    Ignored {
        /// The file.
        file: PathBuf,
        /// Why, as the tail of "the file ...".
        why: String,
    },
    /// No file.
    Absent,
}

impl RepoFile {
    /// The ignored file and why, for `describe`.
    pub fn ignored(&self) -> Option<(&Path, &str)> {
        match self {
            Self::Ignored { file, why } => Some((file, why)),
            _ => None,
        }
    }
}

/// Where the file sits for a project.
pub fn path_in(project_path: &str) -> PathBuf {
    Path::new(project_path).join(FILE)
}

/// Parse the file's text, with a parse error folded to one line: where, and
/// what, which for an unknown key is the line that names it.
pub fn parse(text: &str) -> Result<RepoLayout, String> {
    let layout: RepoLayout = toml::from_str(text).map_err(|e| {
        let text = e.to_string();
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('|') && !l.starts_with(char::is_numeric))
            .collect();
        let first = lines.first().copied().unwrap_or("").to_string();
        match lines.last() {
            Some(last) if *last != first => format!("{first}: {last}"),
            _ => first,
        }
    })?;
    if layout.window.is_empty() && layout.layout.trim().is_empty() {
        return Err("names no layout and has no [[window]]".to_string());
    }
    if let Some(w) = layout.window.iter().find(|w| w.name.trim().is_empty()) {
        return Err(format!(
            "has a [[window]] with no name{}",
            if w.command.is_empty() {
                String::new()
            } else {
                format!(" (command = {:?})", w.command)
            }
        ));
    }
    Ok(layout)
}

/// Read the file for a project, when there is one.
pub fn load(project_path: &str) -> RepoFile {
    let file = path_in(project_path);
    let Ok(text) = std::fs::read_to_string(&file) else {
        return RepoFile::Absent;
    };
    match parse(&text) {
        Ok(layout) => RepoFile::Layout { file, layout },
        Err(why) => RepoFile::Ignored {
            file,
            why: format!("does not parse: {why}"),
        },
    }
}

/// Whether a project sits under one of the trusted prefixes.
///
/// A prefix takes `~` for home and a trailing `*` or `/`, like
/// `[[project.override]]`, but the match is by whole path components:
/// trusting `~/work/api` does not trust `~/work/api-fork`, which a string
/// prefix would, and this is the one list where that matters. An empty
/// prefix matches nothing, so an empty string does not trust the whole disk.
///
/// Both sides are also compared resolved, since the picker hands over
/// `/private/var/...` for a directory the config wrote as `/var/...`, and a
/// symlinked home is the same story.
pub fn is_trusted(project_path: &str, trusted: &[String], home: &str) -> bool {
    let project = Path::new(project_path);
    let project_real = std::fs::canonicalize(project).ok();
    trusted.iter().any(|t| {
        let expanded = match t.trim().strip_prefix("~/") {
            Some(rest) => format!("{home}/{rest}"),
            None if t.trim() == "~" => home.to_string(),
            None => t.trim().to_string(),
        };
        let prefix = expanded.trim_end_matches('*').trim_end_matches('/');
        if prefix.is_empty() {
            return false;
        }
        let prefix = Path::new(prefix);
        if project.starts_with(prefix) {
            return true;
        }
        match (&project_real, std::fs::canonicalize(prefix)) {
            (Some(real), Ok(prefix_real)) => real.starts_with(prefix_real),
            _ => false,
        }
    })
}

/// The windows with every command blanked: the shape without the programs.
pub fn names_only(windows: &[LayoutWindow]) -> Vec<LayoutWindow> {
    windows
        .iter()
        .map(|w| {
            let mut w = w.clone();
            w.command = String::new();
            for p in &mut w.pane {
                p.command = String::new();
            }
            w
        })
        .collect()
}

/// Whether any window or pane in the file would run something.
pub fn has_commands(windows: &[LayoutWindow]) -> bool {
    windows.iter().any(|w| {
        !w.command.trim().is_empty() || w.pane.iter().any(|p| !p.command.trim().is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_takes_windows_or_a_layout_name_and_nothing_else() {
        let l = parse("[[window]]\nname = \"edit\"\ncommand = \"nvim\"\n").unwrap();
        assert_eq!(l.window.len(), 1);
        assert_eq!(l.window[0].command, "nvim");
        assert!(l.window[0].hold_name, "the layout's default holds");
        let named = parse("layout = \"work\"\n").unwrap();
        assert_eq!(named.layout, "work");
        assert!(named.window.is_empty());
        // A key this does not know is refused by name, like config.toml.
        let err = parse("[[window]]\nname = \"x\"\nrun = \"rm -rf /\"\n").unwrap_err();
        assert!(err.contains("run"), "{err}");
        // A file that says nothing is not a layout.
        assert!(parse("").unwrap_err().contains("names no layout"));
        assert!(
            parse("[[window]]\ncommand = \"nvim\"\n")
                .unwrap_err()
                .contains("name")
        );
    }

    #[test]
    fn trust_is_a_path_prefix_with_a_tilde_and_nothing_trusts_everything() {
        let t = |s: &str| vec![s.to_string()];
        assert!(is_trusted("/home/me/work/api", &t("~/work"), "/home/me"));
        assert!(is_trusted("/home/me/work/api", &t("~/work/*"), "/home/me"));
        assert!(is_trusted("/srv/api", &t("/srv"), "/home/me"));
        assert!(!is_trusted("/home/me/other/api", &t("~/work"), "/home/me"));
        // Whole components: a sibling that starts the same is a stranger.
        assert!(!is_trusted(
            "/home/me/work/api-fork",
            &t("~/work/api"),
            "/home/me"
        ));
        assert!(is_trusted(
            "/home/me/work/api",
            &t("~/work/api/"),
            "/home/me"
        ));
        assert!(is_trusted(
            "/home/me/work/api/sub",
            &t("~/work/api"),
            "/home/me"
        ));
        assert!(!is_trusted("/anything", &t(""), "/home/me"));
        assert!(!is_trusted("/anything", &[], "/home/me"));
    }

    #[test]
    fn names_only_keeps_the_shape_and_drops_every_command() {
        let l = parse(
            "[[window]]\nname = \"edit\"\ncommand = \"nvim\"\n\
             [[window]]\nname = \"work\"\n[[window.pane]]\ncommand = \"claude\"\n[[window.pane]]\ncwd = \"~/x\"\n",
        )
        .unwrap();
        assert!(has_commands(&l.window));
        let plain = names_only(&l.window);
        assert!(!has_commands(&plain));
        assert_eq!(plain.len(), 2);
        assert_eq!(plain[0].name, "edit");
        assert_eq!(plain[1].pane.len(), 2);
        assert_eq!(plain[1].pane[1].cwd.as_deref(), Some("~/x"));
    }

    #[test]
    fn loading_reports_absent_a_layout_or_the_reason_it_was_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().display().to_string();
        assert_eq!(load(&root), RepoFile::Absent);
        std::fs::write(path_in(&root), "[[window]]\nname = \"edit\"\n").unwrap();
        assert!(matches!(load(&root), RepoFile::Layout { .. }));
        std::fs::write(path_in(&root), "[[window]]\nname = \n").unwrap();
        let (file, why) = load(&root)
            .ignored()
            .map(|(f, w)| (f.to_path_buf(), w.to_string()))
            .unwrap();
        assert!(file.ends_with(FILE));
        assert!(why.starts_with("does not parse"), "{why}");
    }
}
