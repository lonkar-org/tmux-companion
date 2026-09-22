//! Theme arithmetic: the readable text colour for a theme, a visible border
//! colour, and the lighter and darker siblings of a cube colour.
//!
//! This is `scripts/tmux-theme-gen.py` in Rust, and it exists because tmux
//! cannot do arithmetic on a colour. Two jobs came out of that. Themes painted
//! the session name with black on their own main colour, so indigo and violet
//! rendered as dark text on a dark block; and ten projects were sharing nine
//! distinct colours, with azure landing on two of them.
//!
//! Everything here except `terminal_background` and the file walking is pure,
//! which is what makes the WCAG arithmetic testable against known values rather
//! than against a screenshot.

use std::path::{Path, PathBuf};

/// The six levels each channel takes in the xterm 6x6x6 colour cube.
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The 16 ANSI colours, needed because a few themes use the low end.
const ANSI16: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (128, 0, 0),
    (0, 128, 0),
    (128, 128, 0),
    (0, 0, 128),
    (128, 0, 128),
    (0, 128, 128),
    (192, 192, 192),
    (128, 128, 128),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (0, 0, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

/// Black, as a 256-colour index.
///
/// Not the word `black`: tmux resolves that to palette entry 0, which a
/// terminal theme is free to redefine. Ghostty's Birds of Paradise paints
/// entry 0 as a mid brown and entry 7 as a cream, so computing against
/// `#000000` and then writing `black` answered a question the terminal was
/// never asked, and 48 of 76 themes came out below 4.5:1 because of it.
pub const TEXT_DARK: &str = "colour16";

/// White, as a 256-colour index, for the same reason as [`TEXT_DARK`].
pub const TEXT_LIGHT: &str = "colour231";

/// WCAG 2.1 SC 1.4.11 asks 3.0:1 of a border, which carries meaning by colour
/// alone.
///
/// 17 dark themes were below it: blue-dark's colour17 measured 1.13 against
/// the terminal background, which is a border you cannot see.
pub const BORDER_MIN: f64 = 3.0;

/// The RGB of a 256-colour palette index.
pub fn rgb(index: u8) -> (u8, u8, u8) {
    let i = index as usize;
    if i < 16 {
        return ANSI16[i];
    }
    if i < 232 {
        let c = i - 16;
        return (
            CUBE_LEVELS[c / 36],
            CUBE_LEVELS[(c / 6) % 6],
            CUBE_LEVELS[c % 6],
        );
    }
    let grey = 8 + 10 * (i - 232) as u8;
    (grey, grey, grey)
}

/// The r, g, b levels in 0..=5 of a cube colour, or `None` when the index is
/// an ANSI colour or on the greyscale ramp and so has no siblings.
pub fn cube_parts(index: u8) -> Option<[usize; 3]> {
    if !(16..232).contains(&index) {
        return None;
    }
    let c = index as usize - 16;
    Some([c / 36, (c / 6) % 6, c % 6])
}

/// The index of a cube colour from its levels.
pub fn cube_index(parts: [usize; 3]) -> u8 {
    (16 + 36 * parts[0] + 6 * parts[1] + parts[2]) as u8
}

/// Relative luminance, per WCAG 2.1.
pub fn luminance(colour: (u8, u8, u8)) -> f64 {
    fn channel(v: u8) -> f64 {
        let v = v as f64 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(colour.0) + 0.7152 * channel(colour.1) + 0.0722 * channel(colour.2)
}

/// The contrast ratio between two colours, from 1.0 to 21.0.
pub fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// [`TEXT_DARK`] or [`TEXT_LIGHT`], whichever reads better on this colour, and
/// the ratio it achieves.
pub fn readable_on(index: u8) -> (&'static str, f64) {
    let bg = rgb(index);
    let on_dark = contrast(bg, rgb(16));
    let on_light = contrast(bg, rgb(231));
    if on_dark >= on_light {
        (TEXT_DARK, on_dark)
    } else {
        (TEXT_LIGHT, on_light)
    }
}

/// A colour with this one's hue that clears [`BORDER_MIN`] on `background`.
///
/// Walks the cube one step lighter at a time, the same shift the shades use, so
/// the border stays recognisably the theme's colour rather than becoming a grey
/// that clears the threshold and says nothing. A greyscale or ANSI colour has
/// no cube siblings, so it walks the greyscale ramp instead.
pub fn border_for(index: u8, background: (u8, u8, u8)) -> (u8, f64) {
    if contrast(rgb(index), background) >= BORDER_MIN {
        return (index, contrast(rgb(index), background));
    }
    if let Some(parts) = cube_parts(index) {
        let mut current = parts;
        for _ in 0..5 {
            let shifted = [
                current[0].min(4) + 1,
                current[1].min(4) + 1,
                current[2].min(4) + 1,
            ];
            let shifted = [shifted[0].min(5), shifted[1].min(5), shifted[2].min(5)];
            if shifted == current {
                break;
            }
            current = shifted;
            let i = cube_index(current);
            if contrast(rgb(i), background) >= BORDER_MIN {
                return (i, contrast(rgb(i), background));
            }
        }
        let i = cube_index(current);
        return (i, contrast(rgb(i), background));
    }
    for i in index.max(232)..=255 {
        if contrast(rgb(i), background) >= BORDER_MIN {
            return (i, contrast(rgb(i), background));
        }
    }
    (255, contrast(rgb(255), background))
}

/// The lighter and darker siblings of a cube colour.
///
/// One step along every axis, clamped: a channel already at an end stays there
/// rather than wrapping the hue somewhere else.
pub fn shades(index: u8) -> Vec<(&'static str, u8)> {
    let Some(parts) = cube_parts(index) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (label, step) in [("light", 1i32), ("dark", -1)] {
        let shifted = [
            (parts[0] as i32 + step).clamp(0, 5) as usize,
            (parts[1] as i32 + step).clamp(0, 5) as usize,
            (parts[2] as i32 + step).clamp(0, 5) as usize,
        ];
        if shifted == parts {
            continue;
        }
        out.push((label, cube_index(shifted)));
    }
    out
}

// ── Theme files ──────────────────────────────────────────────────────────────

/// One theme file, as much of it as this command cares about.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// The file it came from.
    pub path: PathBuf,
    /// Its `@theme-name`.
    pub name: String,
    /// Its `@theme-color-main-1`, as a palette index.
    pub index: u8,
    /// The file's contents.
    pub text: String,
}

/// Read the `@theme-name` and `@theme-color-main-1` out of a theme file.
///
/// Returns `None` for a file that has neither, which is how the `_`-prefixed
/// helpers in the themes directory are skipped without naming them.
pub fn parse_theme(path: &Path, text: &str) -> Option<Theme> {
    let mut name = None;
    let mut index = None;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("set") {
            continue;
        }
        match words.next() {
            Some("@theme-name") => {
                let rest: Vec<&str> = words.collect();
                name = Some(rest.join(" ").trim_matches('"').to_string());
            }
            Some("@theme-color-main-1") => {
                index = words
                    .next()
                    .and_then(|v| v.strip_prefix("colour"))
                    .and_then(|n| n.parse::<u8>().ok());
            }
            _ => {}
        }
    }
    Some(Theme {
        path: path.to_path_buf(),
        name: name?,
        index: index?,
        text: text.to_string(),
    })
}

/// The file with `@theme-color-on-main` and `@theme-color-border` set, or
/// `None` when it already says exactly that.
///
/// Idempotent on purpose: rerunning has to change nothing that is already
/// right, or nobody can tell a rerun from a change.
pub fn with_computed_colours(theme: &Theme, background: (u8, u8, u8)) -> Option<String> {
    let (fg, _) = readable_on(theme.index);
    let (border, _) = border_for(theme.index, background);
    let lines =
        format!("set @theme-color-on-main  {fg}\nset @theme-color-border   colour{border}\n");
    if theme.text.contains(&lines) {
        return None;
    }

    let mut out = String::with_capacity(theme.text.len() + lines.len());
    let mut inserted = false;
    for line in theme.text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("set @theme-color-on-main")
            || trimmed.starts_with("set @theme-color-border")
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
        if !inserted && trimmed.starts_with("set @theme-color-main-1") {
            out.push_str(&lines);
            inserted = true;
        }
    }
    Some(out)
}

/// The contents of a generated shade file.
pub fn shade_file(theme: &Theme, label: &str, index: u8, background: (u8, u8, u8)) -> String {
    let (fg, _) = readable_on(index);
    let (border, _) = border_for(index, background);
    let stem = theme
        .path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = {
        let mut c = label.chars();
        match c.next() {
            Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    };
    format!(
        "# {} {label}\n\
         # Generated by `tmux-companion theme gen --shades` from {stem}.\n\n\
         source-file \"~/.config/tmux/themes/_reset.tmux\"\n\n\
         set @theme-name         \"{} {title}\"\n\
         set @theme-color-main-1 colour{index}\n\
         set @theme-color-on-main  {fg}\n\
         set @theme-color-border   colour{border}\n\n\
         source-file \"~/.config/tmux/themes/_apply.tmux\"\n",
        theme.name, theme.name
    )
}

/// The terminal's background colour, and where that was read from.
///
/// tmux resolves colours against whatever the terminal theme says, so this is
/// read rather than assumed. Ghostty is asked when it is on `PATH`; otherwise
/// the xterm default for colour232 stands in, and the caller is told which.
pub fn terminal_background() -> ((u8, u8, u8), String) {
    let fallback = (rgb(232), "xterm defaults (ghostty not found)".to_string());
    let Ok(out) = std::process::Command::new("ghostty")
        .arg("+show-config")
        .output()
    else {
        return fallback;
    };
    if !out.status.success() {
        return fallback;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("background = ")
            && let Some(c) = parse_hex(value.trim())
        {
            return (c, "ghostty +show-config".to_string());
        }
    }
    fallback
}

/// `#rrggbb` or `rrggbb` to a colour.
pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&s[0..2], 16).ok()?,
        u8::from_str_radix(&s[2..4], 16).ok()?,
        u8::from_str_radix(&s[4..6], 16).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cube_endpoints_are_black_and_white() {
        assert_eq!(rgb(16), (0, 0, 0));
        assert_eq!(rgb(231), (255, 255, 255));
    }

    #[test]
    fn indigo_is_where_the_python_said_it_was() {
        // colour60, one of the two themes that started all this.
        assert_eq!(rgb(60), (95, 95, 135));
    }

    #[test]
    fn the_greyscale_ramp_is_evenly_spaced() {
        assert_eq!(rgb(232), (8, 8, 8));
        assert_eq!(rgb(255), (238, 238, 238));
    }

    #[test]
    fn ansi_colours_come_from_the_table_not_the_cube() {
        assert_eq!(rgb(0), (0, 0, 0));
        assert_eq!(rgb(9), (255, 0, 0));
        assert_eq!(rgb(15), (255, 255, 255));
    }

    #[test]
    fn cube_parts_and_index_round_trip() {
        for i in 16..232u8 {
            let parts = cube_parts(i).expect("cube colour");
            assert_eq!(cube_index(parts), i);
        }
        assert_eq!(cube_parts(15), None);
        assert_eq!(cube_parts(232), None);
    }

    #[test]
    fn black_on_white_is_the_maximum_contrast() {
        let ratio = contrast((0, 0, 0), (255, 255, 255));
        assert!((ratio - 21.0).abs() < 0.01, "{ratio}");
    }

    #[test]
    fn a_colour_has_no_contrast_with_itself() {
        assert!((contrast((95, 95, 135), (95, 95, 135)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn indigo_needs_light_text() {
        // The bug that started this: indigo was painted with black on its own
        // dark block, at 3.47:1 against 6.05:1 the other way round.
        let (fg, ratio) = readable_on(60);
        assert_eq!(fg, TEXT_LIGHT);
        assert!((ratio - 6.05).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn violet_is_the_close_one_and_keeps_dark_text() {
        // colour98 is (135, 95, 215) and measures 4.65:1 on black against
        // 4.52:1 on white, so the arithmetic keeps black. Worth pinning
        // because it is close enough that a change in the luminance code
        // would flip it without failing anything else.
        let (fg, ratio) = readable_on(98);
        assert_eq!(fg, TEXT_DARK);
        assert!((ratio - 4.65).abs() < 0.05, "{ratio}");
        let on_light = contrast(rgb(98), rgb(231));
        assert!(ratio > on_light, "{ratio} vs {on_light}");
    }

    #[test]
    fn a_pale_colour_needs_dark_text() {
        assert_eq!(readable_on(231).0, TEXT_DARK);
        assert_eq!(readable_on(230).0, TEXT_DARK);
    }

    #[test]
    fn a_border_already_visible_is_left_alone() {
        let bg = (42, 31, 29);
        let (index, ratio) = border_for(231, bg);
        assert_eq!(index, 231);
        assert!(ratio >= BORDER_MIN);
    }

    #[test]
    fn an_invisible_border_is_lifted_until_it_clears_the_threshold() {
        // colour17 against a dark terminal measured 1.13, which is the case
        // the threshold exists for.
        let bg = (42, 31, 29);
        assert!(contrast(rgb(17), bg) < BORDER_MIN);
        let (index, ratio) = border_for(17, bg);
        assert_ne!(index, 17);
        assert!(ratio >= BORDER_MIN, "lifted to {index} at {ratio}");
    }

    #[test]
    fn a_lifted_border_keeps_the_hue_rather_than_going_grey() {
        let bg = (42, 31, 29);
        let (index, _) = border_for(17, bg);
        let parts = cube_parts(index).expect("still a cube colour");
        let original = cube_parts(17).expect("cube colour");
        // Blue was the dominant channel and still is.
        assert!(parts[2] >= parts[0], "{parts:?} from {original:?}");
    }

    #[test]
    fn shades_step_one_level_along_every_axis() {
        let out = shades(60);
        let light = out.iter().find(|(l, _)| *l == "light").expect("light");
        let dark = out.iter().find(|(l, _)| *l == "dark").expect("dark");
        assert_eq!(cube_parts(light.1), Some([2, 2, 3]));
        assert_eq!(cube_parts(dark.1), Some([0, 0, 1]));
    }

    #[test]
    fn a_colour_at_the_edge_of_the_cube_loses_that_sibling() {
        // colour16 is the black corner: there is no darker sibling to mint.
        let labels: Vec<&str> = shades(16).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, vec!["light"]);
    }

    #[test]
    fn a_greyscale_colour_has_no_shades() {
        assert!(shades(240).is_empty());
        assert!(shades(7).is_empty());
    }

    #[test]
    fn parsing_a_theme_file_finds_its_name_and_colour() {
        let text = "source-file \"~/.config/tmux/themes/_reset.tmux\"\n\
                    set @theme-name         \"Indigo\"\n\
                    set @theme-color-main-1 colour60\n";
        let t = parse_theme(Path::new("indigo.tmux"), text).expect("parses");
        assert_eq!(t.name, "Indigo");
        assert_eq!(t.index, 60);
    }

    #[test]
    fn a_file_without_the_two_settings_is_not_a_theme() {
        assert_eq!(
            parse_theme(Path::new("_reset.tmux"), "set -g status on\n"),
            None
        );
    }

    #[test]
    fn computed_colours_are_inserted_after_the_main_colour() {
        let text = "set @theme-name         \"Indigo\"\nset @theme-color-main-1 colour60\n";
        let t = parse_theme(Path::new("indigo.tmux"), text).unwrap();
        let out = with_computed_colours(&t, (42, 31, 29)).expect("changes something");
        let lines: Vec<&str> = out.lines().collect();
        let main = lines
            .iter()
            .position(|l| l.starts_with("set @theme-color-main-1"))
            .unwrap();
        assert!(lines[main + 1].starts_with("set @theme-color-on-main"));
        assert!(lines[main + 2].starts_with("set @theme-color-border"));
    }

    #[test]
    fn rewriting_a_file_that_is_already_right_changes_nothing() {
        let text = "set @theme-name         \"Indigo\"\nset @theme-color-main-1 colour60\n";
        let t = parse_theme(Path::new("indigo.tmux"), text).unwrap();
        let once = with_computed_colours(&t, (42, 31, 29)).unwrap();

        let t2 = parse_theme(Path::new("indigo.tmux"), &once).unwrap();
        assert_eq!(
            with_computed_colours(&t2, (42, 31, 29)),
            None,
            "a second run must be a no-op"
        );
    }

    #[test]
    fn an_old_wrong_value_is_replaced_rather_than_duplicated() {
        let text = "set @theme-name         \"Indigo\"\n\
                    set @theme-color-main-1 colour60\n\
                    set @theme-color-on-main  colour16\n";
        let t = parse_theme(Path::new("indigo.tmux"), text).unwrap();
        let out = with_computed_colours(&t, (42, 31, 29)).expect("changes");
        assert_eq!(out.matches("@theme-color-on-main").count(), 1, "{out}");
        assert!(out.contains(TEXT_LIGHT), "{out}");
    }

    #[test]
    fn a_shade_file_names_its_parent_and_carries_its_own_colours() {
        let text = "set @theme-name         \"Indigo\"\nset @theme-color-main-1 colour60\n";
        let t = parse_theme(Path::new("indigo.tmux"), text).unwrap();
        let body = shade_file(&t, "light", 97, (42, 31, 29));
        assert!(body.contains("\"Indigo Light\""), "{body}");
        assert!(body.contains("set @theme-color-main-1 colour97"), "{body}");
        assert!(body.contains("from indigo.tmux"), "{body}");
        assert!(body.contains("@theme-color-on-main"), "{body}");
    }

    #[test]
    fn hex_parsing_takes_both_spellings_and_rejects_the_rest() {
        assert_eq!(parse_hex("#2a1f1d"), Some((42, 31, 29)));
        assert_eq!(parse_hex("2a1f1d"), Some((42, 31, 29)));
        assert_eq!(parse_hex("#abc"), None);
        assert_eq!(parse_hex("not a colour"), None);
    }
}
