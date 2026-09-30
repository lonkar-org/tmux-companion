//! The cheat sheet: four boxes in a 2x2 grid, showing the bindings you keep
//! having to look up, so you learn them and stop looking.
//!
//! It reads the same rows the picker does, so a binding appears here the moment
//! its note is written and the two can never disagree about what exists.
//!
//! The usage log counts picks made through the key search, which are bindings
//! somebody looked up instead of pressing. A binding looked up recently is one
//! still being learned, and goes at the top of its box with a mark. One with
//! lookups but none for `[usage] learned_after_days` has been learned, and
//! leaves the sheet. The bindings you wrote and never looked up follow the
//! marked ones, alphabetical.
//!
//! Three boxes hold the bindings noted `companion: `, by the word after it.
//! The fourth holds tmux's own and plugin bindings, and only the ones you have
//! looked up: tmux ships about a hundred noted defaults, and a sheet listing
//! them all is the man page again.
//!
//! Four boxes rather than four columns because a note like the copy-mode menu
//! runs to 90 characters, and a quarter of the width would cut it in half.

use std::collections::HashMap;

use crate::keys::{KeyRow, Use};

/// The prefix every binding written by hand carries.
pub const COMPANION: &str = "companion: ";

/// The four box titles, in reading order.
pub const TITLES: [&str; 4] = [
    " Panes, windows, sessions, projects ",
    " Copy mode, opening, searching ",
    " Config and help ",
    " tmux and plugins ",
];

/// The box that holds bindings without the `companion: ` note.
pub const OTHERS: usize = 3;

/// Which of the three companion boxes a note belongs in, from the word after
/// `companion: `.
///
/// Anything unrecognised goes in the config-and-help box rather than being
/// dropped: a binding nobody categorised is still a binding, and an empty
/// corner is a worse answer than a slightly crowded one.
pub fn box_for(note: &str) -> usize {
    let group = note.split_whitespace().next().unwrap_or("");
    match group {
        "pane" | "window" | "session" | "project" | "go" => 0,
        "copy" | "copy-mode" | "open" | "search" => 1,
        _ => 2,
    }
}

/// Where a binding stands with the person reading the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Looked up within the learning window: still being learned.
    Learn,
    /// Never looked up, or the usage log is off.
    Unused,
    /// Looked up before, but not within the learning window.
    Learned,
}

impl State {
    /// The word `--print` writes for it.
    pub fn word(self) -> &'static str {
        match self {
            State::Learn => "learn",
            State::Unused => "unused",
            State::Learned => "learned",
        }
    }
}

/// Whether the sheet learns, and against what clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Learning {
    /// The usage log is off: every binding is shown, none is ranked.
    Off,
    /// The log is on.
    On {
        /// Now, in unix seconds.
        now: u64,
        /// `[usage] learned_after_days`.
        learned_after_days: u32,
    },
}

/// Where one binding stands, from what the log says about it.
///
/// A binding whose picks carry no time, which is every pick written before
/// the log had times, reads as learned: a count with no date is no evidence
/// it is still being looked up, and the next lookup brings it back.
pub fn state(u: Option<&Use>, learning: Learning) -> State {
    let Learning::On {
        now,
        learned_after_days,
    } = learning
    else {
        return State::Unused;
    };
    let Some(u) = u.filter(|u| u.count > 0) else {
        return State::Unused;
    };
    if learned_after_days == 0 {
        return State::Learn;
    }
    let window = u64::from(learned_after_days) * 86_400;
    match u.last {
        Some(t) if now.saturating_sub(t) < window => State::Learn,
        _ => State::Learned,
    }
}

/// One line of a box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Which table the binding is in.
    pub table: String,
    /// The key, as tmux spells it.
    pub key: String,
    /// How the chord is written.
    pub shown: String,
    /// The note, with `companion: ` taken off; the command when there is no
    /// note.
    pub note: String,
    /// How many times it has been looked up. Zero unless `state` is `Learn`.
    pub count: usize,
    /// Where it stands.
    pub state: State,
}

/// The sheet before it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    /// The four boxes, in the order of [`TITLES`].
    pub boxes: [Vec<Entry>; 4],
    /// How many bindings were learned and left off.
    pub learned: usize,
    /// Whether the usage log is on.
    pub learning: bool,
}

/// Sort the rows into boxes: the ones still being looked up first, most
/// lookups first, then the ones never looked up, alphabetical.
///
/// The group word stays in the note on purpose: "open the selection" and
/// "search the line" lose their verb without it, and "pane focus down" only
/// repeats its box title, which costs nothing.
pub fn sheet(rows: &[KeyRow], usage: &HashMap<(String, String), Use>, learning: Learning) -> Sheet {
    let mut boxes: [Vec<Entry>; 4] = Default::default();
    let mut learned = 0;
    for row in rows {
        let u = usage.get(&(row.table.clone(), row.key.clone()));
        let st = state(u, learning);
        let companion = row.note.strip_prefix(COMPANION);
        // Only a binding somebody wrote is worth listing unlooked-up. tmux's
        // own and a plugin's get a line once they have been looked up.
        let which = match (companion, st) {
            (_, State::Learned) => {
                learned += 1;
                continue;
            }
            (Some(note), _) => box_for(note),
            (None, State::Learn) => OTHERS,
            (None, State::Unused) => continue,
        };
        let note = companion.unwrap_or_else(|| crate::keys::described(row));
        boxes[which].push(Entry {
            table: row.table.clone(),
            key: row.key.clone(),
            shown: row.shown.clone(),
            note: note.to_string(),
            count: if st == State::Learn {
                u.map_or(0, |u| u.count)
            } else {
                0
            },
            state: st,
        });
    }
    for entries in &mut boxes {
        // Looked-up first, most lookups first, then alphabetical ignoring
        // case. Case matters here: `open URL or file` and `open the
        // selection` are neighbours a reader expects in that order, and byte
        // order puts every capital letter first, which is how the sheet ends
        // up looking sorted by nothing.
        entries.sort_by(|a, b| {
            (a.state != State::Learn)
                .cmp(&(b.state != State::Learn))
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.note.to_lowercase().cmp(&b.note.to_lowercase()))
                .then_with(|| a.note.cmp(&b.note))
        });
    }
    Sheet {
        boxes,
        learned,
        learning: matches!(learning, Learning::On { .. }),
    }
}

/// The line under the sheet: how many were learned, or that nothing is
/// being learned at all. Empty when there is nothing to say.
pub fn footer(sheet: &Sheet) -> String {
    if !sheet.learning {
        return "usage log off: the sheet learns nothing until [usage] enabled = true".to_string();
    }
    if sheet.learned == 0 {
        return String::new();
    }
    format!("{} learned, off the sheet", sheet.learned)
}

/// `cheatsheet --print`: one tab-separated line per entry, box by box.
///
/// `box state count table key shown note`, where box is 1 to 4 in the order
/// of [`TITLES`] and state is `learn` or `unused`. Learned bindings are not
/// listed, the same as on the drawn sheet; the footer says how many.
pub fn tsv(sheet: &Sheet) -> String {
    let mut out = String::new();
    for (i, entries) in sheet.boxes.iter().enumerate() {
        for e in entries {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                i + 1,
                e.state.word(),
                e.count,
                e.table,
                e.key,
                e.shown,
                e.note
            ));
        }
    }
    out
}

/// The mark in front of a binding still being learned.
///
/// A glyph rather than bold or reverse video: the sheet pads every cell by
/// counting characters, and an escape sequence would count as columns it does
/// not take up.
pub const LEARN_MARK: char = '▸';

/// Pad or cut a string to exactly `width` columns.
///
/// Counting characters rather than bytes, which the awk this replaces could
/// not do: it measured box-drawing characters as three bytes each and built
/// every rule by count to avoid the problem. Here the problem does not arise.
fn pad(s: &str, width: usize) -> String {
    let mut out: String = s.chars().take(width).collect();
    let len = out.chars().count();
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
}

/// One entry line: the mark and count when it is still being learned, then
/// the chord, then the note.
fn cell(entries: &[Entry], i: usize, width: usize) -> String {
    let Some(e) = entries.get(i) else {
        return pad("", width);
    };
    let mark = if e.state == State::Learn {
        format!("{LEARN_MARK}{:>3} ", e.count)
    } else {
        "     ".to_string()
    };
    pad(&format!("{mark}{} {}", pad(&e.shown, 15), e.note), width)
}

/// A box's top rule with its title, cut to fit.
fn top(title: &str, box_w: usize) -> String {
    let inner = box_w.saturating_sub(2);
    let title: String = title.chars().take(inner).collect();
    let rule = "─".repeat(inner.saturating_sub(title.chars().count()));
    format!("┌{title}{rule}┐")
}

/// The icon each box's title carries on a terminal, in `TITLES` order.
const ICONS: [&str; 4] = [
    crate::tmux::icons::PANE,
    crate::tmux::icons::SEARCH,
    crate::tmux::icons::SETUP,
    crate::tmux::icons::KEY,
];

/// Render the whole sheet for a terminal of this size.
pub fn render(boxes: &[Vec<Entry>; 4], cols: usize, lines: usize) -> String {
    render_in(boxes, cols, lines, &crate::picker::Paint::plain())
}

/// A string cut into pieces at character positions.
fn pieces(s: &str, at: &[usize]) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut from = 0;
    for &to in at.iter().chain(std::iter::once(&chars.len())) {
        let to = to.clamp(from, chars.len());
        out.push(chars[from..to].iter().collect());
        from = to;
    }
    out
}

/// One entry line, painted: the learning mark in the accent, because it is
/// the one thing on the sheet that changes as you learn; the chord in bold,
/// because it is what you press; the group word in grey; and a binding
/// already learned grey as a whole, because you no longer need to read it.
///
/// Laid out plain first and inked after, by position, so the padding is
/// counted over what the terminal draws.
fn cell_in(entries: &[Entry], i: usize, width: usize, paint: &crate::picker::Paint) -> String {
    use crate::picker::Tone;
    let plain = cell(entries, i, width);
    let Some(e) = entries.get(i) else {
        return plain;
    };
    if !paint.escapes {
        return plain;
    }
    let group = e.note.split(' ').next().unwrap_or("").chars().count();
    let parts = pieces(&plain, &[5, 20, 21, 21 + group]);
    let tone = |t: Tone| {
        if e.state == State::Learned {
            Tone::Dim
        } else {
            t
        }
    };
    [
        (&parts[0], Tone::Accent),
        (&parts[1], tone(Tone::Strong)),
        (&parts[2], Tone::Plain),
        (&parts[3], Tone::Dim),
        (&parts[4], tone(Tone::Plain)),
    ]
    .iter()
    .map(|(text, t)| paint.ink(text, *t))
    .collect()
}

/// A top rule, painted: the rule in the frame colour and the title in the
/// accent.
fn top_in(title: &str, box_w: usize, paint: &crate::picker::Paint) -> String {
    use ratatui::style::Modifier;
    let plain = top(title, box_w);
    if !paint.escapes {
        return plain;
    }
    let shown = title.chars().take(box_w.saturating_sub(2)).count();
    let parts = pieces(&plain, &[1, 1 + shown]);
    format!(
        "{}{}{}",
        paint.ink_style(&parts[0], paint.frame()),
        paint.ink_style(
            &parts[1],
            paint
                .style(crate::picker::Tone::Accent)
                .add_modifier(Modifier::BOLD)
        ),
        paint.ink_style(&parts[2], paint.frame()),
    )
}

/// The whole sheet, painted. With [`crate::picker::Paint::plain`] this is
/// [`render`] exactly.
pub fn render_in(
    boxes: &[Vec<Entry>; 4],
    cols: usize,
    lines: usize,
    paint: &crate::picker::Paint,
) -> String {
    // Two boxes and a two-space gutter; two lines held back for the prompt.
    let box_w = (cols.saturating_sub(2)) / 2;
    let box_h = (lines.saturating_sub(2)) / 2;
    let inner = box_w.saturating_sub(4);
    let rows = box_h.saturating_sub(2);
    let frame = |s: &str| paint.ink_style(s, paint.frame());
    let title = |i: usize| -> String {
        let glyph = paint.glyph(ICONS[i]);
        if paint.escapes && !glyph.trim().is_empty() {
            format!(" {}{}", glyph.trim_end(), TITLES[i])
        } else {
            TITLES[i].to_string()
        }
    };

    let mut out = String::new();
    for half in 0..2 {
        let (l, r) = (half * 2, half * 2 + 1);
        out.push_str(&format!(
            "{}  {}\n",
            top_in(&title(l), box_w, paint),
            top_in(&title(r), box_w, paint)
        ));
        for i in 0..rows {
            out.push_str(&format!(
                "{} {} {}  {} {} {}\n",
                frame("│"),
                cell_in(&boxes[l], i, inner, paint),
                frame("│"),
                frame("│"),
                cell_in(&boxes[r], i, inner, paint),
                frame("│"),
            ));
        }
        let bottom = format!("└{}┘", "─".repeat(box_w.saturating_sub(2)));
        out.push_str(&format!("{}  {}\n", frame(&bottom), frame(&bottom)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;
    const NOW: u64 = 1_790_000_000;

    fn on() -> Learning {
        Learning::On {
            now: NOW,
            learned_after_days: 14,
        }
    }

    fn row(table: &str, key: &str, shown: &str, note: &str) -> KeyRow {
        KeyRow {
            table: table.into(),
            key: key.into(),
            shown: shown.into(),
            note: note.into(),
            command: "cmd".into(),
        }
    }

    fn used(count: usize, days_ago: u64) -> Use {
        Use {
            count,
            first: Some(NOW - days_ago * DAY),
            last: Some(NOW - days_ago * DAY),
        }
    }

    fn log(entries: &[(&str, &str, Use)]) -> HashMap<(String, String), Use> {
        entries
            .iter()
            .map(|(t, k, u)| ((t.to_string(), k.to_string()), *u))
            .collect()
    }

    fn notes(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.note.as_str()).collect()
    }

    #[test]
    fn the_group_word_decides_the_box() {
        assert_eq!(box_for("pane focus down"), 0);
        assert_eq!(box_for("window rename"), 0);
        assert_eq!(box_for("session switch"), 0);
        assert_eq!(box_for("project open"), 0);
        assert_eq!(box_for("go home"), 0);
        assert_eq!(box_for("copy the line"), 1);
        assert_eq!(box_for("copy-mode enter"), 1);
        assert_eq!(box_for("open the selection"), 1);
        assert_eq!(box_for("search the line"), 1);
        assert_eq!(box_for("config reload"), 2);
        assert_eq!(box_for("help keys"), 2);
    }

    #[test]
    fn an_unrecognised_group_lands_in_config_and_help_rather_than_vanishing() {
        assert_eq!(box_for("frobnicate everything"), 2);
        assert_eq!(box_for(""), 2);
    }

    #[test]
    fn a_lookup_inside_the_window_is_learn_and_at_the_boundary_is_learned() {
        let s = |days_ago| state(Some(&used(3, days_ago)), on());
        assert_eq!(s(0), State::Learn);
        assert_eq!(s(13), State::Learn);
        assert_eq!(
            state(
                Some(&Use {
                    count: 1,
                    first: Some(NOW - 14 * DAY + 1),
                    last: Some(NOW - 14 * DAY + 1),
                }),
                on()
            ),
            State::Learn,
            "one second inside fourteen days"
        );
        assert_eq!(s(14), State::Learned, "exactly fourteen days");
        assert_eq!(s(30), State::Learned);
        assert_eq!(state(None, on()), State::Unused);
    }

    #[test]
    fn a_lookup_with_no_time_counts_as_learned() {
        // What every pick written before the log had times reads as.
        let old = Use {
            count: 40,
            first: None,
            last: None,
        };
        assert_eq!(state(Some(&old), on()), State::Learned);
    }

    #[test]
    fn zero_days_means_nothing_is_ever_learned() {
        let never = Learning::On {
            now: NOW,
            learned_after_days: 0,
        };
        assert_eq!(state(Some(&used(1, 900)), never), State::Learn);
        let old = Use {
            count: 2,
            first: None,
            last: None,
        };
        assert_eq!(state(Some(&old), never), State::Learn);
    }

    #[test]
    fn a_clock_behind_the_log_does_not_make_a_lookup_learned() {
        let future = Use {
            count: 1,
            first: Some(NOW + DAY),
            last: Some(NOW + DAY),
        };
        assert_eq!(state(Some(&future), on()), State::Learn);
    }

    #[test]
    fn learn_these_come_first_most_lookups_first_then_the_unused_alphabetical() {
        let rows = vec![
            row("prefix", "a", "prefix a", "companion: pane aaa"),
            row("prefix", "b", "prefix b", "companion: pane bbb"),
            row("prefix", "c", "prefix c", "companion: pane ccc"),
            row("prefix", "d", "prefix d", "companion: pane ddd"),
        ];
        let usage = log(&[("prefix", "c", used(2, 1)), ("prefix", "d", used(9, 3))]);
        let s = sheet(&rows, &usage, on());
        assert_eq!(
            notes(&s.boxes[0]),
            vec!["pane ddd", "pane ccc", "pane aaa", "pane bbb"]
        );
        assert_eq!(s.boxes[0][0].state, State::Learn);
        assert_eq!(s.boxes[0][0].count, 9);
        assert_eq!(s.boxes[0][2].state, State::Unused);
        assert_eq!(s.boxes[0][2].count, 0);
    }

    #[test]
    fn a_learned_binding_leaves_the_sheet_and_is_counted() {
        let rows = vec![
            row("prefix", "a", "prefix a", "companion: pane aaa"),
            row("prefix", "b", "prefix b", "companion: pane bbb"),
            row("prefix", "%", "prefix %", "Split window horizontally"),
        ];
        let usage = log(&[("prefix", "a", used(5, 20)), ("prefix", "%", used(3, 40))]);
        let s = sheet(&rows, &usage, on());
        assert_eq!(notes(&s.boxes[0]), vec!["pane bbb"]);
        assert!(s.boxes[OTHERS].is_empty(), "{:?}", s.boxes[OTHERS]);
        assert_eq!(s.learned, 2);
        assert_eq!(footer(&s), "2 learned, off the sheet");
    }

    #[test]
    fn a_looked_up_tmux_default_goes_in_the_fourth_box_with_its_own_note() {
        let rows = vec![
            row("prefix", "z", "prefix z", "companion: pane zoom"),
            row("prefix", "%", "prefix %", "Split window horizontally"),
        ];
        let usage = log(&[("prefix", "%", used(4, 2))]);
        let s = sheet(&rows, &usage, on());
        assert_eq!(notes(&s.boxes[OTHERS]), vec!["Split window horizontally"]);
        assert_eq!(s.boxes[OTHERS][0].state, State::Learn);
        assert_eq!(notes(&s.boxes[0]), vec!["pane zoom"]);
    }

    #[test]
    fn a_tmux_default_nobody_looked_up_never_appears() {
        // The old sheet filled a thin box from tmux's own notes. That is
        // the man page again, and it buried the point of the sheet.
        let rows = vec![
            row("prefix", "z", "prefix z", "companion: pane zoom"),
            row("prefix", "%", "prefix %", "Split window horizontally"),
            row("prefix", "\"", "prefix \"", "Split window vertically"),
            row("prefix", "t", "prefix t", "Show a clock"),
        ];
        let s = sheet(&rows, &HashMap::new(), on());
        let every: Vec<&str> = s.boxes.iter().flat_map(|b| notes(b)).collect();
        assert_eq!(every, vec!["pane zoom"]);
    }

    #[test]
    fn a_plugin_binding_with_no_note_shows_its_command() {
        let mut plugin = row("prefix", "C-s", "prefix C-s", "");
        plugin.command = "run-shell ~/.tmux/plugins/tmux-resurrect/scripts/save.sh".into();
        let usage = log(&[("prefix", "C-s", used(1, 0))]);
        let s = sheet(&[plugin], &usage, on());
        assert_eq!(
            notes(&s.boxes[OTHERS]),
            vec!["run-shell ~/.tmux/plugins/tmux-resurrect/scripts/save.sh"]
        );
    }

    #[test]
    fn with_the_log_off_every_companion_binding_shows_unranked() {
        let rows = vec![
            row("prefix", "c", "prefix c", "companion: pane ccc"),
            row("prefix", "a", "prefix a", "companion: pane aaa"),
            row("prefix", "o", "prefix o", "companion: open it"),
            row("prefix", "%", "prefix %", "Split window horizontally"),
        ];
        // A log left over from when it was on is not read as anything.
        let usage = log(&[("prefix", "c", used(9, 0)), ("prefix", "%", used(9, 0))]);
        let s = sheet(&rows, &usage, Learning::Off);
        assert_eq!(notes(&s.boxes[0]), vec!["pane aaa", "pane ccc"]);
        assert_eq!(notes(&s.boxes[1]), vec!["open it"]);
        assert!(s.boxes[OTHERS].is_empty());
        assert!(s.boxes.iter().flatten().all(|e| e.state == State::Unused));
        assert_eq!(s.learned, 0);
        assert!(footer(&s).contains("[usage] enabled"), "{}", footer(&s));
    }

    #[test]
    fn nothing_learned_says_nothing() {
        let rows = vec![row("prefix", "a", "prefix a", "companion: pane aaa")];
        assert_eq!(footer(&sheet(&rows, &HashMap::new(), on())), "");
    }

    #[test]
    fn the_custom_prefix_is_taken_off_the_note() {
        let rows = vec![row("prefix", "z", "prefix z", "companion: pane zoom")];
        let s = sheet(&rows, &HashMap::new(), on());
        assert_eq!(s.boxes[0][0].note, "pane zoom");
    }

    #[test]
    fn alphabetical_ordering_ignores_case() {
        // Byte order puts every capital first, which makes a box look sorted
        // by nothing at all.
        let rows = vec![
            row(
                "my-keys",
                "O",
                "copy-mode  g O",
                "companion: open the selection",
            ),
            row(
                "my-keys",
                "o",
                "copy-mode  g o",
                "companion: open URL under the cursor",
            ),
        ];
        let s = sheet(&rows, &HashMap::new(), on());
        assert_eq!(
            notes(&s.boxes[1]),
            vec!["open the selection", "open URL under the cursor"]
        );
    }

    #[test]
    fn print_writes_one_line_per_entry_with_its_box_and_state() {
        let rows = vec![
            row("prefix", "a", "prefix a", "companion: pane aaa"),
            row("prefix", "%", "prefix %", "Split window horizontally"),
        ];
        let usage = log(&[("prefix", "%", used(2, 1))]);
        let text = tsv(&sheet(&rows, &usage, on()));
        assert_eq!(
            text,
            "1\tunused\t0\tprefix\ta\tprefix a\tpane aaa\n\
             4\tlearn\t2\tprefix\t%\tprefix %\tSplit window horizontally\n"
        );
    }

    fn entry(note: &str, count: usize, state: State) -> Entry {
        Entry {
            table: "prefix".into(),
            key: "z".into(),
            shown: "prefix z".into(),
            note: note.into(),
            count,
            state,
        }
    }

    #[test]
    fn a_binding_being_learned_shows_the_mark_and_its_count() {
        let entries = vec![
            entry("pane zoom", 3, State::Learn),
            entry("pane close", 0, State::Unused),
        ];
        let learn = cell(&entries, 0, 40);
        let unused = cell(&entries, 1, 40);
        assert!(learn.starts_with("▸  3 "), "{learn:?}");
        // The mark column is five wide either way, so the chords line up
        // whether or not a binding has been looked up.
        assert!(unused.starts_with("     prefix"), "{unused:?}");
        assert_eq!(
            learn.chars().position(|c| c == 'p'),
            unused.chars().position(|c| c == 'p'),
            "chords must start in the same column"
        );
    }

    #[test]
    fn a_cell_past_the_end_of_a_box_is_blank_rather_than_missing() {
        // Boxes hold different numbers of rows, so every grid has holes in it.
        let cell = cell(&[], 0, 12);
        assert_eq!(cell.chars().count(), 12);
        assert_eq!(cell.trim(), "");
    }

    #[test]
    fn every_line_of_the_sheet_is_the_same_width() {
        // The awk this replaces counted bytes, so a box-drawing character
        // measured three and every rule had to be built by count rather than
        // measured. Counting characters is what makes this assertion possible.
        let rows: Vec<KeyRow> = (0..6)
            .map(|i| {
                row(
                    "prefix",
                    &i.to_string(),
                    &format!("prefix {i}"),
                    &format!("companion: pane number {i}"),
                )
            })
            .collect();
        let usage = log(&[("prefix", "2", used(3, 1))]);
        for cols in [120, 80, 50, 30] {
            let sheet = render(&sheet(&rows, &usage, on()).boxes, cols, 40);
            let widths: Vec<usize> = sheet.lines().map(|l| l.chars().count()).collect();
            assert!(
                widths.windows(2).all(|w| w[0] == w[1]),
                "ragged sheet at {cols}: {widths:?}"
            );
        }
    }

    #[test]
    fn the_sheet_holds_two_lines_back_for_the_prompt() {
        let sheet = render(&Default::default(), 120, 40);
        assert!(
            sheet.lines().count() <= 38,
            "{} lines in 40",
            sheet.lines().count()
        );
    }

    #[test]
    fn a_long_note_is_cut_rather_than_wrapping_the_box() {
        let rows = vec![row(
            "prefix",
            "m",
            "prefix m",
            "companion: pane a note that runs on and on and on past any sensible width",
        )];
        let sheet = render(&sheet(&rows, &HashMap::new(), on()).boxes, 80, 30);
        let widths: Vec<usize> = sheet.lines().map(|l| l.chars().count()).collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}");
    }

    #[test]
    fn a_tiny_terminal_does_not_panic() {
        // popup geometry is somebody else's decision.
        let _ = render(&Default::default(), 10, 6);
        let _ = render(&Default::default(), 0, 0);
    }

    #[test]
    fn the_titles_are_drawn() {
        let sheet = render(&Default::default(), 120, 40);
        for title in TITLES {
            assert!(sheet.contains(title.trim()), "{title} missing");
        }
    }
}
