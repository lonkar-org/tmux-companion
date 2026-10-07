//! The picker: a fuzzy-matched list in a terminal, shared by every chooser in
//! the tool.
//!
//! The screen is drawn here rather than by a library. It was skim, which is fzf
//! ported to Rust, and skim looks for a while as though it can draw what fzf
//! draws: it has `border_label`, `preview_label` and `footer` fields and it
//! accepts the flags that set them. All three are private, carry
//! `#[builder(setter(skip))]` and `arg(hide = true)`, and appear nowhere in its
//! drawing code. They are there so an fzf command line parses, not so it
//! renders, and a library user gets a border or no border with nothing on it.
//!
//! What a person coming from fzf has in their hands is a label on the bottom
//! border saying which picker this is, a line at the top saying what the keys
//! do, and a preview pane with a label of its own. Those are the settings in
//! [`crate::picker::Look`], and they are settings because somebody's tmux
//! already looks a particular way and the picker has to join it.
//!
//! The matching is [`nucleo_matcher`], which is the matcher helix uses, and the
//! drawing is ratatui. Both were already dependencies before this replaced
//! skim; nucleo was in `Cargo.toml` and used nowhere.
//!
//! The parts worth pinning are here rather than in a widget: how a row's
//! columns are measured so every line agrees, and how a tmux colour becomes a
//! terminal one. Both are pure, and both are what went wrong before -- the rows
//! used to be laid out with `{:<22}` over strings holding ANSI escapes, so the
//! padding was computed across bytes the terminal never draws and no two lines
//! landed in the same place.
//!
//! This runs in the **client** process, not the daemon. A daemon has no
//! terminal, and the whole point of a picker is that it owns one for as long as
//! somebody is looking at it.

pub mod ansi;
pub mod paint;
mod screen;
pub mod style;

use ratatui::style::Color;

pub use paint::{Paint, Tone};
pub use screen::show_framed;
pub use style::{BorderKind, Edge, LabelPosition, Look, PreviewBorder};

/// One row a picker can show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    /// What the matcher searches, and what the row shows when it has no
    /// columns of its own.
    pub label: String,
    /// Shown in the preview pane. Empty means this row has nothing to show.
    pub preview: String,
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
    /// What kind of text each column is, by the same position as `columns`.
    ///
    /// A column with no tone here is plain. This is what replaced colouring
    /// the whole row: a waiting pane's state is amber and its path is not.
    pub tones: Vec<Tone>,
    /// A glyph in front of the row saying what kind of row it is, and the
    /// tone it is drawn in. One of the constants in `src/tmux/icons.rs`, so
    /// the glyph preset reaches it.
    pub icon: Option<(String, Tone)>,
}

/// One column of a row and what kind of text it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// What it says.
    pub text: String,
    /// What kind of text it is.
    pub tone: Tone,
}

impl Cell {
    /// A cell in a tone.
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
    /// Text with nothing to say about itself.
    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, Tone::Plain)
    }
    /// The thing you act on.
    pub fn strong(text: impl Into<String>) -> Self {
        Self::new(text, Tone::Strong)
    }
    /// Detail.
    pub fn dim(text: impl Into<String>) -> Self {
        Self::new(text, Tone::Dim)
    }
    /// Words that are not the tool's own.
    pub fn quote(text: impl Into<String>) -> Self {
        Self::new(text, Tone::Quote)
    }
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

    /// The same item, drawn as cells that each say what kind of text they
    /// are. The columns line up as `in_columns` lines them up.
    pub fn in_cells(mut self, cells: Vec<Cell>) -> Self {
        self.tones = cells.iter().map(|c| c.tone).collect();
        self.columns = cells.into_iter().map(|c| c.text).collect();
        self
    }

    /// The same item, with a glyph in front saying what kind of row it is.
    pub fn with_icon(mut self, glyph: &str, tone: Tone) -> Self {
        self.icon = Some((glyph.trim_end().to_string(), tone));
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

/// One row's cells in the order the config asks for.
///
/// A position no row has is skipped rather than drawn as a blank column, and a
/// position left out is a column that is not drawn at all. An empty order is
/// the row as the picker built it.
pub fn reordered<T: Copy>(cells: &[T], order: &[usize]) -> Vec<T> {
    if order.is_empty() {
        return cells.to_vec();
    }
    order
        .iter()
        .filter_map(|i| cells.get(*i).copied())
        .collect()
}

/// Every row laid out against one set of column widths.
///
/// Pure, and the thing the ragged-column bug lived in, so it is tested
/// directly rather than through a terminal.
pub fn laid_out(items: &[Item]) -> Vec<String> {
    laid_out_in(items, &[])
}

/// Every row laid out, with the columns in the order the config asks for.
pub fn laid_out_in(items: &[Item], order: &[usize]) -> Vec<String> {
    laid_out_toned(items, order)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

/// Where each tone sits in a laid-out row, as a range of characters.
pub type Spans = Vec<(std::ops::Range<usize>, Tone)>;

/// Every row laid out, and the character range each column's tone covers.
///
/// The ranges are what let a row be drawn cell by cell after the matcher has
/// had the whole line: the matcher sees one string, the drawing sees where
/// each column starts and what kind of text it is.
pub fn laid_out_toned(items: &[Item], order: &[usize]) -> Vec<(String, Spans)> {
    let cells: Vec<Vec<&str>> = items.iter().map(|i| reordered(&i.cells(), order)).collect();
    let tones: Vec<Vec<Tone>> = items
        .iter()
        .map(|i| {
            let all: Vec<Tone> = (0..i.cells().len())
                .map(|c| i.tones.get(c).copied().unwrap_or_default())
                .collect();
            reordered(&all, order)
        })
        .collect();
    let widths = column_widths(&cells);
    cells
        .iter()
        .zip(tones)
        .map(|(row, tones)| {
            let mut spans = Vec::new();
            let mut at = 0;
            for (i, cell) in row.iter().enumerate() {
                let len = cell.chars().count();
                spans.push((at..at + len, tones.get(i).copied().unwrap_or_default()));
                at += widths.get(i).copied().unwrap_or(len).max(len) + COLUMN_GAP;
            }
            (lay_out(row, &widths), spans)
        })
        .collect()
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
    /// The pane this asks for, or `None` when it asks for no pane at all.
    fn pane(self) -> Option<Preview> {
        (self != Preview::None).then_some(self)
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
    /// The border, the labels, the rules and where each one sits.
    pub look: Look,
    /// The glyph inside the title's brackets, one of the constants in
    /// `src/tmux/icons.rs`. Empty draws the title alone.
    pub icon: String,
    /// Letters that pick a row straight away when pressed with alt, each with
    /// the row it picks, by index into the list as given. For the key that
    /// opened the picker, so pressing it twice does the usual thing: alt-s
    /// twice goes back to the previous session.
    pub alt_picks: Vec<(char, usize)>,
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
            look: Look::default(),
            icon: String::new(),
            alt_picks: Vec::new(),
        }
    }
}

impl Chrome {
    /// Take the shape from a resolved layout, leaving the labels alone.
    pub fn laid_out_by(mut self, resolved: &crate::config::Resolved) -> Self {
        self.preview = resolved.preview;
        // Zero is "no preview at all", which `run` uses, and clamping that up
        // to twenty would give it a pane holding nothing.
        self.preview_percent = if resolved.preview_percent == 0 {
            0
        } else {
            resolved.preview_percent.clamp(20, 80)
        };
        self.look = resolved.look.clone();
        self
    }

    /// The same chrome with one picker's config folded in, labels and all.
    ///
    /// `laid_out_by` takes the shape and leaves the words alone, which is what
    /// every call site wanted until the words became settable too.
    pub fn configured(
        mut self,
        layout: &crate::config::PickerLayout,
        which: crate::config::Picker,
    ) -> Self {
        let resolved = layout.resolved(which);
        let over = layout.overrides(which);
        if let Some(v) = over.label.clone() {
            self.title = v;
        }
        if let Some(v) = over.hint.clone() {
            self.footer = v;
        }
        // Only when the call site has a pane at all: a picker that says it has
        // nothing to preview must not grow one because a label was set.
        if let Some(v) = over.preview_label.clone()
            && !self.preview_title.is_empty()
        {
            self.preview_title = v;
        }
        self.laid_out_by(&resolved)
    }
}

// ── Running one ────────────────────────────────────────────────────

/// What somebody did with the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Picked a row, by index into the original list.
    Chosen(usize),
    /// Pressed enter on a query that matched nothing, or asked for the query
    /// itself.
    ///
    /// This is the only way to run a command that was never in the history, or
    /// to open a directory the source has never seen.
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

    // A picker with nothing to preview gets the popup for its list, whatever
    // the layout says, so a list of rows that carry no preview is not drawn
    // beside half a popup of empty box.
    let chrome = if previewable(chrome, &items) {
        chrome.clone()
    } else {
        Chrome {
            preview: Preview::None,
            ..chrome.clone()
        }
    };

    Ok(match screen::run(&items, query, &chrome)? {
        screen::Ended::Chosen(i) => Outcome::Chosen(i),
        screen::Ended::Typed(q) => Outcome::Typed(q),
        screen::Ended::Cancelled => Outcome::Cancelled,
        // No keys were asked for, so none can end it.
        screen::Ended::Key(_, i) => Outcome::Chosen(i),
    })
}

/// What somebody did with a picker that has keys of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keyed {
    /// Enter on a row, by index into the original list.
    Chosen(usize),
    /// One of the caller's ctrl keys on a row: the letter and the index.
    Key(char, usize),
    /// Esc, or enter on nothing.
    Cancelled,
}

/// Show the picker with ctrl keys of the caller's own, starting on a row.
///
/// For a list somebody acts on more than once: the caller does what the key
/// asked and opens the picker again at the same row. The keys are letters
/// pressed with ctrl; ones the picker already uses (cancel, clear, the
/// preview) stay the picker's.
pub fn run_keyed(
    items: Vec<Item>,
    query: &str,
    chrome: &Chrome,
    keys: &[char],
    start_at: Option<usize>,
) -> anyhow::Result<Keyed> {
    if items.is_empty() {
        return Ok(Keyed::Cancelled);
    }
    let chrome = if previewable(chrome, &items) {
        chrome.clone()
    } else {
        Chrome {
            preview: Preview::None,
            ..chrome.clone()
        }
    };
    Ok(
        match screen::run_keyed(&items, query, &chrome, keys, start_at)? {
            screen::Ended::Chosen(i) => Keyed::Chosen(i),
            screen::Ended::Key(c, i) => Keyed::Key(c, i),
            screen::Ended::Typed(_) | screen::Ended::Cancelled => Keyed::Cancelled,
        },
    )
}

/// Where the preview really goes, once the popup has had its say.
///
/// A percentage-sized popup on a small terminal is a small popup, and a split
/// of whatever it is given can leave the list too narrow to read a row in.
/// Turning the split sideways rather than obeying is the difference between a
/// readable list and two unreadable columns.
///
/// What decides it is how many columns the **list** would be left with, not
/// how wide the popup is. Those are not the same question once the preview's
/// share is a setting: the same popup leaves 37 columns at a 55% preview and
/// 25 at 70%, and a rule written against the popup width answers the same for
/// both. It used to be a popup-width constant of 96, which on a 170-column
/// terminal flipped tmux's own default popup -- 83 columns -- onto its back,
/// so a theme list configured to sit beside its card was stacked above it
/// instead. fzf, which these pickers are shaped after, has no such rule at
/// all and simply truncates.
///
/// Pure, and the arithmetic is the whole of it.
pub fn fitting_preview(
    preview: Preview,
    preview_percent: u16,
    width: u16,
    min_list_width: u16,
) -> Option<Preview> {
    let sideways = matches!(preview, Preview::Right | Preview::Left);
    if !sideways {
        return preview.pane();
    }
    let list = u32::from(width) * u32::from(100 - preview_percent.min(100)) / 100;
    if list < u32::from(min_list_width) {
        return Some(Preview::Bottom);
    }
    preview.pane()
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

/// Say that a picker has nothing to show, on stderr and, from a picker, on
/// tmux's message line as well.
///
/// The pickers run in `display-popup -E`, which closes the popup the moment
/// the command exits, so a line on stderr alone goes with it and the popup
/// only flashes. The message line outlives the popup. `--print` keeps to
/// stderr: its reader is a script, and its output stays what it was.
pub async fn say_nothing_to_show(text: &str, print: bool) {
    eprintln!("{text}");
    if !also_in_tmux(std::env::var_os("TMUX").is_some(), print) {
        return;
    }
    let text = format!("tmux-companion: {}", literal_for_tmux(text));
    // Output captured and dropped: tmux's own complaint, such as no client to
    // show it on, is not something the caller printed.
    let _ = crate::tmux::command()
        .args(["display-message", &text])
        .output()
        .await;
}

/// Whether a nothing-to-show line goes to tmux too: only inside tmux, and
/// never for `--print`.
fn also_in_tmux(in_tmux: bool, print: bool) -> bool {
    in_tmux && !print
}

/// Text for `display-message`, which expands formats: every `#` doubled so
/// the text is shown as written. `display-message -l` would do the same, but
/// it is newer than the tmux 3.2 the pickers are documented to need.
fn literal_for_tmux(text: &str) -> String {
    text.replace('#', "##")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── saying there is nothing ─────────────────────────────────────────────

    #[test]
    fn nothing_to_show_reaches_tmux_only_from_a_picker_inside_tmux() {
        assert!(also_in_tmux(true, false));
        assert!(!also_in_tmux(true, true), "--print stays on stderr");
        assert!(!also_in_tmux(false, false), "no tmux to tell");
        assert!(!also_in_tmux(false, true));
    }

    #[test]
    fn a_hash_reaches_tmux_as_a_hash() {
        assert_eq!(
            literal_for_tmux("no session idle for 3 days"),
            "no session idle for 3 days"
        );
        assert_eq!(literal_for_tmux("#{session_name}"), "##{session_name}");
        assert_eq!(literal_for_tmux("a ## b #"), "a #### b ##");
    }

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
    fn the_columns_are_drawn_in_the_order_the_config_asks_for() {
        // Whether the key or what it does comes first is a preference, and
        // people arrive with one.
        let items = vec![
            Item::new("a").in_columns(vec!["prefix k", "pane focus up"]),
            Item::new("b").in_columns(vec!["M-s", "pick a project"]),
        ];
        assert_eq!(
            laid_out_in(&items, &[1, 0]),
            vec![
                "pane focus up   prefix k".to_string(),
                "pick a project  M-s".to_string(),
            ]
        );
    }

    #[test]
    fn a_column_left_out_of_the_order_is_not_drawn() {
        // Which is how a picker is made narrower without touching the code
        // that fills it.
        let items = vec![Item::new("a").in_columns(vec!["prefix k", "pane focus up"])];
        assert_eq!(laid_out_in(&items, &[1]), vec!["pane focus up".to_string()]);
    }

    #[test]
    fn a_column_that_does_not_exist_is_skipped_rather_than_drawn_blank() {
        let cells = ["one", "two"];
        assert_eq!(reordered(&cells, &[1, 7, 0]), vec!["two", "one"]);
        // An empty order is the row as the picker built it.
        assert_eq!(reordered(&cells, &[]), vec!["one", "two"]);
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
    fn a_blank_swatch_is_not_a_colour() {
        assert_eq!(Item::new("x").with_swatch(Some(String::new())).swatch, None);
    }

    #[test]
    fn each_tone_covers_its_own_column_and_nothing_else() {
        // The whole point of cells: the state is amber, the path beside it is
        // not, and the padding between them belongs to neither.
        let items = vec![
            Item::new("a").in_cells(vec![
                Cell::strong("api:0.0"),
                Cell::new("asked", Tone::Waiting),
                Cell::dim("~/code/api"),
            ]),
            Item::new("b").in_cells(vec![
                Cell::strong("billing-worker:1.0"),
                Cell::plain("idle"),
                Cell::dim("~/b"),
            ]),
        ];
        let laid = laid_out_toned(&items, &[]);
        let (text, spans) = &laid[0];
        let at = |r: &std::ops::Range<usize>| -> String {
            text.chars().skip(r.start).take(r.len()).collect()
        };
        assert_eq!(at(&spans[0].0), "api:0.0");
        assert_eq!(spans[0].1, Tone::Strong);
        assert_eq!(at(&spans[1].0), "asked");
        assert_eq!(spans[1].1, Tone::Waiting);
        assert_eq!(at(&spans[2].0), "~/code/api");
        assert_eq!(spans[2].1, Tone::Dim);
        // Aligned with the longer row's columns.
        assert_eq!(spans[1].0.start, laid[1].1[1].0.start);
    }

    #[test]
    fn a_tone_follows_its_column_when_the_config_reorders_them() {
        let items = vec![Item::new("x").in_cells(vec![Cell::strong("key"), Cell::dim("does")])];
        let (text, spans) = &laid_out_toned(&items, &[1, 0])[0];
        assert!(text.starts_with("does"));
        assert_eq!(spans[0].1, Tone::Dim);
        assert_eq!(spans[1].1, Tone::Strong);
    }

    #[test]
    fn a_row_built_from_columns_alone_is_plain() {
        let items = vec![Item::new("x").in_columns(vec!["a", "b"])];
        let (_, spans) = &laid_out_toned(&items, &[])[0];
        assert!(spans.iter().all(|(_, t)| *t == Tone::Plain));
    }

    // ── layout ───────────────────────────────────────────────────

    #[test]
    fn the_preview_pane_is_off_when_no_row_has_one() {
        let chrome = Chrome {
            preview_title: "[ Where ]".into(),
            ..Chrome::default()
        };
        let items = vec![Item::new("a"), Item::new("b")];
        assert!(!previewable(&chrome, &items));
    }

    #[test]
    fn the_preview_pane_is_off_when_the_layout_says_none() {
        let chrome = Chrome {
            preview_title: "[ Where ]".into(),
            preview: Preview::None,
            ..Chrome::default()
        };
        let items = vec![Item::with_preview("a", "something")];
        assert!(!previewable(&chrome, &items));
    }

    #[test]
    fn a_picker_with_no_preview_label_draws_no_pane() {
        // The label is how a call site says it has nothing to show, which is
        // separate from the layout saying where a pane would go.
        let chrome = Chrome::default();
        let items = vec![Item::with_preview("a", "something")];
        assert!(!previewable(&chrome, &items));
    }

    #[test]
    fn a_popup_too_narrow_to_split_puts_the_preview_underneath() {
        // What decides it is the columns the list is left with, not the width
        // of the popup. tmux's own default popup on a 170-column terminal is
        // 83 columns, and the rule this replaces -- a popup-width constant of
        // 96 -- flipped it onto its back, so a theme list configured to sit
        // beside its card was stacked above it instead.
        assert_eq!(
            fitting_preview(Preview::Right, 70, 83, 24),
            Some(Preview::Right),
            "25 columns of list is a column of names, which is what it holds"
        );
        // Genuinely too narrow: a 40-column popup at 70% leaves 12.
        assert_eq!(
            fitting_preview(Preview::Right, 70, 40, 24),
            Some(Preview::Bottom)
        );
        assert_eq!(
            fitting_preview(Preview::Left, 70, 40, 24),
            Some(Preview::Bottom)
        );
        // The same popup with a smaller preview keeps its side-by-side split,
        // which a rule written against the popup width could not express.
        assert_eq!(
            fitting_preview(Preview::Right, 30, 40, 24),
            Some(Preview::Right)
        );
        // A preview already below, or switched off, is not second-guessed.
        assert_eq!(
            fitting_preview(Preview::Bottom, 70, 40, 24),
            Some(Preview::Bottom)
        );
        assert_eq!(fitting_preview(Preview::None, 70, 200, 24), None);
    }

    #[test]
    fn nothing_flips_when_the_minimum_is_zero() {
        // Which is how somebody turns the rule off: fzf has no such rule and
        // simply truncates.
        assert_eq!(
            fitting_preview(Preview::Right, 90, 30, 0),
            Some(Preview::Right)
        );
    }

    #[test]
    fn an_absurd_preview_share_is_clamped_rather_than_obeyed() {
        let layout = crate::config::PickerLayout {
            global: crate::config::PickerOverride {
                preview_percent: Some(99),
                ..Default::default()
            },
            ..Default::default()
        };
        let resolved = layout.resolved(crate::config::Picker::Theme);
        assert_eq!(Chrome::default().laid_out_by(&resolved).preview_percent, 80);

        let layout = crate::config::PickerLayout {
            global: crate::config::PickerOverride {
                preview_percent: Some(1),
                ..Default::default()
            },
            ..Default::default()
        };
        let resolved = layout.resolved(crate::config::Picker::Theme);
        assert_eq!(Chrome::default().laid_out_by(&resolved).preview_percent, 20);
    }

    #[test]
    fn the_look_comes_from_the_config_rather_than_from_the_call_site() {
        // Six pickers, one answer to what a picker looks like. The labels stay
        // with the call site, because only it knows what this one is.
        let layout = crate::config::PickerLayout {
            global: crate::config::PickerOverride {
                border: Some(BorderKind::Double),
                counter: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let chrome = Chrome {
            title: "[ Keys ]".into(),
            ..Chrome::default()
        }
        .configured(&layout, crate::config::Picker::Keys);
        assert_eq!(chrome.look.border, BorderKind::Double);
        assert!(chrome.look.counter);
        assert_eq!(chrome.title, "[ Keys ]", "the label is the call site's");
    }

    #[test]
    fn a_picker_says_what_the_config_tells_it_to_say() {
        let layout = crate::config::PickerLayout {
            keys: crate::config::PickerOverride {
                label: Some("[ Bindings ]".into()),
                hint: Some("enter runs it".into()),
                preview_label: Some("[ Command ]".into()),
                preview: Some(Preview::Bottom),
                ..crate::config::PickerOverride::default()
            },
            ..crate::config::PickerLayout::default()
        };
        let chrome = Chrome {
            title: "[ Keys ]".into(),
            footer: "the shipped wording".into(),
            preview_title: "[ What it runs ]".into(),
            ..Chrome::default()
        }
        .configured(&layout, crate::config::Picker::Keys);

        assert_eq!(chrome.title, "[ Bindings ]");
        assert_eq!(chrome.footer, "enter runs it");
        assert_eq!(chrome.preview_title, "[ Command ]");
        assert_eq!(chrome.preview, Preview::Bottom);

        // And another picker's table is not this one's.
        let other = Chrome {
            title: "[ Theme ]".into(),
            ..Chrome::default()
        }
        .configured(&layout, crate::config::Picker::Theme);
        assert_eq!(other.title, "[ Theme ]");
    }

    #[test]
    fn a_preview_label_does_not_give_a_pane_to_a_picker_that_has_none() {
        // `preview_title` empty is how a call site says it has nothing to
        // show, and a label set in the config must not override that into a
        // pane full of nothing.
        let layout = crate::config::PickerLayout {
            run: crate::config::PickerOverride {
                preview_label: Some("[ Command ]".into()),
                ..crate::config::PickerOverride::default()
            },
            ..crate::config::PickerLayout::default()
        };
        let chrome = Chrome {
            preview_title: String::new(),
            ..Chrome::default()
        }
        .configured(&layout, crate::config::Picker::Run);
        assert!(chrome.preview_title.is_empty());
        assert!(!previewable(&chrome, &[Item::with_preview("a", "b")]));
    }

    #[test]
    fn an_empty_picker_with_an_empty_query_is_a_cancel() {
        // No terminal is opened for it: there is nothing to pick and nothing
        // typed to fall back on.
        let outcome = run_with_query(vec![], "  ", &Chrome::default()).expect("no terminal needed");
        assert_eq!(outcome, Outcome::Cancelled);
    }
}
