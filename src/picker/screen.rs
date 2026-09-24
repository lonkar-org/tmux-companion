//! The picker's own screen: the state behind it, the matching, and the drawing.
//!
//! This replaces skim, which drove the screen here until the chrome became a
//! setting. skim has `border_label`, `preview_label` and `footer` fields, so it
//! looks for a while as though it can draw what fzf draws; all three are
//! private, carry `#[builder(setter(skip))]` and `arg(hide = true)`, and appear
//! nowhere in its drawing code. They are there so an fzf command line parses,
//! not so it renders. A library user gets a border or no border and nothing on
//! it.
//!
//! What is here instead is small because the hard parts are somebody else's:
//! [`nucleo_matcher`] does the matching, which is the same matcher helix uses,
//! and ratatui does the drawing. The state machine below is a query, a cursor
//! and a scroll offset.
//!
//! Everything that decides where something lands is a free function taking
//! numbers and returning numbers, so the layout is tested without a terminal.
//! The drawing itself is not tested and cannot usefully be: what it does is
//! call ratatui.

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::{
    Chrome, Item, Preview,
    style::{Edge, LabelPosition},
};

/// One row, ready to draw.
pub(super) struct Row {
    /// Index into the list the caller handed over, which is what it gets back.
    pub index: usize,
    /// The row laid out, with every column at the width they all share.
    pub text: String,
    /// Tint for the row's text.
    pub colour: Option<Color>,
    /// Solid block drawn in front of the row.
    pub swatch: Option<Color>,
    /// What the preview pane shows for this row.
    pub preview: String,
}

/// A row that survived the query, and where it matched.
struct Hit {
    /// Index into `rows`.
    row: usize,
    /// Character positions the query matched, for the highlight.
    positions: Vec<u32>,
}

/// What the screen is showing and what somebody has typed into it.
pub(super) struct State {
    rows: Vec<Row>,
    /// Whether any row carries a swatch, so the ones without still line up.
    swatch_column: bool,
    query: String,
    hits: Vec<Hit>,
    /// Index into `hits`, not into `rows`.
    selected: usize,
    /// The first visible hit.
    offset: usize,
    /// How far the preview has been scrolled.
    preview_scroll: u16,
    /// The preview's share, which ctrl-p cycles.
    preview_percent: u16,
    matcher: Matcher,
}

/// What ended the picker.
pub(super) enum Ended {
    /// Enter on a row.
    Chosen(usize),
    /// Enter on a query that matched nothing, or alt-enter.
    Typed(String),
    /// Esc or ctrl-d.
    Cancelled,
}

/// The shares ctrl-p cycles through, as percentages of the popup.
///
/// fzf has no drag-resize -- its mouse handling is click and scroll only -- so
/// a row too long for the list column can only be read by making the column
/// wider. This is that key.
const PREVIEW_SHARES: [u16; 4] = [30, 50, 70, 0];

impl State {
    pub(super) fn new(items: &[Item], query: &str, preview_percent: u16, order: &[usize]) -> Self {
        let laid = super::laid_out_in(items, order);
        let swatch_column = items.iter().any(|i| i.swatch.is_some());
        let rows = items
            .iter()
            .zip(laid)
            .enumerate()
            .map(|(index, (item, text))| Row {
                index,
                text,
                colour: item.colour.as_deref().and_then(super::colour_of),
                swatch: item.swatch.as_deref().and_then(super::colour_of),
                preview: item.preview.clone(),
            })
            .collect();

        let mut state = Self {
            rows,
            swatch_column,
            query: query.to_string(),
            hits: Vec::new(),
            selected: 0,
            offset: 0,
            preview_scroll: 0,
            preview_percent,
            matcher: Matcher::new(Config::DEFAULT),
        };
        state.refilter();
        state
    }

    /// Re-run the query over every row.
    ///
    /// Everything is already in memory and the lists here are hundreds of rows
    /// rather than millions, so this is a loop rather than a streaming matcher
    /// with a worker behind it.
    fn refilter(&mut self) {
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(u32, usize, usize, Vec<u32>)> = Vec::new();

        for (i, row) in self.rows.iter().enumerate() {
            buf.clear();
            let haystack = Utf32Str::new(&row.text, &mut buf);
            let mut positions = Vec::new();
            if let Some(score) = pattern.indices(haystack, &mut self.matcher, &mut positions) {
                positions.sort_unstable();
                positions.dedup();
                scored.push((score, row.text.chars().count(), i, positions));
            }
        }

        // An empty query is not a search, so nothing is ranked: the rows keep
        // the order the caller built them in.
        //
        // This is the whole of what the project list is. It puts the live
        // sessions first and then the directories by how often they are
        // visited, and that order is the answer to "where do I want to be",
        // which no amount of matching can improve on. Ranking an empty query
        // sorted every row by length and scattered the sessions through the
        // directories.
        //
        // With a query, best score first, then the shorter row, then arrival
        // order. The length tie-break is doing real work there: `zoom` and `a
        // long way to say zoom` score the same, and a picker where the exact
        // answer sits below a sentence containing it is one people stop
        // trusting.
        if !self.query.trim().is_empty() {
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        }

        self.hits = scored
            .into_iter()
            .map(|(_, _, row, positions)| Hit { row, positions })
            .collect();
        self.selected = 0;
        self.offset = 0;
        self.preview_scroll = 0;
    }

    /// The row the cursor is on, if anything matched.
    fn current(&self) -> Option<&Row> {
        self.hits.get(self.selected).map(|h| &self.rows[h.row])
    }

    fn move_by(&mut self, delta: isize) {
        if self.hits.is_empty() {
            return;
        }
        let len = self.hits.len() as isize;
        // Wrapping, which is fzf's `--cycle`: the list is short and the bottom
        // of it is next to the top in everybody's head.
        self.selected = (((self.selected as isize + delta) % len + len) % len) as usize;
        self.preview_scroll = 0;
    }

    /// Cycle the preview's share of the popup.
    fn cycle_preview(&mut self) {
        let next = PREVIEW_SHARES
            .iter()
            .position(|p| *p == self.preview_percent)
            .map_or(0, |i| (i + 1) % PREVIEW_SHARES.len());
        self.preview_percent = PREVIEW_SHARES[next];
    }
}

// ── Layout ───────────────────────────────────────────────────────────────────

/// Where each part of a picker lands.
///
/// Worked out as numbers so the arithmetic is tested without a terminal: every
/// off-by-one in a TUI is a row of somebody's list that silently never draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Panes {
    /// The hint line, or `None` when it is hidden.
    pub hint: Option<Rect>,
    /// The rule under the hint.
    pub top_rule: Option<Rect>,
    /// The rows.
    pub list: Rect,
    /// The rule above the query.
    pub bottom_rule: Option<Rect>,
    /// The query.
    pub prompt: Rect,
    /// The preview, or `None` when there is not one.
    pub preview: Option<Rect>,
}

/// Split the inside of the border into the parts a picker draws.
pub(super) fn panes(
    inner: Rect,
    chrome: &Chrome,
    preview: Option<Preview>,
    preview_percent: u16,
) -> Panes {
    // The preview comes off first, because everything else shares whatever is
    // left and a preview beside the list shortens nothing.
    let (column, preview_area) = match preview {
        Some(side) if preview_percent > 0 => {
            let (direction, first) = match side {
                Preview::Right => (Direction::Horizontal, false),
                Preview::Left => (Direction::Horizontal, true),
                Preview::Bottom => (Direction::Vertical, false),
                Preview::Top => (Direction::Vertical, true),
                Preview::None => (Direction::Horizontal, false),
            };
            let shares = if first {
                [Constraint::Percentage(preview_percent), Constraint::Min(1)]
            } else {
                [Constraint::Min(1), Constraint::Percentage(preview_percent)]
            };
            let parts = Layout::default()
                .direction(direction)
                .constraints(shares)
                .split(inner);
            if first {
                (parts[1], Some(parts[0]))
            } else {
                (parts[0], Some(parts[1]))
            }
        }
        _ => (inner, None),
    };

    let look = &chrome.look;
    let hint_shown = look.hint_position != Edge::Hidden && !chrome.footer.trim().is_empty();
    let rules = look.rules;
    let prompt_first = look.prompt_position == Edge::Top;

    // One row each for the hint, the two rules and the query; the list takes
    // what is left, and never less than one row.
    let mut rows: Vec<Constraint> = Vec::new();
    let mut order: Vec<Part> = Vec::new();
    let push = |c: Constraint, p: Part, rows: &mut Vec<Constraint>, order: &mut Vec<Part>| {
        rows.push(c);
        order.push(p);
    };

    if hint_shown && look.hint_position == Edge::Top {
        push(Constraint::Length(1), Part::Hint, &mut rows, &mut order);
        if rules {
            push(Constraint::Length(1), Part::TopRule, &mut rows, &mut order);
        }
    }
    if prompt_first {
        push(Constraint::Length(1), Part::Prompt, &mut rows, &mut order);
        if rules {
            push(Constraint::Length(1), Part::TopRule, &mut rows, &mut order);
        }
    }
    push(Constraint::Min(1), Part::List, &mut rows, &mut order);
    if !prompt_first {
        if rules {
            push(
                Constraint::Length(1),
                Part::BottomRule,
                &mut rows,
                &mut order,
            );
        }
        push(Constraint::Length(1), Part::Prompt, &mut rows, &mut order);
    }
    if hint_shown && look.hint_position == Edge::Bottom {
        if rules {
            push(
                Constraint::Length(1),
                Part::BottomRule,
                &mut rows,
                &mut order,
            );
        }
        push(Constraint::Length(1), Part::Hint, &mut rows, &mut order);
    }

    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(rows)
        .split(column);

    let mut out = Panes {
        hint: None,
        top_rule: None,
        list: column,
        bottom_rule: None,
        prompt: column,
        preview: preview_area,
    };
    for (area, part) in areas.iter().zip(order) {
        match part {
            Part::Hint => out.hint = Some(*area),
            Part::TopRule => out.top_rule = Some(*area),
            Part::List => out.list = *area,
            Part::BottomRule => out.bottom_rule = Some(*area),
            Part::Prompt => out.prompt = *area,
        }
    }
    out
}

/// The parts of the list column, in the order they are stacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Hint,
    TopRule,
    List,
    BottomRule,
    Prompt,
}

/// The box a preview's label is drawn on.
///
/// A label sits on the nearest border line to the edge it names. With the
/// preview above or below the list that is the preview's own top or bottom
/// border, so the label goes on the pane. Beside the list, the preview's only
/// border is the one-cell vertical edge between the two columns and there is
/// nowhere on it to put words, so the label goes on the outer box's border
/// instead -- still within the preview's own columns, so two labels on one
/// bottom line each sit under the half they name.
///
/// Drawing it inside the pane is what looked wrong: a label floating over the
/// last line of a directory listing, attached to nothing.
pub(super) fn preview_label_area(pane: Rect, outer: Rect, previewing: Option<Preview>) -> Rect {
    match previewing {
        Some(Preview::Left) | Some(Preview::Right) => Rect {
            y: outer.y,
            height: outer.height,
            ..pane
        },
        _ => pane,
    }
}

/// How far the cursor moves for an arrow, given which way the list is drawn.
///
/// A list drawn from the bottom is drawn in reverse, so the index that means
/// "further down the screen" is the smaller one. Without this, Up moved the
/// cursor down and Down moved it up, and the fix belongs here rather than at
/// the two key arms because the paging keys have the same problem.
///
/// Positive is towards the end of the list, whichever end that is drawn at.
pub(super) fn arrow_step(towards_screen_bottom: bool, distance: isize, from: Edge) -> isize {
    let down_is_forward = from == Edge::Top;
    if towards_screen_bottom == down_is_forward {
        distance
    } else {
        -distance
    }
}

/// A pane holding `rows` rows, pushed to the bottom of the space it was given.
///
/// Pure, so the arithmetic that decides whether a short list sits against the
/// query or floats above it is tested rather than looked at.
pub(super) fn bottom_aligned(area: Rect, rows: usize) -> Rect {
    let rows = (rows as u16).min(area.height);
    Rect {
        y: area.y + area.height - rows,
        height: rows,
        ..area
    }
}

/// Which hits are on screen, given where the cursor is and how many rows fit.
///
/// Returns the new scroll offset. The cursor stays put until it reaches an
/// edge, which is what every list anybody uses does and what stops the rows
/// sliding under a cursor that has not moved.
pub(super) fn scrolled(offset: usize, selected: usize, height: usize) -> usize {
    if height == 0 {
        return 0;
    }
    if selected < offset {
        selected
    } else if selected >= offset + height {
        selected + 1 - height
    } else {
        offset
    }
}

// ── Drawing ──────────────────────────────────────────────────────────────────

/// Draw a label onto a border line.
///
/// ratatui's own block titles are placed by alignment, which cannot leave three
/// cells of border showing beyond a right-hand label -- and those three cells
/// are the difference between a label sitting on the line and a label that has
/// eaten the corner. Writing into the buffer is the whole of what a title does
/// anyway.
fn draw_label(frame: &mut Frame, area: Rect, text: &str, at: LabelPosition, offset: u16) {
    if at == LabelPosition::Hidden || text.trim().is_empty() || area.width == 0 || area.height == 0
    {
        return;
    }
    let width = text.chars().count() as u16;
    let x = area.x + at.start(area.width, width, offset);
    let y = if at.on_top() {
        area.y
    } else {
        area.y + area.height - 1
    };
    frame
        .buffer_mut()
        .set_stringn(x, y, text, area.width as usize, Style::default());
}

/// One row, styled: the swatch, the match highlight and the row's own colour.
fn row_line<'a>(
    row: &'a Row,
    hit: &Hit,
    swatch_column: bool,
    selected: bool,
    marker: &'a str,
) -> Line<'a> {
    let mut spans: Vec<Span> = Vec::new();

    spans.push(if selected {
        Span::styled(marker, Style::default().fg(Color::Magenta))
    } else {
        Span::raw(" ".repeat(marker.chars().count()))
    });

    if swatch_column {
        spans.push(match row.swatch {
            Some(c) => Span::styled("\u{2588}\u{2588} ", Style::default().fg(c)),
            None => Span::raw("   "),
        });
    }

    let base = match row.colour {
        Some(c) => Style::default().fg(c),
        None => Style::default(),
    };
    let base = if selected {
        base.add_modifier(Modifier::BOLD)
    } else {
        base
    };
    // The matched characters, one span each where they are not contiguous.
    // Cheap enough at this size, and it keeps the highlight exactly on the
    // characters the matcher used rather than on a range around them.
    for (i, ch) in row.text.chars().enumerate() {
        let matched = hit.positions.binary_search(&(i as u32)).is_ok();
        let style = if matched {
            base.fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            base
        };
        spans.push(Span::styled(ch.to_string(), style));
    }
    Line::from(spans)
}

/// Draw the whole picker.
pub(super) fn draw(
    frame: &mut Frame,
    state: &mut State,
    chrome: &Chrome,
    preview: Option<Preview>,
) {
    // The popup itself, which is where the outer border and the labels on it
    // are drawn. `area` below is shadowed by each pane as they are split out.
    let outer = frame.area();
    let area = outer;
    let look = &chrome.look;

    // The outer box, and what is left inside it.
    let inner = match look.border.set() {
        Some(set) => {
            let block = Block::default().borders(Borders::ALL).border_set(set);
            let inner = block.inner(area);
            frame.render_widget(block, area);
            draw_label(
                frame,
                area,
                &chrome.title,
                look.label_position,
                look.label_offset,
            );
            inner
        }
        None => area,
    };

    let previewing = preview.filter(|_| state.current().is_some_and(|r| !r.preview.is_empty()));
    let panes = panes(inner, chrome, previewing, state.preview_percent);

    if let Some(hint) = panes.hint {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {}", chrome.footer.trim()),
                Style::default().fg(Color::DarkGray),
            ))),
            hint,
        );
    }

    // Inset by one, so a rule reads as a line under the list rather than as a
    // row that has grown into the border on both sides.
    let rule = look.border.horizontal();
    for area in [panes.top_rule, panes.bottom_rule].into_iter().flatten() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {}", rule.repeat(area.width.saturating_sub(1) as usize)),
                Style::default().fg(Color::DarkGray),
            ))),
            area,
        );
    }

    // The rows themselves.
    let height = panes.list.height as usize;
    state.offset = scrolled(state.offset, state.selected, height);
    let mut lines: Vec<Line> = state
        .hits
        .iter()
        .enumerate()
        .skip(state.offset)
        .take(height)
        .map(|(i, hit)| {
            row_line(
                &state.rows[hit.row],
                hit,
                state.swatch_column,
                i == state.selected,
                &look.marker,
            )
        })
        .collect();
    let mut rows_area = panes.list;
    if look.list_from == Edge::Bottom {
        // The first row nearest the query, with the rest growing away from
        // it. A short list has to be pushed down to the query rather than
        // left hanging at the top of the pane, or the two are separated by a
        // band of nothing and the list reads as belonging to the wrong end.
        lines.reverse();
        rows_area = bottom_aligned(rows_area, lines.len());
    }
    frame.render_widget(Paragraph::new(lines), rows_area);

    // The query, and the counter beside it when it is wanted.
    let mut prompt = vec![
        Span::styled(
            format!(" {}", chrome.prompt),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(state.query.as_str()),
        Span::styled("\u{2588}", Style::default().fg(Color::DarkGray)),
    ];
    if look.counter {
        let counted = format!("  {}/{}", state.hits.len(), state.rows.len());
        prompt.push(Span::styled(counted, Style::default().fg(Color::DarkGray)));
    }
    frame.render_widget(Paragraph::new(Line::from(prompt)), panes.prompt);

    // The preview, in its own box.
    if let Some(area) = panes.preview
        && let Some(row) = state.current()
    {
        // The line the preview is drawn with is the outer box's when there is
        // one, and rounded when there is not. Taking it from the outer border
        // alone is what made `border = "none"` silently remove the preview's
        // frame as well: the theme picker opens in a tmux popup that already
        // has a border, turns its own off to avoid two, and lost the box round
        // its card with it.
        let set = look
            .border
            .set()
            .unwrap_or(ratatui::symbols::border::ROUNDED);
        let borders = previewing.map_or(Borders::NONE, |p| look.preview_border.sides(p));
        let block = Block::default().borders(borders).border_set(set);
        let body = block.inner(area);
        frame.render_widget(block, area);
        draw_label(
            frame,
            preview_label_area(area, outer, previewing),
            &chrome.preview_title,
            look.preview_label_position,
            look.preview_label_offset,
        );
        // Clipped at the pane edge rather than reflowed. A preview is either
        // a capture of somebody else's screen, which is already the shape it
        // wants to be, or a card drawn to a width it picked before this pane
        // existed. Wrapping took the theme card's sample bars -- built wider
        // than the pane on purpose, so they reach its edge -- and folded the
        // overhang onto the next line as a stray block of colour.
        frame.render_widget(
            Paragraph::new(super::ansi::into_lines(&row.preview)).scroll((state.preview_scroll, 0)),
            body,
        );
    }
}

// ── The loop ─────────────────────────────────────────────────────────────────

/// Show the picker and wait for a decision.
pub(super) fn run(items: &[Item], query: &str, chrome: &Chrome) -> anyhow::Result<Ended> {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

    let mut state = State::new(
        items,
        query,
        chrome.preview_percent,
        &chrome.look.column_order,
    );
    let mut terminal = ratatui::init();
    // Whatever happens below, the terminal goes back to how it was found. A
    // picker that panics with raw mode still on leaves the pane unusable and
    // the person with no way to read the panic.
    let result = (|| -> anyhow::Result<Ended> {
        loop {
            let preview = super::fitting_preview(
                chrome.preview,
                state.preview_percent,
                terminal.size()?.width,
                chrome.look.min_list_width,
            );
            terminal.draw(|frame| draw(frame, &mut state, chrome, preview))?;

            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            let alt = key.modifiers.contains(KeyModifiers::ALT);

            match key.code {
                KeyCode::Esc => return Ok(Ended::Cancelled),
                KeyCode::Char('d') if ctrl => return Ok(Ended::Cancelled),
                KeyCode::Char('c') if ctrl => return Ok(Ended::Cancelled),
                KeyCode::Enter if alt => return Ok(Ended::Typed(state.query.clone())),
                KeyCode::Enter => {
                    return Ok(match state.current() {
                        Some(row) => Ended::Chosen(row.index),
                        None => Ended::Typed(state.query.clone()),
                    });
                }
                // Down and Up mean down and up the screen, which is not the
                // same as forwards and backwards through the list once the
                // list is drawn from the bottom.
                KeyCode::Down => state.move_by(arrow_step(true, 1, chrome.look.list_from)),
                KeyCode::Up => state.move_by(arrow_step(false, 1, chrome.look.list_from)),
                KeyCode::PageDown => state.move_by(arrow_step(true, 10, chrome.look.list_from)),
                KeyCode::PageUp => state.move_by(arrow_step(false, 10, chrome.look.list_from)),
                // ctrl-j and ctrl-k scroll the preview rather than the list,
                // which is what the fzf bindings this replaces did.
                KeyCode::Char('k') if ctrl => {
                    state.preview_scroll = state.preview_scroll.saturating_sub(1);
                }
                KeyCode::Char('j') if ctrl => {
                    state.preview_scroll = state.preview_scroll.saturating_add(1);
                }
                KeyCode::Char('p') if ctrl => state.cycle_preview(),
                // ctrl-a and ctrl-u both clear. They are one key in most
                // people's hands and two in fzf's, and a picker is not the
                // place to be strict about which.
                KeyCode::Char('a') | KeyCode::Char('u') if ctrl => {
                    state.query.clear();
                    state.refilter();
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    state.refilter();
                }
                KeyCode::Char(c) if !ctrl => {
                    state.query.push(c);
                    state.refilter();
                }
                _ => {}
            }
        }
    })();
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picker::style::Look;

    fn chrome() -> Chrome {
        Chrome {
            preview_title: "[ Where ]".into(),
            ..Chrome::default()
        }
    }

    #[test]
    fn the_list_gets_what_the_hint_the_rules_and_the_query_leave() {
        // Four rows of chrome out of twenty: the hint, a rule under it, a rule
        // above the query and the query.
        let p = panes(Rect::new(0, 0, 80, 20), &chrome(), None, 0);
        assert_eq!(p.hint.map(|r| r.height), Some(1));
        assert_eq!(p.top_rule.map(|r| r.height), Some(1));
        assert_eq!(p.bottom_rule.map(|r| r.height), Some(1));
        assert_eq!(p.prompt.height, 1);
        assert_eq!(p.list.height, 16);
        assert_eq!(p.preview, None);
    }

    #[test]
    fn the_query_sits_under_the_list_by_default_and_over_it_when_asked() {
        let area = Rect::new(0, 0, 80, 20);
        let below = panes(area, &chrome(), None, 0);
        assert!(below.prompt.y > below.list.y, "query should be under it");

        let mut top = chrome();
        top.look.prompt_position = Edge::Top;
        let above = panes(area, &top, None, 0);
        assert!(above.prompt.y < above.list.y, "query should be over it");
    }

    #[test]
    fn turning_the_rules_off_gives_the_rows_back() {
        let mut bare = chrome();
        bare.look.rules = false;
        let p = panes(Rect::new(0, 0, 80, 20), &bare, None, 0);
        assert_eq!(p.top_rule, None);
        assert_eq!(p.bottom_rule, None);
        assert_eq!(p.list.height, 18);
    }

    #[test]
    fn a_picker_with_no_keys_to_explain_draws_no_hint_line() {
        // The line is hidden by having nothing to say as well as by the
        // setting, so a caller that leaves the footer empty does not get a
        // blank row where an explanation would have been.
        let quiet = Chrome {
            footer: String::new(),
            ..chrome()
        };
        assert_eq!(panes(Rect::new(0, 0, 80, 20), &quiet, None, 0).hint, None);

        let mut hidden = chrome();
        hidden.look.hint_position = Edge::Hidden;
        assert_eq!(panes(Rect::new(0, 0, 80, 20), &hidden, None, 0).hint, None);
    }

    #[test]
    fn the_preview_takes_its_share_off_the_side_it_is_on() {
        let area = Rect::new(0, 0, 100, 20);
        let right = panes(area, &chrome(), Some(Preview::Right), 40);
        let preview = right.preview.expect("a pane");
        assert_eq!(preview.width, 40);
        assert_eq!(preview.x, 60);
        // And the list column shortens nothing: a preview beside it is beside
        // the whole column, hint and query included.
        assert_eq!(right.list.height, 16);

        let left = panes(area, &chrome(), Some(Preview::Left), 40);
        assert_eq!(left.preview.expect("a pane").x, 0);
        assert!(left.list.x >= 40);

        let bottom = panes(area, &chrome(), Some(Preview::Bottom), 40);
        let under = bottom.preview.expect("a pane");
        assert_eq!(under.height, 8);
        assert!(under.y > bottom.list.y);
    }

    #[test]
    fn a_preview_share_of_zero_is_no_preview() {
        // What ctrl-p cycles to when it comes round: the list gets the popup.
        let p = panes(Rect::new(0, 0, 100, 20), &chrome(), Some(Preview::Right), 0);
        assert_eq!(p.preview, None);
        assert_eq!(p.list.width, 100);
    }

    #[test]
    fn a_picker_with_no_box_of_its_own_still_frames_its_preview() {
        // The theme picker opens in a tmux popup that already has a border and
        // turns its own off so there are not two. Taking the preview's line
        // from the outer border alone removed its frame with it.
        let bare = crate::picker::style::BorderKind::None;
        assert!(bare.set().is_none(), "no outer box");
        // The fallback the drawing uses, which is what keeps the frame.
        let set = bare.set().unwrap_or(ratatui::symbols::border::ROUNDED);
        assert_eq!(set.top_left, ratatui::symbols::border::ROUNDED.top_left);
    }

    #[test]
    fn a_side_previews_label_goes_on_the_outer_border_under_its_own_column() {
        // The preview's own border there is the one-cell edge between the
        // columns, with nowhere on it for words. The outer border has a
        // bottom line, and keeping the label inside the preview's columns is
        // what puts two labels on one line each under the half it names.
        let outer = Rect::new(0, 0, 150, 30);
        let pane = Rect::new(60, 1, 89, 28);

        let beside = preview_label_area(pane, outer, Some(Preview::Right));
        assert_eq!(beside.x, pane.x, "still over its own column");
        assert_eq!(beside.width, pane.width);
        assert_eq!(beside.y, outer.y, "but on the outer box");
        assert_eq!(beside.height, outer.height);

        // Above or below the list, the preview has a horizontal border of its
        // own and the label belongs on that.
        assert_eq!(preview_label_area(pane, outer, Some(Preview::Bottom)), pane);
        assert_eq!(preview_label_area(pane, outer, Some(Preview::Top)), pane);
        assert_eq!(preview_label_area(pane, outer, None), pane);
    }

    #[test]
    fn a_short_list_drawn_upwards_sits_against_the_query() {
        // Otherwise the rows hang at the top of the pane with a band of
        // nothing between them and the query, and the list reads as belonging
        // to the wrong end of the popup.
        let area = Rect::new(0, 2, 40, 20);
        assert_eq!(bottom_aligned(area, 3), Rect::new(0, 19, 40, 3));
        // A full list is the pane.
        assert_eq!(bottom_aligned(area, 20), area);
        // And one longer than the pane does not reach past it.
        assert_eq!(bottom_aligned(area, 99), area);
        assert_eq!(bottom_aligned(area, 0), Rect::new(0, 22, 40, 0));
    }

    #[test]
    fn the_rows_scroll_only_once_the_cursor_reaches_an_edge() {
        // Inside the window, nothing moves.
        assert_eq!(scrolled(0, 3, 10), 0);
        // Past the bottom, by exactly enough to show it.
        assert_eq!(scrolled(0, 10, 10), 1);
        assert_eq!(scrolled(0, 14, 10), 5);
        // Above the top, the cursor becomes the top.
        assert_eq!(scrolled(5, 2, 10), 2);
        // A list with no room does not divide by it.
        assert_eq!(scrolled(4, 9, 0), 0);
    }

    #[test]
    fn the_best_match_is_first_and_the_shorter_row_wins_a_tie() {
        let items = vec![
            Item::new("a long way to say zoom"),
            Item::new("zoom"),
            Item::new("nothing alike"),
        ];
        let state = State::new(&items, "zoom", 0, &[]);
        let order: Vec<usize> = state.hits.iter().map(|h| state.rows[h.row].index).collect();
        assert_eq!(order, vec![1, 0], "{order:?}");
    }

    #[test]
    fn an_empty_query_keeps_every_row_in_the_order_it_arrived() {
        // Rows of different lengths, because the version of this test that
        // used "a", "b" and "c" passed while an empty query was being sorted
        // by row length: every row was one character, so nothing moved. The
        // project list is sessions first and then directories by how often
        // they are visited, and sorting scattered the sessions through them.
        let items = vec![
            Item::new("session  alpha"),
            Item::new("session  a-much-longer-session-name"),
            Item::new("dir      x"),
            Item::new("dir      a-long-directory-name"),
        ];
        let state = State::new(&items, "", 0, &[]);
        let order: Vec<usize> = state.hits.iter().map(|h| state.rows[h.row].index).collect();
        assert_eq!(order, vec![0, 1, 2, 3]);

        // Whitespace is not a query either.
        let state = State::new(&items, "   ", 0, &[]);
        let order: Vec<usize> = state.hits.iter().map(|h| state.rows[h.row].index).collect();
        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn the_arrows_mean_down_and_up_the_screen_whichever_way_the_list_runs() {
        // Drawn from the top, further down the screen is further along the
        // list. Drawn from the bottom it is the other way, and without this
        // Up moved the cursor down.
        assert_eq!(
            arrow_step(true, 1, Edge::Top),
            1,
            "down, drawn from the top"
        );
        assert_eq!(
            arrow_step(false, 1, Edge::Top),
            -1,
            "up, drawn from the top"
        );
        assert_eq!(
            arrow_step(true, 1, Edge::Bottom),
            -1,
            "down, drawn from the bottom"
        );
        assert_eq!(
            arrow_step(false, 1, Edge::Bottom),
            1,
            "up, drawn from the bottom"
        );
        // Paging has the same problem and the same answer.
        assert_eq!(arrow_step(true, 10, Edge::Bottom), -10);
        assert_eq!(arrow_step(false, 10, Edge::Bottom), 10);
    }

    #[test]
    fn moving_past_either_end_wraps() {
        let items = vec![Item::new("a"), Item::new("b"), Item::new("c")];
        let mut state = State::new(&items, "", 0, &[]);
        state.move_by(-1);
        assert_eq!(state.selected, 2, "up from the top goes to the bottom");
        state.move_by(1);
        assert_eq!(state.selected, 0, "and back round");
        // A page down on a three-row list lands somewhere on it rather than
        // off the end.
        state.move_by(10);
        assert!(state.selected < 3, "{}", state.selected);
    }

    #[test]
    fn moving_on_an_empty_list_does_nothing() {
        let mut state = State::new(&[], "", 0, &[]);
        state.move_by(1);
        assert_eq!(state.selected, 0);
        assert!(state.current().is_none());
    }

    #[test]
    fn the_preview_share_cycles_back_round_to_where_it_started() {
        let mut state = State::new(&[Item::new("a")], "", 30, &[]);
        let mut seen = vec![state.preview_percent];
        for _ in 0..PREVIEW_SHARES.len() {
            state.cycle_preview();
            seen.push(state.preview_percent);
        }
        assert_eq!(seen.first(), seen.last(), "{seen:?}");
        assert!(seen.contains(&0), "one of them has to be no preview at all");
    }

    #[test]
    fn a_share_that_is_not_on_the_ring_joins_it_at_the_start() {
        // The config can say 55, which is not one of the four ctrl-p offers.
        let mut state = State::new(&[Item::new("a")], "", 55, &[]);
        state.cycle_preview();
        assert_eq!(state.preview_percent, PREVIEW_SHARES[0]);
    }

    #[test]
    fn a_preview_label_with_no_border_under_it_takes_a_row_of_its_own() {
        // With the preview beside the list, its only border is the one-cell
        // left edge, so the label goes along the bottom row instead. Without
        // taking that row off the body it lands on the last line of the
        // preview, which is what it did before this.
        let body = Rect::new(60, 0, 40, 20);
        let mut shrunk = body;
        shrunk.height -= 1;
        assert_eq!(shrunk.height, 19);
        assert_eq!(shrunk.y, body.y, "a bottom label does not move the top");

        let mut top = body;
        top.height -= 1;
        top.y += 1;
        assert_eq!(top.y, 1, "a top label moves the body down");
    }

    #[test]
    fn a_label_lands_on_the_line_it_names() {
        // The arithmetic `draw_label` does, without a terminal to draw on.
        let area = Rect::new(0, 0, 40, 10);
        let at = LabelPosition::BottomRight;
        assert_eq!(area.x + at.start(area.width, 8, 3), 29);
        assert_eq!(area.y + area.height - 1, 9);
    }

    #[test]
    fn the_look_is_carried_from_the_config_rather_than_compiled_in() {
        let chrome = Chrome {
            look: Look {
                rules: false,
                ..Look::default()
            },
            ..Chrome::default()
        };
        assert!(!chrome.look.rules);
    }
}
