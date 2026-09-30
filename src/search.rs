//! Searching the scrollback of every pane at once, and landing on the line.
//!
//! The question is "where did I see that", asked a minute after an error
//! scrolled past in one of nine panes. tmux's own search answers it for the
//! pane you are in, once you are in copy mode in it, which is the pane you
//! have to find first. `tmux-plugins/tmux-copycat` added stored searches to
//! that and has not been pushed since May 2023; `roosta/tmux-fuzzback` is the
//! fuzzy version and is one pane too. Both are worth installing on a machine
//! that does not run this.
//!
//! What is here is one row per line of scrollback, across every pane, newest
//! first, in the picker every other chooser uses. Enter moves the client to
//! the pane, puts it in copy mode scrolled to the line, and selects the line,
//! so the thing found is the thing highlighted and `y` copies it.
//!
//! Lines are physical lines, as the terminal drew them. `capture-pane -J`
//! would join a wrapped line back together and find a URL that broke across
//! two rows, and it would also make the index of a line in the capture
//! something other than its row in the history, which is the number the jump
//! needs. Landing on the right line was worth more than the wrapped match.
//!
//! The picker runs in the **client** and talks to tmux directly, for the
//! reasons `panes` does: nothing here is worth a daemon caching, and the
//! picker needs a terminal.

use std::collections::{HashMap, HashSet};

use regex::{Regex, RegexBuilder};

/// How many lines of each pane's history are read when nobody says.
pub const DEFAULT_LINES: u32 = 1000;

/// The most rows a list holds, newest kept.
///
/// Every row carries its own preview, so the list is memory as well as
/// matching time: twenty thousand rows of eleven-line context is tens of
/// megabytes for the second the popup is open, and past that a pattern on the
/// command line is the better tool than a longer list.
pub const MAX_ROWS: usize = 20_000;

/// Lines of context on each side of a hit in the preview.
const CONTEXT: usize = 5;

/// A stored search: a pattern somebody should not have to retype.
///
/// These are copycat's, less the ones that were bindings for its own prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Kind {
    /// `http://`, `https://`, `ssh://`, `git@host:`
    Url,
    /// A path with at least one slash, relative or absolute
    Path,
    /// A git object name, seven to forty hex digits
    Sha,
    /// An IPv4 address
    Ip,
}

impl Kind {
    /// The regular expression this kind stands for.
    pub fn pattern(self) -> &'static str {
        match self {
            Kind::Url => r"(https?|ssh|git|ftp)://[^\s<>\x22']+|git@[\w.-]+:[^\s<>\x22']+",
            Kind::Path => r"(^|[\s:=(\x22'])(~|\.{1,2})?/?[\w.@+-]+(/[\w.@+-]+)+",
            Kind::Sha => r"\b[0-9a-f]{7,40}\b",
            Kind::Ip => r"\b(\d{1,3}\.){3}\d{1,3}\b",
        }
    }
}

/// The pattern the rows are held against, or none when every line is a row.
///
/// Case is smart, the way the picker's own matching is: a pattern in lower
/// case matches either, and one capital letter makes it exact. `fixed` takes
/// the pattern as text, for an error message full of brackets.
pub fn matcher(
    pattern: Option<&str>,
    fixed: bool,
    kind: Option<Kind>,
) -> anyhow::Result<Option<Regex>> {
    if let Some(kind) = kind {
        // Hex digits in a hash are lower case and a path is whatever it is;
        // a stored search is matched as written.
        return Ok(Some(Regex::new(kind.pattern())?));
    }
    let Some(pattern) = pattern.map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok(None);
    };
    let source = if fixed {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    let exact = pattern.chars().any(char::is_uppercase);
    RegexBuilder::new(&source)
        .case_insensitive(!exact)
        .build()
        .map(Some)
        .map_err(|e| anyhow::anyhow!("`{pattern}` is not a pattern: {e}; --fixed takes it as text"))
}

/// How much history a pane holds and how tall it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Geometry {
    /// `#{history_size}`: lines above the screen.
    pub history: u32,
    /// `#{pane_height}`.
    pub height: u32,
}

/// The `-F` string [`parse_geometry`] reads.
pub fn geometry_format() -> &'static str {
    "#{pane_id}\t#{history_size}\t#{pane_height}"
}

/// Each pane's geometry, by pane id.
pub fn parse_geometry(text: &str) -> HashMap<String, Geometry> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let id = f.next()?.trim();
            let history = f.next()?.trim().parse().ok()?;
            let height = f.next()?.trim().parse().ok()?;
            (!id.is_empty()).then(|| (id.to_string(), Geometry { history, height }))
        })
        .collect()
}

/// One line of one pane that the list will show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Where the line is, in tmux's own numbering at the moment of the
    /// capture: 0 is the top row of the screen and history counts down from
    /// -1.
    pub y: i64,
    /// Its index in the capture, for the preview's context.
    pub index: usize,
    /// The text, trailing space gone.
    pub text: String,
}

/// The lines of one capture worth a row, newest first.
///
/// `history` is what the pane held and `asked` is how far back the capture
/// reached, so the first line of the capture is `-min(history, asked)`. A
/// blank line is nothing to find. A line that appears twice is kept once, at
/// its newest: a prompt drawn forty times is one row, and the one nearest the
/// bottom is the one somebody just saw.
pub fn lines_of(capture: &str, history: u32, asked: u32, matcher: Option<&Regex>) -> Vec<Line> {
    let above = i64::from(history.min(asked));
    let mut seen: HashSet<&str> = HashSet::new();
    capture
        .lines()
        .enumerate()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .filter_map(|(index, raw)| {
            let text = raw.trim_end();
            if text.trim().is_empty() || matcher.is_some_and(|m| !m.is_match(text)) {
                return None;
            }
            seen.insert(text).then(|| Line {
                y: index as i64 - above,
                index,
                text: text.to_string(),
            })
        })
        .collect()
}

/// The hit with the lines around it, the hit drawn reversed.
///
/// The picker's preview reads SGR, so the mark is the terminal's own rather
/// than a `>` in a gutter that would push the text out of line with itself.
pub fn context(capture: &[&str], index: usize) -> String {
    let from = index.saturating_sub(CONTEXT);
    let to = (index + CONTEXT + 1).min(capture.len());
    (from..to)
        .map(|i| {
            let text = capture[i].trim_end();
            if i == index {
                format!("\u{1b}[1;7m{text}\u{1b}[0m")
            } else {
                text.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where copy mode has to be for a line to be on screen with room above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// What `goto-line` takes: how many lines of history the view is scrolled
    /// back by.
    pub scroll: u32,
    /// The row of the view the line is then on, counted from the top.
    pub row: u32,
}

/// Where to put the view for a line captured at `y`.
///
/// `grown` is how many lines the pane has written since the capture, which
/// pushed the line that far up. A line still on the screen needs no scroll.
/// One in the history is put in the middle of the view rather than on its top
/// row, because what led up to an error is usually the reason for looking,
/// unless the history above it is too short to scroll that far.
pub fn place(y: i64, grown: u32, now: Geometry) -> Place {
    let y = y - i64::from(grown);
    if y >= 0 {
        let last_row = now.height.saturating_sub(1);
        return Place {
            scroll: 0,
            row: u32::try_from(y).unwrap_or(last_row).min(last_row),
        };
    }
    let above = u32::try_from(-y).unwrap_or(u32::MAX).min(now.history);
    let scroll = above.saturating_add(now.height / 2).min(now.history);
    Place {
        scroll,
        row: scroll - above,
    }
}

/// One row of the picker, or one line of `--print`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// `session:window.pane`.
    pub at: String,
    /// What the pane runs.
    pub program: String,
    /// The `%N` the jump targets.
    pub id: String,
    /// The line.
    pub line: Line,
    /// The history the pane held when it was read, to measure growth against.
    pub history: u32,
    /// The lines around the hit.
    pub preview: String,
}

/// What `search` was asked for.
#[derive(Debug, Clone, Default)]
pub struct Asked {
    /// The pattern, a regular expression unless `fixed`.
    pub pattern: Option<String>,
    /// Take the pattern as text.
    pub fixed: bool,
    /// A stored search in place of a pattern.
    pub kind: Option<Kind>,
    /// Only this session's panes.
    pub target: Option<String>,
    /// Only this pane.
    pub pane: Option<String>,
    /// How far back to read each pane.
    pub lines: u32,
    /// Print the rows and open nothing.
    pub print: bool,
    /// Go to the newest hit without asking.
    pub first: bool,
}

/// The panes to read, most recently active first.
///
/// The order is the list's order before anything is typed, so the pane that
/// drew last is at the top: what somebody is looking for is nearly always
/// something they saw a moment ago.
pub fn ordered(
    mut panes: Vec<crate::panes::Pane>,
    session: Option<&str>,
    pane: Option<&str>,
) -> Vec<crate::panes::Pane> {
    panes.retain(|p| session.is_none_or(|s| s == p.session) && pane.is_none_or(|id| id == p.id));
    panes.sort_by_key(|p| std::cmp::Reverse(p.activity));
    panes
}

/// Read every pane and build the rows.
async fn rows(asked: &Asked, matcher: Option<Regex>) -> Vec<Row> {
    let listing = ["list-panes", "-a", "-F", geometry_format()];
    let (panes, geometry) = tokio::join!(crate::panes::list(), crate::cli::tmux_capture(&listing));
    let geometry = parse_geometry(&geometry);
    let panes = ordered(panes, asked.target.as_deref(), asked.pane.as_deref());

    // Captured concurrently, for the reason `panes` captures its previews
    // that way: a tmux call per pane in sequence is a popup that opens late.
    let mut set = tokio::task::JoinSet::new();
    let start = format!("-{}", asked.lines);
    for (i, pane) in panes.iter().enumerate() {
        let (id, start) = (pane.id.clone(), start.clone());
        set.spawn(async move {
            let text =
                crate::cli::tmux_capture(&["capture-pane", "-p", "-t", &id, "-S", &start]).await;
            (i, text)
        });
    }
    let mut captures = vec![String::new(); panes.len()];
    while let Some(Ok((i, text))) = set.join_next().await {
        captures[i] = text;
    }

    let mut rows = Vec::new();
    for (pane, capture) in panes.iter().zip(&captures) {
        let held = geometry.get(&pane.id).copied().unwrap_or_default();
        let all: Vec<&str> = capture.lines().collect();
        for line in lines_of(capture, held.history, asked.lines, matcher.as_ref()) {
            if rows.len() >= MAX_ROWS {
                return rows;
            }
            rows.push(Row {
                at: format!("{}:{}.{}", pane.session, pane.window_index, pane.pane_index),
                program: crate::panes::program(pane),
                id: pane.id.clone(),
                preview: context(&all, line.index),
                line,
                history: held.history,
            });
        }
    }
    rows
}

/// Where a line is now, found again by its text in a fresh capture.
///
/// Counting how much the history grew says where a line went only while the
/// history can grow. A pane at its `history-limit` holds the same number of
/// lines however much it writes, so the count says nothing moved when
/// everything did. Reading the pane again and looking for the text has no
/// such blind spot.
///
/// A line only ever moves up, so anything below where it was is another line
/// that happens to say the same thing. Of what is left the one nearest to
/// `expected` is taken, which is where the growth count put it, and between
/// two equally near the newer. `None` when the text is nowhere at or above
/// where it was: the screen was redrawn, or the line has left the history.
pub fn relocate(
    capture: &str,
    history: u32,
    asked: u32,
    text: &str,
    was: i64,
    expected: i64,
) -> Option<i64> {
    let above = i64::from(history.min(asked));
    capture
        .lines()
        .enumerate()
        .filter(|(_, l)| l.trim_end() == text)
        .map(|(i, _)| i as i64 - above)
        .filter(|y| *y <= was)
        .min_by_key(|y| ((y - expected).abs(), -y))
}

/// Put the client on the pane, in copy mode, with the line selected.
///
/// The pane kept writing while the picker was open, and every line it wrote
/// moved the hit one row further up, so the pane is read again here and the
/// line looked for by its text; see [`relocate`]. When the text is not
/// found the growth of the history is what is left to go by.
///
/// A pane that was in copy mode already is taken out of it first. Copy mode
/// shows the pane as it was when the mode was entered, and a line written
/// since is not in that picture to be scrolled to.
pub async fn land(row: &Row, asked: u32) {
    let format = format!("{}\t#{{pane_in_mode}}", geometry_format());
    let answered =
        crate::cli::tmux_capture(&["display-message", "-p", "-t", &row.id, &format]).await;
    let in_mode = answered.trim_end().ends_with("\t1");
    let now = parse_geometry(&answered)
        .remove(&row.id)
        .unwrap_or_default();
    let grown = now.history.saturating_sub(row.history);

    let start = format!("-{asked}");
    let capture =
        crate::cli::tmux_capture(&["capture-pane", "-p", "-t", &row.id, "-S", &start]).await;
    let expected = row.line.y - i64::from(grown);
    let place = match relocate(
        &capture,
        now.history,
        asked,
        &row.line.text,
        row.line.y,
        expected,
    ) {
        Some(y) => place(y, 0, now),
        None => place(row.line.y, grown, now),
    };

    crate::panes::jump(&row.id).await;
    if in_mode {
        crate::cli::tmux(&["send-keys", "-X", "-t", &row.id, "cancel"]).await;
    }
    for args in land_commands(&row.id, place) {
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        crate::cli::tmux(&borrowed).await;
    }
}

/// The tmux commands that put a pane's copy mode on a place.
pub fn land_commands(id: &str, place: Place) -> Vec<Vec<String>> {
    let x = |command: &[&str]| -> Vec<String> {
        ["send-keys", "-X", "-t", id]
            .iter()
            .chain(command)
            .map(|s| s.to_string())
            .collect()
    };
    let mut out = vec![
        vec!["copy-mode".to_string(), "-t".to_string(), id.to_string()],
        x(&["goto-line", &place.scroll.to_string()]),
        x(&["top-line"]),
    ];
    if place.row > 0 {
        out.push(
            [
                "send-keys",
                "-X",
                "-N",
                &place.row.to_string(),
                "-t",
                id,
                "cursor-down",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        );
    }
    out.push(x(&["select-line"]));
    out
}

/// `search`: every line of every pane, and the pane and line picked.
pub async fn run(asked: Asked) -> anyhow::Result<()> {
    let matcher = matcher(asked.pattern.as_deref(), asked.fixed, asked.kind)?;
    if asked.first && matcher.is_none() {
        anyhow::bail!("--first needs a pattern or --kind: the newest line of all is no search");
    }
    let config = crate::cli::config_or_default();
    let rows = rows(&asked, matcher).await;

    if rows.is_empty() {
        crate::picker::say_nothing_to_show("nothing in the scrollback matches", asked.print).await;
        return Ok(());
    }
    if asked.print {
        for r in &rows {
            println!("{}\t{}\t{}\t{}", r.at, r.id, r.line.y, r.line.text);
        }
        return Ok(());
    }
    if asked.first {
        land(&rows[0], asked.lines).await;
        return Ok(());
    }

    let items: Vec<crate::picker::Item> = rows
        .iter()
        .map(|r| {
            crate::picker::Item::with_preview(
                format!("{} {} {}", r.at, r.program, r.line.text),
                r.preview.clone(),
            )
            // The line is what was searched for, so it is the one thing at
            // full strength; where it is sits in grey beside it.
            .in_cells(vec![
                crate::picker::Cell::dim(r.at.clone()),
                crate::picker::Cell::dim(r.program.clone()),
                crate::picker::Cell::plain(r.line.text.trim_start().to_string()),
            ])
        })
        .collect();

    let chrome = crate::picker::Chrome {
        title: "[ Scrollback ]".into(),
        icon: crate::tmux::icons::SEARCH.into(),
        footer: "enter goes to the line   ctrl-a clears the filter   esc cancels".into(),
        preview_title: "[ Around it ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Panes);

    if let Some(index) = crate::picker::run(items, "", &chrome)?
        && let Some(row) = rows.get(index)
    {
        land(row, asked.lines).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(lines: &[Line]) -> Vec<&str> {
        lines.iter().map(|l| l.text.as_str()).collect()
    }

    // ── the pattern ─────────────────────────────────────────────────────────

    #[test]
    fn no_pattern_is_no_matcher_and_every_line_is_a_row() {
        assert!(matcher(None, false, None).unwrap().is_none());
        assert!(matcher(Some("  "), false, None).unwrap().is_none());
    }

    #[test]
    fn case_is_smart_the_way_the_picker_is() {
        let lower = matcher(Some("error"), false, None).unwrap().unwrap();
        assert!(lower.is_match("ERROR: no such file"));
        assert!(lower.is_match("an error"));
        let exact = matcher(Some("Error"), false, None).unwrap().unwrap();
        assert!(exact.is_match("Error: x"));
        assert!(!exact.is_match("error: x"));
    }

    #[test]
    fn fixed_takes_brackets_as_text_and_a_bad_pattern_says_so() {
        let m = matcher(Some("error[E0308]"), true, None).unwrap().unwrap();
        assert!(m.is_match("error[E0308]: mismatched types"));
        assert!(!m.is_match("errorE"));
        let e = matcher(Some("error[E03"), false, None).unwrap_err();
        assert!(e.to_string().contains("--fixed"), "{e}");
    }

    #[test]
    fn the_stored_searches_find_what_they_are_named_for() {
        let is = |kind: Kind, text: &str| Regex::new(kind.pattern()).unwrap().is_match(text);
        assert!(is(Kind::Url, "see https://example.org/a?b=1 for it"));
        assert!(is(
            Kind::Url,
            "git@github.com:lonkar-org/tmux-companion.git"
        ));
        assert!(!is(Kind::Url, "no address here"));
        assert!(is(Kind::Path, "  --> src/segments/git.rs:12:5"));
        assert!(is(Kind::Path, "wrote ~/w/api/out.log"));
        assert!(is(Kind::Path, "/usr/local/bin/tmux"));
        assert!(!is(Kind::Path, "a word and another"));
        assert!(is(Kind::Sha, "commit afda9fb Make the nudge"));
        assert!(!is(Kind::Sha, "commit of nothing"));
        assert!(is(Kind::Ip, "listening on 127.0.0.1:8080"));
        assert!(!is(Kind::Ip, "version 1.2.3"));
    }

    // ── the geometry ────────────────────────────────────────────────────────

    #[test]
    fn the_format_and_the_parser_agree() {
        assert_eq!(geometry_format().split('\t').count(), 3);
        let g = parse_geometry("%1\t120\t32\n%2\t0\t10\nnonsense\n%3\tx\t1\n");
        assert_eq!(g.len(), 2);
        assert_eq!(
            g["%1"],
            Geometry {
                history: 120,
                height: 32
            }
        );
        assert_eq!(g["%2"].history, 0);
    }

    // ── the lines ───────────────────────────────────────────────────────────

    #[test]
    fn lines_come_newest_first_numbered_the_way_tmux_numbers_them() {
        // Two lines of history above a three-row screen.
        let capture = "one\ntwo\nthree\nfour\n\n";
        let got = lines_of(capture, 2, 1000, None);
        assert_eq!(texts(&got), vec!["four", "three", "two", "one"]);
        let ys: Vec<i64> = got.iter().map(|l| l.y).collect();
        assert_eq!(ys, vec![1, 0, -1, -2]);
        assert_eq!(got[0].index, 3);
    }

    #[test]
    fn a_capture_shorter_than_the_history_starts_where_it_was_asked_to() {
        // The pane holds 500 lines and the capture reached back 2.
        let got = lines_of("a\nb\nc\n", 500, 2, None);
        let ys: Vec<i64> = got.iter().map(|l| l.y).collect();
        assert_eq!(ys, vec![0, -1, -2]);
    }

    #[test]
    fn a_repeated_line_is_one_row_at_its_newest() {
        let capture = "demo % \nmake\nok\ndemo % \nmake\nfailed\ndemo % \n";
        let got = lines_of(capture, 0, 1000, None);
        assert_eq!(texts(&got), vec!["demo %", "failed", "make", "ok"]);
        assert_eq!(got[0].index, 6, "the prompt nearest the bottom");
        assert_eq!(got[2].index, 4, "the second make, not the first");
    }

    #[test]
    fn a_pattern_keeps_only_the_lines_it_matches() {
        let m = matcher(Some("fail"), false, None).unwrap();
        let got = lines_of("ok\nFAILED one\n\nfailed two\n", 0, 1000, m.as_ref());
        assert_eq!(texts(&got), vec!["failed two", "FAILED one"]);
    }

    #[test]
    fn the_context_marks_the_hit_and_stops_at_the_edges() {
        let capture: Vec<&str> = "a\nb\nc\nd\ne\nf\ng\nh".lines().collect();
        let got = context(&capture, 0);
        assert_eq!(got, "\u{1b}[1;7ma\u{1b}[0m\nb\nc\nd\ne\nf");
        let got = context(&capture, 7);
        assert!(got.starts_with("c\n"), "{got:?}");
        assert!(got.ends_with("\u{1b}[1;7mh\u{1b}[0m"), "{got:?}");
    }

    // ── the place ───────────────────────────────────────────────────────────

    const PANE: Geometry = Geometry {
        history: 500,
        height: 30,
    };

    #[test]
    fn a_line_on_the_screen_needs_no_scroll() {
        assert_eq!(place(0, 0, PANE), Place { scroll: 0, row: 0 });
        assert_eq!(place(12, 0, PANE), Place { scroll: 0, row: 12 });
        // Still on the screen after the pane wrote four more lines.
        assert_eq!(place(12, 4, PANE), Place { scroll: 0, row: 8 });
    }

    #[test]
    fn a_line_in_the_history_lands_in_the_middle_of_the_view() {
        // A hundred lines up: scrolled back 115, and the line is on row 15.
        assert_eq!(
            place(-100, 0, PANE),
            Place {
                scroll: 115,
                row: 15
            }
        );
    }

    #[test]
    fn a_line_that_scrolled_off_while_the_picker_was_open_is_followed() {
        // On row 3 when captured, and ten lines were written since.
        assert_eq!(
            place(3, 10, PANE),
            Place {
                scroll: 22,
                row: 15
            }
        );
    }

    #[test]
    fn the_top_of_the_history_is_as_far_as_the_view_goes() {
        // The oldest line there is cannot have fifteen rows put above it.
        assert_eq!(
            place(-500, 0, PANE),
            Place {
                scroll: 500,
                row: 0
            }
        );
        assert_eq!(
            place(-495, 0, PANE),
            Place {
                scroll: 500,
                row: 5
            }
        );
        // And a line the history no longer holds lands on the oldest.
        assert_eq!(
            place(-900, 0, PANE),
            Place {
                scroll: 500,
                row: 0
            }
        );
    }

    #[test]
    fn landing_is_copy_mode_a_scroll_a_row_and_a_selection() {
        let got = land_commands(
            "%7",
            Place {
                scroll: 115,
                row: 15,
            },
        );
        let got: Vec<String> = got.iter().map(|c| c.join(" ")).collect();
        assert_eq!(
            got,
            vec![
                "copy-mode -t %7",
                "send-keys -X -t %7 goto-line 115",
                "send-keys -X -t %7 top-line",
                "send-keys -X -N 15 -t %7 cursor-down",
                "send-keys -X -t %7 select-line",
            ]
        );
        // The top row needs no cursor movement, and `-N 0` is not a count
        // tmux takes.
        let got = land_commands("%7", Place { scroll: 0, row: 0 });
        assert_eq!(got.len(), 4);
    }

    // ── finding the line again ──────────────────────────────────────────────

    #[test]
    fn a_line_is_found_again_where_it_moved_to() {
        // Captured on row 3 of a pane with two lines of history; four lines
        // later the history holds six and the line is one row up in it.
        let capture = "a\nb\nc\nd\ne\nneedle\nf\ng\nh\ni\n";
        assert_eq!(relocate(capture, 6, 1000, "needle", 3, -1), Some(-1));
    }

    #[test]
    fn a_full_history_does_not_hide_the_move() {
        // The history is at its limit of four before and after, so the
        // growth count says the line is still on row 3. It is two rows up.
        let capture = "c\nd\ne\nneedle\nf\ng\nh\ni\n";
        assert_eq!(relocate(capture, 4, 1000, "needle", 3, 3), Some(-1));
    }

    #[test]
    fn a_line_that_says_the_same_lower_down_is_another_line() {
        // `make` was on row 0 and has been run again since, on row 5.
        let capture = "make\nok\n\n\n\nmake\n";
        assert_eq!(relocate(capture, 0, 1000, "make", 0, 0), Some(0));
        // And of two above, the one nearest to where it should be.
        let capture = "make\nx\nmake\ny\nz\n";
        assert_eq!(relocate(capture, 3, 1000, "make", 0, -1), Some(-1));
        assert_eq!(relocate(capture, 3, 1000, "make", 0, -3), Some(-3));
    }

    #[test]
    fn a_line_that_is_gone_is_not_found() {
        assert_eq!(relocate("a\nb\n", 0, 1000, "needle", 1, 1), None);
        // Only below where it was, which is a different line.
        assert_eq!(relocate("a\nb\nneedle\n", 0, 1000, "needle", 0, 0), None);
        assert_eq!(relocate("", 0, 1000, "needle", 0, 0), None);
    }

    #[test]
    fn trailing_space_on_the_screen_does_not_hide_a_match() {
        assert_eq!(relocate("needle   \n", 0, 1000, "needle", 0, 0), Some(0));
    }

    // ── the panes ───────────────────────────────────────────────────────────

    #[test]
    fn panes_are_read_most_recently_active_first_and_filtered() {
        let line = |session: &str, id: &str, activity: u64| {
            format!(
                "{session}\t1\tedit\t0\t{id}\tzsh\t/w\tlaptop\t1\t1\t1\t{activity}\t0\tlaptop\t0\n"
            )
        };
        let panes = || {
            crate::panes::parse(
                &[
                    line("api", "%1", 10),
                    line("web", "%2", 30),
                    line("api", "%3", 20),
                ]
                .concat(),
            )
        };
        let ids = |p: Vec<crate::panes::Pane>| p.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(ordered(panes(), None, None)), vec!["%2", "%3", "%1"]);
        assert_eq!(ids(ordered(panes(), Some("api"), None)), vec!["%3", "%1"]);
        assert_eq!(ids(ordered(panes(), None, Some("%1"))), vec!["%1"]);
        assert!(ordered(panes(), Some("nowhere"), None).is_empty());
    }
}
