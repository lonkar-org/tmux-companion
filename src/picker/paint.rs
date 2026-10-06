//! What colour, weight, slant and underline mean on a picker, one meaning each.
//!
//! The rule the whole module exists to keep: a style says something about the
//! text it is on, or it is not used. Colouring a whole row because one of its
//! columns is a state paints the path and the pane id in the state's colour
//! too, and a list where everything is coloured has nothing that stands out.
//! So a row is built as cells, each cell says what kind of text it is, and
//! this is the one place that decides what that kind looks like.
//!
//! | Tone | Drawn as | Means |
//! |---|---|---|
//! | `Plain` | the terminal's own | text with nothing to say about itself |
//! | `Strong` | bold | the thing you act on: a session, a port, a key |
//! | `Dim` | grey | detail: a pid, a time, a path, a repeat of the row above |
//! | `Quote` | italic | words that are not ours: an agent's question, a note |
//! | `Link` | underline | something `open` can open |
//! | `Asked` | bright red, bold | an agent asked you something and is stopped on it |
//! | `Waiting` | amber | wants you |
//! | `Busy` | green | working, needs nothing |
//! | `Done` | steel blue | an agent answered and waits for the next prompt |
//! | `Reading` | muted mauve | somebody is reading the pane in copy mode |
//! | `Ok` | green | on, fine |
//! | `Failed` | red | broken |
//! | `Accent` | the theme's colour | the cursor, the query's matches, a label |
//!
//! Focus is the band behind the row the cursor is on, and nothing else. The
//! accent is the theme's `@theme-color-main-1`, so a popup matches the bar it
//! was opened from.

use ratatui::style::{Color, Modifier, Style};

/// What kind of text a cell is. See the module table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    /// Nothing to say about itself.
    #[default]
    Plain,
    /// The thing you act on.
    Strong,
    /// Detail, or a repeat of the row above.
    Dim,
    /// Words that are not the tool's own.
    Quote,
    /// Something `open` can open.
    Link,
    /// An agent asked you something and is stopped on it: the one thing on
    /// any screen here that is blocked on you, so the loudest tone there is.
    Asked,
    /// Wants you.
    Waiting,
    /// Working, and needs nothing.
    Busy,
    /// An agent answered and waits for the next prompt: nothing blocked.
    Done,
    /// Somebody is reading the pane in copy mode.
    Reading,
    /// On, fine.
    Ok,
    /// Broken.
    Failed,
    /// The theme's colour.
    Accent,
}

/// Amber: the colour the bar draws a waiting agent in, so the row that wants
/// you here is the same colour as the count that sent you here.
pub const WAITING: Color = Color::Indexed(214);
/// Bright red, drawn bold: an agent stopped on a question. Brighter than
/// `FAILED`, which is a thing broken rather than a thing waiting on you.
pub const ASKED: Color = Color::Indexed(196);
/// Steel blue: an agent that answered. Passive, and apart from `DIM`, which
/// is detail rather than a state.
pub const DONE: Color = Color::Indexed(67);
/// Muted mauve: a pane in copy mode, being read.
pub const READING: Color = Color::Indexed(139);
/// Green: the bar's busy count.
pub const BUSY: Color = Color::Indexed(114);
/// Red.
pub const FAILED: Color = Color::Indexed(203);
/// Grey for detail. Light enough to read on the band.
pub const DIM: Color = Color::Indexed(245);
/// The frame and the rules: there, and quieter than anything inside them.
pub const FRAME: Color = Color::Indexed(240);
/// Behind the row the cursor is on, when there is no theme colour to tint it
/// with.
pub const BAND: Color = Color::Indexed(238);
/// What the band is tinted from: dark enough that `ON_BAND` reads on it
/// whatever the accent, and not the terminal's own black, so the band shows on
/// a background that is.
const BAND_BASE: (u8, u8, u8) = (0x1c, 0x1c, 0x1c);
/// How much of the accent is in the band. Enough to say "this theme", little
/// enough that the band is a surface rather than a highlight.
const BAND_TINT: f64 = 0.28;
/// Plain text on the band. The band sets its own foreground because the
/// terminal's default could be dark, and dark on 236 cannot be read.
pub const ON_BAND: Color = Color::Indexed(253);
/// The accent when there is no theme to take one from.
pub const FALLBACK_ACCENT: Color = Color::Magenta;

/// Everything a picker needs to turn a tone into a style.
#[derive(Debug, Clone)]
pub struct Paint {
    /// The theme's colour.
    pub accent: Color,
    /// Behind the row the cursor is on: the accent, mostly dark.
    pub band: Color,
    /// False under `NO_COLOR`: weight, slant and underline stay, colour goes.
    pub colour: bool,
    /// Whether text printed to stdout carries escapes and icons at all.
    ///
    /// Off for a pipe, for `--print`, and in tests, so what a script reads
    /// and what the tests pin is the plain text it always was. A picker draws
    /// through ratatui and never looks at this.
    pub escapes: bool,
    /// The glyph preset, applied to icons and labels the way the bar applies
    /// it to segments.
    pub glyphs: crate::config::GlyphMap,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            accent: FALLBACK_ACCENT,
            band: BAND,
            colour: true,
            escapes: false,
            glyphs: crate::config::GlyphMap::default(),
        }
    }
}

impl Paint {
    /// The paint for this terminal: the attached client's theme colour, the
    /// configured glyphs, and whether `NO_COLOR` is set.
    ///
    /// One `list-clients` and one config read, once per picker. Anything that
    /// fails leaves the default rather than stopping the picker: a popup in
    /// the wrong colour is still a popup.
    pub fn detect() -> Self {
        let glyphs = crate::config::load()
            .map(|(c, _)| crate::config::GlyphMap::new(&c.glyphs))
            .unwrap_or_default();
        let ask = |args: &[&str]| -> String {
            crate::tmux::command_sync()
                .args(args)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default()
        };
        let accent = std::env::var_os("TMUX")
            .and_then(|_| {
                // `list-clients` rather than `display-message`: a popup is not
                // a client, and an untargeted question is answered for the
                // session tmux touched last rather than the one on screen.
                // With no client attached at all -- a picker run from a
                // script -- that last-touched session is the best there is.
                accent_from(&ask(&[
                    "list-clients",
                    "-F",
                    "#{client_activity}\t#{@theme-color-main-1}",
                ]))
                .or_else(|| {
                    accent_from(&ask(&[
                        "display-message",
                        "-p",
                        "0\t#{@theme-color-main-1}",
                    ]))
                })
            })
            .unwrap_or(FALLBACK_ACCENT);
        let accent = readable_accent(accent);
        Self {
            accent,
            band: band_for(accent),
            colour: no_color_unset(std::env::var_os("NO_COLOR").as_deref()),
            escapes: false,
            glyphs,
        }
    }

    /// The paint for text printed to stdout: the picker's, with escapes on
    /// when stdout is a terminal and `print` did not ask for plain text.
    pub fn for_stdout(print: bool) -> Self {
        use std::io::IsTerminal;
        if print || !std::io::stdout().is_terminal() {
            return Self::plain();
        }
        Self {
            escapes: true,
            ..Self::detect()
        }
    }

    /// No escapes and no icons: text exactly as it reads in a pipe.
    pub fn plain() -> Self {
        Self::default()
    }

    /// Text in a tone, as a terminal draws it.
    pub fn ink(&self, text: &str, tone: Tone) -> String {
        self.ink_style(text, self.style(tone))
    }

    /// Text in a style, as a terminal draws it. Unchanged without escapes.
    pub fn ink_style(&self, text: &str, style: Style) -> String {
        if !self.escapes || text.is_empty() {
            return text.to_string();
        }
        let codes = sgr(style);
        if codes.is_empty() {
            return text.to_string();
        }
        format!("\x1b[{codes}m{text}\x1b[0m")
    }

    /// An icon and the space after it, or nothing when escapes are off.
    pub fn icon(&self, glyph: &str, tone: Tone) -> String {
        let glyph = self.glyph(glyph);
        let glyph = glyph.trim_end();
        if !self.escapes || glyph.is_empty() {
            return String::new();
        }
        format!("{} ", self.ink(glyph, tone))
    }

    /// An icon and the space after it for a ratatui span, where escapes are
    /// not this struct's business; nothing when the preset maps it away.
    pub fn icon_cell(&self, glyph: &str) -> String {
        let glyph = self.glyph(glyph);
        let glyph = glyph.trim_end();
        if glyph.is_empty() {
            String::new()
        } else {
            format!("{glyph} ")
        }
    }

    /// A section heading: its icon and its words in the accent, in bold.
    pub fn heading(&self, glyph: &str, text: &str) -> String {
        let style = self.style(Tone::Accent).add_modifier(Modifier::BOLD);
        format!(
            "{}{}",
            self.icon(glyph, Tone::Accent),
            self.ink_style(text, style)
        )
    }

    /// A tone as a style, off the band.
    pub fn style(&self, tone: Tone) -> Style {
        let s = Style::default();
        let s = match tone {
            Tone::Plain => s,
            Tone::Strong => s.add_modifier(Modifier::BOLD),
            Tone::Dim => s.fg(DIM),
            Tone::Quote => s.add_modifier(Modifier::ITALIC),
            Tone::Link => s.add_modifier(Modifier::UNDERLINED),
            Tone::Asked => s.fg(ASKED).add_modifier(Modifier::BOLD),
            Tone::Waiting => s.fg(WAITING),
            Tone::Busy | Tone::Ok => s.fg(BUSY),
            Tone::Done => s.fg(DONE),
            Tone::Reading => s.fg(READING),
            Tone::Failed => s.fg(FAILED),
            Tone::Accent => s.fg(self.accent),
        };
        self.checked(s)
    }

    /// A tone as a style on the band. Plain, bold, italic and underlined text
    /// take the band's own foreground; a coloured tone keeps its colour.
    pub fn on_band(&self, tone: Tone) -> Style {
        let s = self.style(tone);
        if !self.colour {
            return s.add_modifier(Modifier::REVERSED);
        }
        let s = if s.fg.is_none() { s.fg(ON_BAND) } else { s };
        s.bg(self.band)
    }

    /// What the query matched: the accent, in bold.
    pub fn matched(&self, band: bool) -> Style {
        let s = self
            .checked(Style::default().fg(self.accent))
            .add_modifier(Modifier::BOLD);
        if band && self.colour {
            s.bg(self.band)
        } else if band {
            s.add_modifier(Modifier::REVERSED)
        } else {
            s
        }
    }

    /// The border and the rules.
    pub fn frame(&self) -> Style {
        self.checked(Style::default().fg(FRAME))
    }

    /// A glyph with the preset applied.
    pub fn glyph(&self, glyph: &str) -> String {
        self.glyphs.apply(glyph).into_owned()
    }

    /// The style with its colours taken away when colour is off.
    fn checked(&self, s: Style) -> Style {
        if self.colour {
            s
        } else {
            Style {
                fg: None,
                bg: None,
                ..s
            }
        }
    }
}

/// A style as the parameters of one SGR escape, `1;38;5;214`.
pub fn sgr(style: Style) -> String {
    let mut codes: Vec<String> = Vec::new();
    let m = style.add_modifier;
    for (flag, code) in [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::REVERSED, "7"),
    ] {
        if m.contains(flag) {
            codes.push(code.to_string());
        }
    }
    let colour = |c: Color, base: u8| -> Option<String> {
        Some(match c {
            Color::Indexed(n) => format!("{};5;{n}", base + 8),
            Color::Rgb(r, g, b) => format!("{};2;{r};{g};{b}", base + 8),
            Color::Black => format!("{base}"),
            Color::Red => format!("{}", base + 1),
            Color::Green => format!("{}", base + 2),
            Color::Yellow => format!("{}", base + 3),
            Color::Blue => format!("{}", base + 4),
            Color::Magenta => format!("{}", base + 5),
            Color::Cyan => format!("{}", base + 6),
            Color::Gray => format!("{}", base + 7),
            _ => return None,
        })
    };
    if let Some(c) = style.fg.and_then(|c| colour(c, 30)) {
        codes.push(c);
    }
    if let Some(c) = style.bg.and_then(|c| colour(c, 40)) {
        codes.push(c);
    }
    codes.join(";")
}

/// The band for an accent: the accent mixed into a near-black.
///
/// As much of the accent as `BAND_TINT` allows, backed off until the band's
/// own text clears 7:1, which is WCAG AAA and the bar the themes are held to.
/// A dark accent keeps its full tint; a pale one like colour231 gets less,
/// because at the full tint its band is a grey the text no longer reads on.
///
/// An accent that is not a palette index or an RGB triple -- the named
/// fallback -- gets the plain grey band, because there is nothing to mix.
pub fn band_for(accent: Color) -> Color {
    let rgb = match accent {
        Color::Indexed(i) => crate::theme::rgb(i),
        Color::Rgb(r, g, b) => (r, g, b),
        _ => return BAND,
    };
    let mix = |a: u8, base: u8, t: f64| -> u8 {
        (f64::from(base) + (f64::from(a) - f64::from(base)) * t).round() as u8
    };
    let on = crate::theme::rgb(253);
    let mut tint = BAND_TINT;
    loop {
        let band = (
            mix(rgb.0, BAND_BASE.0, tint),
            mix(rgb.1, BAND_BASE.1, tint),
            mix(rgb.2, BAND_BASE.2, tint),
        );
        if tint <= 0.0 || crate::theme::contrast(on, band) >= 7.0 {
            return Color::Rgb(band.0, band.1, band.2);
        }
        tint -= 0.02;
    }
}

/// `NO_COLOR` set to anything but the empty string turns colour off, which is
/// what <https://no-color.org> says.
pub fn no_color_unset(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_none_or(|v| v.is_empty())
}

/// The theme colour of the client that did something last, out of
/// `list-clients -F '#{client_activity}\t#{@theme-color-main-1}'`.
///
/// The most recent rather than the first, because the popup was opened by a
/// keypress and the client that pressed it is the one that just did
/// something. A client with no theme set is skipped rather than chosen.
pub fn accent_from(list: &str) -> Option<Color> {
    list.lines()
        .filter_map(|l| {
            let (at, colour) = l.split_once('\t')?;
            let c = super::colour_of(colour)?;
            Some((at.trim().parse::<u64>().unwrap_or(0), c))
        })
        .max_by_key(|(at, _)| *at)
        .map(|(_, c)| c)
}

/// The accent, stepped lighter in its own hue until it reads as text.
///
/// It draws the query's matches, the picker's label and the jump overlay's
/// letters, all of them text somebody has to read, and a theme colour is
/// chosen for the bar rather than for that: a dark blue reads as a block of
/// background and not as a letter on a dark terminal. The background tested
/// against is the band's base, the darkest surface the accent is drawn on
/// that is not the terminal's own black, so a pass here passes on black too.
/// A colour that isn't a palette index is left as it is.
pub fn readable_accent(accent: Color) -> Color {
    match accent {
        Color::Indexed(i) => {
            Color::Indexed(crate::theme::lifted(i, BAND_BASE, crate::theme::TEXT_MIN_AA).0)
        }
        other => other,
    }
}

/// The words of a key hint, each marked as a key or not.
///
/// `enter jumps there   ctrl-a clears the filter` is split into its groups by
/// the three spaces between them, and the first word of a group is a key when
/// it looks like one. `type a path` does not start with a key and is left as
/// words.
pub fn hint_parts(hint: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for (i, group) in hint.trim().split("   ").enumerate() {
        if i > 0 {
            out.push(("   ".to_string(), false));
        }
        let group = group.trim();
        match group.split_once(' ') {
            Some((first, rest)) if is_key(first) => {
                out.push((first.to_string(), true));
                out.push((format!(" {rest}"), false));
            }
            None if is_key(group) => out.push((group.to_string(), true)),
            _ => out.push((group.to_string(), false)),
        }
    }
    out
}

/// Whether a word names a key: a named key, a chord, or one character, with
/// or without the `[enter]` brackets the resurrect screen writes them in.
pub fn is_key(word: &str) -> bool {
    let word = word
        .strip_prefix('[')
        .and_then(|w| w.strip_suffix(']'))
        .unwrap_or(word);
    const NAMED: [&str; 12] = [
        "enter",
        "esc",
        "tab",
        "up",
        "down",
        "left",
        "right",
        "space",
        "backspace",
        "pgup",
        "pgdn",
        "del",
    ];
    let lower = word.to_ascii_lowercase();
    if NAMED.contains(&lower.as_str()) {
        return true;
    }
    if let Some((mods, key)) = lower.rsplit_once('-') {
        return !key.is_empty()
            && mods
                .split('-')
                .all(|m| matches!(m, "ctrl" | "alt" | "shift" | "c" | "m" | "s"));
    }
    word.chars().count() == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dark_accent_is_lifted_until_it_reads_as_text() {
        use crate::theme::{TEXT_MIN_AA, contrast, rgb};
        // blue-dark's colour17, which measured 1.13 against the background.
        let Color::Indexed(lifted) = readable_accent(Color::Indexed(17)) else {
            panic!("an index stays an index");
        };
        assert!(contrast(rgb(lifted), BAND_BASE) >= TEXT_MIN_AA, "{lifted}");
        assert_eq!(readable_accent(Color::Indexed(214)), Color::Indexed(214));
        assert_eq!(readable_accent(Color::Magenta), Color::Magenta);
    }

    #[test]
    fn the_accent_is_the_most_recently_active_clients_theme() {
        let list = "100\tcolour68\n300\tcolour208\n200\tcolour114\n";
        assert_eq!(accent_from(list), Some(Color::Indexed(208)));
    }

    #[test]
    fn a_client_with_no_theme_is_not_the_accent() {
        // The newest client has nothing set; the older one's colour wins
        // rather than no colour at all.
        assert_eq!(
            accent_from("100\tcolour68\n300\t\n"),
            Some(Color::Indexed(68))
        );
        assert_eq!(accent_from("300\t\n"), None);
        assert_eq!(accent_from(""), None);
    }

    #[test]
    fn the_state_colours_are_the_bars() {
        // A waiting agent is one colour on the bar and in the picker it
        // opens, or the eye has to learn two.
        assert_eq!(
            super::super::colour_of(crate::panes::WAITING_COLOUR),
            Some(WAITING)
        );
        assert_eq!(
            super::super::colour_of(crate::panes::BUSY_COLOUR),
            Some(BUSY)
        );
    }

    #[test]
    fn the_band_is_the_accent_mostly_dark_and_text_on_it_reads() {
        // Ember, colour208: a brown the band's own text clears easily.
        let Color::Rgb(r, g, b) = band_for(Color::Indexed(208)) else {
            panic!("an indexed accent mixes to rgb");
        };
        assert!(
            r > g && g > b,
            "still reads as the accent's hue: {r},{g},{b}"
        );
        let on = crate::theme::rgb(253);
        for accent in [68u8, 104, 114, 170, 179, 208, 231, 46] {
            let Color::Rgb(r, g, b) = band_for(Color::Indexed(accent)) else {
                panic!()
            };
            let c = crate::theme::contrast(on, (r, g, b));
            assert!(c >= 7.0, "colour{accent}: {c:.1}:1 on the band");
        }
        assert_eq!(band_for(Color::Magenta), BAND);
    }

    #[test]
    fn printed_text_is_plain_without_escapes_and_styled_with_them() {
        let plain = Paint::plain();
        assert_eq!(plain.ink("asked", Tone::Waiting), "asked");
        assert_eq!(plain.icon(crate::tmux::icons::WAITING, Tone::Waiting), "");
        assert_eq!(
            plain.heading(crate::tmux::icons::WAITING, "Waiting"),
            "Waiting"
        );

        let on = Paint {
            escapes: true,
            ..Paint::default()
        };
        assert_eq!(on.ink("asked", Tone::Waiting), "\x1b[38;5;214masked\x1b[0m");
        assert_eq!(on.ink("b", Tone::Strong), "\x1b[1mb\x1b[0m");
        assert_eq!(on.ink("q", Tone::Quote), "\x1b[3mq\x1b[0m");
        // Plain text is not wrapped in an escape that says nothing.
        assert_eq!(on.ink("x", Tone::Plain), "x");
    }

    #[test]
    fn no_color_on_a_terminal_keeps_the_weight_and_drops_the_colour() {
        let p = Paint {
            escapes: true,
            colour: false,
            ..Paint::default()
        };
        assert_eq!(p.ink("asked", Tone::Waiting), "asked");
        assert_eq!(p.heading("", "Health"), "\x1b[1mHealth\x1b[0m");
    }

    #[test]
    fn no_color_set_to_anything_turns_colour_off() {
        use std::ffi::OsStr;
        assert!(no_color_unset(None));
        assert!(no_color_unset(Some(OsStr::new(""))));
        assert!(!no_color_unset(Some(OsStr::new("1"))));
    }

    #[test]
    fn without_colour_a_state_keeps_nothing_but_its_weight() {
        let p = Paint {
            colour: false,
            ..Paint::default()
        };
        assert_eq!(p.style(Tone::Waiting), Style::default());
        assert_eq!(
            p.style(Tone::Strong),
            Style::default().add_modifier(Modifier::BOLD)
        );
        // The band has to show without colour too, so it reverses instead.
        assert!(
            p.on_band(Tone::Plain)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn on_the_band_plain_text_takes_the_bands_foreground_and_a_state_keeps_its_own() {
        let p = Paint::default();
        assert_eq!(p.on_band(Tone::Plain).fg, Some(ON_BAND));
        assert_eq!(p.on_band(Tone::Waiting).fg, Some(WAITING));
        assert_eq!(p.on_band(Tone::Waiting).bg, Some(BAND));
    }

    #[test]
    fn each_tone_says_one_thing() {
        // Two tones drawn the same would be two meanings nobody can tell
        // apart. Busy and Ok share green on purpose: both say "needs nothing".
        let p = Paint::default();
        let tones = [
            Tone::Plain,
            Tone::Strong,
            Tone::Dim,
            Tone::Quote,
            Tone::Link,
            Tone::Waiting,
            Tone::Busy,
            Tone::Failed,
            Tone::Accent,
        ];
        for (i, a) in tones.iter().enumerate() {
            for b in &tones[i + 1..] {
                assert_ne!(p.style(*a), p.style(*b), "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn a_hint_marks_the_key_at_the_start_of_each_group() {
        let parts = hint_parts("enter jumps there   ctrl-a clears the filter   esc cancels");
        let keys: Vec<&str> = parts
            .iter()
            .filter(|(_, k)| *k)
            .map(|(w, _)| w.as_str())
            .collect();
        assert_eq!(keys, ["enter", "ctrl-a", "esc"]);
        // Nothing is lost on the way.
        let joined: String = parts.iter().map(|(w, _)| w.as_str()).collect();
        assert_eq!(
            joined,
            "enter jumps there   ctrl-a clears the filter   esc cancels"
        );
    }

    #[test]
    fn a_group_that_starts_with_a_word_is_not_a_key() {
        let parts = hint_parts("up = last session   type a path for a new one");
        assert_eq!(parts[0], ("up".to_string(), true));
        assert!(
            parts
                .iter()
                .any(|(w, k)| w == "type a path for a new one" && !k)
        );
    }

    #[test]
    fn chords_and_single_characters_are_keys_and_words_are_not() {
        for k in [
            "ctrl-x",
            "C-t",
            "M-s",
            "alt-enter",
            "g",
            "?",
            "enter",
            "[tab]",
            "[q]",
        ] {
            assert!(is_key(k), "{k}");
        }
        for w in ["type", "jumps", "rate-limit", "-", "ctrl-"] {
            assert!(!is_key(w), "{w}");
        }
    }
}
