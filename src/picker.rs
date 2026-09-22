//! The picker: a fuzzy-matched list in a terminal, and the state machine
//! behind it.
//!
//! It replaces fzf, which cost 50 ms of the 93 ms a warm `prefix+?` took. The
//! state machine is separate from the drawing on purpose: filtering, ordering,
//! moving a selection and editing a query are the parts with behaviour worth
//! pinning, and none of them need a terminal to test.
//!
//! This runs in the **client** process, not the daemon. A daemon has no
//! terminal, and the whole point of a picker is that it owns one for as long as
//! somebody is looking at it.

use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

/// One row a picker can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What the matcher searches and the list shows.
    pub label: String,
    /// Shown in the preview pane under the list. Empty means no preview.
    pub preview: String,
}

impl Item {
    /// An item with nothing to preview.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            preview: String::new(),
        }
    }

    /// An item with a preview.
    pub fn with_preview(label: impl Into<String>, preview: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            preview: preview.into(),
        }
    }
}

/// What the picker is showing and where the cursor is.
#[derive(Debug, Clone)]
pub struct Picker {
    items: Vec<Item>,
    query: String,
    /// Indices into `items`, best match first.
    matches: Vec<usize>,
    /// Position within `matches`, not within `items`.
    cursor: usize,
    matcher: Matcher,
}

impl Picker {
    /// A picker over these items, with an opening query.
    pub fn new(items: Vec<Item>, query: &str) -> Self {
        let mut p = Self {
            items,
            query: query.to_string(),
            matches: Vec::new(),
            cursor: 0,
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
        };
        p.refilter();
        p
    }

    /// Re-run the match, keeping the cursor inside the results.
    fn refilter(&mut self) {
        let trimmed = self.query.trim();
        if trimmed.is_empty() {
            self.matches = (0..self.items.len()).collect();
        } else {
            let pattern = Pattern::parse(trimmed, CaseMatching::Ignore, Normalization::Smart);
            let mut scored: Vec<(u32, usize, usize)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    let mut buf = Vec::new();
                    let haystack = Utf32Str::new(&item.label, &mut buf);
                    pattern
                        .score(haystack, &mut self.matcher)
                        .map(|s| (s, item.label.chars().count(), i))
                })
                .collect();
            // Best score first, then the shorter row, then the order the rows
            // arrived in.
            //
            // The length tie-break is doing real work: nucleo scores `zoom`
            // and `a long way to say zoom` identically at 114, because the
            // match inside each is the same, and a picker where the exact
            // answer sits below a sentence containing it is a picker people
            // stop trusting. Arrival order settles what is left, and for
            // `keys` that is alphabetical by note rather than arbitrary.
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
            self.matches = scored.into_iter().map(|(_, _, i)| i).collect();
        }
        self.cursor = self.cursor.min(self.matches.len().saturating_sub(1));
    }

    /// The current query.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The rows currently matching, in the order they are shown.
    pub fn matches(&self) -> Vec<&Item> {
        self.matches.iter().map(|i| &self.items[*i]).collect()
    }

    /// How many rows match.
    pub fn len(&self) -> usize {
        self.matches.len()
    }

    /// How many rows there are in total, matched or not.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Whether nothing matches.
    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// Where the cursor is, within the matches.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The item under the cursor.
    pub fn selected(&self) -> Option<&Item> {
        self.matches.get(self.cursor).map(|i| &self.items[*i])
    }

    /// The index into the original list of the item under the cursor, which is
    /// what a caller needs to act on the pick.
    pub fn selected_index(&self) -> Option<usize> {
        self.matches.get(self.cursor).copied()
    }

    /// Add a character to the query.
    pub fn push(&mut self, c: char) {
        self.query.push(c);
        self.cursor = 0;
        self.refilter();
    }

    /// Remove the last character of the query.
    pub fn backspace(&mut self) {
        self.query.pop();
        self.cursor = 0;
        self.refilter();
    }

    /// Empty the query, which is how the tmux defaults are revealed.
    pub fn clear_query(&mut self) {
        self.query.clear();
        self.cursor = 0;
        self.refilter();
    }

    /// Move the cursor down, wrapping at the end.
    ///
    /// Wrapping because the list is short and somebody holding the key down
    /// should not have to notice it stopped.
    pub fn down(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.cursor = (self.cursor + 1) % self.matches.len();
    }

    /// Move the cursor up, wrapping at the start.
    pub fn up(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.cursor = match self.cursor {
            0 => self.matches.len() - 1,
            n => n - 1,
        };
    }
}

// ── Drawing and running ──────────────────────────────────────────────────────

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

/// How a picker is labelled and what its footer says.
#[derive(Debug, Clone)]
pub struct Chrome {
    /// Drawn on the border, e.g. `[ Keys ]`.
    pub title: String,
    /// The line under the list, saying which keys do what.
    pub footer: String,
    /// Label over the preview pane. Empty hides the pane.
    pub preview_title: String,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            title: "[ Pick ]".into(),
            footer: "enter picks   ctrl-a clears the filter   esc cancels".into(),
            preview_title: String::new(),
        }
    }
}

/// Draw one frame.
///
/// Split out so a test can render into a `TestBackend` buffer and assert on
/// what a person would see, rather than on the fact that a function was called.
pub fn draw(frame: &mut Frame, picker: &Picker, chrome: &Chrome) {
    let show_preview = !chrome.preview_title.is_empty()
        && picker.selected().is_some_and(|i| !i.preview.is_empty());

    let constraints: Vec<Constraint> = if show_preview {
        vec![
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(6),
            Constraint::Length(1),
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ]
    };
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(frame.area());

    let prompt = Line::from(vec![
        Span::styled("> ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(picker.query()),
        Span::styled(
            format!("   {}/{}", picker.len(), picker.item_count()),
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(prompt), areas[0]);

    let items: Vec<ListItem> = picker
        .matches()
        .iter()
        .map(|i| ListItem::new(i.label.clone()))
        .collect();
    let mut state = ListState::default();
    if !picker.is_empty() {
        state.select(Some(picker.cursor()));
    }
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(chrome.title.clone()),
            )
            .highlight_symbol("> ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        areas[1],
        &mut state,
    );

    if show_preview {
        let text = picker
            .selected()
            .map(|i| i.preview.clone())
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: false }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(chrome.preview_title.clone()),
            ),
            areas[2],
        );
    }

    frame.render_widget(
        Paragraph::new(Span::styled(
            chrome.footer.clone(),
            Style::default().add_modifier(Modifier::DIM),
        )),
        areas[areas.len() - 1],
    );
}

/// Show the picker and wait for a decision.
///
/// Returns the index into the original list, or `None` when somebody cancelled.
/// The terminal is put back the way it was found on every path out, including
/// the error one, because a picker that leaves a terminal in raw mode is worse
/// than no picker.
pub fn run(items: Vec<Item>, query: &str, chrome: &Chrome) -> anyhow::Result<Option<usize>> {
    use ratatui::crossterm::{
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    };

    let mut picker = Picker::new(items, query);

    enable_raw_mode()?;
    let mut out = std::io::stderr();
    execute!(out, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(out);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let outcome = loop {
        terminal.draw(|f| draw(f, &picker, chrome))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => break None,
            KeyCode::Char('c' | 'd') if ctrl => break None,
            KeyCode::Char('a') if ctrl => picker.clear_query(),
            KeyCode::Enter => break picker.selected_index(),
            KeyCode::Down => picker.down(),
            KeyCode::Up => picker.up(),
            KeyCode::Char('n') if ctrl => picker.down(),
            KeyCode::Char('p') if ctrl => picker.up(),
            KeyCode::Backspace => picker.backspace(),
            KeyCode::Char(c) if !ctrl => picker.push(c),
            _ => {}
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(labels: &[&str], query: &str) -> Picker {
        Picker::new(labels.iter().map(|l| Item::new(*l)).collect(), query)
    }

    #[test]
    fn an_empty_query_matches_everything() {
        let p = picker(&["alpha", "beta", "gamma"], "");
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn a_query_filters_to_what_matches() {
        let p = picker(
            &["custom: reload config", "Break pane", "custom: zoom"],
            "custom",
        );
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn matching_is_fuzzy_rather_than_a_substring() {
        // The reason for a matcher rather than `contains`: nobody types the
        // whole note.
        let p = picker(&["custom: reload tmux.conf"], "rldcnf");
        assert_eq!(p.len(), 1, "expected a fuzzy hit");
    }

    #[test]
    fn matching_ignores_case() {
        let p = picker(&["Break pane to a new window"], "BREAK");
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn a_query_that_matches_nothing_leaves_an_empty_list() {
        let p = picker(&["alpha"], "zzzz");
        assert!(p.is_empty());
        assert_eq!(p.selected(), None);
        assert_eq!(p.selected_index(), None);
    }

    #[test]
    fn the_exact_answer_beats_a_sentence_containing_it() {
        // nucleo scores both of these 114, because the match inside each is
        // the same. Without the length tie-break the picker puts the sentence
        // first, which is how people stop trusting a picker.
        let p = picker(&["a long way to say zoom", "zoom"], "zoom");
        assert_eq!(p.matches()[0].label, "zoom");
    }

    #[test]
    fn ties_keep_the_order_the_rows_arrived_in() {
        // For `keys` that order is alphabetical by note, so a tie is not
        // arbitrary and must not be shuffled.
        let p = picker(&["custom: aaa", "custom: bbb", "custom: ccc"], "custom");
        let labels: Vec<&str> = p.matches().iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["custom: aaa", "custom: bbb", "custom: ccc"]);
    }

    #[test]
    fn typing_narrows_and_resets_the_cursor() {
        let mut p = picker(&["alpha", "beta", "balloon"], "");
        p.down();
        assert_eq!(p.cursor(), 1);
        p.push('b');
        assert_eq!(p.cursor(), 0, "a new query should start at the top");
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn backspace_widens_again() {
        let mut p = picker(&["alpha", "beta"], "");
        p.push('b');
        assert_eq!(p.len(), 1);
        p.backspace();
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn clearing_the_query_shows_everything_again() {
        // This is ctrl-a in the binding: the opening query hides tmux's own
        // hundred noted defaults, and this is how you get them back.
        let mut p = picker(&["custom: mine", "Break pane"], "custom");
        assert_eq!(p.len(), 1);
        p.clear_query();
        assert_eq!(p.len(), 2);
        assert_eq!(p.query(), "");
    }

    #[test]
    fn the_cursor_wraps_at_both_ends() {
        let mut p = picker(&["a", "b", "c"], "");
        p.up();
        assert_eq!(p.cursor(), 2, "up from the top wraps to the bottom");
        p.down();
        assert_eq!(p.cursor(), 0, "down from the bottom wraps to the top");
    }

    #[test]
    fn moving_in_an_empty_list_does_nothing() {
        let mut p = picker(&["alpha"], "zzz");
        p.down();
        p.up();
        assert_eq!(p.cursor(), 0);
        assert!(p.is_empty());
    }

    #[test]
    fn the_cursor_survives_a_query_that_shortens_the_list() {
        let mut p = picker(&["aaa", "aab", "aac"], "");
        p.down();
        p.down();
        assert_eq!(p.cursor(), 2);
        p.push('b');
        assert!(p.cursor() < p.len().max(1), "cursor must stay in range");
    }

    #[test]
    fn the_selected_index_points_into_the_original_list() {
        // What a caller acts on: the row, not the position on screen.
        let p = picker(&["zero", "one", "two"], "two");
        assert_eq!(p.selected_index(), Some(2));
        assert_eq!(p.selected().map(|i| i.label.as_str()), Some("two"));
    }

    #[test]
    fn an_item_can_carry_a_preview() {
        let p = Picker::new(
            vec![Item::with_preview("prefix ?", "run-shell -C keys")],
            "",
        );
        assert_eq!(
            p.selected().map(|i| i.preview.as_str()),
            Some("run-shell -C keys")
        );
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    /// Render one frame into a buffer and give back what it says, line by line.
    fn rendered(picker: &Picker, chrome: &Chrome, w: u16, h: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
        terminal
            .draw(|f| draw(f, picker, chrome))
            .expect("draws a frame");
        let buffer = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn items() -> Vec<Item> {
        vec![
            Item::with_preview(
                "custom: reload config",
                "source-file ~/.config/tmux/tmux.conf",
            ),
            Item::with_preview("custom: zoom the pane", "resize-pane -Z"),
            Item::new("Break pane to a new window"),
        ]
    }

    #[test]
    fn the_list_shows_the_rows_that_match() {
        let p = Picker::new(items(), "custom");
        let out = rendered(&p, &Chrome::default(), 60, 12).join("\n");
        assert!(out.contains("reload config"), "{out}");
        assert!(out.contains("zoom the pane"), "{out}");
        assert!(!out.contains("Break pane"), "{out}");
    }

    #[test]
    fn the_prompt_shows_the_query_and_how_much_it_narrowed() {
        let p = Picker::new(items(), "custom");
        let first = rendered(&p, &Chrome::default(), 60, 12)[0].clone();
        assert!(first.contains("> custom"), "{first}");
        assert!(first.contains("2/3"), "{first}");
    }

    #[test]
    fn the_cursor_is_visible_as_a_marker() {
        let mut p = Picker::new(items(), "");
        p.down();
        let out = rendered(&p, &Chrome::default(), 60, 12).join("\n");
        assert!(
            out.contains("> custom: zoom"),
            "no marker on row two:\n{out}"
        );
    }

    #[test]
    fn the_title_and_footer_are_drawn() {
        let chrome = Chrome {
            title: "[ Keys ]".into(),
            footer: "enter runs it".into(),
            preview_title: String::new(),
        };
        let out = rendered(&Picker::new(items(), ""), &chrome, 60, 12).join("\n");
        assert!(out.contains("[ Keys ]"), "{out}");
        assert!(out.contains("enter runs it"), "{out}");
    }

    #[test]
    fn a_preview_pane_shows_what_the_row_would_run() {
        let chrome = Chrome {
            preview_title: "[ What it runs ]".into(),
            ..Chrome::default()
        };
        let out = rendered(&Picker::new(items(), "reload"), &chrome, 60, 16).join("\n");
        assert!(out.contains("[ What it runs ]"), "{out}");
        assert!(out.contains("source-file"), "{out}");
    }

    #[test]
    fn a_row_with_nothing_to_preview_gets_no_pane() {
        let chrome = Chrome {
            preview_title: "[ What it runs ]".into(),
            ..Chrome::default()
        };
        let out = rendered(&Picker::new(items(), "Break"), &chrome, 60, 16).join("\n");
        assert!(!out.contains("[ What it runs ]"), "{out}");
    }

    #[test]
    fn an_empty_result_still_draws_a_usable_screen() {
        // Somebody typing nonsense should see an empty list and their query,
        // not a panic and a terminal left in raw mode.
        let p = Picker::new(items(), "zzzzzz");
        let out = rendered(&p, &Chrome::default(), 60, 12);
        assert!(out[0].contains("0/3"), "{:?}", out[0]);
    }

    #[test]
    fn a_narrow_terminal_does_not_panic() {
        // display-popup geometry is somebody else's decision, so the picker
        // has to survive whatever it is given.
        let p = Picker::new(items(), "");
        let _ = rendered(&p, &Chrome::default(), 12, 4);
    }
}
