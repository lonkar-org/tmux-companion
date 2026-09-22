//! The cheat sheet: four boxes in a 2x2 grid, showing the bindings somebody
//! wrote rather than the ones tmux ships.
//!
//! It reads the same rows the picker does, so a binding appears here the moment
//! its note is written and the two can never disagree about what exists.
//!
//! Within a box, the keys actually reached for come first, counted from the
//! usage log the picker appends to. A key never used sits at the bottom with no
//! count, which makes the sheet a list of what is going unused as well as a
//! list of what exists.
//!
//! Four boxes rather than four columns because a note like the copy-mode menu
//! runs to 90 characters, and a quarter of the width would cut it in half.

use std::collections::HashMap;

use crate::keys::KeyRow;

/// The prefix every binding written by hand carries.
pub const CUSTOM: &str = "custom: ";

/// The four box titles, in reading order.
pub const TITLES: [&str; 4] = [
    " Panes ",
    " Windows, sessions, projects ",
    " Copy mode, opening, searching ",
    " Config and help ",
];

/// Which box a note belongs in, from the word after `custom: `.
///
/// Anything unrecognised goes in the last box rather than being dropped: a
/// binding nobody categorised is still a binding, and an empty corner is a
/// worse answer than a slightly crowded one.
pub fn box_for(note: &str) -> usize {
    let group = note.split_whitespace().next().unwrap_or("");
    match group {
        "pane" => 0,
        "window" | "session" | "project" | "go" => 1,
        "copy" | "copy-mode" | "open" | "search" => 2,
        _ => 3,
    }
}

/// One line of a box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// How the chord is written.
    pub shown: String,
    /// The note, with `custom: ` taken off.
    pub note: String,
    /// How many times it has been picked.
    pub count: usize,
}

/// Sort the rows into boxes, most-used first.
///
/// The group word stays in the note on purpose: "open the selection" and
/// "search the line" lose their verb without it, and "pane focus down" only
/// repeats its box title, which costs nothing.
pub fn boxes(rows: &[KeyRow], usage: &HashMap<(String, String), usize>) -> [Vec<Entry>; 4] {
    let mut out: [Vec<Entry>; 4] = Default::default();
    for row in rows {
        let Some(note) = row.note.strip_prefix(CUSTOM) else {
            continue;
        };
        let count = usage
            .get(&(row.table.clone(), row.key.clone()))
            .copied()
            .unwrap_or(0);
        out[box_for(note)].push(Entry {
            shown: row.shown.clone(),
            note: note.to_string(),
            count,
        });
    }
    for entries in &mut out {
        // Most used first, then alphabetical ignoring case. Case matters here:
        // `open URL or file` and `open the selection` are neighbours a reader
        // expects in that order, and byte order puts every capital letter
        // first, which is how the sheet ends up looking sorted by nothing.
        entries.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.note.to_lowercase().cmp(&b.note.to_lowercase()))
                .then_with(|| a.note.cmp(&b.note))
        });
    }

    top_up(&mut out, rows);
    out
}

/// The fewest entries a box should have before tmux's own bindings are used
/// to fill it out.
const MIN_PER_BOX: usize = 3;

/// Fill a thin box with tmux's own noted bindings.
///
/// A config that binds one pane key leaves the Panes box with one line in it
/// and three quarters of the sheet blank, which reads as broken rather than as
/// sparse. tmux ships notes for about a hundred of its own bindings, and the
/// ones that belong in a thin box are better than the empty space.
///
/// Custom bindings keep their place at the top: they are the point of the
/// sheet, and these are only what is left over.
fn top_up(out: &mut [Vec<Entry>; 4], rows: &[KeyRow]) {
    let mut spare: [Vec<Entry>; 4] = Default::default();
    for row in rows {
        if row.note.starts_with(CUSTOM) {
            continue;
        }
        let Some(box_index) = box_for_own(&row.note) else {
            continue;
        };
        spare[box_index].push(Entry {
            shown: row.shown.clone(),
            note: row.note.clone(),
            count: 0,
        });
    }
    for (entries, mut extra) in out.iter_mut().zip(spare) {
        if entries.len() >= MIN_PER_BOX {
            continue;
        }
        // By how ordinary the binding is, then alphabetical. Straight
        // alphabetical put "Break pane to a new window" and "Clear the marked
        // pane" in the Panes box and left both splits out, which is the wrong
        // three to show somebody learning tmux.
        extra.sort_by_key(|e| (rank(&e.note), e.note.to_lowercase()));
        extra.dedup_by(|a, b| a.note == b.note);
        let room = MIN_PER_BOX.saturating_sub(entries.len());
        entries.extend(extra.into_iter().take(room));
    }
}

/// How ordinary one of tmux's own bindings is: lower is shown first.
///
/// An explicit order rather than a keyword test, because a test gets this
/// wrong in ways that are hard to see: "Break pane to a new window" contains
/// "new ", so a rule that promoted anything with "new" in it promoted exactly
/// the binding it was written to demote.
fn rank(note: &str) -> usize {
    const ORDER: [&str; 8] = [
        "split window",
        "select pane",
        "select window",
        "resize",
        "next window",
        "previous window",
        "new window",
        "copy mode",
    ];
    let n = note.to_lowercase();
    ORDER
        .iter()
        .position(|w| n.contains(w))
        .unwrap_or(ORDER.len())
}

/// Which box one of tmux's own notes belongs in, or `None` when it is not
/// clearly any of them.
///
/// tmux's notes are sentences rather than the `custom: group thing` shape, so
/// this reads the words instead of the first one. Unmatched goes nowhere: the
/// fourth box is for a binding somebody wrote and did not categorise, and
/// filling it with tmux's leftovers would bury them.
fn box_for_own(note: &str) -> Option<usize> {
    let n = note.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["pane", "split window", "layout", "zoom"]) {
        Some(0)
    } else if has(&["window", "session", "client"]) {
        Some(1)
    } else if has(&["copy", "search", "paste", "buffer"]) {
        Some(2)
    } else {
        None
    }
}

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

/// One entry line: the count when it has one, then the chord, then the note.
fn cell(entries: &[Entry], i: usize, width: usize) -> String {
    let Some(e) = entries.get(i) else {
        return pad("", width);
    };
    let mark = if e.count > 0 {
        format!("{:>3} ", e.count)
    } else {
        "    ".to_string()
    };
    pad(&format!("{mark}{} {}", pad(&e.shown, 15), e.note), width)
}

/// Render the whole sheet for a terminal of this size.
pub fn render(boxes: &[Vec<Entry>; 4], cols: usize, lines: usize) -> String {
    // Two boxes and a two-space gutter; two lines held back for the prompt.
    let box_w = (cols.saturating_sub(2)) / 2;
    let box_h = (lines.saturating_sub(2)) / 2;
    let inner = box_w.saturating_sub(4);
    let rows = box_h.saturating_sub(2);

    let mut out = String::new();
    for half in 0..2 {
        let (l, r) = (half * 2, half * 2 + 1);
        out.push_str(&format!(
            "┌{}{}┐  ┌{}{}┐\n",
            TITLES[l],
            "─".repeat(box_w.saturating_sub(2 + TITLES[l].chars().count())),
            TITLES[r],
            "─".repeat(box_w.saturating_sub(2 + TITLES[r].chars().count())),
        ));
        for i in 0..rows {
            out.push_str(&format!(
                "│ {} │  │ {} │\n",
                cell(&boxes[l], i, inner),
                cell(&boxes[r], i, inner),
            ));
        }
        out.push_str(&format!(
            "└{}┘  └{}┘\n",
            "─".repeat(box_w.saturating_sub(2)),
            "─".repeat(box_w.saturating_sub(2)),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(table: &str, key: &str, shown: &str, note: &str) -> KeyRow {
        KeyRow {
            table: table.into(),
            key: key.into(),
            shown: shown.into(),
            note: note.into(),
            command: "cmd".into(),
        }
    }

    #[test]
    fn the_group_word_decides_the_box() {
        assert_eq!(box_for("pane focus down"), 0);
        assert_eq!(box_for("window rename"), 1);
        assert_eq!(box_for("session switch"), 1);
        assert_eq!(box_for("copy the line"), 2);
        assert_eq!(box_for("open the selection"), 2);
        assert_eq!(box_for("config reload"), 3);
    }

    #[test]
    fn an_unrecognised_group_lands_in_the_last_box_rather_than_vanishing() {
        assert_eq!(box_for("frobnicate everything"), 3);
        assert_eq!(box_for(""), 3);
    }

    #[test]
    fn bindings_somebody_wrote_come_first_whatever_else_is_on_the_sheet() {
        // This used to assert that tmux's own bindings never appear at all,
        // which left a config with one pane binding showing one line and three
        // empty boxes. They appear now, under the written ones and only where
        // a box would otherwise be nearly empty.
        let rows = vec![
            row("prefix", "z", "prefix z", "custom: pane zoom"),
            row("prefix", "!", "prefix !", "Break pane to a new window"),
        ];
        let b = boxes(&rows, &HashMap::new());
        assert_eq!(b[0][0].note, "pane zoom", "{:?}", b[0]);
        assert!(b[0].len() > 1, "the box was not filled out: {:?}", b[0]);
    }

    #[test]
    fn the_custom_prefix_is_taken_off_the_note() {
        let rows = vec![row("prefix", "z", "prefix z", "custom: pane zoom")];
        let b = boxes(&rows, &HashMap::new());
        assert_eq!(b[0][0].note, "pane zoom");
    }

    #[test]
    fn the_keys_you_reach_for_come_first() {
        let rows = vec![
            row("prefix", "a", "prefix a", "custom: pane aaa"),
            row("prefix", "b", "prefix b", "custom: pane bbb"),
        ];
        let usage = HashMap::from([(("prefix".to_string(), "b".to_string()), 7)]);
        let b = boxes(&rows, &usage);
        assert_eq!(b[0][0].note, "pane bbb");
        assert_eq!(b[0][0].count, 7);
        assert_eq!(b[0][1].count, 0);
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
                "custom: open the selection",
            ),
            row(
                "my-keys",
                "o",
                "copy-mode  g o",
                "custom: open URL under the cursor",
            ),
        ];
        let b = boxes(&rows, &HashMap::new());
        assert_eq!(b[2][0].note, "open the selection");
        assert_eq!(b[2][1].note, "open URL under the cursor");
    }

    #[test]
    fn unused_keys_are_alphabetical_so_the_sheet_reads_as_a_list() {
        let rows = vec![
            row("prefix", "c", "prefix c", "custom: pane ccc"),
            row("prefix", "a", "prefix a", "custom: pane aaa"),
        ];
        let b = boxes(&rows, &HashMap::new());
        assert_eq!(b[0][0].note, "pane aaa");
        assert_eq!(b[0][1].note, "pane ccc");
    }

    #[test]
    fn a_used_key_shows_its_count_and_an_unused_one_shows_nothing() {
        let entries = vec![
            Entry {
                shown: "prefix z".into(),
                note: "pane zoom".into(),
                count: 3,
            },
            Entry {
                shown: "prefix x".into(),
                note: "pane close".into(),
                count: 0,
            },
        ];
        let used = cell(&entries, 0, 40);
        let unused = cell(&entries, 1, 40);
        assert!(used.starts_with("  3 "), "{used:?}");
        // The count column is four wide either way, so the chords line up
        // whether or not a binding has ever been reached for.
        assert!(unused.starts_with("    prefix"), "{unused:?}");
        assert_eq!(
            used.chars().position(|c| c == 'p'),
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
                    &format!("custom: pane number {i}"),
                )
            })
            .collect();
        let sheet = render(&boxes(&rows, &HashMap::new()), 120, 40);
        let widths: Vec<usize> = sheet.lines().map(|l| l.chars().count()).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "ragged sheet: {widths:?}"
        );
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
            "custom: pane a note that runs on and on and on past any sensible width",
        )];
        let sheet = render(&boxes(&rows, &HashMap::new()), 80, 30);
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

#[cfg(test)]
mod top_up_tests {
    use super::*;

    fn row(table: &str, key: &str, note: &str) -> KeyRow {
        KeyRow {
            shown: crate::keys::shown_for(table, key),
            table: table.to_string(),
            key: key.to_string(),
            note: note.to_string(),
            command: "whatever".to_string(),
        }
    }

    #[test]
    fn a_box_with_one_binding_is_filled_from_tmuxs_own() {
        // The shipped config binds one pane key, so the Panes box held one
        // line and three quarters of the sheet was blank, which reads as
        // broken rather than as sparse.
        let rows = vec![
            row("prefix", "z", "custom: pane zoom this one"),
            row("prefix", "%", "Split window horizontally"),
            row("prefix", "\"", "Split window vertically"),
            row("prefix", "!", "Break pane to a new window"),
        ];
        let boxes = boxes(&rows, &HashMap::new());
        let notes: Vec<&str> = boxes[0].iter().map(|e| e.note.as_str()).collect();
        assert_eq!(notes.len(), MIN_PER_BOX, "{notes:?}");
        assert_eq!(
            notes[0], "pane zoom this one",
            "custom goes first: {notes:?}"
        );
        assert!(
            notes.contains(&"Split window horizontally"),
            "the splits are the two to show: {notes:?}"
        );
    }

    #[test]
    fn a_full_box_is_left_alone() {
        let rows = vec![
            row("prefix", "a", "custom: pane one"),
            row("prefix", "b", "custom: pane two"),
            row("prefix", "c", "custom: pane three"),
            row("prefix", "%", "Split window horizontally"),
        ];
        let boxes = boxes(&rows, &HashMap::new());
        assert_eq!(boxes[0].len(), 3);
        assert!(boxes[0].iter().all(|e| !e.note.starts_with("Split")));
    }

    #[test]
    fn a_new_window_does_not_outrank_a_split() {
        // "Break pane to a new window" contains "new ", so the first attempt
        // at this promoted exactly the binding it was written to demote.
        assert!(rank("Split window horizontally") < rank("Break pane to a new window"));
        assert!(rank("Select pane to the left") < rank("Clear the marked pane"));
    }

    #[test]
    fn tmuxs_own_leftovers_do_not_fill_the_last_box() {
        // The fourth box is for a binding somebody wrote and did not
        // categorise. Filling it with tmux's unmatched notes would bury them.
        let rows = vec![
            row("prefix", "a", "custom: something uncategorised"),
            row("prefix", "t", "Show a clock"),
            row("prefix", "~", "Show messages"),
        ];
        let boxes = boxes(&rows, &HashMap::new());
        assert_eq!(boxes[3].len(), 1, "{:?}", boxes[3]);
    }
}
