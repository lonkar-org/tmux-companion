//! Enough ANSI to draw a preview.
//!
//! Previews arrive as text somebody already coloured: a theme card, a `git
//! status`, the body of a key binding. skim took those as `AnsiText` and parsed
//! them itself, so replacing skim means parsing them here.
//!
//! This handles SGR and nothing else. A cursor movement or a scroll region in a
//! preview would be a preview trying to drive the terminal, which is not a
//! thing any caller here does and not a thing a picker should let it start
//! doing: the sequence is dropped and its text is kept, so the worst case is a
//! preview that loses a colour rather than one that redraws the screen.
//!
//! `ansi-to-tui` does this properly and is one dependency away. It is not here
//! because what a preview needs is the colour half of SGR, which is this file,
//! and the crate brings a parser for the rest of the escape space along with
//! it.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Split coloured text into lines ratatui can draw.
pub fn into_lines(text: &str) -> Vec<Line<'static>> {
    text.split('\n')
        .map(|l| into_line(l.trim_end_matches('\r')))
        .collect()
}

/// One line of coloured text.
///
/// The style carries across spans within the line, which is what a terminal
/// does; it does not carry across lines, because a preview that leaves a colour
/// open should not paint the rest of the pane with it.
fn into_line(line: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut text = String::new();
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            text.push(c);
            continue;
        }
        // `ESC [ ... <final>`. Anything else -- ESC ] for a title, a bare ESC
        // at the end of the line -- is dropped along with what follows it up to
        // the next byte that could end a sequence.
        if chars.peek() != Some(&'[') {
            continue;
        }
        chars.next();
        let mut body = String::new();
        let mut final_byte = None;
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                final_byte = Some(c);
                break;
            }
            body.push(c);
        }
        if final_byte != Some('m') {
            continue;
        }
        if !text.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut text), style));
        }
        style = apply(style, &body);
    }
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// One SGR parameter list, applied to a style.
///
/// Pure, and where every colour bug will be, so it is tested directly.
pub fn apply(style: Style, body: &str) -> Style {
    // `ESC[m` is `ESC[0m`, which is the reset every preview ends on.
    if body.is_empty() {
        return Style::default();
    }
    let codes: Vec<u32> = body
        .split(';')
        .map(|p| p.trim().parse::<u32>().unwrap_or(0))
        .collect();

    let mut style = style;
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            c @ 30..=37 => style = style.fg(basic(c - 30)),
            38 => {
                let (colour, used) = extended(&codes[i..]);
                if let Some(c) = colour {
                    style = style.fg(c);
                }
                i += used;
            }
            39 => style.fg = None,
            c @ 40..=47 => style = style.bg(basic(c - 40)),
            48 => {
                let (colour, used) = extended(&codes[i..]);
                if let Some(c) = colour {
                    style = style.bg(c);
                }
                i += used;
            }
            49 => style.bg = None,
            c @ 90..=97 => style = style.fg(Color::Indexed((c - 90 + 8) as u8)),
            c @ 100..=107 => style = style.bg(Color::Indexed((c - 100 + 8) as u8)),
            _ => {}
        }
        i += 1;
    }
    style
}

/// One of the eight, as a palette index rather than as a named colour.
///
/// An index, because a named colour is the terminal's idea of red and the
/// palette index is the one the rest of this tool writes: a swatch built from
/// `colour1` and a preview built from `ESC[31m` have to be the same red.
fn basic(n: u32) -> Color {
    Color::Indexed(n as u8)
}

/// `38;5;n` and `38;2;r;g;b`, and how many parameters they ate.
fn extended(codes: &[u32]) -> (Option<Color>, usize) {
    match codes.get(1) {
        Some(5) => (codes.get(2).map(|n| Color::Indexed(*n as u8)), 2),
        Some(2) => match (codes.get(2), codes.get(3), codes.get(4)) {
            (Some(r), Some(g), Some(b)) => (Some(Color::Rgb(*r as u8, *g as u8, *b as u8)), 4),
            // Truncated, which is a preview that was cut off mid-sequence.
            // Eat what is there and colour nothing.
            _ => (None, codes.len() - 1),
        },
        _ => (None, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(line: &Line<'_>) -> Vec<(String, Style)> {
        line.spans
            .iter()
            .map(|s| (s.content.to_string(), s.style))
            .collect()
    }

    #[test]
    fn plain_text_is_one_span_with_no_style() {
        let line = into_line("just words");
        assert_eq!(spans(&line), vec![("just words".into(), Style::default())]);
    }

    #[test]
    fn a_colour_starts_a_span_and_a_reset_ends_it() {
        let line = into_line("before \u{1b}[31mred\u{1b}[0m after");
        let got = spans(&line);
        assert_eq!(got[0].0, "before ");
        assert_eq!(got[0].1, Style::default());
        assert_eq!(got[1].0, "red");
        assert_eq!(got[1].1.fg, Some(Color::Indexed(1)));
        assert_eq!(got[2].0, " after");
        assert_eq!(got[2].1, Style::default());
    }

    #[test]
    fn the_two_extended_colour_spellings_both_arrive() {
        assert_eq!(
            apply(Style::default(), "38;5;215").fg,
            Some(Color::Indexed(215))
        );
        assert_eq!(
            apply(Style::default(), "38;2;255;175;95").fg,
            Some(Color::Rgb(255, 175, 95))
        );
        assert_eq!(
            apply(Style::default(), "48;5;238").bg,
            Some(Color::Indexed(238))
        );
    }

    #[test]
    fn a_palette_index_and_not_a_named_colour() {
        // A swatch drawn from `colour1` and a preview drawn from `ESC[31m`
        // have to be the same red, and `Color::Red` is the terminal's own idea
        // of it rather than palette entry one.
        assert_eq!(
            apply(Style::default(), "31").fg,
            Some(Color::Indexed(1)),
            "31 is palette 1"
        );
        assert_eq!(
            apply(Style::default(), "91").fg,
            Some(Color::Indexed(9)),
            "the bright eight are 8 to 15"
        );
    }

    #[test]
    fn several_parameters_in_one_sequence_all_apply() {
        let style = apply(Style::default(), "1;38;5;42;48;5;7");
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(style.fg, Some(Color::Indexed(42)));
        assert_eq!(style.bg, Some(Color::Indexed(7)));
    }

    #[test]
    fn a_bare_reset_is_a_reset() {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        assert_eq!(apply(bold, ""), Style::default());
        assert_eq!(apply(bold, "0"), Style::default());
    }

    #[test]
    fn style_carries_along_a_line_and_stops_at_its_end() {
        let lines = into_lines("\u{1b}[31mred\nstill plain");
        assert_eq!(spans(&lines[0])[0].1.fg, Some(Color::Indexed(1)));
        assert_eq!(spans(&lines[1])[0].1, Style::default());
    }

    #[test]
    fn a_sequence_that_is_not_a_colour_is_dropped_and_its_text_kept() {
        // A cursor move in a preview is a preview trying to drive the
        // terminal. The text around it survives; the sequence does not.
        let line = into_line("a\u{1b}[2Jb\u{1b}[Hc");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "abc");
    }

    #[test]
    fn a_line_cut_off_mid_sequence_does_not_panic() {
        assert_eq!(into_line("text\u{1b}").spans.len(), 1);
        assert_eq!(into_line("text\u{1b}[").spans.len(), 1);
        assert_eq!(into_line("text\u{1b}[38;2;1").spans.len(), 1);
    }

    #[test]
    fn carriage_returns_do_not_become_a_glyph() {
        // Previews built by a shell command arrive with CRLF often enough.
        let lines = into_lines("one\r\ntwo");
        let first: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(first, "one");
    }
}
