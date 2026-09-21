//! Renders the git segment for a spread of repo states in every color style.
//!
//! The segment is produced in tmux format (exactly what the status bar gets)
//! and then translated to ANSI so it can be eyeballed in a plain terminal —
//! the no-tmux render path drops arrow glyphs, so it is not a faithful preview.

use std::sync::LazyLock;

use regex::Regex;

use crate::{
    segments::git::{Area, GitStatus, status_line_capped, status_line_styled},
    tmux::{
        format::{BG_BAR, Style},
        icons::{ARROW_RIGHT, CAP_CHEVRON, CAP_EIGHTH, CAP_NONE, CAP_RULE, CAP_SLASH, SEPARATOR},
    },
};

static TMUX_COLOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#\[fg=color(\d+),bg=color(\d+)\]").unwrap());

/// Translate `#[fg=colorN,bg=colorM]` markers into 256-color ANSI escapes.
fn tmux_to_ansi(s: &str) -> String {
    TMUX_COLOR
        .replace_all(s, |c: &regex::Captures| {
            let fg: u16 = c[1].parse().unwrap_or(15);
            let bg: u16 = c[2].parse().unwrap_or(0);
            format!("\x1b[38;5;{}m\x1b[48;5;{}m", fg, bg)
        })
        .into_owned()
}

fn base(branch: &str) -> GitStatus {
    GitStatus {
        branch: branch.into(),
        upstream: format!("origin/{}", branch),
        remote_success: true,
        ..Default::default()
    }
}

fn states() -> Vec<(&'static str, GitStatus)> {
    vec![
        ("clean", base("main")),
        (
            "clean + stash",
            GitStatus {
                stashed: 2,
                ..base("main")
            },
        ),
        (
            "unstaged",
            GitStatus {
                untracked: 2,
                unstaged: Area {
                    modified: 3,
                    deleted: 1,
                    ..Default::default()
                },
                ..base("main")
            },
        ),
        (
            "staged",
            GitStatus {
                staged: Area {
                    added: 2,
                    modified: 1,
                    ..Default::default()
                },
                ..base("feature/tts-from-tmux")
            },
        ),
        (
            "ahead / behind",
            GitStatus {
                ahead: 2,
                behind: 5,
                ..base("main")
            },
        ),
        (
            "unmerged",
            GitStatus {
                unmerged: 3,
                unstaged: Area {
                    modified: 1,
                    ..Default::default()
                },
                ..base("fix/conflict")
            },
        ),
        (
            "everything",
            GitStatus {
                ahead: 1,
                behind: 1,
                unmerged: 1,
                untracked: 1,
                stashed: 1,
                staged: Area {
                    modified: 1,
                    added: 1,
                    deleted: 1,
                    renamed: 1,
                    copied: 1,
                },
                unstaged: Area {
                    modified: 1,
                    added: 1,
                    deleted: 1,
                    renamed: 1,
                    copied: 1,
                },
                ..base("hotfix/urgent-thing")
            },
        ),
        (
            "long branch",
            GitStatus {
                unstaged: Area {
                    modified: 1,
                    ..Default::default()
                },
                ..base("chore/this-is-a-very-long-branch-name")
            },
        ),
        (
            "new (no upstream)",
            GitStatus {
                is_new: true,
                upstream: String::new(),
                ..base("wip/local-only")
            },
        ),
        (
            "gone upstream",
            GitStatus {
                is_gone: true,
                ..base("release/1.0")
            },
        ),
        (
            "loading",
            GitStatus {
                loading: true,
                ..base("main")
            },
        ),
        (
            "remote failed",
            GitStatus {
                remote_success: false,
                ..base("main")
            },
        ),
    ]
}

/// End-cap candidates for the outline styles, shown after the style sections.
fn caps() -> Vec<(&'static str, &'static str)> {
    vec![
        ("separator e621 (now)", SEPARATOR),
        ("solid triangle", ARROW_RIGHT),
        ("thin slash", CAP_SLASH),
        ("thin chevron", CAP_CHEVRON),
        ("vertical rule", CAP_RULE),
        ("eighth block", CAP_EIGHTH),
        ("none", CAP_NONE),
    ]
}

/// Render one sample of every style and state, for eyeballing in a terminal.
pub fn render() -> String {
    let styles = [
        ("fill (current)", Style::Fill),
        ("outline (icon colors untouched)", Style::Outline),
        (
            "outline-bright (icon colors lightened)",
            Style::OutlineBright,
        ),
    ];
    let bar: u16 = BG_BAR.parse().unwrap_or(233);
    let mut out = String::new();

    for (title, style) in styles {
        out.push_str(&format!("\n\x1b[1m{}\x1b[0m\n", title));
        for (label, status) in states() {
            let line = tmux_to_ansi(&status_line_styled(&status, false, false, style));
            out.push_str(&format!(
                "  {:<18}\x1b[48;5;{}m {}\x1b[0m\n",
                label, bar, line
            ));
        }
    }

    // Cap candidates: each rendered against a clean, a dirty and a gone state,
    // with a window-ish block after it so the boundary is visible.
    out.push_str("\n\x1b[1moutline-bright end caps\x1b[0m\n");
    // clean, unstaged, gone — three different cap colors.
    let cap_states = [
        states().remove(0).1,
        states().remove(2).1,
        states().remove(9).1,
    ];
    for (label, cap) in caps() {
        let mut row = String::new();
        for status in &cap_states {
            row.push_str(&tmux_to_ansi(&status_line_capped(
                status,
                false,
                false,
                Style::OutlineBright,
                Some(cap),
            )));
            row.push_str(&format!("\x1b[48;5;{}m   ", bar));
        }
        out.push_str(&format!(
            "  {:<18}\x1b[48;5;{}m {}\x1b[0m\n",
            label, bar, row
        ));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_to_ansi_translates_color_markers() {
        assert_eq!(
            tmux_to_ansi("#[fg=color025,bg=color120]x"),
            "\x1b[38;5;25m\x1b[48;5;120mx"
        );
    }

    #[test]
    fn tmux_to_ansi_leaves_other_text_alone() {
        assert_eq!(tmux_to_ansi("plain"), "plain");
    }

    #[test]
    fn render_has_no_leftover_tmux_markers() {
        let out = render();
        assert!(!out.contains("#["), "unconverted tmux marker in preview");
    }

    #[test]
    fn render_covers_every_style_and_state() {
        let out = render();
        assert!(out.contains("fill (current)"));
        assert!(out.contains("outline-bright"));
        for (label, _) in states() {
            assert!(out.contains(label), "missing state: {label}");
        }
    }
}
