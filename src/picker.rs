//! The picker: a fuzzy-matched list in a terminal, shared by every chooser in
//! the tool.
//!
//! This is [skim] driving the screen, which is fzf ported to Rust and used here
//! as a library rather than as a process. The spawn is what cost 50 ms of the
//! 93 ms a warm `prefix+?` used to take, and a library has no spawn; what the
//! port adds over the hand-written picker it replaces is fzf's whole keymap,
//! its preview window, and a layout that is a setting rather than a shape
//! compiled into one function.
//!
//! The parts worth pinning are here rather than in skim: how a row's columns
//! are measured so every line agrees, and how a tmux colour becomes a terminal
//! one. Both are pure, and both are what went wrong before -- the rows used to
//! be laid out with `{:<22}` over strings holding ANSI escapes, so the padding
//! was computed across bytes the terminal never draws and no two lines landed
//! in the same place.
//!
//! This runs in the **client** process, not the daemon. A daemon has no
//! terminal, and the whole point of a picker is that it owns one for as long as
//! somebody is looking at it.
//!
//! [skim]: https://github.com/skim-rs/skim

use std::{borrow::Cow, sync::Arc};

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use skim::{
    DisplayContext, ItemPreview, PreviewContext, RankCriteria, Skim, SkimItem, SkimItemReceiver,
    SkimItemSender, SkimOptions,
    tui::{
        Direction as PreviewDirection, Size,
        options::{PreviewLayout, TuiLayout},
    },
};

/// One row a picker can show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    /// What the matcher searches, and what the row shows when it has no
    /// columns of its own.
    pub label: String,
    /// Shown in the preview pane. Empty means this row has nothing to show.
    pub preview: String,
    /// The colour to draw the row's text in. `None` leaves it the default.
    ///
    /// Carried as the string the source produced, a tmux `colourNNN` or a
    /// `#rrggbb`, and turned into a terminal colour at draw time. Keeping the
    /// string means the item type does not drag a ratatui type into every
    /// module that builds one.
    pub colour: Option<String>,
    /// The row split into columns, aligned against every other row's.
    ///
    /// A label built with `{:<22}` cannot line up once anything in it is
    /// styled: the padding is counted over bytes the terminal does not draw,
    /// so the column lands somewhere different on every line. Handing the
    /// parts over separately lets the width be measured across all of them.
    pub columns: Vec<String>,
    /// A solid block of this colour at the start of the row.
    ///
    /// Separate from `colour`, which tints the text: a row can carry a
    /// project's colour as a block and stay readable, which colouring a whole
    /// line on a dark bar does not manage.
    pub swatch: Option<String>,
}

impl Item {
    /// An item with nothing to preview.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    /// An item with a preview.
    pub fn with_preview(label: impl Into<String>, preview: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            preview: preview.into(),
            ..Self::default()
        }
    }

    /// The same item, drawn in a colour.
    pub fn in_colour(mut self, colour: Option<String>) -> Self {
        self.colour = colour.filter(|c| !c.trim().is_empty());
        self
    }

    /// The same item, drawn as columns that line up with every other row's.
    ///
    /// The label is left alone, so what a caller reads back does not change
    /// with how the row was laid out.
    pub fn in_columns(mut self, columns: Vec<impl Into<String>>) -> Self {
        self.columns = columns.into_iter().map(Into::into).collect();
        self
    }

    /// The same item, with a solid colour block in front of it.
    pub fn with_swatch(mut self, colour: Option<String>) -> Self {
        self.swatch = colour.filter(|c| !c.trim().is_empty());
        self
    }

    /// The cells this row draws: its columns, or its label when it has none.
    fn cells(&self) -> Vec<&str> {
        if self.columns.is_empty() {
            vec![self.label.as_str()]
        } else {
            self.columns.iter().map(String::as_str).collect()
        }
    }
}

/// The gap between two columns, in cells.
const COLUMN_GAP: usize = 2;

/// How wide each column has to be for every row's to line up.
///
/// The widest cell in each column, and nothing cleverer: a column nobody fills
/// costs nothing, and one long row widening a column is what anybody reading a
/// table expects to happen.
pub fn column_widths(rows: &[Vec<&str>]) -> Vec<usize> {
    let mut widths: Vec<usize> = Vec::new();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            let w = cell.chars().count();
            match widths.get_mut(i) {
                Some(slot) => *slot = (*slot).max(w),
                None => widths.push(w),
            }
        }
    }
    widths
}

/// Lay one row out against the widths every row shares.
///
/// The last column is not padded: trailing spaces would be matched against and
/// would widen the row past what it needs.
pub fn lay_out(cells: &[&str], widths: &[usize]) -> String {
    let mut out = String::new();
    for (i, cell) in cells.iter().enumerate() {
        out.push_str(cell);
        let last = i + 1 == cells.len();
        if !last {
            let pad = widths
                .get(i)
                .copied()
                .unwrap_or(0)
                .saturating_sub(cell.chars().count());
            out.push_str(&" ".repeat(pad + COLUMN_GAP));
        }
    }
    out
}

/// Every row laid out against one set of column widths.
///
/// Pure, and the thing the ragged-column bug lived in, so it is tested
/// directly rather than through a terminal.
pub fn laid_out(items: &[Item]) -> Vec<String> {
    let cells: Vec<Vec<&str>> = items.iter().map(Item::cells).collect();
    let widths = column_widths(&cells);
    cells.iter().map(|row| lay_out(row, &widths)).collect()
}

/// A tmux colour string as a terminal colour.
///
/// Understands the two spellings tmux itself writes: `colour29` and `#rrggbb`.
/// Anything else is `None` rather than a guess, because a colour nobody can
/// parse should leave the row readable instead of painting it something
/// arbitrary.
pub fn colour_of(spec: &str) -> Option<Color> {
    let s = spec.trim();
    if let Some(hex) = s.strip_prefix('#')
        && hex.len() == 6
        && let Ok(v) = u32::from_str_radix(hex, 16)
    {
        return Some(Color::Rgb(
            ((v >> 16) & 0xff) as u8,
            ((v >> 8) & 0xff) as u8,
            (v & 0xff) as u8,
        ));
    }
    let digits = s
        .strip_prefix("colour")
        .or_else(|| s.strip_prefix("color"))?;
    digits.parse::<u8>().ok().map(Color::Indexed)
}

// ── Layout ───────────────────────────────────────────────────────────────────

/// Where the preview sits relative to the list.
///
/// A setting rather than a shape baked into the drawing, because the right
/// answer depends on what is being previewed: a theme wants a tall pane beside
/// a narrow list, a command wants a couple of lines under a wide one, and a
/// directory wants nothing at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preview {
    /// No preview pane, whatever the rows carry.
    None,
    /// Beside the list, on the right.
    #[default]
    Right,
    /// Under the list.
    Bottom,
    /// Above the list.
    Top,
    /// Beside the list, on the left.
    Left,
}

impl Preview {
    fn direction(self) -> Option<PreviewDirection> {
        match self {
            Preview::None => None,
            Preview::Right => Some(PreviewDirection::Right),
            Preview::Bottom => Some(PreviewDirection::Down),
            Preview::Top => Some(PreviewDirection::Up),
            Preview::Left => Some(PreviewDirection::Left),
        }
    }
}

/// How a picker is laid out and labelled.
///
/// One of these drives every picker in the tool. What differs between them is
/// the title, the footer and what the preview is called; the shape comes from
/// `[picker]` in the config, so changing it changes all of them at once rather
/// than five call sites drifting apart.
#[derive(Debug, Clone)]
pub struct Chrome {
    /// Drawn above the list, e.g. `[ Keys ]`.
    pub title: String,
    /// The line saying which keys do what.
    pub footer: String,
    /// Label over the preview pane. Empty hides the pane whatever the layout
    /// says, which is what a picker with nothing to show sets.
    pub preview_title: String,
    /// Where the preview pane goes.
    pub preview: Preview,
    /// The preview's share of the popup, as a percentage.
    pub preview_percent: u16,
    /// What sits in front of the query.
    pub prompt: String,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            title: "[ Pick ]".into(),
            footer: "enter picks   ctrl-a clears the filter   esc cancels".into(),
            preview_title: String::new(),
            preview: Preview::Right,
            preview_percent: 55,
            prompt: "> ".into(),
        }
    }
}

impl Chrome {
    /// Take the shape from the config, leaving the labels alone.
    pub fn laid_out_by(mut self, layout: &crate::config::PickerLayout) -> Self {
        self.preview = layout.preview;
        self.preview_percent = layout.preview_percent.clamp(20, 80);
        self
    }
}

// ── The rows skim draws ──────────────────────────────────────────────────────

/// One row, as skim sees it.
struct Row {
    /// Index into the list the caller handed over, which is what it gets back.
    index: usize,
    /// The row laid out, with every column at the width they all share.
    text: String,
    /// Tint for the row's text.
    colour: Option<Color>,
    /// Solid block drawn in front of the row.
    swatch: Option<Color>,
    /// What the preview pane shows for this row.
    preview: String,
    /// Whether any row in this list has a swatch, so the ones without still
    /// line up with the ones that do.
    swatch_column: bool,
}

impl SkimItem for Row {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn display(&self, context: DisplayContext) -> Line<'_> {
        // skim highlights the characters the query matched, so the line is
        // built from its own rather than from scratch.
        let mut line = context.to_line(Cow::Borrowed(&self.text));

        // A tint applies only where skim has not already coloured something,
        // so the match highlight stays visible on a coloured row.
        if let Some(c) = self.colour {
            for span in &mut line.spans {
                if span.style.fg.is_none() {
                    span.style = span.style.fg(c);
                }
            }
        }

        if self.swatch_column {
            let block = match self.swatch {
                Some(c) => Span::styled("\u{2588}\u{2588} ", Style::default().fg(c)),
                None => Span::raw("   "),
            };
            line.spans.insert(0, block);
        }
        line
    }

    fn preview(&self, _context: PreviewContext) -> ItemPreview {
        if self.preview.is_empty() {
            ItemPreview::Text(String::new())
        } else {
            ItemPreview::AnsiText(self.preview.clone())
        }
    }
}

// ── Running one ──────────────────────────────────────────────────────────────

/// What somebody did with the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Picked a row, by index into the original list.
    Chosen(usize),
    /// Pressed enter on a query that matched nothing, or asked for the query
    /// itself.
    ///
    /// This is the only way to run a command that was never in the history, or
    /// to open a directory zoxide has never seen.
    Typed(String),
    /// Cancelled.
    Cancelled,
}

/// Show the picker and wait for a decision.
///
/// Returns the index into the original list, or `None` when somebody cancelled
/// or typed something that was not on it.
pub fn run(items: Vec<Item>, query: &str, chrome: &Chrome) -> anyhow::Result<Option<usize>> {
    Ok(match run_with_query(items, query, chrome)? {
        Outcome::Chosen(i) => Some(i),
        _ => None,
    })
}

/// Show the picker and give back the query as well as the pick.
pub fn run_with_query(items: Vec<Item>, query: &str, chrome: &Chrome) -> anyhow::Result<Outcome> {
    if items.is_empty() && query.trim().is_empty() {
        return Ok(Outcome::Cancelled);
    }

    let laid = laid_out(&items);
    let swatch_column = items.iter().any(|i| i.swatch.is_some());

    let rows: Vec<Arc<dyn SkimItem>> = items
        .iter()
        .zip(laid)
        .enumerate()
        .map(|(index, (item, text))| {
            Arc::new(Row {
                index,
                text,
                colour: item.colour.as_deref().and_then(colour_of),
                swatch: item.swatch.as_deref().and_then(colour_of),
                preview: item.preview.clone(),
                swatch_column,
            }) as Arc<dyn SkimItem>
        })
        .collect();

    // One send of the whole list: everything is already in memory, so there
    // is nothing to stream and nothing to wait for.
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = skim::prelude::unbounded();
    let _ = tx.send(rows);
    drop(tx);

    // On its own thread, because skim wants a tokio runtime and the client
    // runs on a `current_thread` one: skim calls `block_in_place` when it
    // finds a handle, and that panics on a current-thread runtime. Off the
    // runtime entirely it builds the one it wants, and the client keeps the
    // cheap runtime it has for everything else.
    //
    // The options are built on that thread rather than moved onto it:
    // `SkimOptions` holds `Rc`s and so is not `Send`. What crosses is the
    // chrome, the query and the previewable flag, which are all plain data.
    let chrome = chrome.clone();
    let query = query.to_string();
    let previewable = previewable(&chrome, &items);
    // Read here rather than on the picker thread: this is the terminal the
    // client owns, and asking for its size is one syscall.
    let width = terminal_width();
    let output = std::thread::spawn(move || {
        Skim::run_with(options_with(&chrome, &query, previewable, width), Some(rx))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("the picker thread panicked"))?
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    if output.is_abort {
        return Ok(Outcome::Cancelled);
    }

    match output.selected_items.first() {
        Some(selected) => match (**selected).as_any().downcast_ref::<Row>() {
            Some(row) => Ok(Outcome::Chosen(row.index)),
            // Cannot happen: every item sent is a `Row`. Answering with the
            // query rather than panicking, because a picker that panics takes
            // the terminal down with it.
            None => Ok(Outcome::Typed(output.query)),
        },
        // Nothing matched, so what was typed is the answer.
        None => Ok(Outcome::Typed(output.query)),
    }
}

/// The skim options one picker runs with.
///
/// Only the tests call this: the picker itself builds them on its own thread,
/// because `SkimOptions` holds `Rc`s and cannot cross one.
#[cfg(test)]
fn options_for(chrome: &Chrome, query: &str, items: &[Item]) -> SkimOptions {
    // A width no narrower than the threshold, so a test asserting the
    // configured side gets the configured side rather than whatever terminal
    // the suite happens to run under.
    options_with(chrome, query, previewable(chrome, items), 160)
}

/// Narrower than this and a side-by-side preview leaves neither half readable.
///
/// Measured against what the popups in the shipped config actually get: a
/// picker opened at 70% of a 100-column terminal is 70 columns, and splitting
/// that 45/55 gives the list 31 columns, which truncates most rows in it. Under
/// this the preview goes below the list instead, where it has the full width.
const MIN_SIDE_BY_SIDE: u16 = 96;

/// Where the preview really goes, once the terminal has had its say.
///
/// A percentage-sized popup on a small terminal is a small popup, and skim
/// splits whatever it is given however narrow that is. Turning the split
/// sideways rather than obeying is the difference between a readable list and
/// two unreadable columns.
pub fn fitting_preview(preview: Preview, width: u16) -> Option<PreviewDirection> {
    match preview {
        Preview::Right | Preview::Left if width < MIN_SIDE_BY_SIDE => Some(PreviewDirection::Down),
        other => other.direction(),
    }
}

/// How wide the terminal is, or a sane guess when there is no terminal.
fn terminal_width() -> u16 {
    ratatui::crossterm::terminal::size()
        .map(|(cols, _)| cols)
        .unwrap_or(120)
}

/// The keys every picker has, whatever it is picking.
///
/// Worth spelling out because they are skim's rather than this tool's, and
/// nobody arrives knowing them. Dropped when the header would not fit: a line
/// cut off mid-word says less than a shorter one.
const SHARED_KEYS: &str = "\u{2191}\u{2193} move   ctrl-u clear";

/// The line above the list: what this picker is, then what its keys do.
///
/// `room` is how wide the list is, which is not how wide the popup is: the
/// header is drawn in the list column, so with a preview beside it there is
/// roughly half a popup to write in.
fn header_line(chrome: &Chrome, room: usize) -> String {
    let title = chrome.title.trim();
    let keys = chrome.footer.trim();
    let own = if keys.is_empty() {
        title.to_string()
    } else {
        format!("{title}   {keys}")
    };
    let with_shared = format!("{own}   {SHARED_KEYS}");
    if with_shared.chars().count() <= room {
        with_shared
    } else {
        own
    }
}

/// How much of the popup the list gets, which is where the header is drawn.
fn list_width(chrome: &Chrome, previewable: bool, width: u16) -> usize {
    let side_by_side = previewable
        && matches!(
            fitting_preview(chrome.preview, width),
            Some(PreviewDirection::Right) | Some(PreviewDirection::Left)
        );
    let cols = if side_by_side {
        u32::from(width) * u32::from(100 - chrome.preview_percent.min(95)) / 100
    } else {
        u32::from(width)
    };
    // Two for the border and two for the selection marker.
    (cols as usize).saturating_sub(4)
}

/// Whether this list has a preview pane at all.
///
/// A picker whose rows carry nothing to show gets the full width for its list
/// rather than half a popup of empty box.
fn previewable(chrome: &Chrome, items: &[Item]) -> bool {
    !chrome.preview_title.is_empty()
        && chrome.preview != Preview::None
        && items.iter().any(|i| !i.preview.is_empty())
}

/// The options themselves, once the preview question is settled.
fn options_with(chrome: &Chrome, query: &str, previewable: bool, width: u16) -> SkimOptions {
    // Built by mutation rather than by a struct literal: `SkimOptions` has
    // private fields, so `..Default::default()` cannot reach past them.
    let mut options = SkimOptions::default();

    // The query starts filled for the pickers that open on a directory, and
    // ctrl-u clears it, which is the readline habit.
    options.query = Some(query.to_string());
    options.prompt = chrome.prompt.clone();
    // The title and the keys, on the header line above the list.
    //
    // skim's own `footer` is private with `setter(skip)`, so a library cannot
    // put anything on the bottom border; the header is the one place a
    // picker can say what its keys do. It said only the title until now,
    // which meant `Chrome::footer` -- the line every picker used to draw --
    // was being built and thrown away.
    options.header = Some(header_line(chrome, list_width(chrome, previewable, width)));
    // The query on top with the list growing down from it, which is fzf's
    // `--reverse` and the way this was drawn before. `options.reverse` is the
    // flag the command line parses; `layout` is what the drawing reads, and
    // setting only the first leaves the list growing up off the bottom.
    options.layout = TuiLayout::Reverse;
    // Fill the popup tmux opened. Anything less leaves a band of the pane's
    // old contents showing through under the picker.
    options.height = "100%".to_string();
    // Best score first, then the shorter row, then the order they arrived in.
    // The length tie-break is doing real work: `zoom` and `a long way to say
    // zoom` score the same, and a picker where the exact answer sits below a
    // sentence containing it is one people stop trusting. Arrival order
    // settles the rest, which for `keys` is alphabetical by note.
    options.tiebreak = vec![
        RankCriteria::Score,
        RankCriteria::Length,
        RankCriteria::Index,
    ];

    if previewable && let Some(direction) = fitting_preview(chrome.preview, width) {
        // A preview command has to be set for the pane to exist at all, and
        // for skim to ask each row what it shows. It is never run: a row
        // answering with `AnsiText` is already "ready", so the command is
        // skipped. `true` rather than an empty string so that a row which
        // somehow answers `Global` runs something harmless.
        options.preview = Some("true".to_string());
        options.preview_window = PreviewLayout {
            direction,
            size: Size::Percent(chrome.preview_percent),
            wrap: true,
            ..Default::default()
        };
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── laying rows out ──────────────────────────────────────────────────────

    #[test]
    fn a_column_is_as_wide_as_its_widest_cell() {
        let rows = vec![
            vec!["session", "tmux-companion"],
            vec!["dir", "y"],
            vec!["dir", "a-much-longer-name"],
        ];
        assert_eq!(column_widths(&rows), vec![7, 18]);
    }

    #[test]
    fn every_row_puts_its_second_column_in_the_same_place() {
        // The bug this replaces: the label was padded with `{:<22}` over a
        // string holding ANSI escapes, so the padding counted bytes the
        // terminal never draws and no two rows lined up.
        let items = vec![
            Item::new("a").in_columns(vec!["session", "tmux-companion"]),
            Item::new("b").in_columns(vec!["dir", "y"]),
            Item::new("c").in_columns(vec!["dir", "notes"]),
        ];
        let laid = laid_out(&items);
        assert_eq!(
            laid,
            vec![
                "session  tmux-companion".to_string(),
                "dir      y".to_string(),
                "dir      notes".to_string(),
            ]
        );
    }

    #[test]
    fn the_last_column_is_not_padded() {
        // Trailing spaces would be matched against, and would push the row
        // wider than anything in it.
        let items = vec![
            Item::new("a").in_columns(vec!["dir", "short"]),
            Item::new("b").in_columns(vec!["dir", "a-much-longer-name"]),
        ];
        for row in laid_out(&items) {
            assert_eq!(row.trim_end(), row, "row was padded: {row:?}");
        }
    }

    #[test]
    fn a_row_without_columns_lays_out_as_its_label() {
        let items = vec![Item::new("just a label")];
        assert_eq!(laid_out(&items), vec!["just a label".to_string()]);
    }

    // ── colours ──────────────────────────────────────────────────────────────

    #[test]
    fn a_palette_index_and_a_hex_both_resolve() {
        assert_eq!(colour_of("colour29"), Some(Color::Indexed(29)));
        assert_eq!(colour_of("color29"), Some(Color::Indexed(29)));
        assert_eq!(colour_of("#ff8700"), Some(Color::Rgb(0xff, 0x87, 0x00)));
    }

    #[test]
    fn an_unreadable_colour_leaves_the_row_alone() {
        assert_eq!(colour_of("chartreuse"), None);
        assert_eq!(colour_of(""), None);
        assert_eq!(colour_of("#ff87"), None);
    }

    #[test]
    fn a_blank_colour_is_not_a_colour() {
        assert_eq!(Item::new("x").in_colour(Some("  ".into())).colour, None);
        assert_eq!(Item::new("x").with_swatch(Some(String::new())).swatch, None);
        assert_eq!(
            Item::new("x").in_colour(Some("colour4".into())).colour,
            Some("colour4".to_string())
        );
    }

    // ── options ──────────────────────────────────────────────────────────────

    #[test]
    fn the_header_says_what_the_keys_do() {
        // The footer was built by every call site and then dropped on the
        // floor: skim draws no footer a library can set, and the header was
        // being given the title alone.
        let chrome = Chrome {
            title: "[ Theme ]".into(),
            footer: "enter applies it".into(),
            ..Chrome::default()
        };
        let header = header_line(&chrome, 120);
        assert!(header.contains("[ Theme ]"), "{header}");
        assert!(header.contains("enter applies it"), "{header}");
        assert!(header.contains("ctrl-u clear"), "{header}");

        // A picker with nothing of its own still says the shared keys.
        let bare = Chrome {
            title: "[ Pick ]".into(),
            footer: String::new(),
            ..Chrome::default()
        };
        assert!(header_line(&bare, 120).contains("ctrl-u clear"));

        // No room, so the shared keys go rather than being cut off mid-word.
        let tight = header_line(&chrome, 30);
        assert!(tight.contains("enter applies it"), "{tight}");
        assert!(!tight.contains("ctrl-u"), "{tight}");
    }

    #[test]
    fn the_header_is_measured_against_the_list_not_the_popup() {
        // With a preview beside it the list is a fraction of the popup, and a
        // header measured against the popup is a header cut in half.
        let chrome = Chrome {
            preview_title: "[ Where ]".into(),
            preview_percent: 55,
            ..Chrome::default()
        };
        assert_eq!(list_width(&chrome, true, 200), 200 * 45 / 100 - 4);
        // No preview, so the list has all of it.
        assert_eq!(list_width(&chrome, false, 200), 196);
        // Narrow enough that the preview went underneath, so again all of it.
        assert_eq!(list_width(&chrome, true, 80), 76);
    }

    #[test]
    fn the_preview_pane_is_off_when_no_row_has_one() {
        let chrome = Chrome {
            preview_title: "[ Where ]".into(),
            ..Chrome::default()
        };
        let items = vec![Item::new("a"), Item::new("b")];
        assert!(options_for(&chrome, "", &items).preview.is_none());
    }

    #[test]
    fn the_preview_pane_is_off_when_the_layout_says_none() {
        let chrome = Chrome {
            preview_title: "[ Where ]".into(),
            preview: Preview::None,
            ..Chrome::default()
        };
        let items = vec![Item::with_preview("a", "something")];
        assert!(options_for(&chrome, "", &items).preview.is_none());
    }

    #[test]
    fn a_narrow_popup_puts_the_preview_underneath_instead() {
        // 45/55 of a 70-column popup gives the list 31 columns, which cuts
        // most rows in half. Below the threshold the split turns sideways.
        assert_eq!(
            fitting_preview(Preview::Right, 70),
            Some(PreviewDirection::Down)
        );
        assert_eq!(
            fitting_preview(Preview::Left, 70),
            Some(PreviewDirection::Down)
        );
        // Wide enough, and it is left alone.
        assert_eq!(
            fitting_preview(Preview::Right, 160),
            Some(PreviewDirection::Right)
        );
        // A preview already below, or switched off, is not second-guessed.
        assert_eq!(
            fitting_preview(Preview::Bottom, 70),
            Some(PreviewDirection::Down)
        );
        assert_eq!(fitting_preview(Preview::None, 200), None);
    }

    #[test]
    fn the_layout_decides_which_side_the_preview_is_on() {
        let items = vec![Item::with_preview("a", "something")];
        for (preview, expected) in [
            (Preview::Right, PreviewDirection::Right),
            (Preview::Left, PreviewDirection::Left),
            (Preview::Bottom, PreviewDirection::Down),
            (Preview::Top, PreviewDirection::Up),
        ] {
            let chrome = Chrome {
                preview_title: "[ Where ]".into(),
                preview,
                preview_percent: 40,
                ..Chrome::default()
            };
            let options = options_for(&chrome, "", &items);
            assert!(options.preview.is_some(), "{preview:?} drew no pane");
            assert_eq!(options.preview_window.direction, expected, "{preview:?}");
            assert_eq!(
                options.preview_window.size,
                Size::Percent(40),
                "{preview:?}"
            );
        }
    }

    #[test]
    fn an_absurd_preview_share_is_clamped_rather_than_obeyed() {
        let layout = crate::config::PickerLayout {
            preview: Preview::Right,
            preview_percent: 99,
        };
        assert_eq!(Chrome::default().laid_out_by(&layout).preview_percent, 80);

        let layout = crate::config::PickerLayout {
            preview: Preview::Right,
            preview_percent: 1,
        };
        assert_eq!(Chrome::default().laid_out_by(&layout).preview_percent, 20);
    }

    #[test]
    fn the_query_sits_above_the_list_rather_than_below_it() {
        // `options.reverse` is the flag the command line parses and the
        // drawing never reads; setting only that left the list growing up off
        // the bottom of the popup.
        let options = options_for(&Chrome::default(), "", &[Item::new("a")]);
        assert_eq!(options.layout, TuiLayout::Reverse);
    }

    #[test]
    fn the_opening_query_is_carried_into_the_picker() {
        let options = options_for(&Chrome::default(), "~/src", &[Item::new("a")]);
        assert_eq!(options.query.as_deref(), Some("~/src"));
    }

    #[test]
    fn the_shorter_row_wins_a_tie() {
        // Not a behaviour this module implements any more, but one it must
        // keep asking for: without it `zoom` sorts below `a long way to say
        // zoom`, which both score the same.
        let options = options_for(&Chrome::default(), "", &[Item::new("a")]);
        assert_eq!(
            options.tiebreak,
            vec![
                RankCriteria::Score,
                RankCriteria::Length,
                RankCriteria::Index
            ]
        );
    }
}
