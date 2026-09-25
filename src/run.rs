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

/// The `split-window` that opens the side pane.
///
/// `-t` is the whole point of this function. The picker runs inside
/// `display-popup -E`, and a popup is not a client: an untargeted split is
/// resolved against whichever session the server touched most recently, which
/// after a project switch is not the session on screen. The pane opened, ran
/// the command and drew its dialog in a window nobody was looking at, and the
/// reel recorded a picker that closed onto an empty prompt.
pub fn split_args(pane: Option<&str>, opening: u16, command: &str) -> Vec<String> {
    let mut args: Vec<String> = vec!["split-window".into(), "-fh".into()];
    if let Some(p) = pane {
        if !p.is_empty() {
            args.push("-t".into());
            args.push(p.to_string());
        }
    }
    args.push("-l".into());
    args.push(opening.to_string());
    args.push(command.to_string());
    args
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

/// How wide and tall the exit dialog is, in cells.
///
/// Fixed, because the dialog holds three buttons and two lines of text and
/// nothing in it grows: a popup sized to its content would move under somebody
/// every time a command exited with a longer status.
pub const DIALOG_WIDTH: u16 = 44;
/// How tall the exit dialog is. See [`DIALOG_WIDTH`].
pub const DIALOG_HEIGHT: u16 = 7;

/// Where to put the dialog so it lands centred on the pane it belongs to.
///
/// Over the pane rather than over the terminal: the pane is where the command
/// ran and where the person is looking, and a dialog centred on a wide
/// terminal opens over whatever else is on screen.
///
/// Pure, and clamped at zero, because a pane narrower than the dialog gives a
/// negative offset and `display-popup -x -3` is an error rather than a nudge.
pub fn dialog_at(left: u16, top: u16, width: u16, height: u16) -> (u16, u16) {
    let x = left + width.saturating_sub(DIALOG_WIDTH) / 2;
    let y = top + height.saturating_sub(DIALOG_HEIGHT) / 2;
    (x, y)
}

/// The popup's title, which is the only place the exit status is shown.
pub fn dialog_title(code: i32) -> String {
    if code == 0 {
        "#[fg=brightgreen][ \u{2714} done ]".to_string()
    } else {
        format!("#[fg=brightred][ \u{2718} exit {code} ]")
    }
}

/// The three buttons, in the order they are drawn.
pub const BUTTONS: [Choice; 3] = [Choice::Close, Choice::View, Choice::Restart];

/// The button `delta` steps along, wrapping at both ends.
///
/// Wrapping because three buttons in a row are a ring in everybody's hands,
/// and a Tab that stops at Restart is a Tab somebody presses twice.
pub fn step_button(current: usize, delta: isize) -> usize {
    let len = BUTTONS.len() as isize;
    (((current as isize + delta) % len + len) % len) as usize
}

/// Which button starts selected.
pub fn default_button(code: i32) -> usize {
    BUTTONS
        .iter()
        .position(|c| *c == default_choice(code))
        .unwrap_or(0)
}

/// What one of the buttons says.
pub fn button_label(choice: Choice) -> &'static str {
    match choice {
        Choice::Close => " Close ",
        Choice::View => " View ",
        Choice::Restart => " Restart ",
    }
}

/// A choice, as the word written to the answer file and read back.
///
/// A file rather than the popup's exit status: `display-popup -E` gives back
/// whether the command succeeded and nothing else, and there are three answers
/// here.
pub fn choice_word(choice: Choice) -> &'static str {
    match choice {
        Choice::Close => "close",
        Choice::View => "view",
        Choice::Restart => "restart",
    }
}

/// One of those words, read back.
///
/// Anything unrecognised is `View`, which is what a dialog that was killed
/// rather than answered should do: it leaves the pane open and read-only, so
/// nothing is lost while somebody works out what happened.
pub fn choice_of_word(word: &str) -> Choice {
    match word.trim() {
        "close" => Choice::Close,
        "restart" => Choice::Restart,
        _ => Choice::View,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── the exit dialog ──────────────────────────────────────────────────────

    #[test]
    fn the_dialog_centres_on_the_pane_it_belongs_to() {
        // On the pane and not on the terminal: the pane is where the command
        // ran and where somebody is looking.
        let (x, y) = dialog_at(100, 10, 60, 30);
        assert_eq!(x, 100 + (60 - DIALOG_WIDTH) / 2);
        assert_eq!(y, 10 + (30 - DIALOG_HEIGHT) / 2);
    }

    #[test]
    fn a_pane_smaller_than_the_dialog_does_not_give_a_negative_offset() {
        // `display-popup -x -3` is an error rather than a nudge.
        assert_eq!(dialog_at(0, 0, 10, 3), (0, 0));
        assert_eq!(dialog_at(5, 2, 4, 2), (5, 2));
    }

    #[test]
    fn the_title_carries_the_exit_status_and_its_colour() {
        assert!(dialog_title(0).contains("done"));
        assert!(dialog_title(0).contains("brightgreen"));
        assert!(dialog_title(3).contains("exit 3"));
        assert!(dialog_title(3).contains("brightred"));
    }

    #[test]
    fn the_buttons_are_a_ring() {
        // Three in a row are a ring in everybody's hands, and a Tab that
        // stops at the end is a Tab somebody presses twice.
        assert_eq!(step_button(0, 1), 1);
        assert_eq!(step_button(2, 1), 0);
        assert_eq!(step_button(0, -1), 2);
    }

    #[test]
    fn the_default_button_is_the_default_choice() {
        assert_eq!(BUTTONS[default_button(0)], Choice::Close);
        assert_eq!(BUTTONS[default_button(1)], Choice::Restart);
    }

    #[test]
    fn a_choice_survives_the_answer_file() {
        for choice in BUTTONS {
            assert_eq!(choice_of_word(choice_word(choice)), choice);
        }
        // A dialog that was killed rather than answered leaves the pane open
        // and read-only, so nothing is lost while somebody works out why.
        assert_eq!(choice_of_word(""), Choice::View);
        assert_eq!(choice_of_word("nonsense"), Choice::View);
    }

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
    fn the_split_is_aimed_at_the_pane_the_binding_named() {
        let args = split_args(Some("%12"), 1, "tc run --exec 'git log'");
        assert_eq!(
            args,
            vec![
                "split-window",
                "-fh",
                "-t",
                "%12",
                "-l",
                "1",
                "tc run --exec 'git log'"
            ]
        );
    }

    #[test]
    fn without_a_pane_the_split_is_left_to_tmux() {
        // Typed at a shell rather than pressed, where tmux's own current pane
        // is the right answer and $TMUX_PANE is already set.
        let args = split_args(None, 56, "tc run --exec 'ls'");
        assert!(!args.iter().any(|a| a == "-t"), "{args:?}");
        assert_eq!(args.first().map(String::as_str), Some("split-window"));
    }

    #[test]
    fn an_empty_pane_is_the_same_as_no_pane() {
        // `#{pane_id}` from a binding that fired outside a pane comes through
        // as an empty string, and `-t ''` is an error rather than a default.
        assert!(!split_args(Some(""), 1, "x").iter().any(|a| a == "-t"));
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
