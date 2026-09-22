//! Probes: ask the terminal what it does, rather than assuming.
//!
//! Both of these exist because a config cannot assert a property of a parser.
//! The bytes a key sends and the number of cells a string advances are
//! measurements, and they differ between a bare terminal and the same terminal
//! inside tmux.

/// Render a byte the way a person reading a key sequence wants to see it.
///
/// Control characters as caret notation, escape as `ESC`, anything printable as
/// itself, and everything else in hex. A raw dump of `^[[1;3D` tells somebody
/// nothing they can put in a config file.
pub fn describe_byte(b: u8) -> String {
    match b {
        0x1b => "ESC".to_string(),
        0x7f => "DEL".to_string(),
        0x20 => "SPC".to_string(),
        0..=0x1f => format!("^{}", (b + 0x40) as char),
        0x21..=0x7e => (b as char).to_string(),
        _ => format!("\\x{b:02x}"),
    }
}

/// Describe a whole key sequence.
pub fn describe_sequence(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| describe_byte(*b))
        .collect::<Vec<String>>()
        .join(" ")
}

/// The same sequence as hex, for pasting into a bug report.
pub fn hex_sequence(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<String>>()
        .join(" ")
}

/// Parse a Device Status Report reply, `ESC [ row ; col R`.
///
/// The column is what the probe is after: minus one, it is the number of cells
/// the terminal advanced for whatever was printed.
pub fn parse_dsr(reply: &[u8]) -> Option<(u32, u32)> {
    let text = String::from_utf8_lossy(reply);
    let start = text.find("\u{1b}[")? + 2;
    let end = start + text[start..].find('R')?;
    let (row, col) = text[start..end].split_once(';')?;
    Some((row.trim().parse().ok()?, col.trim().parse().ok()?))
}

/// The cells a terminal advanced, from a DSR reply and the column it started at.
pub fn cells_advanced(reply: &[u8], start_column: u32) -> Option<u32> {
    let (_, col) = parse_dsr(reply)?;
    Some(col.saturating_sub(start_column))
}

/// The strings the cell-width probe measures when given none.
///
/// A Devanagari word, an IPA string and a mixed sentence, because those are
/// where the three layers disagree: grapheme-cluster rules give `रांगोळी` three
/// cells and libc `wcwidth` gives six, since a spacing vowel sign counts one
/// each.
pub const DEFAULT_PROBES: [&str; 3] = ["रांगोळी", "ʃiːkʰ", "hello रांगोळी world"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_byte_reads_as_caret_notation() {
        assert_eq!(describe_byte(0x01), "^A");
        assert_eq!(describe_byte(0x04), "^D");
        assert_eq!(describe_byte(0x00), "^@");
    }

    #[test]
    fn the_special_ones_have_names() {
        assert_eq!(describe_byte(0x1b), "ESC");
        assert_eq!(describe_byte(0x7f), "DEL");
        assert_eq!(describe_byte(0x20), "SPC");
    }

    #[test]
    fn a_printable_byte_is_itself() {
        assert_eq!(describe_byte(b'a'), "a");
        assert_eq!(describe_byte(b'['), "[");
    }

    #[test]
    fn a_high_byte_is_hex_rather_than_mojibake() {
        assert_eq!(describe_byte(0xe2), "\\xe2");
    }

    #[test]
    fn alt_bracket_is_the_sequence_that_started_all_this() {
        // With macos-option-as-alt, Alt+[ sends ESC then `[`, which is exactly
        // the CSI introducer. Whether tmux reads that as a keypress or as the
        // start of a sequence it should swallow is a property of the parser,
        // which is why it gets measured.
        assert_eq!(describe_sequence(&[0x1b, 0x5b]), "ESC [");
        assert_eq!(describe_sequence(&[0x1b, 0x5d]), "ESC ]");
        assert_eq!(hex_sequence(&[0x1b, 0x5b]), "1b 5b");
    }

    #[test]
    fn a_dsr_reply_gives_a_row_and_a_column() {
        assert_eq!(parse_dsr(b"\x1b[12;34R"), Some((12, 34)));
        assert_eq!(parse_dsr(b"junk\x1b[1;7Rmore"), Some((1, 7)));
    }

    #[test]
    fn something_that_is_not_a_reply_parses_to_nothing() {
        assert_eq!(parse_dsr(b""), None);
        assert_eq!(parse_dsr(b"\x1b[12;34"), None, "no terminator");
        assert_eq!(parse_dsr(b"\x1b[abcR"), None);
    }

    #[test]
    fn the_cells_advanced_are_the_columns_moved() {
        // Printed at column 1, the cursor ends at column 4: three cells.
        assert_eq!(cells_advanced(b"\x1b[1;4R", 1), Some(3));
    }

    #[test]
    fn a_cursor_that_did_not_move_advanced_nothing() {
        assert_eq!(cells_advanced(b"\x1b[1;1R", 1), Some(0));
        // And a reply from before the start does not wrap around.
        assert_eq!(cells_advanced(b"\x1b[1;1R", 5), Some(0));
    }
}
