//! Colours, styles and the string builder every segment renders through.
//!
//! Colours are 256-palette indices as strings, because that is what tmux's
//! `#[fg=colourN]` wants and converting back and forth buys nothing. The
//! contrast figures in the comments are measured, not guessed: several of
//! these were raised to clear 4.5:1 after a check against WCAG.

use super::icons::ARROW_RIGHT;

/// Clean work tree, nothing to report.
pub const BG_CLEAN: &str = "120";
/// The ordinary dirty-tree background.
pub const BG_DEFAULT: &str = "209";
/// A remote operation failed.
pub const BG_ERROR: &str = "160";
/// The upstream branch is gone.
pub const BG_GONE: &str = "088";
/// A fetch or push is in flight.
pub const BG_LOADING: &str = "056";
/// A branch with no upstream yet.
pub const BG_NEW: &str = "251";
/// The terminal's own background, used behind the suspended-editor marker.
pub const BG_TERMINAL: &str = "235";

/// Blue text on a light fill.
pub const FG_BLUE: &str = "33";
/// Text on the clean fill.
pub const FG_CLEAN: &str = "000";
/// Darker blue, for counts that sit beside blue text.
pub const FG_DARK_BLUE: &str = "24";
/// Text on the ordinary dirty fill.
pub const FG_DEFAULT: &str = "235";
/// Text on the gone-upstream fill.
pub const FG_GONE: &str = "255";
/// Green text, for staged counts.
pub const FG_GREEN: &str = "22";
/// Near-white text, the lightest foreground in the fill palette.
pub const FG_GREY89: &str = "254";
/// Text for the previous branch, on the dirty fill background.  colour025
/// measured 2.73:1 against colour209; colour017 is the same blue and 7.62:1.
pub const FG_PREVIOUS: &str = "017";
/// Purple text, for the stash count.
pub const FG_PURPLE: &str = "53";

/// Status-bar background, which is what a segment sits on.
///
/// Used as the segment background in the outline styles and as the
/// trailing-arrow background everywhere.
pub const BG_BAR: &str = "233";

// Outline-mode accents.  The fill palette picks colors readable on a *light*
// segment background; on the dark status bar those same colors disappear, so
// the bright outline style swaps in lighter equivalents.
/// Gone upstream, as an accent on the bar.
pub const AC_GONE: &str = "203";
/// In-flight fetch or push, as an accent on the bar.
pub const AC_LOADING: &str = "105";
/// Staged counts, as an accent on the bar.
pub const AC_GREEN: &str = "84";
/// Blue counts, as an accent on the bar.
pub const AC_DARK_BLUE: &str = "75";
/// Stash count, as an accent on the bar.
pub const AC_PURPLE: &str = "141";
/// A branch with no upstream, as an accent on the bar.
pub const AC_NEW: &str = "39";
/// The error state drawn as an accent on the bar.  BG_ERROR reads 3.47:1 there,
/// below the 4.5 WCAG asks of text; colour196 is the same red at 4.69:1.
pub const AC_ERROR: &str = "196";
/// Text on the error fill.  FG_GREY89 measured 4.25:1 on BG_ERROR, just under.
pub const FG_ON_ERROR: &str = "255";

/// Fill = solid state-colored background (the original look).
/// Outline = state color moves to the foreground, background becomes the bar,
/// icon colors untouched.  OutlineBright = same, with icon colors lightened so
/// they stay readable on the dark bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Style {
    /// Solid state-coloured background, the original look.
    Fill,
    /// State colour in the foreground, bar colour behind.
    Outline,
    /// Outline with the icon colours lightened for the dark bar.
    #[default]
    OutlineBright,
}

impl Style {
    /// Parse a `--style` value. `bright` is accepted as a synonym for
    /// `outline-bright`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fill" => Some(Style::Fill),
            "outline" => Some(Style::Outline),
            "outline-bright" | "bright" => Some(Style::OutlineBright),
            _ => None,
        }
    }
}

/// Every color the git segment draws with, resolved for one status + style.
/// The rendering code only reads from here, so a new style is a new palette
/// and no rendering changes.
#[derive(Clone, Copy, Debug)]
/// Every colour the git segment draws with, resolved for one status and one
/// [`Style`].
pub struct Palette {
    /// Segment background.
    pub bg: &'static str,
    /// Segment foreground.
    pub fg: &'static str,
    /// Filler that restores the main text color after a colored icon.
    /// Foreground to return to after a coloured run.
    pub reset_fg: &'static str,
    /// Color of the trailing end cap.
    /// Colour of the end cap.
    pub cap: &'static str,
    /// Glyph the segment ends with — solid arrow when filled, thin when not.
    /// Glyph the end cap draws.
    pub cap_glyph: &'static str,
    /// Foreground while a fetch or push is in flight.
    pub loading_fg: &'static str,
    /// Background while a fetch or push is in flight.
    pub loading_bg: &'static str,
    /// End-cap colour while a fetch or push is in flight.
    pub loading_cap: &'static str,
    /// Foreground after a failed remote operation.
    pub error_fg: &'static str,
    /// Background after a failed remote operation.
    pub error_bg: &'static str,
    /// End-cap colour after a failed remote operation.
    pub error_cap: &'static str,
    /// Colour of the previous branch name.
    pub prev_fg: &'static str,
    /// Colour of the untracked-file count.
    pub new_fg: &'static str,
    /// Colour of the staged-file count.
    pub green_fg: &'static str,
    /// Colour of the modified-file count.
    pub dirty_fg: &'static str,
    /// Colour of the ahead and behind counts.
    pub ahead_fg: &'static str,
    /// Colour of the conflicted-file count.
    pub unmerged_fg: &'static str,
    /// Colour of the stash count.
    pub stash_fg: &'static str,
}

/// Segment is an ordered list of string parts joined together to form a tmux status string.
/// Mirrors Go's pkg/ansi/segment.go `Segment []string`.
#[derive(Default, Clone)]
pub struct Segment(Vec<String>);

impl Segment {
    /// An empty segment.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Whether anything has been added yet.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Append a new element.
    pub fn add(&mut self, s: impl Into<String>) {
        self.0.push(s.into());
    }

    /// Append text to the last existing element; if empty, add as new.
    pub fn append(&mut self, s: &str) {
        if let Some(last) = self.0.last_mut() {
            last.push_str(s);
        } else {
            self.0.push(s.to_string());
        }
    }

    /// append only if segment is non-empty.
    pub fn append_only(&mut self, s: &str) {
        if !self.0.is_empty() {
            self.append(s);
        }
    }

    /// Prepend text to the first element; if empty, add as new.
    pub fn prepend(&mut self, s: &str) {
        if let Some(first) = self.0.first_mut() {
            let orig = first.clone();
            *first = format!("{}{}", s, orig);
        } else {
            self.0.push(s.to_string());
        }
    }

    /// Prepend only if segment is non-empty.
    pub fn prepend_only(&mut self, s: &str) {
        if !self.0.is_empty() {
            self.prepend(s);
        }
    }

    /// Add a formatted count+segment if count > 0.
    pub fn counter(&mut self, count: i32, segment: &str) {
        if count > 0 {
            self.0.push(format!("{}{}", count, segment));
        }
    }

    /// Add a segment if condition is true.
    pub fn when(&mut self, condition: bool, s: &str) {
        if condition {
            self.0.push(s.to_string());
        }
    }

    /// Join the parts with `sep` between them.
    pub fn join(&self, sep: &str) -> String {
        self.0.join(sep)
    }
}

/// `Segment` renders by concatenating its parts with no separator, so
/// `to_string` comes from `Display` rather than from an inherent method that
/// shadows it.
impl std::fmt::Display for Segment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.join(""))
    }
}

/// Build a tmux or ANSI color+content segment.
///
/// When no_tmux=true and content is exactly ARROW_RIGHT, emits only the color
/// escape codes (no arrow glyph) — matching Go's ColoredSegment special case.
pub fn colored_segment(no_tmux: bool, fg: &str, bg: &str, content: &str) -> String {
    if no_tmux {
        if content == ARROW_RIGHT {
            return format!("\x1b[38;5;{}m\x1b[48;5;{}m", fg, bg);
        }
        return format!("\x1b[38;5;{}m\x1b[48;5;{}m{}", fg, bg, content);
    }
    format!("#[fg=color{},bg=color{}]{}", fg, bg, content)
}

/// Build a tmux powerline segment (always tmux format, used for the trailing arrow).
pub fn powerline_segment(fg: &str, bg: &str, content: &str) -> String {
    format!("#[fg=color{},bg=color{}]{}", fg, bg, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::icons::ARROW_RIGHT;

    // ── Segment ──────────────────────────────────────────────────────────────

    #[test]
    fn new_segment_is_empty() {
        let s = Segment::new();
        assert!(s.is_empty());
        assert_eq!(s.to_string(), "");
    }

    #[test]
    fn add_makes_non_empty() {
        let mut s = Segment::new();
        s.add("hello");
        assert!(!s.is_empty());
        assert_eq!(s.to_string(), "hello");
    }

    #[test]
    fn add_multiple_elements_joined() {
        let mut s = Segment::new();
        s.add("a");
        s.add("b");
        s.add("c");
        assert_eq!(s.to_string(), "abc");
        assert_eq!(s.join("-"), "a-b-c");
    }

    #[test]
    fn append_to_last_element() {
        let mut s = Segment::new();
        s.add("foo");
        s.add("bar");
        s.append("!");
        // "!" appended to last element "bar" → "bar!"
        assert_eq!(s.to_string(), "foobar!");
    }

    #[test]
    fn append_to_empty_creates_element() {
        let mut s = Segment::new();
        s.append("x");
        assert_eq!(s.to_string(), "x");
    }

    #[test]
    fn append_only_no_op_on_empty() {
        let mut s = Segment::new();
        s.append_only("x");
        assert!(s.is_empty());
    }

    #[test]
    fn append_only_appends_when_non_empty() {
        let mut s = Segment::new();
        s.add("base");
        s.append_only("-suffix");
        assert_eq!(s.to_string(), "base-suffix");
    }

    #[test]
    fn prepend_to_first_element() {
        let mut s = Segment::new();
        s.add("world");
        s.add("!");
        s.prepend("hello ");
        // "hello " prepended to first element "world" → "hello world"
        assert_eq!(s.to_string(), "hello world!");
    }

    #[test]
    fn prepend_to_empty_creates_element() {
        let mut s = Segment::new();
        s.prepend("x");
        assert_eq!(s.to_string(), "x");
    }

    #[test]
    fn prepend_only_no_op_on_empty() {
        let mut s = Segment::new();
        s.prepend_only("x");
        assert!(s.is_empty());
    }

    #[test]
    fn prepend_only_prepends_when_non_empty() {
        let mut s = Segment::new();
        s.add("world");
        s.prepend_only("hello ");
        assert_eq!(s.to_string(), "hello world");
    }

    #[test]
    fn counter_zero_is_noop() {
        let mut s = Segment::new();
        s.counter(0, "icon");
        assert!(s.is_empty());
    }

    #[test]
    fn counter_positive_adds_formatted() {
        let mut s = Segment::new();
        s.counter(3, "⬆");
        assert_eq!(s.to_string(), "3⬆");
    }

    #[test]
    fn counter_negative_is_noop() {
        let mut s = Segment::new();
        s.counter(-1, "x");
        assert!(s.is_empty());
    }

    #[test]
    fn when_true_adds() {
        let mut s = Segment::new();
        s.when(true, "yes");
        assert_eq!(s.to_string(), "yes");
    }

    #[test]
    fn when_false_noop() {
        let mut s = Segment::new();
        s.when(false, "no");
        assert!(s.is_empty());
    }

    #[test]
    fn join_with_separator() {
        let mut s = Segment::new();
        s.add("a");
        s.add("b");
        s.add("c");
        assert_eq!(s.join("|"), "a|b|c");
        assert_eq!(s.join(""), "abc");
    }

    // ── colored_segment ───────────────────────────────────────────────────────

    #[test]
    fn colored_segment_tmux_mode() {
        assert_eq!(
            colored_segment(false, "025", "120", "hello"),
            "#[fg=color025,bg=color120]hello"
        );
    }

    #[test]
    fn colored_segment_tmux_with_arrow() {
        // In tmux mode, ARROW_RIGHT is included as-is (no special case).
        let s = colored_segment(false, "025", "120", ARROW_RIGHT);
        assert_eq!(s, format!("#[fg=color025,bg=color120]{}", ARROW_RIGHT));
    }

    #[test]
    fn colored_segment_tmux_empty_content() {
        assert_eq!(
            colored_segment(false, "025", "120", ""),
            "#[fg=color025,bg=color120]"
        );
    }

    #[test]
    fn colored_segment_no_tmux_mode() {
        assert_eq!(
            colored_segment(true, "025", "120", "hello"),
            "\x1b[38;5;025m\x1b[48;5;120mhello"
        );
    }

    #[test]
    fn colored_segment_no_tmux_arrow_right_special_case() {
        // When content is exactly ARROW_RIGHT in no-tmux mode: only color codes emitted.
        let s = colored_segment(true, "025", "120", ARROW_RIGHT);
        assert_eq!(s, "\x1b[38;5;025m\x1b[48;5;120m");
        assert!(!s.contains(ARROW_RIGHT), "arrow should not appear: {s:?}");
    }

    #[test]
    fn colored_segment_no_tmux_empty_content() {
        assert_eq!(
            colored_segment(true, "025", "120", ""),
            "\x1b[38;5;025m\x1b[48;5;120m"
        );
    }

    // ── powerline_segment ────────────────────────────────────────────────────

    #[test]
    fn powerline_segment_always_tmux_format() {
        assert_eq!(
            powerline_segment("120", "235", ARROW_RIGHT),
            format!("#[fg=color120,bg=color235]{}", ARROW_RIGHT)
        );
    }

    #[test]
    fn powerline_segment_empty_content() {
        assert_eq!(
            powerline_segment("120", "235", ""),
            "#[fg=color120,bg=color235]"
        );
    }
}
