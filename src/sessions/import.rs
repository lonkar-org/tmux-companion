//! Reading the tab-separated format tmux-resurrect writes.
//!
//! So nobody's history dies on the way in. A person switching over has months
//! of saves in `~/.local/share/tmux/resurrect`, and a tool that cannot read
//! them asks them to throw the lot away on the day they try it.
//!
//! It is also the only corpus of real saves this code has. Every fixture here
//! is a file that laptop actually wrote, so the parser is tested against what
//! the format does rather than against what its documentation says.
//!
//! Three line kinds matter. The columns are positional and undocumented, so
//! they are written out here from a file rather than from a specification:
//!
//! ```text
//! pane    <session>  <window> <active>  :<flags>  <index>   <title>      :<dir>  <?>  <process>  :<command>
//! window  <session>  <index>  :<name>   <active>  :<flags>  <layout>     <automatic-rename>
//! state   <attached session>  <last session>
//! ```
//!
//! Panes come first in the file and windows after, which is why this reads the
//! whole thing before assembling anything. resurrect's own restore does not
//! care, since it reads the file once per kind of thing it builds.
//!
//! The leading colons are resurrect's own escaping, so a field that is empty
//! reads as `:` rather than as nothing and the column count stays fixed.

use crate::saved::Confidence;

use super::{Header, Pane, Session, Snapshot, Window};

/// Where tmux-resurrect keeps its saves, under a home directory.
pub fn dir_in(home: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(home).join(".local/share/tmux/resurrect")
}

/// The file its `last` symlink points at, when there is one.
pub fn last_in(home: &str) -> Option<std::path::PathBuf> {
    let link = dir_in(home).join("last");
    let target = std::fs::read_link(&link).ok()?;
    Some(if target.is_absolute() {
        target
    } else {
        dir_in(home).join(target)
    })
}

/// Strip resurrect's leading colon from a field.
fn unescape(field: &str) -> &str {
    field.strip_prefix(':').unwrap_or(field)
}

/// tmux's own spelling of a flag.
fn flag(field: &str) -> bool {
    field.trim() == "1"
}

/// Whether a window's flags say it was zoomed.
///
/// resurrect keeps the flags verbatim, and `Z` is tmux's marker for a zoomed
/// window, so the information is there without a column of its own.
fn zoomed(flags: &str) -> bool {
    unescape(flags).contains('Z')
}

/// Parse a tmux-resurrect save into a snapshot.
///
/// A line this cannot read is skipped rather than reported. The file was
/// written by another tool, possibly years and several of its versions ago, and
/// refusing the whole import over one unfamiliar row would be refusing the
/// history this exists to rescue. A capture of our own is held to the opposite
/// standard, because there the odd line means tmux answered strangely now.
pub fn parse(text: &str, from: &str) -> Snapshot {
    // Two passes, because panes come before windows in the file and a pane
    // cannot be filed until the window it belongs to exists. Reading it in one
    // pass dropped every pane and produced sessions of empty windows, which
    // looked like a working import right up to the point somebody used it.
    let mut windows: Vec<(String, Window)> = Vec::new();
    let mut panes: Vec<(String, u32, Pane)> = Vec::new();
    let mut attached = String::new();

    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("state") if f.len() >= 2 => attached = f[1].to_string(),
            Some("window") if f.len() >= 7 => {
                let Ok(index) = f[2].parse::<u32>() else {
                    continue;
                };
                windows.push((
                    f[1].to_string(),
                    Window {
                        index,
                        name: unescape(f[3]).to_string(),
                        layout: f[6].to_string(),
                        active: flag(f[4]),
                        zoomed: zoomed(f[5]),
                        pane: Vec::new(),
                    },
                ));
            }
            Some("pane") if f.len() >= 11 => {
                let (Ok(window), Ok(index)) = (f[2].parse::<u32>(), f[5].parse::<u32>()) else {
                    continue;
                };
                let command = unescape(f[10]).trim().to_string();
                panes.push((
                    f[1].to_string(),
                    window,
                    Pane {
                        index,
                        cwd: unescape(f[7]).to_string(),
                        // resurrect records the full command line, which is the
                        // same thing the process table gives a capture of our
                        // own, so it earns the same grade.
                        confidence: if command.is_empty() {
                            Confidence::Shell
                        } else {
                            Confidence::Exact
                        },
                        command,
                        active: flag(f[3]),
                        title: String::new(),
                    },
                ));
            }
            _ => {}
        }
    }

    let mut sessions: Vec<Session> = Vec::new();
    for (name, window) in windows {
        let at = match sessions.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => {
                sessions.push(Session {
                    name: name.clone(),
                    path: String::new(),
                    window: Vec::new(),
                });
                sessions.len() - 1
            }
        };
        sessions[at].window.push(window);
    }
    for (name, index, pane) in panes {
        // A pane whose window is not in the file at all belongs to a truncated
        // save, and inventing a window for it would restore a shape nobody had.
        if let Some(session) = sessions.iter_mut().find(|s| s.name == name)
            && let Some(window) = session.window.iter_mut().find(|w| w.index == index)
        {
            window.pane.push(pane);
        }
    }

    // A session's path is not in the format at all, so it is taken from the
    // first pane that has one. Without it every window would open at whatever
    // directory the restore happened to be run from.
    for session in &mut sessions {
        session.path = session
            .window
            .iter()
            .flat_map(|w| w.pane.iter())
            .map(|p| p.cwd.clone())
            .find(|d| !d.is_empty())
            .unwrap_or_default();
        session.window.sort_by_key(|w| w.index);
        for window in &mut session.window {
            window.pane.sort_by_key(|p| p.index);
        }
    }
    sessions.retain(|s| !s.window.is_empty());

    Snapshot {
        header: Header {
            format: super::FORMAT,
            captured_at: String::new(),
            clean: false,
            tmux_version: String::new(),
            companion_version: String::new(),
            hostname: String::new(),
            attached,
            imported_from: from.to_string(),
        },
        session: sessions,
    }
}

/// Read tmux-resurrect's newest save, when there is one to read.
pub fn newest_in(home: &str) -> Option<Snapshot> {
    let file = last_in(home)?;
    let text = std::fs::read_to_string(&file).ok()?;
    let snap = parse(&text, &file.display().to_string());
    (!snap.session.is_empty()).then_some(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seven lines from a save this laptop wrote on 2026-09-25, with the home
    /// directory rewritten and nothing else touched.
    ///
    /// Panes first and windows after, which is the order the file is in. An
    /// earlier version of this fixture had it the other way round because that
    /// is the order the columns were read in, and it hid a parser that dropped
    /// every pane.
    const REAL: &str = "\
pane\ticf-c_com\t1\t1\t:*\t1\tsoftware-engineering.html - Nvim\t:/Users/you/lonkar-org/icf-c.com\t1\tnvim\t:nvim
pane\ticf-c_com\t2\t0\t:##-\t1\t\u{2733} Remove agentic AI service\t:/Users/you/lonkar-org/icf-c.com\t1\t2.1.281\t:claude --resume cfba62df-ffde-43e2-944b-5fc36aec3ed5
pane\ty\t1\t1\t:*\t1\tzsh\t:/Users/you\t1\tzsh\t:
window\ticf-c_com\t1\t:edit\t1\t:*\tb644,170x52,0,0,7\toff
window\ticf-c_com\t2\t:ai\t0\t:##-\tb645,170x52,0,0,8\toff
window\ty\t1\t:zsh\t1\t:*\tb63d,170x52,0,0,0\t:
state\ttmux-companion\tmysetup
";

    #[test]
    fn a_real_save_comes_back_as_a_snapshot() {
        let snap = parse(REAL, "last");
        assert_eq!(snap.session.len(), 2);
        assert_eq!(snap.window_count(), 3);
        assert_eq!(snap.pane_count(), 3);
        assert_eq!(snap.header.imported_from, "last");
    }

    #[test]
    fn the_conversation_survives_the_import() {
        // The whole reason for reading this format at all: months of saves
        // already hold the resume id, and it is the thing worth rescuing.
        let snap = parse(REAL, "last");
        let ai = &snap.session[0].window[1].pane[0];
        assert_eq!(
            ai.command,
            "claude --resume cfba62df-ffde-43e2-944b-5fc36aec3ed5"
        );
        // resurrect records the full command line, so this is as exact as a
        // capture of our own reading the process table.
        assert_eq!(ai.confidence, Confidence::Exact);
    }

    #[test]
    fn a_pane_with_no_command_comes_back_as_a_prompt() {
        let snap = parse(REAL, "last");
        let scratch = snap.session.iter().find(|s| s.name == "y").expect("y");
        assert_eq!(scratch.window[0].pane[0].command, "");
        assert_eq!(scratch.window[0].pane[0].confidence, Confidence::Shell);
    }

    #[test]
    fn the_layout_and_the_names_come_through() {
        let snap = parse(REAL, "last");
        assert_eq!(snap.session[0].window[0].name, "edit");
        assert_eq!(snap.session[0].window[0].layout, "b644,170x52,0,0,7");
        assert!(snap.session[0].window[0].active);
        assert!(!snap.session[0].window[1].active);
    }

    #[test]
    fn a_session_takes_its_path_from_the_first_pane_that_has_one() {
        // The format has no column for it, so without this every window would
        // open wherever the restore was run from.
        let snap = parse(REAL, "last");
        assert_eq!(snap.session[0].path, "/Users/you/lonkar-org/icf-c.com");
        assert_eq!(snap.session[1].path, "/Users/you");
    }

    #[test]
    fn the_attached_session_is_read_from_the_state_line() {
        assert_eq!(parse(REAL, "last").header.attached, "tmux-companion");
    }

    #[test]
    fn panes_are_filed_whichever_order_the_file_puts_them_in() {
        // The real file writes every pane and then every window. A parser that
        // reads it in one pass files nothing and produces sessions of empty
        // windows, which looks like a working import until somebody uses it.
        let windows_first = "\
window\ts\t1\t:edit\t1\t:*\tlay\toff
pane\ts\t1\t1\t:*\t1\ttitle\t:/w\t1\tnvim\t:nvim
";
        let panes_first = "\
pane\ts\t1\t1\t:*\t1\ttitle\t:/w\t1\tnvim\t:nvim
window\ts\t1\t:edit\t1\t:*\tlay\toff
";
        for text in [windows_first, panes_first] {
            let snap = parse(text, "x");
            assert_eq!(snap.pane_count(), 1, "{text}");
            assert_eq!(snap.session[0].path, "/w");
        }
    }

    #[test]
    fn a_zoomed_window_is_read_out_of_the_flags() {
        let zoom = "pane\ts\t1\t1\t:*Z\t1\ttitle\t:/w\t1\tnvim\t:nvim
window\ts\t1\t:edit\t1\t:*Z\tb644,170x52,0,0,7\toff
";
        let snap = parse(zoom, "x");
        assert!(snap.session[0].window[0].zoomed);
        assert!(!parse(REAL, "last").session[0].window[0].zoomed);
    }

    #[test]
    fn a_line_this_cannot_read_is_skipped_rather_than_refusing_the_file() {
        // The opposite of what a capture of our own does. That file was written
        // by this code a moment ago, so an odd line means tmux answered
        // strangely. This one may be years and several versions old, and
        // refusing it over one row would refuse the history worth rescuing.
        let odd = format!("{REAL}something\tnew\tin\ta\tlater\tversion\nwindow\tbroken\n");
        let snap = parse(&odd, "last");
        assert_eq!(snap.session.len(), 2);
        assert_eq!(snap.pane_count(), 3);
    }

    #[test]
    fn a_pane_whose_window_is_missing_is_dropped_rather_than_inventing_one() {
        let truncated = "pane\ts\t9\t1\t:*\t1\ttitle\t:/w\t1\tnvim\t:nvim\n";
        assert!(parse(truncated, "x").session.is_empty());
    }

    #[test]
    fn an_empty_file_is_an_empty_snapshot_and_not_a_panic() {
        assert!(parse("", "x").session.is_empty());
        assert!(parse("\n\n", "x").session.is_empty());
    }

    #[test]
    fn windows_and_panes_come_back_in_index_order() {
        let jumbled = "pane\ts\t1\t1\t:*\t2\tt\t:/w\t1\tnvim\t:nvim
pane\ts\t1\t1\t:*\t1\tt\t:/w\t1\tnvim\t:nvim
window\ts\t2\t:b\t0\t:-\tlay2\toff
window\ts\t1\t:a\t1\t:*\tlay1\toff
";
        let snap = parse(jumbled, "x");
        assert_eq!(
            snap.session[0]
                .window
                .iter()
                .map(|w| w.index)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            snap.session[0].window[0]
                .pane
                .iter()
                .map(|p| p.index)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn the_directory_is_where_resurrect_puts_it() {
        assert!(
            dir_in("/Users/you")
                .display()
                .to_string()
                .ends_with(".local/share/tmux/resurrect")
        );
    }
}
