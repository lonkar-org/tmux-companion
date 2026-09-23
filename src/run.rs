//! Run a command from history in a pane beside the one you are in.
//!
//! The picker is the easy half. What this file mostly holds is the behaviour
//! around it: a pane that slides out rather than appearing, a dialog when the
//! command exits, and a fallback for when the dialog cannot open.

/// Where the commands come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistorySource {
    /// zsh's `$HISTFILE`, extended format or plain.
    Zsh,
    /// bash's `$HISTFILE`.
    Bash,
    /// fish's history file.
    Fish,
    /// atuin, asked through its own command.
    Atuin,
}

/// Parse a zsh history file, newest first.
///
/// zsh writes either plain lines or the extended format,
/// `: <start>:<elapsed>;<command>`, and a command continued onto another line
/// ends with a backslash. Both shapes appear in one file, because the option
/// can be turned on halfway through a file's life.
pub fn parse_zsh(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;

    for raw in text.lines() {
        let mut escaped_colon = false;
        // The timestamp only prefixes the first physical line of an entry, so
        // it is stripped only when one is not already being continued.
        let line = match (&pending, raw.strip_prefix(": ")) {
            (None, Some(rest)) => rest.split_once(';').map(|(_, c)| c).unwrap_or(rest),
            // A command that really does start with a colon is written `\:`,
            // so zsh does not read it back as the extended format's prefix.
            // `fc -ln` unescapes it and so does this.
            (None, None) => raw.strip_prefix("\\:").map_or(raw, |rest| {
                escaped_colon = true;
                rest
            }),
            _ => raw,
        };

        let continues = line.ends_with('\\');
        let body = if continues {
            &line[..line.len() - 1]
        } else {
            line
        };

        match &mut pending {
            Some(acc) => {
                // Joined with a literal backslash-n, which is what `fc -ln`
                // prints and what a one-line picker row needs. A real newline
                // here would make one history entry look like several rows.
                acc.push_str("\\n");
                acc.push_str(body);
                if !continues {
                    if let Some(done) = pending.take() {
                        out.push(done);
                    }
                }
            }
            None => {
                let body = if escaped_colon {
                    format!(":{body}")
                } else {
                    body.to_string()
                };
                if continues {
                    pending = Some(body);
                } else if !body.trim().is_empty() {
                    out.push(body);
                }
            }
        }
    }
    // A file whose last line ends in a backslash still has an entry in it.
    if let Some(last) = pending {
        out.push(last);
    }
    out.reverse();
    out
}

/// Undo zsh's meta encoding.
///
/// zsh stores a byte above 0x7f as `0x83` followed by that byte with bit 0x20
/// flipped, so the history file stays free of bytes it treats specially.
/// `fc -ln` undoes this on the way out; reading the file directly means doing
/// it here, or a command with a box-drawing character in it comes back as
/// mojibake.
pub fn unmetafy(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x83 && i + 1 < bytes.len() {
            out.push(bytes[i + 1] ^ 0x20);
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Parse a fish history file, newest first.
///
/// fish writes YAML-ish records, and only the `cmd:` line is wanted.
pub fn parse_fish(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .lines()
        .filter_map(|l| l.strip_prefix("- cmd: "))
        .map(str::to_string)
        .collect();
    out.reverse();
    out
}

/// Drop repeats, keeping the first occurrence.
///
/// Applied after the reverse, so "first" is the most recent use of a command
/// and a thing run twenty times appears once, at the top.
pub fn dedupe(commands: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    commands
        .into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .filter(|c| seen.insert(c.clone()))
        .collect()
}

/// The widths a pane passes through while it slides out.
///
/// tmux has no animation primitive, so a stepped `resize-pane` is the closest
/// thing to an IDE sliding a panel out. Ease-out, because a linear slide reads
/// as a jump: `1 - (1 - t)²` moves most of the distance early.
///
/// The step count is a tradeoff rather than a taste: every step sends
/// `SIGWINCH` to the neighbouring pane, whose shell repaints its prompt, so
/// more steps is smoother here and flickerier next door.
pub fn slide_steps(from: u16, to: u16, steps: u16) -> Vec<u16> {
    if steps == 0 || from == to {
        return Vec::new();
    }
    (1..=steps)
        .map(|i| {
            let t = f64::from(i) / f64::from(steps);
            let eased = 1.0 - (1.0 - t) * (1.0 - t);
            let delta = f64::from(to) - f64::from(from);
            (f64::from(from) + delta * eased).round() as u16
        })
        .collect()
}

/// What the exit dialog offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Close the pane.
    Close,
    /// Leave it open, read-only, to look at what happened.
    View,
    /// Run it again.
    Restart,
}

/// Which button the dialog starts on.
///
/// Close on success and Restart on failure, because those are the two things
/// somebody does next, and a dialog whose default is always the same makes one
/// of them a keystroke longer than it needs to be.
pub fn default_choice(exit_code: i32) -> Choice {
    if exit_code == 0 {
        Choice::Close
    } else {
        Choice::Restart
    }
}

/// The pane width for a percentage of a window.
///
/// Percentages rather than columns, so a narrower terminal still splits
/// sensibly instead of leaving one side unusable.
pub fn pane_width(window_width: u16, percent: u16) -> u16 {
    let w = (u32::from(window_width) * u32::from(percent.min(100))) / 100;
    (w as u16).max(1)
}

// ── Reading a history ────────────────────────────────────────────────────────

/// Read the configured history, newest first and deduplicated.
pub async fn history(config: &crate::config::Run, home: &str) -> Vec<String> {
    use crate::config::HistorySource as S;

    // `auto` is the default, so this is where most machines decide which
    // history they are reading. Resolved once, and used for both the file and
    // the parser, so the two cannot disagree.
    let source = config
        .history
        .resolve(&std::env::var("SHELL").unwrap_or_default());

    let text = match source {
        S::Atuin => {
            // atuin keeps its history in a database, so it is asked rather
            // than read: anybody using it has no history file worth parsing.
            let out = tokio::process::Command::new("atuin")
                .args(["history", "list", "--format", "{command}"])
                .output()
                .await;
            match out {
                Ok(o) => {
                    let text = String::from_utf8_lossy(&o.stdout).into_owned();
                    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
                    lines.reverse();
                    return dedupe(lines);
                }
                Err(_) => String::new(),
            }
        }
        _ => {
            let path = config.history_file.clone().unwrap_or_else(|| {
                let name = match source {
                    S::Bash => ".bash_history",
                    S::Fish => ".local/share/fish/fish_history",
                    _ => ".zsh_history",
                };
                std::path::PathBuf::from(home).join(name)
            });
            // Histories carry whatever bytes a command carried, so this reads
            // lossily rather than refusing a file with one stray byte in it.
            std::fs::read(&path)
                .map(|b| String::from_utf8_lossy(&unmetafy(&b)).into_owned())
                .unwrap_or_default()
        }
    };

    let parsed = match source {
        S::Fish => parse_fish(&text),
        _ => parse_zsh(&text),
    };
    dedupe(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_zsh_history_comes_back_newest_first() {
        let history = "first\nsecond\nthird\n";
        assert_eq!(parse_zsh(history), vec!["third", "second", "first"]);
    }

    #[test]
    fn the_extended_format_loses_its_timestamp() {
        let history = ": 1700000000:0;cargo test\n: 1700000100:5;git status\n";
        assert_eq!(parse_zsh(history), vec!["git status", "cargo test"]);
    }

    #[test]
    fn both_formats_can_appear_in_one_file() {
        // The option can be turned on halfway through a file's life.
        let history = "plain one\n: 1700000000:0;extended one\n";
        assert_eq!(parse_zsh(history), vec!["extended one", "plain one"]);
    }

    #[test]
    fn a_continued_command_is_one_entry_joined_the_way_fc_prints_it() {
        // zsh stores a multi-line command as physical lines ending in a
        // backslash, and `fc -ln` prints it back with a literal backslash-n.
        // A real newline here would turn one history entry into several rows.
        let history = "for f in *; do \\\necho $f\\\ndone\n";
        let parsed = parse_zsh(history);
        assert_eq!(parsed.len(), 1, "{parsed:?}");
        assert_eq!(parsed[0], "for f in *; do \\necho $f\\ndone");
        assert!(!parsed[0].contains('\n'), "no real newline in a row");
    }

    #[test]
    fn a_continuation_does_not_swallow_the_entries_after_it() {
        // The first version treated every trailing backslash as an open
        // continuation and ate whatever followed, which lost 48 commands out
        // of 1175 and invented some that were never run.
        let history = "one \\\ntwo\nthree\nfour\n";
        assert_eq!(parse_zsh(history), vec!["four", "three", "one \\ntwo"]);
    }

    #[test]
    fn a_command_starting_with_a_colon_keeps_its_colon() {
        // zsh writes `\\:qa` so the line is not read back as the extended
        // format's `: <timestamp>;` prefix. Leaving the backslash on turned
        // `:qa` into `\\:qa` in the picker, which would have run the wrong
        // thing.
        assert_eq!(parse_zsh("\\:qa\n"), vec![":qa"]);
    }

    #[test]
    fn a_timestamp_is_stripped_only_from_the_first_line_of_an_entry() {
        let history = ": 1700000000:0;echo \\\n: not a timestamp\n";
        let parsed = parse_zsh(history);
        assert_eq!(parsed, vec!["echo \\n: not a timestamp"]);
    }

    #[test]
    fn the_meta_encoding_is_undone() {
        // zsh writes a byte above 0x7f as 0x83 then that byte xor 0x20, so a
        // command with a box-drawing character in it comes back as mojibake
        // without this. `█` is e2 96 88.
        let metafied = [0x83, 0xc2, 0x83, 0xb6, 0x83, 0xa8];
        assert_eq!(unmetafy(&metafied), vec![0xe2, 0x96, 0x88]);
    }

    #[test]
    fn plain_bytes_pass_through_the_unmetafier() {
        assert_eq!(unmetafy(b"ls -la"), b"ls -la".to_vec());
    }

    #[test]
    fn a_trailing_meta_marker_does_not_read_past_the_end() {
        assert_eq!(unmetafy(&[b'a', 0x83]), vec![b'a', 0x83]);
    }

    #[test]
    fn blank_lines_are_not_commands() {
        assert_eq!(parse_zsh("a\n\n\nb\n"), vec!["b", "a"]);
    }

    #[test]
    fn fish_history_takes_only_the_command_lines() {
        let history =
            "- cmd: cargo test\n  when: 1700000000\n- cmd: git status\n  when: 1700000100\n";
        assert_eq!(parse_fish(history), vec!["git status", "cargo test"]);
    }

    #[test]
    fn a_command_run_twenty_times_appears_once_at_the_top() {
        let commands = vec![
            "git status".to_string(),
            "cargo test".to_string(),
            "git status".to_string(),
        ];
        assert_eq!(dedupe(commands), vec!["git status", "cargo test"]);
    }

    #[test]
    fn whitespace_only_history_entries_are_dropped() {
        assert_eq!(dedupe(vec!["  ".into(), "ls".into()]), vec!["ls"]);
    }

    #[test]
    fn the_slide_ends_where_it_was_asked_to() {
        let steps = slide_steps(1, 56, 5);
        assert_eq!(steps.len(), 5);
        assert_eq!(*steps.last().expect("a last step"), 56);
    }

    #[test]
    fn the_slide_eases_out_rather_than_moving_evenly() {
        // Half the steps should cover well over half the distance, which is
        // what makes it read as a slide rather than a jump.
        let steps = slide_steps(0, 100, 4);
        assert!(steps[1] > 50, "{steps:?}");
    }

    #[test]
    fn the_slide_is_monotonic_in_both_directions() {
        let out = slide_steps(1, 56, 5);
        assert!(out.windows(2).all(|w| w[0] <= w[1]), "{out:?}");
        let back = slide_steps(56, 1, 5);
        assert!(back.windows(2).all(|w| w[0] >= w[1]), "{back:?}");
        assert_eq!(*back.last().expect("a last step"), 1);
    }

    #[test]
    fn a_slide_to_where_it_already_is_does_nothing() {
        assert!(slide_steps(40, 40, 5).is_empty());
        assert!(slide_steps(1, 40, 0).is_empty());
    }

    #[test]
    fn the_dialog_starts_on_what_you_probably_want_next() {
        assert_eq!(default_choice(0), Choice::Close);
        assert_eq!(default_choice(1), Choice::Restart);
        assert_eq!(default_choice(130), Choice::Restart);
    }

    #[test]
    fn a_percentage_width_scales_with_the_window() {
        assert_eq!(pane_width(180, 33), 59);
        assert_eq!(pane_width(80, 33), 26);
    }

    #[test]
    fn a_tiny_window_still_gives_a_pane_one_column() {
        // Better a sliver than a division that asks tmux for a zero-width pane.
        assert_eq!(pane_width(2, 33), 1);
        assert_eq!(pane_width(0, 33), 1);
    }
}
