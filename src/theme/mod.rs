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

pub mod cli;

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
///
/// This no longer generates anything -- `theme gen --shades` sweeps the whole
/// cube now, see [`cube_themes`]. What it still defines is the property the
/// six bundled colours were chosen for: each one and both of its siblings
/// clear [`TEXT_MIN_AA`] with room, which
/// `every_bundled_theme_and_its_shades_clear_wcag_aa` holds them to.
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
/// A generated cube theme, as a file.
///
/// Named from its own stem and nothing else. `shade_file` builds its name as
/// `"<parent> <label>"`, which was right when every generated theme really was a
/// sibling of one on disk; used for a whole-cube sweep it named all 145 after
/// whichever theme happened to sort first, so a picker full of colours all
/// called Ember. It also put the generated set back at the mercy of what was
/// already in the directory, which is the thing the sweep exists to avoid.
pub fn cube_theme_file(stem: &str, index: u8, background: (u8, u8, u8), dir: &str) -> String {
    let (fg, _) = readable_on(index);
    let (border, _) = border_for(index, background);
    let title = cube_theme_title(stem);
    format!(
        "# {title}\n\
         # Generated by `tmux-companion theme gen --shades`: cube colour{index},\n\
         # whose text clears WCAG AAA. Yours to edit or delete.\n\n\
         source-file \"{dir}/_reset.tmux\"\n\n\
         set @theme-name         \"{title}\"\n\
         set @theme-color-main-1 colour{index}\n\
         set @theme-color-on-main  {fg}\n\
         set @theme-color-border   colour{border}\n\n\
         source-file \"{dir}/_apply.tmux\"\n"
    )
}

/// `ember-04` reads as `Ember 04` in the picker.
pub fn cube_theme_title(stem: &str) -> String {
    let (base, number) = stem.rsplit_once('-').unwrap_or((stem, ""));
    let mut chars = base.chars();
    let capitalised = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    if number.is_empty() {
        return capitalised;
    }
    // `ember-04` stays a number; `ember-light` is a word and reads as one.
    let suffix = if number.chars().all(|c| c.is_ascii_digit()) {
        number.to_string()
    } else {
        let mut c = number.chars();
        match c.next() {
            Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    };
    format!("{capitalised} {suffix}")
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

// ── Choosing and applying ────────────────────────────────────────────────────

/// The eight base colour names tmux accepts, and their bright forms.
const BASE_NAMES: [(&str, (u8, u8, u8)); 16] = [
    ("black", (0x00, 0x00, 0x00)),
    ("red", (0x80, 0x00, 0x00)),
    ("green", (0x00, 0x80, 0x00)),
    ("yellow", (0x80, 0x80, 0x00)),
    ("blue", (0x00, 0x00, 0x80)),
    ("magenta", (0x80, 0x00, 0x80)),
    ("cyan", (0x00, 0x80, 0x80)),
    ("white", (0xc0, 0xc0, 0xc0)),
    ("brblack", (0x80, 0x80, 0x80)),
    ("brred", (0xff, 0x00, 0x00)),
    ("brgreen", (0x00, 0xff, 0x00)),
    ("bryellow", (0xff, 0xff, 0x00)),
    ("brblue", (0x00, 0x00, 0xff)),
    ("brmagenta", (0xff, 0x00, 0xff)),
    ("brcyan", (0x00, 0xff, 0xff)),
    ("brwhite", (0xff, 0xff, 0xff)),
];

/// Resolve any colour a theme file can carry.
///
/// A palette index (`colour125` or `color125`), one of the base names, or a
/// hex triplet. `default` and `terminal` resolve to nothing, because they mean
/// "whatever the terminal says" and this cannot know that.
pub fn resolve_colour(value: &str) -> Option<(u8, u8, u8)> {
    let v = value.trim().to_lowercase();
    if v.is_empty() || v == "default" || v == "terminal" {
        return None;
    }
    if v.starts_with('#') {
        return parse_hex(&v);
    }
    if let Some((_, rgb)) = BASE_NAMES.iter().find(|(name, _)| *name == v) {
        return Some(*rgb);
    }
    let digits = v
        .strip_prefix("colour")
        .or_else(|| v.strip_prefix("color"))?;
    let index: u16 = digits.parse().ok()?;
    if index > 255 {
        return None;
    }
    Some(rgb(index as u8))
}

/// An ANSI true-colour block, for a swatch in a list.
pub fn swatch(value: &str) -> String {
    match resolve_colour(value) {
        Some((r, g, b)) => format!("\x1b[38;2;{r};{g};{b}m██\x1b[0m"),
        None => "  ".to_string(),
    }
}

/// Foreground and background, as one escape sequence.
///
/// Both optional: an empty colour, `default` or `terminal` leaves that half
/// alone, which is what a theme means when it does not set one.
pub fn sgr(fg: &str, bg: &str) -> String {
    let mut out = "\x1b[0m".to_string();
    for (lead, value) in [(38, fg), (48, bg)] {
        if let Some((r, g, b)) = resolve_colour(value) {
            out.push_str(&format!("\x1b[{lead};2;{r};{g};{b}m"));
        }
    }
    out
}

/// A colour and its hex, the way the preview names one.
///
/// `colour215 (#ffaf5f)` rather than either alone: the index is what the theme
/// file says and the hex is what it looks like, and a person reading a preview
/// wants to match both against something else.
pub fn label_of(value: &str) -> String {
    match resolve_colour(value) {
        Some((r, g, b)) => format!("{value} (#{r:02x}{g:02x}{b:02x})"),
        None => value.to_string(),
    }
}

/// The five steps of the main colour, darkest to lightest, as hex.
///
/// Multipliers rather than a colour-space walk: this is a swatch strip showing
/// that a theme has somewhere to go in both directions, not a palette anybody
/// computes from.
pub fn shade_ramp(main: &str) -> Vec<String> {
    let Some((r, g, b)) = resolve_colour(main) else {
        return Vec::new();
    };
    [40u32, 70, 100, 130, 170]
        .iter()
        .map(|factor| {
            let step = |c: u8| ((u32::from(c) * factor / 100).min(255)) as u8;
            format!("#{:02x}{:02x}{:02x}", step(r), step(g), step(b))
        })
        .collect()
}

/// What `_apply.tmux` draws an inactive pane border in when a theme names no
/// secondary colour.
///
/// Pinned there with its reasoning, and repeated here so the card shows what
/// tmux will show: colour238 measures 1.64 to 1 against the terminal
/// background, and this is the first step up the greyscale ramp that clears
/// 3.0 without the inactive border starting to compete with the active one.
pub const INACTIVE_BORDER: &str = "colour242";

/// How the text reads on the background, as WCAG measures it.
///
/// `None` when either colour cannot be resolved, because a made-up number here
/// is worse than none: this is the one figure somebody would act on.
pub fn contrast_of(text: &str, background: &str) -> Option<f64> {
    Some(contrast(resolve_colour(text)?, resolve_colour(background)?))
}

/// The ratio, and whether it clears the bar, as a few words for a card.
///
/// 4.5 is WCAG AA for body text and 3.0 is the large-text floor. Below that
/// the sample underneath is the evidence and this is the reason.
pub fn contrast_note(text: &str, background: &str) -> String {
    match contrast_of(text, background) {
        Some(r) if r >= 4.5 => format!("   {r:.1}:1"),
        Some(r) if r >= 3.0 => format!("   {r:.1}:1 (large text only)"),
        Some(r) => format!("   {r:.1}:1 \u{2717} unreadable"),
        None => String::new(),
    }
}

/// What a theme looks like, as a card.
///
/// This is `preview-tmux-theme.zsh` without the shell: the three colours it is
/// built from, a ramp showing where the main one can go, and then the four
/// places tmux actually paints -- the status line, a message, a copy-mode
/// selection and the pane borders. Reading a list of `@theme-color-*` values
/// tells you nothing about whether the text on that background can be read,
/// and that is the only question anybody opens this to answer.
///
/// `width` is how wide the sample bars are drawn. They are meant to reach the
/// edge of whatever pane shows the card, and the card is built before that
/// pane exists, so a caller passes something wider than any pane and the pane
/// clips it. Passing a guess instead is what put a stray block of colour on
/// the line under each bar.
///
/// Pure: settings in, ANSI out, so the whole card is tested without a terminal.
pub fn preview_card(settings: &std::collections::HashMap<String, String>, width: usize) -> String {
    let get = |k: &str| settings.get(k).cloned().unwrap_or_default();

    let name = {
        let n = get("@theme-name");
        if n.is_empty() { "theme".to_string() } else { n }
    };
    let main = {
        let m = get("@theme-color-main-1");
        if m.is_empty() {
            let s = get("@theme-session-name-bg");
            if s.is_empty() {
                "colour245".to_string()
            } else {
                s
            }
        } else {
            m
        }
    };
    // What tmux will actually draw the two pane borders in, which is not one
    // colour and a shade of it. `_apply.tmux` sets the active border to
    // `@theme-color-border`, falling back to the main colour, and the inactive
    // one to `@theme-color-secondary`, which it pins to colour242 with the
    // reasoning beside it: colour238 measures 1.64 to 1 against the terminal
    // background, and colour242 is the first step up the greyscale ramp that
    // clears 3.0 without starting to compete with the active border.
    //
    // The card had these the wrong way round and drew the active border in the
    // main colour, which on a dark theme is very nearly the background. Two
    // lines that looked the same were two lines drawn in the wrong colours.
    let active_border = {
        let b = get("@theme-color-border");
        if b.is_empty() { main.clone() } else { b }
    };
    let inactive_border = {
        let s = get("@theme-color-secondary");
        if s.is_empty() {
            INACTIVE_BORDER.to_string()
        } else {
            s
        }
    };

    // The text colour is `@theme-color-on-main`, which `theme gen` chose for
    // contrast against the main colour and which `_apply.tmux` uses. Reading
    // `@theme-color-black` instead -- another name from the shell script that
    // no theme file has -- fell back to literal black, so every sample in this
    // card was black on the theme's own background. On Blue Dark that is black
    // on #00005f, a ratio of 1.1 to 1: the sample exists to show whether the
    // text can be read, and it could not be read.
    //
    // With nothing on disk to go on the readable one is computed rather than
    // guessed, because a card that cannot say is worse than one that works it
    // out.
    let text_colour = {
        let on_main = get("@theme-color-on-main");
        if !on_main.is_empty() {
            on_main
        } else {
            let black = get("@theme-color-black");
            if !black.is_empty() {
                black
            } else {
                match resolve_colour(&main) {
                    Some(rgb) => readable_on_rgb(rgb).0.to_string(),
                    None => "colour231".to_string(),
                }
            }
        }
    };

    // The bar the status line is drawn on, which is not part of the theme:
    // it is what tmux.conf sets, and the preview needs something behind the
    // sample for the sample to read as a bar at all.
    let status_bg = "colour233";
    let reset = "\x1b[0m";
    let room = width.max(8) - 4;

    let bar = |fg: &str, bg: &str, text: &str| {
        format!(
            "  {}{:<room$}{reset}",
            sgr(fg, bg),
            format!(" {text}"),
            room = room
        )
    };

    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!("  {}▉▉▉{reset}  {name}\n", sgr(&main, "")));
    out.push_str(&format!("     main       {}\n", label_of(&main)));
    out.push_str(&format!("     border     {}\n", label_of(&active_border)));
    // The ratio, because the one thing anybody wants from a theme preview is
    // whether the text on that background can be read, and a pair of colour
    // names does not answer it. WCAG wants 4.5 to 1 for body text.
    out.push_str(&format!(
        "     text       {}{}\n\n",
        label_of(&text_colour),
        contrast_note(&text_colour, &main)
    ));

    let ramp = shade_ramp(&main);
    if !ramp.is_empty() {
        out.push_str("  shades   ");
        for hex in &ramp {
            out.push_str(&format!("{}     {reset}", sgr("", hex)));
        }
        out.push_str("   dark to light\n\n");
    }

    out.push_str("  status line\n");
    out.push_str(&format!(
        "  {} {name} {}{} {} 1 zsh  2 nvim {reset}\n\n",
        sgr(&text_colour, &main),
        sgr(&main, status_bg),
        sgr("colour240", status_bg),
        sgr("colour250", status_bg),
    ));

    out.push_str("  message-style\n");
    out.push_str(&bar(&text_colour, &main, "Config Reloaded!"));
    out.push('\n');
    out.push_str("  mode-style (copy mode selection)\n");
    out.push_str(&bar(&text_colour, &main, "search: theme"));
    out.push_str("\n\n");

    let rule = "\u{2500}".repeat(20);
    out.push_str("  pane borders\n");
    out.push_str(&format!(
        "  {}{rule}{reset} active\n",
        sgr(&active_border, "")
    ));
    out.push_str(&format!(
        "  {}{rule}{reset} inactive\n\n",
        sgr(&inactive_border, "")
    ));

    out.push_str(&format!(
        "  clock-mode  {}▄▀▄ ▀█▀ ▄▀▄{reset}\n",
        sgr(&main, "")
    ));
    out
}

/// Every `set @theme-… value` in a theme file.
pub fn parse_settings(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words
            .first()
            .is_none_or(|w| *w != "set" && *w != "set-option")
        {
            continue;
        }
        // Flags sit between `set` and the key, so the key is found rather than
        // assumed to be second.
        if let Some(i) = words.iter().position(|w| w.starts_with("@theme-"))
            && let Some(value) = quoted_value(&words[i + 1..])
        {
            out.insert(words[i].to_string(), value);
        }
    }
    out
}

/// The value after a key, which may be quoted and so may be several words.
///
/// `@theme-name "Amber Light"` is two words, and taking the first gave every
/// shade the name of the theme it came from: 42 of 76 themes came back called
/// `Amber` or `Azure` rather than `Amber Light` or `Azure Dark`.
fn quoted_value(rest: &[&str]) -> Option<String> {
    let first = rest.first()?;
    let Some(stripped) = first.strip_prefix('"') else {
        return Some(first.to_string());
    };
    if let Some(done) = stripped.strip_suffix('"') {
        return Some(done.to_string());
    }
    let mut out = stripped.to_string();
    for word in &rest[1..] {
        out.push(' ');
        match word.strip_suffix('"') {
            Some(last) => {
                out.push_str(last);
                return Some(out);
            }
            None => out.push_str(word),
        }
    }
    Some(out)
}

/// One row of the theme picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeRow {
    /// The file to source.
    pub path: PathBuf,
    /// The theme's own name.
    pub name: String,
    /// Its main colour, as written.
    pub colour: String,
}

impl ThemeRow {
    /// The label a picker shows: a swatch, the name, and the colour resolved.
    pub fn label(&self) -> String {
        let hex = resolve_colour(&self.colour)
            .map(|(r, g, b)| format!(" (#{r:02x}{g:02x}{b:02x})"))
            .unwrap_or_default();
        format!(
            "{} {:<22} {}{hex}",
            swatch(&self.colour),
            self.name,
            self.colour
        )
    }

    /// The row as the picker searches it.
    ///
    /// No swatch and no padding: `swatch` returns a string of ANSI escapes,
    /// and a label built around it is padded over bytes the terminal never
    /// draws, which is why the theme list came out ragged. The block is drawn
    /// by the picker now, from the colour rather than from a string.
    pub fn search_text(&self) -> String {
        format!("{} {}", self.name, self.colour)
    }

    /// The row as columns the picker lines up for itself.
    pub fn columns(&self) -> Vec<String> {
        let hex = resolve_colour(&self.colour)
            .map(|(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"))
            .unwrap_or_default();
        vec![self.name.clone(), self.colour.clone(), hex]
    }
}

/// Read the themes directory, skipping the `_`-prefixed helpers.
pub fn rows(dir: &Path) -> Vec<ThemeRow> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<ThemeRow> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "tmux"))
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('_'))
        })
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let settings = parse_settings(&text);
            Some(ThemeRow {
                name: settings.get("@theme-name").cloned().unwrap_or_else(|| {
                    // A theme with no name is named after its file, which is
                    // better than an empty row nobody can pick.
                    path.file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                }),
                colour: settings
                    .get("@theme-color-main-1")
                    .cloned()
                    .unwrap_or_default(),
                path,
            })
        })
        .collect();
    // By file name, not by the name a theme calls itself. The two differ and
    // the file is the one that groups a family together: `amber-light.tmux`
    // sorts before `amber.tmux` because `-` is below `.`, so a theme and its
    // lighter sibling land next to each other instead of an alphabetical list
    // scattering them.
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// The theme a session should get, before any picker is involved.
///
/// The project map first, so a project keeps its colour across restarts. Then
/// the namespace map from the config, then the configured default. `None` when
/// none of them names a file that is there.
///
/// Returning an `Option` rather than a path is the whole point: this runs from
/// the `session-created` hook, once per session, and a path that does not exist
/// became a "No such file or directory" on the terminal every time somebody
/// opened a session. A theme nobody has chosen is a normal state, not an error.
pub fn theme_for_session(
    session: &str,
    map: &std::collections::HashMap<String, String>,
    config: &crate::config::Theme,
    dir: &Path,
) -> Option<PathBuf> {
    let namespace = session.split('/').next().unwrap_or(session);
    let candidates = [
        map.get(session),
        config.namespace.get(namespace),
        Some(&config.default),
    ];
    candidates
        .into_iter()
        .flatten()
        .filter(|name| !name.is_empty())
        .map(|name| dir.join(format!("{name}.tmux")))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod picker_tests {
    use super::*;

    #[test]
    fn a_palette_index_resolves_both_spellings() {
        assert_eq!(resolve_colour("colour60"), Some((95, 95, 135)));
        assert_eq!(resolve_colour("color60"), Some((95, 95, 135)));
        assert_eq!(resolve_colour("COLOUR60"), Some((95, 95, 135)));
    }

    #[test]
    fn a_base_name_resolves_to_the_shade_tmux_means() {
        assert_eq!(resolve_colour("red"), Some((0x80, 0, 0)));
        assert_eq!(resolve_colour("brred"), Some((0xff, 0, 0)));
        assert_eq!(resolve_colour("white"), Some((0xc0, 0xc0, 0xc0)));
    }

    #[test]
    fn a_hex_triplet_resolves() {
        assert_eq!(resolve_colour("#af005f"), Some((0xaf, 0x00, 0x5f)));
    }

    #[test]
    fn default_and_terminal_resolve_to_nothing() {
        // They mean "whatever the terminal says", which this cannot know.
        assert_eq!(resolve_colour("default"), None);
        assert_eq!(resolve_colour("terminal"), None);
        assert_eq!(resolve_colour(""), None);
    }

    #[test]
    fn an_index_past_the_palette_is_not_a_colour() {
        assert_eq!(resolve_colour("colour256"), None);
        assert_eq!(resolve_colour("colour999"), None);
    }

    #[test]
    fn a_swatch_is_a_block_in_the_colour_or_two_blank_columns() {
        assert!(swatch("colour60").contains("38;2;95;95;135"));
        assert_eq!(swatch("default"), "  ", "a blank swatch keeps the column");
    }

    #[test]
    fn settings_are_found_past_any_flags() {
        let text = "set -g @theme-name \"Indigo\"\nset @theme-color-main-1 colour60\n";
        let s = parse_settings(text);
        assert_eq!(s.get("@theme-name").map(String::as_str), Some("Indigo"));
        assert_eq!(
            s.get("@theme-color-main-1").map(String::as_str),
            Some("colour60")
        );
    }

    #[test]
    fn a_quoted_name_survives_its_spaces() {
        // Taking the first word gave every shade the name of the theme it came
        // from: 42 of 76 themes came back as `Amber` rather than `Amber Light`.
        let s = parse_settings("set @theme-name         \"Amber Light\"\n");
        assert_eq!(
            s.get("@theme-name").map(String::as_str),
            Some("Amber Light")
        );
    }

    #[test]
    fn an_unquoted_value_is_taken_whole() {
        let s = parse_settings("set @theme-color-main-1 colour60\n");
        assert_eq!(
            s.get("@theme-color-main-1").map(String::as_str),
            Some("colour60")
        );
    }

    #[test]
    fn an_unclosed_quote_takes_the_rest_of_the_line_rather_than_nothing() {
        let s = parse_settings("set @theme-name \"Amber Light\n");
        assert_eq!(
            s.get("@theme-name").map(String::as_str),
            Some("Amber Light")
        );
    }

    #[test]
    fn a_line_that_is_not_a_setting_is_skipped() {
        let s =
            parse_settings("source-file \"~/.config/tmux/themes/_reset.tmux\"\n# @theme-name\n");
        assert!(s.is_empty(), "{s:?}");
    }

    #[test]
    fn a_row_label_carries_the_swatch_the_name_and_the_hex() {
        let row = ThemeRow {
            path: PathBuf::from("indigo.tmux"),
            name: "Indigo".into(),
            colour: "colour60".into(),
        };
        let label = row.label();
        assert!(label.contains("Indigo"), "{label}");
        assert!(label.contains("colour60"), "{label}");
        assert!(label.contains("#5f5f87"), "{label}");
    }

    #[test]
    fn the_project_map_decides_before_the_namespace_does() {
        // A project keeps its colour across restarts, which is the whole point
        // of the map.
        let dir = std::env::temp_dir().join(format!("tc-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(dir.join("indigo.tmux"), "set @theme-name Indigo\n").expect("write");

        let map = std::collections::HashMap::from([("w/thing".to_string(), "indigo".to_string())]);
        assert_eq!(
            theme_for_session("w/thing", &map, &crate::config::Theme::default(), &dir),
            Some(dir.join("indigo.tmux"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_namespace_decides_when_nothing_claims_the_session() {
        let dir = std::env::temp_dir().join(format!("tc-ns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");
        for stem in ["slate", "plum", "ink"] {
            std::fs::write(dir.join(format!("{stem}.tmux")), "set @theme-name X\n").expect("write");
        }
        let empty = std::collections::HashMap::new();
        let config = crate::config::Theme {
            default: "ink".to_string(),
            namespace: std::collections::HashMap::from([
                ("w".to_string(), "slate".to_string()),
                ("a".to_string(), "plum".to_string()),
            ]),
        };

        assert_eq!(
            theme_for_session("w/x", &empty, &config, &dir),
            Some(dir.join("slate.tmux"))
        );
        assert_eq!(
            theme_for_session("a/x", &empty, &config, &dir),
            Some(dir.join("plum.tmux"))
        );
        // No namespace rule, so the default.
        assert_eq!(
            theme_for_session("other", &empty, &config, &dir),
            Some(dir.join("ink.tmux"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_mapped_theme_that_is_not_there_falls_back_rather_than_failing() {
        let dir = std::env::temp_dir().join(format!("tc-gone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(dir.join("ink.tmux"), "set @theme-name Ink\n").expect("write");

        let map = std::collections::HashMap::from([("x".to_string(), "gone".to_string())]);
        assert_eq!(
            theme_for_session("x", &map, &crate::config::Theme::default(), &dir),
            Some(dir.join("ink.tmux"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_on_disk_means_nothing_to_source_rather_than_an_error() {
        // The session-created hook runs this for every session. Before the
        // themes exist -- which is every session until `theme init` is run --
        // every one of them printed a missing-file error.
        assert_eq!(
            theme_for_session(
                "anything",
                &std::collections::HashMap::new(),
                &crate::config::Theme::default(),
                Path::new("/nowhere"),
            ),
            None
        );
    }

    #[test]
    fn an_empty_default_leaves_a_session_unpainted() {
        let config = crate::config::Theme {
            default: String::new(),
            namespace: std::collections::HashMap::new(),
        };
        assert_eq!(
            theme_for_session(
                "x",
                &std::collections::HashMap::new(),
                &config,
                Path::new("/nowhere"),
            ),
            None
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── the preview card ─────────────────────────────────────────────────────

    fn amber() -> std::collections::HashMap<String, String> {
        // The keys the generator actually writes, which is the point: the card
        // used to read two names that no theme file has ever carried.
        [
            ("@theme-name", "Amber Light"),
            ("@theme-color-main-1", "colour215"),
            ("@theme-color-border", "colour238"),
            ("@theme-color-on-main", "colour16"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// The card with every escape sequence taken back out.
    fn plain(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c != '\u{1b}' {
                out.push(c);
                continue;
            }
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        }
        out
    }

    #[test]
    fn the_card_shows_the_four_places_tmux_paints() {
        // Reading a list of @theme-color values answers none of the question
        // somebody opens a preview to ask, which is whether this is legible.
        let card = plain(&preview_card(&amber(), 60));
        for expected in [
            "Amber Light",
            "main",
            "border",
            "text",
            "shades",
            "dark to light",
            "status line",
            "message-style",
            "Config Reloaded!",
            "mode-style (copy mode selection)",
            "search: theme",
            "pane borders",
            "active",
            "inactive",
            "clock-mode",
        ] {
            assert!(
                card.contains(expected),
                "{expected:?} missing from:\n{card}"
            );
        }
    }

    #[test]
    fn the_card_reads_the_keys_the_generator_writes() {
        // `@theme-color-on-main` is the text colour `theme gen` picked for
        // contrast and `_apply.tmux` uses. The card read `@theme-color-black`,
        // which is a name from the shell script it came from and which no
        // theme file has, so it fell back to literal black: on Blue Dark that
        // is black on #00005f, and the sample meant to show whether the text
        // could be read could not be read.
        let blue: std::collections::HashMap<String, String> = [
            ("@theme-name", "Blue Dark"),
            ("@theme-color-main-1", "colour17"),
            ("@theme-color-on-main", "colour231"),
            ("@theme-color-border", "colour103"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let card = plain(&preview_card(&blue, 60));
        assert!(card.contains("colour231"), "the text colour:\n{card}");
        assert!(card.contains("colour103"), "the border colour:\n{card}");
        assert!(!card.contains("black (#000000)"), "not black:\n{card}");
    }

    #[test]
    fn a_theme_with_no_text_colour_gets_a_readable_one_rather_than_black() {
        // Nothing on disk to go on, so it is computed. Guessing black here is
        // what produced an unreadable card.
        let dark: std::collections::HashMap<String, String> =
            [("@theme-name", "Ink"), ("@theme-color-main-1", "colour17")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
        let card = plain(&preview_card(&dark, 60));
        assert!(!card.contains("black (#000000)"), "{card}");
        assert!(
            contrast_of("colour231", "colour17").expect("both resolve") > 4.5,
            "the computed answer has to clear the bar"
        );
    }

    #[test]
    fn the_card_says_how_readable_the_text_is() {
        // A pair of colour names does not answer the only question anybody
        // opens a theme preview to ask.
        assert!(contrast_note("colour231", "colour17").contains(":1"));
        assert!(!contrast_note("colour231", "colour17").contains("unreadable"));
        // Black on Blue Dark, which is what this used to draw.
        let bad = contrast_note("black", "colour17");
        assert!(bad.contains("unreadable"), "{bad}");
        // Nothing to measure is no claim at all.
        assert_eq!(contrast_note("chartreuse", "colour17"), "");
    }

    #[test]
    fn the_ratio_is_the_one_wcag_defines() {
        // White on black is 21:1, and a colour against itself is 1:1.
        let white_on_black = contrast_of("#ffffff", "#000000").expect("resolves");
        assert!((white_on_black - 21.0).abs() < 0.01, "{white_on_black}");
        let same = contrast_of("colour17", "colour17").expect("resolves");
        assert!((same - 1.0).abs() < 0.001, "{same}");
    }

    #[test]
    fn a_colour_is_named_by_its_index_and_its_hex() {
        // The index is what the file says and the hex is what it looks like,
        // and somebody reading a preview is matching one of the two against
        // something else.
        assert_eq!(label_of("colour215"), "colour215 (#ffaf5f)");
        assert_eq!(label_of("#ffaf5f"), "#ffaf5f (#ffaf5f)");
        // Nothing to resolve, so nothing invented.
        assert_eq!(label_of("default"), "default");
        assert_eq!(label_of(""), "");
    }

    #[test]
    fn the_ramp_runs_dark_to_light_and_stops_at_white() {
        let ramp = shade_ramp("#808080");
        assert_eq!(ramp.len(), 5);
        assert_eq!(ramp[0], "#333333", "40 percent of 0x80");
        assert_eq!(ramp[2], "#808080", "the middle step is the colour itself");
        // 170 percent of 0x80 is 0xd9, which fits; a bright colour clamps.
        assert_eq!(shade_ramp("#ffffff")[4], "#ffffff");
        // A colour nothing can resolve has no ramp rather than a black one.
        assert!(shade_ramp("default").is_empty());
    }

    #[test]
    fn a_theme_that_names_nothing_still_draws_a_card() {
        // Half the fields fall back, and a preview that panicked on a theme
        // file somebody was midway through writing would be worse than one
        // that shows the defaults.
        let card = plain(&preview_card(&std::collections::HashMap::new(), 60));
        assert!(card.contains("theme"), "{card}");
        assert!(card.contains("colour245"), "the fallback main colour");
    }

    #[test]
    fn the_sample_bars_are_as_wide_as_the_card_asks_for() {
        // A message-style sample narrower than the pane reads as text rather
        // than as a bar, which is the thing it exists to show. Callers pass a
        // width wider than any pane and let the pane clip it, because the card
        // is built before the pane exists.
        let card = plain(&preview_card(&amber(), 40));
        let bar = card
            .lines()
            .find(|l| l.contains("Config Reloaded!"))
            .expect("the message sample");
        assert_eq!(bar.chars().count(), 38, "{bar:?}");

        // And at the width the picker actually passes, so nothing else on the
        // card grows with it and starts wrapping too.
        let wide = plain(&preview_card(&amber(), 400));
        for line in wide.lines() {
            let n = line.chars().count();
            assert!(
                n <= 398,
                "a line wider than the bars would wrap before they do: {n} {line:?}"
            );
        }
    }

    #[test]
    fn the_two_pane_borders_are_the_colours_tmux_will_draw() {
        // `_apply.tmux` gives the active border `@theme-color-border` and the
        // inactive one `@theme-color-secondary`, which it pins to colour242.
        // The card had them the wrong way round and drew the active border in
        // the main colour, which on a dark theme is nearly the background, so
        // the two lines looked the same and both were wrong.
        let card = preview_card(&amber(), 60);
        let active = card
            .lines()
            .find(|l| l.contains("active") && !l.contains("inactive"))
            .expect("the active sample");
        let inactive = card
            .lines()
            .find(|l| l.contains("inactive"))
            .expect("the inactive sample");

        assert!(
            active.contains(&sgr("colour238", "")),
            "active is the border colour"
        );
        assert!(
            inactive.contains(&sgr(INACTIVE_BORDER, "")),
            "inactive is the greyscale step _apply.tmux pins"
        );
        // Same glyphs by design; it is the colour that has to differ, and two
        // lines drawn in one colour say nothing.
        assert_ne!(
            sgr("colour238", ""),
            sgr(INACTIVE_BORDER, ""),
            "two lines that look the same say nothing"
        );
    }

    #[test]
    fn a_theme_naming_its_own_secondary_keeps_it() {
        let mut theme = amber();
        theme.insert("@theme-color-secondary".into(), "colour59".into());
        let card = preview_card(&theme, 60);
        assert!(
            card.contains(&sgr("colour59", "")),
            "the theme's own choice"
        );
    }

    #[test]
    fn a_theme_sorts_next_to_its_own_lighter_sibling() {
        // By file name, not by the name the theme calls itself: `-` is below
        // `.`, so amber-light.tmux lands beside amber.tmux instead of an
        // alphabetical list scattering a family.
        let dir = tempfile::tempdir().expect("tempdir");
        for (file, name) in [
            ("amber.tmux", "Amber"),
            ("amber-light.tmux", "Amber Light"),
            ("azure.tmux", "Azure"),
        ] {
            std::fs::write(
                dir.path().join(file),
                format!("set -g @theme-name \"{name}\"\nset -g @theme-color-main-1 colour215\n"),
            )
            .expect("write");
        }
        let names: Vec<String> = rows(dir.path()).into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["Amber Light", "Amber", "Azure"]);
    }

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
    fn the_ladder_gets_shorter_as_it_gets_stricter() {
        // The property that makes it a ladder rather than five settings.
        use ShadeLevel::*;
        let counts: Vec<usize> = [Aa, Aaa, A4, A5]
            .into_iter()
            .map(|l| themes_for(l).len())
            .collect();
        for pair in counts.windows(2) {
            assert!(pair[0] > pair[1], "{counts:?} is not monotonic");
        }
        assert_eq!(
            counts[0],
            216 - BASE_THEMES.len(),
            "aa should keep the cube"
        );
    }

    #[test]
    fn every_level_name_parses_and_every_parsed_name_is_a_level() {
        for name in SHADE_LEVELS {
            assert!(ShadeLevel::from_name(name).is_some(), "{name}");
        }
        assert_eq!(ShadeLevel::from_name("AAA"), Some(ShadeLevel::Aaa));
        assert_eq!(ShadeLevel::from_name("a7"), None);
        assert_eq!(ShadeLevel::from_name(""), None);
    }

    #[test]
    fn the_tightest_rung_is_the_six_and_their_siblings() {
        // a6 is the behaviour --shades had before it swept the cube, so the
        // count is the one the docs have always claimed: six times three.
        let out = themes_for(ShadeLevel::A6);
        assert_eq!(out.len(), 12, "six bases, two siblings each");
        assert_eq!(out.len() + BASE_THEMES.len(), 18);
        for (name, _) in &out {
            assert!(
                name.ends_with("-light") || name.ends_with("-dark"),
                "{name} is not a sibling"
            );
        }
    }

    #[test]
    fn a_word_suffix_reads_as_a_word() {
        assert_eq!(cube_theme_title("ember-light"), "Ember Light");
        assert_eq!(cube_theme_title("ember-04"), "Ember 04");
    }

    #[test]
    fn every_level_produces_themes_that_clear_its_own_floor() {
        use ShadeLevel::*;
        for level in [Aa, Aaa, A4, A5] {
            let min = level.min_contrast().unwrap();
            for (stem, index) in themes_for(level) {
                let ratio = readable_on(index).1;
                assert!(ratio >= min, "{level:?}: {stem} is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn a_generated_theme_is_named_after_itself() {
        // The bug this exists for: every generated file took its name from
        // whichever theme sorted first, so a picker of 151 colours offered
        // Ember 145 times.
        assert_eq!(cube_theme_title("ember-04"), "Ember 04");
        assert_eq!(cube_theme_title("pine-11"), "Pine 11");
        assert_eq!(cube_theme_title("slate"), "Slate");
    }

    #[test]
    fn no_generated_file_borrows_another_theme_s_name() {
        let dir = "/themes";
        for (stem, index) in themes_for(ShadeLevel::Aaa) {
            let text = cube_theme_file(&stem, index, (17, 17, 17), dir);
            let title = cube_theme_title(&stem);
            assert!(
                text.contains(&format!("set @theme-name         \"{title}\"")),
                "{stem} is not named {title}"
            );
            let base = stem.rsplit_once('-').map(|(b, _)| b).unwrap_or(&stem);
            for (other, label, _) in BASE_THEMES {
                if other == base {
                    continue;
                }
                assert!(!text.contains(label), "{stem} carries the name {label}");
            }
        }
    }

    #[test]
    fn every_generated_theme_can_carry_readable_text() {
        // The filter is the whole reason this is not 216 rows.
        for (stem, index) in themes_for(ShadeLevel::Aaa) {
            let (_, ratio) = readable_on(index);
            assert!(
                ratio >= TEXT_MIN_AAA,
                "{stem} is colour{index} at {ratio:.2}:1"
            );
        }
    }

    #[test]
    fn an_aa_filter_over_the_cube_would_keep_all_of_it() {
        // Why TEXT_MIN_AAA exists. If this ever starts failing, an AA filter
        // has become meaningful and the generated set can go back to it.
        let kept = (16u8..232)
            .filter(|i| readable_on(*i).1 >= TEXT_MIN_AA)
            .count();
        assert_eq!(kept, 216, "AA no longer keeps the whole cube");
    }

    #[test]
    fn the_generated_set_is_a_pickable_size() {
        // Enough to be worth having, few enough to scroll. If a change to the
        // cube maths or the threshold moves this a long way, it should be a
        // decision and not a surprise.
        let n = themes_for(ShadeLevel::Aaa).len();
        assert!((120..=170).contains(&n), "{n} themes");
    }

    #[test]
    fn generated_names_are_unique_and_stable() {
        let first = themes_for(ShadeLevel::Aaa);
        let second = themes_for(ShadeLevel::Aaa);
        assert_eq!(first, second, "the same call gave two different answers");
        let names: std::collections::HashSet<&String> = first.iter().map(|(n, _)| n).collect();
        assert_eq!(names.len(), first.len(), "two themes share a name");
        let indexes: std::collections::HashSet<u8> = first.iter().map(|(_, i)| *i).collect();
        assert_eq!(indexes.len(), first.len(), "two themes share a colour");
    }

    #[test]
    fn the_six_bundled_colours_are_not_generated_again() {
        let made: std::collections::HashSet<u8> = themes_for(ShadeLevel::Aaa)
            .into_iter()
            .map(|(_, i)| i)
            .collect();
        for (stem, _, index) in BASE_THEMES {
            assert!(!made.contains(&index), "{stem} was generated a second time");
        }
    }

    #[test]
    fn every_generated_name_belongs_to_a_bundled_theme() {
        let stems: Vec<&str> = BASE_THEMES.iter().map(|(s, _, _)| *s).collect();
        for (name, _) in themes_for(ShadeLevel::Aaa) {
            let base = name.rsplit_once('-').map(|(b, _)| b.to_string()).unwrap();
            assert!(stems.contains(&base.as_str()), "{name} names no base theme");
        }
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
    fn hex_parsing_takes_both_spellings_and_rejects_the_rest() {
        assert_eq!(parse_hex("#2a1f1d"), Some((42, 31, 29)));
        assert_eq!(parse_hex("2a1f1d"), Some((42, 31, 29)));
        assert_eq!(parse_hex("#abc"), None);
        assert_eq!(parse_hex("not a colour"), None);
    }
}

// ── starting from nothing ────────────────────────────────────────────────────

/// The contrast a theme's text must clear on its own block.
///
/// WCAG 2.1 AA for body text. [`readable_on`] already picks the better of black
/// and white, which for a 6x6x6 cube colour is never worse than about 4.5:1, so
/// this is a floor the bundled colours are chosen to clear with room rather
/// than a threshold they scrape past.
pub const TEXT_MIN_AA: f64 = 4.5;

/// The floor a generated theme has to clear, which is not [`TEXT_MIN_AA`].
///
/// AA is 4.5:1 and the worst colour in the whole 6x6x6 cube scores 4.60:1, so
/// filtering the cube by AA keeps all 216 of it -- the threshold reads like a
/// filter and is arithmetically a no-op. `readable_on` picking the better of
/// black and white is what makes that true, and it is the same property that
/// makes AA a sensible floor for a colour somebody chose on purpose.
///
/// Generating themes is the other problem: nobody wants to scroll 216 rows to
/// find a session colour. This is WCAG AAA for body text, which is a standard
/// rather than a number picked to reach a pleasant count, and it takes the
/// cube to 148.
pub const TEXT_MIN_AAA: f64 = 7.0;

/// The six colours `theme init` writes.
///
/// Six and not sixty: `theme gen --shades` mints a lighter and a darker sibling
/// of each on the machine that runs it, which turns these into eighteen themes
/// whose borders are measured against that terminal's own background rather
/// than against the one they were picked on.
///
/// Each was chosen so the base and both of its shades clear [`TEXT_MIN_AA`]
/// with margin; the worst of the eighteen is 5.53:1.
pub const BASE_THEMES: [(&str, &str, u8); 6] = [
    ("ember", "Ember", 208),
    ("pine", "Pine", 114),
    ("slate", "Slate", 68),
    ("plum", "Plum", 170),
    ("sand", "Sand", 179),
    ("ink", "Ink", 104),
];

/// How strict `theme gen --shades` is about the text it will have to carry.
///
/// A ladder rather than a number, because the useful question is "how many
/// themes do I want to scroll" and the honest answer is a contrast floor. WCAG
/// names the first two rungs and stops; the rest continue at its own spacing,
/// 2.5 per step, so the ladder is one rule rather than four opinions.
///
/// `A6` is not a floor at all. It is the six bundled colours and the lighter
/// and darker sibling of each, which is what `--shades` did before it swept
/// the cube, kept because eighteen curated colours is a reasonable thing to
/// want and because it is the only rung whose colours were chosen by a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadeLevel {
    /// 4.5:1. Every colour in the cube, because none of them fall below it.
    Aa,
    /// 7:1, and the default.
    Aaa,
    /// 9.5:1.
    A4,
    /// 12:1.
    A5,
    /// The bundled six and their siblings, eighteen in all.
    A6,
}

/// The names `--shades` accepts, for the error and the manual.
pub const SHADE_LEVELS: [&str; 5] = ["aa", "aaa", "a4", "a5", "a6"];

impl ShadeLevel {
    /// A level by name, ignoring case, or `None` for one that is not a rung.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "aa" => Some(Self::Aa),
            "aaa" => Some(Self::Aaa),
            "a4" => Some(Self::A4),
            "a5" => Some(Self::A5),
            "a6" => Some(Self::A6),
            _ => None,
        }
    }

    /// The contrast a colour has to clear, or `None` for the curated rung.
    pub fn min_contrast(self) -> Option<f64> {
        match self {
            Self::Aa => Some(TEXT_MIN_AA),
            Self::Aaa => Some(TEXT_MIN_AAA),
            Self::A4 => Some(9.5),
            Self::A5 => Some(12.0),
            Self::A6 => None,
        }
    }
}

/// The themes a level asks for, named and ordered.
pub fn themes_for(level: ShadeLevel) -> Vec<(String, u8)> {
    match level.min_contrast() {
        Some(min) => cube_themes(min),
        None => sibling_themes(),
    }
}

/// The bundled six, each with a lighter and a darker sibling.
///
/// What `--shades` meant before it swept the cube, and now the tightest rung
/// of the ladder. Eighteen colours chosen by a person rather than by a
/// threshold, which is the one thing no contrast floor can produce.
pub fn sibling_themes() -> Vec<(String, u8)> {
    let mut seen: std::collections::HashSet<u8> =
        BASE_THEMES.iter().map(|(_, _, index)| *index).collect();
    let mut out = Vec::new();
    for (stem, _, index) in BASE_THEMES {
        for (label, shade) in shades(index) {
            if seen.insert(shade) {
                out.push((format!("{stem}-{label}"), shade));
            }
        }
    }
    out
}

/// Every cube colour a theme can be built on, named and ordered.
///
/// `theme gen --shades` used to mint exactly two siblings per theme you
/// already had: one step lighter and one darker. That answered "vary what I
/// have" when the question people actually ask is "show me what there is", and
/// it made the answer depend on which themes happened to be in the directory.
///
/// This walks the whole 6x6x6 cube instead and keeps every colour whose better
/// text colour clears [`TEXT_MIN_AAA`], which is 148 of the 216.
///
/// AAA and not AA, and that is not strictness for its own sake: the worst
/// colour in the cube scores 4.60:1, so an AA filter keeps all 216 and is a
/// no-op dressed up as a threshold. Greyscale is left out as it always was --
/// a session block the colour of the terminal is not a theme.
///
/// Names are the base theme each colour sits nearest to, in cube space, plus a
/// number: `ember-04`, `pine-11`. Naming them by index gives `colour137`,
/// which sorts by accident and tells a person nothing; naming them by their
/// nearest neighbour puts every warm colour together in the picker. The six
/// bases keep their own names and are not repeated here.
pub fn cube_themes(min_contrast: f64) -> Vec<(String, u8)> {
    let bases: Vec<(&str, [usize; 3])> = BASE_THEMES
        .iter()
        .filter_map(|(stem, _, index)| cube_parts(*index).map(|parts| (*stem, parts)))
        .collect();
    let taken: std::collections::HashSet<u8> =
        BASE_THEMES.iter().map(|(_, _, index)| *index).collect();

    let mut grouped: std::collections::BTreeMap<&str, Vec<u8>> = std::collections::BTreeMap::new();
    for index in 16u8..232 {
        if taken.contains(&index) {
            continue;
        }
        if readable_on(index).1 < min_contrast {
            continue;
        }
        let Some(parts) = cube_parts(index) else {
            continue;
        };
        // Nearest base in cube space, and the earliest base wins a tie so the
        // naming does not move when the loop order changes.
        let nearest = bases
            .iter()
            .min_by_key(|(_, b)| {
                (0..3)
                    .map(|axis| (parts[axis] as i32 - b[axis] as i32).pow(2))
                    .sum::<i32>()
            })
            .map(|(stem, _)| *stem)
            .unwrap_or("theme");
        grouped.entry(nearest).or_default().push(index);
    }

    let mut out = Vec::new();
    for (stem, indexes) in grouped {
        for (n, index) in indexes.into_iter().enumerate() {
            out.push((format!("{stem}-{:02}", n + 1), index));
        }
    }
    out
}

/// One bundled theme, as a file.
pub fn base_theme_file(stem: &str, label: &str, index: u8, dir: &str) -> String {
    let (fg, _) = readable_on(index);
    format!(
        "# {label}\n\
         # Written by `tmux-companion theme init`. Yours to edit.\n\
         #\n\
         # @theme-color-main-1 is the only colour that has to be here. Running\n\
         # `tmux-companion theme gen --apply` computes the other two from it:\n\
         # the text colour that reads on this block, and a border that stays\n\
         # this colour while clearing 3:1 on your terminal's background.\n\n\
         source-file \"{dir}/_reset.tmux\"\n\n\
         set @theme-name         \"{label}\"\n\
         set @theme-color-main-1 colour{index}\n\
         set @theme-color-on-main  {fg}\n\n\
         source-file \"{dir}/_apply.tmux\"\n"
    )
    .replace("{stem}", stem)
}

/// The file that puts the themed options back to something neutral.
pub fn reset_file() -> String {
    "# Sourced by every theme before it sets anything.\n\
     #\n\
     # A theme that leaves an option out would otherwise inherit it from\n\
     # whichever theme ran before, so switching from one to another would\n\
     # carry a stray colour across.\n\n\
     set @theme-color-main-1 colour245\n\
     set @theme-color-on-main colour16\n\
     set @theme-color-border colour238\n"
        .to_string()
}

/// The file that decides what a theme actually changes.
///
/// Shipped commented rather than minimal, because this is the opinionated part:
/// it is the difference between a theme colouring one block and a theme
/// repainting everything. Somebody who wants less deletes lines.
pub fn apply_file() -> String {
    "# What a theme actually changes. Sourced by every theme, after it has set\n\
     # its colours.\n\
     #\n\
     # The -F matters and is easy to lose. tmux does not expand an option that\n\
     # refers to another option, so `#{@theme-session-name-bg}` in status-left\n\
     # would come back as the literal text `#{@theme-color-main-1}`. `set -F`\n\
     # resolves the reference when the option is set, which is once per theme\n\
     # change rather than once per redraw.\n\
     #\n\
     # No -g on any of these, and that is the difference between a theme and a\n\
     # wallpaper. `set -g` writes one value for the whole server, so the last\n\
     # session to start would paint every other one and a project could not\n\
     # have its own colour. Without -g, and sourced with `-t`, each session\n\
     # keeps its own.\n\
     #\n\
     # Everything below is a choice. Delete what you do not want coloured.\n\n\
     # The session name block on the left of the status bar.\n\
     set -F @theme-session-name-bg \"#{@theme-color-main-1}\"\n\
     set -F @theme-session-name-fg \"#{@theme-color-on-main}\"\n\n\
     # The border around the pane you are in. `theme gen` picks this colour to\n\
     # clear 3:1 against your terminal background, which is what makes it\n\
     # visible without being a grey that says nothing.\n\
     set -F pane-active-border-style \"fg=#{@theme-color-border}\"\n\n\
     # The window you are on, in the window list.\n\
     set -wF window-status-current-style \"fg=#{@theme-color-main-1},bold\"\n\n\
     # tmux's own messages and prompts.\n\
     set -F message-style \"bg=#{@theme-color-main-1},fg=#{@theme-color-on-main}\"\n\
     set -F message-command-style \"bg=#{@theme-color-main-1},fg=#{@theme-color-on-main}\"\n\n\
     # Copy mode's selection and its indicator.\n\
     set -wF mode-style \"bg=#{@theme-color-main-1},fg=#{@theme-color-on-main}\"\n"
        .to_string()
}

/// Where tmux keeps its configuration on this machine, and so where the themes
/// belong.
///
/// tmux looks for `$XDG_CONFIG_HOME/tmux/tmux.conf` before `~/.tmux.conf`, and
/// people run both. Putting the themes beside whichever config actually exists
/// keeps `source-file` working without anybody editing a path, and a machine
/// with neither gets the XDG one, which is what tmux 3.1 and later prefer.
pub fn themes_dir(home: &str, xdg_config: Option<&str>) -> std::path::PathBuf {
    let xdg = match xdg_config {
        Some(x) if !x.is_empty() => std::path::PathBuf::from(x),
        _ => std::path::PathBuf::from(home).join(".config"),
    };
    if xdg.join("tmux/tmux.conf").is_file() {
        return xdg.join("tmux/themes");
    }
    if std::path::Path::new(home).join(".tmux.conf").is_file() {
        return std::path::PathBuf::from(home).join(".tmux/themes");
    }
    xdg.join("tmux/themes")
}

/// [`themes_dir`] from the environment.
pub fn default_themes_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    themes_dir(&home, xdg.as_deref())
}

#[cfg(test)]
mod themes_dir_tests {
    use super::*;

    #[test]
    fn an_xdg_config_wins_when_the_config_is_there() {
        let t = tempfile::tempdir().unwrap();
        let xdg = t.path().join("cfg");
        std::fs::create_dir_all(xdg.join("tmux")).unwrap();
        std::fs::write(xdg.join("tmux/tmux.conf"), "").unwrap();
        assert_eq!(
            themes_dir(
                &t.path().display().to_string(),
                Some(&xdg.display().to_string())
            ),
            xdg.join("tmux/themes")
        );
    }

    #[test]
    fn the_classic_dotfile_puts_them_under_dot_tmux() {
        // Somebody on ~/.tmux.conf would otherwise get themes in a directory
        // their tmux never looks at, and every source-file line would fail.
        let t = tempfile::tempdir().unwrap();
        let home = t.path().display().to_string();
        std::fs::write(t.path().join(".tmux.conf"), "").unwrap();
        assert_eq!(themes_dir(&home, None), t.path().join(".tmux/themes"));
    }

    #[test]
    fn xdg_wins_when_both_exist() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().display().to_string();
        std::fs::create_dir_all(t.path().join(".config/tmux")).unwrap();
        std::fs::write(t.path().join(".config/tmux/tmux.conf"), "").unwrap();
        std::fs::write(t.path().join(".tmux.conf"), "").unwrap();
        assert_eq!(
            themes_dir(&home, None),
            t.path().join(".config/tmux/themes")
        );
    }

    #[test]
    fn a_machine_with_neither_gets_the_one_tmux_prefers() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().display().to_string();
        assert_eq!(
            themes_dir(&home, None),
            t.path().join(".config/tmux/themes")
        );
    }
}

#[cfg(test)]
mod aa_tests {
    use super::*;

    /// Every colour this ships, and every sibling `--shades` will mint from it,
    /// has to clear WCAG 2.1 AA for body text against the text colour
    /// `readable_on` picks for it.
    ///
    /// Pinned rather than assumed: `readable_on` takes the better of black and
    /// white, and for a 6x6x6 cube colour the worst case sits at about 4.5:1,
    /// which is AA exactly. Bundling a colour that lands there would ship
    /// something that passes on paper and reads badly.
    #[test]
    fn every_bundled_theme_and_its_shades_clear_wcag_aa() {
        let mut worst = f64::MAX;
        let mut worst_at = 0u8;
        for (stem, _, index) in BASE_THEMES {
            let mut check = |i: u8, what: &str| {
                let (fg, ratio) = readable_on(i);
                assert!(
                    ratio >= TEXT_MIN_AA,
                    "{stem} {what} colour{i} draws {fg} at {ratio:.2}:1, under AA"
                );
                if ratio < worst {
                    worst = ratio;
                    worst_at = i;
                }
            };
            check(index, "base");
            for (label, shade) in shades(index) {
                check(shade, label);
            }
        }
        // Margin, not a bare pass. If a future edit drops a colour onto the
        // 4.5 line this fails and says which.
        assert!(
            worst >= 5.0,
            "the weakest bundled colour is colour{worst_at} at {worst:.2}:1, too close to AA"
        );
    }

    #[test]
    fn every_bundled_colour_has_both_a_lighter_and_a_darker_sibling() {
        // A base at the edge of the cube mints only one shade, so six colours
        // would become twelve themes rather than eighteen.
        for (stem, _, index) in BASE_THEMES {
            assert_eq!(shades(index).len(), 2, "{stem} colour{index}");
        }
    }

    #[test]
    fn the_bundled_colours_are_distinct_and_not_grey() {
        let mut seen = std::collections::HashSet::new();
        for (stem, _, index) in BASE_THEMES {
            assert!(seen.insert(index), "{stem} repeats colour{index}");
            let parts = cube_parts(index).expect("a cube colour");
            let spread = parts.iter().max().unwrap() - parts.iter().min().unwrap();
            assert!(
                spread >= 2,
                "{stem} colour{index} is too grey to be a theme"
            );
        }
    }
}

// ── making one by hand ───────────────────────────────────────────────────────

/// The better of black and white on an arbitrary colour, with its ratio.
///
/// [`readable_on`] answers the same question for a 256-colour index. This one
/// takes the colour itself, because `theme add` accepts `#rrggbb` and a hex
/// colour has no index to look up.
pub fn readable_on_rgb(bg: (u8, u8, u8)) -> (&'static str, f64) {
    let on_dark = contrast(bg, rgb(16));
    let on_light = contrast(bg, rgb(231));
    if on_dark >= on_light {
        (TEXT_DARK, on_dark)
    } else {
        (TEXT_LIGHT, on_light)
    }
}

/// A theme file written from a background and, optionally, a chosen text
/// colour.
///
/// The border is left out rather than guessed: it depends on the terminal's
/// own background, which this does not know, and `theme gen --apply` fills it
/// in on the machine that will display it.
pub fn added_theme_file(label: &str, bg: &str, fg: &str, dir: &str) -> String {
    format!(
        "# {label}\n\
         # Written by `tmux-companion theme add`. Yours to edit.\n\
         #\n\
         # Run `tmux-companion theme gen --apply` to add @theme-color-border,\n\
         # which has to be measured against your terminal's own background.\n\n\
         source-file \"{dir}/_reset.tmux\"\n\n\
         set @theme-name         \"{label}\"\n\
         set @theme-color-main-1 {bg}\n\
         set @theme-color-on-main  {fg}\n\n\
         source-file \"{dir}/_apply.tmux\"\n"
    )
}

/// A file name for a theme called `label`.
pub fn stem_for(label: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Every colour tmux will take, as rows of (value, name, rgb).
///
/// The sixteen names first because they are what somebody types, then the cube
/// and the greyscale ramp by number. The named ones repeat as `colour0`
/// upwards, which is the point: the list is what to type, not a set of
/// distinct colours.
pub fn all_colours() -> Vec<(String, &'static str, (u8, u8, u8))> {
    let mut out: Vec<(String, &'static str, (u8, u8, u8))> = BASE_NAMES
        .iter()
        .map(|(name, rgb)| ((*name).to_string(), *name, *rgb))
        .collect();
    for i in 0u16..=255 {
        let i = i as u8;
        out.push((format!("colour{i}"), "", rgb(i)));
    }
    out
}

#[cfg(test)]
mod add_tests {
    use super::*;

    #[test]
    fn a_label_becomes_a_file_name() {
        assert_eq!(stem_for("Ember"), "ember");
        assert_eq!(stem_for("Tokyo Night"), "tokyo-night");
        assert_eq!(stem_for("  Rosé  Pine  "), "ros-pine");
        assert_eq!(stem_for("solarized/dark"), "solarized-dark");
    }

    #[test]
    fn a_label_of_nothing_usable_is_empty_rather_than_a_pile_of_dashes() {
        assert_eq!(stem_for("///"), "");
        assert_eq!(stem_for(""), "");
    }

    #[test]
    fn readable_on_rgb_agrees_with_the_index_version() {
        for i in [16u8, 208, 114, 68, 231, 240] {
            assert_eq!(readable_on_rgb(rgb(i)).0, readable_on(i).0, "colour{i}");
        }
    }

    #[test]
    fn a_hex_background_gets_a_text_colour_too() {
        // The reason readable_on_rgb exists: #rrggbb has no index.
        let (fg, ratio) = readable_on_rgb(parse_hex("#ff8800").unwrap());
        assert_eq!(fg, TEXT_DARK);
        assert!(ratio > 4.5, "{ratio}");
    }

    #[test]
    fn the_written_file_carries_both_colours_and_sources_the_machinery() {
        let body = added_theme_file("Tokyo Night", "colour61", "colour231", "/t/themes");
        assert!(body.contains("set @theme-color-main-1 colour61"), "{body}");
        assert!(
            body.contains("set @theme-color-on-main  colour231"),
            "{body}"
        );
        assert!(
            body.contains("source-file \"/t/themes/_reset.tmux\""),
            "{body}"
        );
        assert!(
            body.contains("source-file \"/t/themes/_apply.tmux\""),
            "{body}"
        );
        assert!(
            !body.contains("set @theme-color-border"),
            "the border is gen's job; the comment may name it, a set line may not"
        );
    }

    #[test]
    fn the_colour_list_covers_every_value_tmux_takes() {
        let all = all_colours();
        assert_eq!(all.len(), BASE_NAMES.len() + 256);
        assert!(all.iter().any(|(v, _, _)| v == "colour0"));
        assert!(all.iter().any(|(v, _, _)| v == "colour255"));
        assert!(all.iter().any(|(v, n, _)| v == "red" && *n == "red"));
    }
}
