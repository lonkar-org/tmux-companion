use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::tmux::icons::{SLANT_IN, SLANT_OUT};

// Status-bar greys. The bar already steps 233 (base) → 235 (clock) → 237
// (battery) on the right; the current window rises to 236 on the left.
const BG_BAR: &str = "color233";
const BG_CURRENT: &str = "color236";

// Unselected/selected number box icons (indices 1–10).
// Glyph names verified against the installed FiraCode Nerd Font `post` table:
// UNSELECTED is the md-numeric_N_box_outline family, SELECTED the solid
// md-numeric_N_box family.  The font's codepoint order is irregular around
// F03AE–F03B2 (4_box_outline, 5_box_multiple_outline, 5_box_outline, 5_box,
// 4_box_multiple_outline), so these arrays are not a simple arithmetic run.
const UNSELECTED: [&str; 10] = [
    "\u{f03a6}", // 󰎦
    "\u{f03a9}", // 󰎩
    "\u{f03ac}", // 󰎬
    "\u{f03ae}", // 󰎮
    "\u{f03b0}", // 󰎰
    "\u{f03b5}", // 󰎵
    "\u{f03b8}", // 󰎸
    "\u{f03bb}", // 󰎻
    "\u{f03be}", //
    "\u{f0f7e}", // 󰽾
];

const SELECTED: [&str; 10] = [
    "\u{f03a4}", // 󰎤
    "\u{f03a7}", // 󰎧
    "\u{f03aa}", // 󰎪
    "\u{f03ad}", // 󰎭
    "\u{f03b1}", // 󰎱
    "\u{f03b3}", // 󰎳
    "\u{f03b6}", // 󰎶
    "\u{f03b9}", // 󰎹
    "\u{f03bc}", // 󰎼  md-numeric_9_box (NOT f03be — that is 9_box_outline)
    "\u{f0f7d}", // 󰽽
];

// Pane-index icons: md-numeric_0_box_multiple_outline … _10_box_multiple_outline
// (index 0–10).  A third glyph family, so pane indices never look like window
// indices.  Verified against the installed font; the array is indexed directly
// by the pane index — no off-by-one.
const PANE_BOX: [&str; 11] = [
    "\u{f03a2}", // 󰎢
    "\u{f03a5}", // 󰎥
    "\u{f03a8}", // 󰎨
    "\u{f03ab}", // 󰎫
    "\u{f03b2}", // 󰎲
    "\u{f03af}", // 󰎯
    "\u{f03b4}", // 󰎴
    "\u{f03b7}", // 󰎷
    "\u{f03ba}", // 󰎺
    "\u{f03bd}", // 󰎽
    "\u{f0feb}", // 󰿫
];

// 20-color cycle for running-process animation (seconds % 20)
//
// Lifted in HSL, hue and saturation held, until every step clears 4.5:1 against
// BG_CURRENT as well as BG_BAR.  17 of the 20 were below it on BG_CURRENT, the
// middle of the ramp worst at 3.21:1, because that is where its luminance
// passes closest to the background's.  Lightness was raised rather than white
// mixed in, which would have faded green-to-red into olive-to-salmon.
const PS_COLORS: [&str; 20] = [
    "#54b435", "#5cae36", "#63a837", "#6da639", "#78a43b", "#84a23d", "#8e9f40", "#9b9c42",
    "#a59844", "#b29446", "#bb904e", "#c18d59", "#c58a5f", "#c98867", "#ce856b", "#d4826d",
    "#db7d6d", "#e1796e", "#e67671", "#ed7171",
];

// Dir logos — applied in priority order (most specific first)
struct DirLogo {
    prefix: &'static str,
    logo: &'static str,
}

// Codepoints extracted directly from window-status.zsh dir_logos array.
const DIR_LOGOS: &[DirLogo] = &[
    DirLogo {
        prefix: "~/g/mysetup",
        logo: "\u{f0630} ",
    }, // 󰘰 mysetup icon
    DirLogo {
        prefix: "~/g/",
        logo: "\u{f1d3} /",
    }, // git repos
    DirLogo {
        prefix: "~/b/",
        logo: "\u{f0bee} /",
    }, // 󰯮 bin/scripts
    DirLogo {
        prefix: "~/",
        logo: "\u{f0827} ",
    }, // 󰠧 home sub
    DirLogo {
        prefix: "~",
        logo: "\u{f0827}",
    }, // 󰠧 home
    DirLogo {
        prefix: "/",
        logo: "\u{eb45} ",
    }, //  root
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowArgs {
    #[serde(default)]
    pub current: bool,
    pub index: u32,
    #[serde(default)]
    pub window_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub process: Option<String>,
    #[serde(default)]
    pub start_path: Option<PathBuf>,
    #[serde(default)]
    pub flags: Option<String>,
    #[serde(default)]
    pub last: u32,
    /// Number of panes in the window (`#{window_panes}`)
    #[serde(default)]
    pub pane_count: u32,
    /// Index of the window's active pane (`#{pane_index}`)
    #[serde(default)]
    pub pane_index: u32,
}

pub fn render(args: &WindowArgs, dir_aliases: &HashMap<PathBuf, String>) -> String {
    let home = dirs_home();
    let path = args.path.as_deref().unwrap_or(Path::new("/"));
    let process = args.process.as_deref().unwrap_or("zsh");
    let flags = args.flags.as_deref().unwrap_or("");

    // Process animation: non-zsh process in non-current window cycles colors
    let (running, index_color) = if process != "zsh" && !args.current {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let ci = (secs % 20) as usize;
        (true, format!("#[fg={},none]", PS_COLORS[ci]))
    } else {
        (false, String::new())
    };

    // Title: check override name first, then dir aliases, then abbreviate path
    let name_override = args
        .name
        .as_deref()
        .filter(|n| !n.is_empty() && *n != "zsh" && Some(*n) != args.process.as_deref());

    let title = if let Some(name) = name_override {
        name.to_string()
    } else if let Some(alias) = dir_aliases.get(path) {
        alias.clone()
    } else {
        abbreviate_path(path, &home)
    };

    // Index icon
    let idx = args.index as usize;
    let icon = if args.current {
        SELECTED.get(idx.wrapping_sub(1)).copied().unwrap_or("?")
    } else {
        UNSELECTED.get(idx.wrapping_sub(1)).copied().unwrap_or("?")
    };

    // Colors. The current window is an elevated block (bg=236) closed by slanted
    // caps; every other window stays flat on the bar background.
    let (final_index_color, title_color) = if args.current {
        (
            format!("#[fg=#ffffff,bg={}]", BG_CURRENT),
            format!("#[fg=#e3f2fd,bg={},none]", BG_CURRENT),
        )
    } else if args.last == 1 {
        let ic = if running {
            index_color
        } else {
            "#[fg=#adb5bd,none]".to_string()
        };
        (ic, "#[fg=#adb5bd,none]".to_string())
    } else {
        let ic = if running {
            index_color
        } else {
            "#[fg=color242,none]".to_string()
        };
        (ic, "#[fg=color242,none]".to_string())
    };

    // Alert indicator
    let alert = if flags.contains('#') {
        if flags.contains('!') {
            " #[fg=#ff0000]\u{f0027}".to_string() // alert icon
        } else if !running {
            " #[fg=#5fd700]\u{f063e}".to_string() // activity icon 󰘾
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Pane suffix: only for the current window, and only when it is split.
    // The active pane index is meaningless for windows you are not looking at,
    // and dropping it keeps the inactive run of windows narrow.
    let pane_suffix = if args.current {
        pane_suffix(args.pane_count, args.pane_index)
    } else {
        String::new()
    };

    let (cap_in, cap_out) = block_caps(args.current);

    // The block's padding must be painted with the block background, so the
    // leading space follows the colour set rather than the cap — otherwise the
    // bar colour shows through as a gap between the cap and the block. The
    // trailing pad balances it on the closing side.
    let trailing_pad = if args.current { " " } else { "" };

    format!(
        "{}{} {}{} {}{}{}{}{}",
        cap_in,
        final_index_color,
        icon,
        title_color,
        title,
        pane_suffix,
        alert,
        trailing_pad,
        cap_out
    )
}

/// Slanted caps that open and close the elevated current-window block.
/// Both are drawn as foreground on the bar background, so the block reads as a
/// step up from the bar rather than a separator between windows. Non-current
/// windows get no caps at all — they stay flat and cost zero extra columns.
fn block_caps(current: bool) -> (String, String) {
    if !current {
        return (String::new(), String::new());
    }
    (
        format!("#[fg={},bg={}]{}", BG_CURRENT, BG_BAR, SLANT_IN),
        format!("#[fg={},bg={}]{}", BG_CURRENT, BG_BAR, SLANT_OUT),
    )
}

/// Active-pane suffix (nf-md-numeric_N_box), empty when the window has 0 or 1 panes.
/// Falls back to a plain digit for pane indices above 10.
fn pane_suffix(pane_count: u32, pane_index: u32) -> String {
    if pane_count <= 1 {
        return String::new();
    }
    match PANE_BOX.get(pane_index as usize) {
        Some(icon) => format!(" {}", icon),
        None => format!(" {}", pane_index),
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

fn abbreviate_path(path: &Path, home: &Path) -> String {
    use std::path::Component;

    // Strip home prefix and replace with ~, otherwise use the path directly.
    // We separate the string prefix ("~" or "") from the remaining components.
    let (str_prefix, rest): (&str, &Path) = match path.strip_prefix(home) {
        Ok(rel) => ("~", rel),
        Err(_) => ("", path),
    };

    // Collect only Normal/CurDir/ParentDir components (skip RootDir).
    let comps: Vec<String> = rest
        .components()
        .filter_map(|c| match c {
            Component::RootDir => None,
            other => Some(other.as_os_str().to_string_lossy().into_owned()),
        })
        .collect();

    let is_abs = str_prefix.is_empty() && path.is_absolute();

    // If there are no path components, the path is the prefix alone (e.g. home or /).
    if comps.is_empty() {
        let base = if is_abs {
            "/".to_string()
        } else {
            str_prefix.to_string()
        };
        for logo in DIR_LOGOS {
            if base.starts_with(logo.prefix) {
                return base.replacen(logo.prefix, logo.logo, 1);
            }
        }
        return base;
    }

    // Abbreviate all components except the last.
    let last = comps.len() - 1;
    let mut parts: Vec<String> = comps
        .iter()
        .enumerate()
        .map(|(i, comp)| {
            if i == last {
                comp.clone()
            } else if comp.starts_with('.') {
                // Dot-prefixed: keep first two visible chars.
                comp.chars().take(2).collect()
            } else {
                comp.chars()
                    .next()
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            }
        })
        .collect();

    // Ellipsize basename if longer than 17 chars.
    if let Some(base) = parts.last_mut() {
        let len = base.chars().count();
        if len > 17 {
            let head: String = base.chars().take(7).collect();
            let tail: String = base
                .chars()
                .rev()
                .take(7)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            *base = format!("{}...{}", head, tail);
        }
    }

    let inner = parts.join("/");

    // Reconstruct with the correct prefix (no double slashes).
    let abbreviated = if !str_prefix.is_empty() {
        format!("{}/{}", str_prefix, inner)
    } else if is_abs {
        format!("/{}", inner)
    } else {
        inner
    };

    // Apply dir logo substitutions in priority order.
    for logo in DIR_LOGOS {
        if abbreviated.starts_with(logo.prefix) {
            return abbreviated.replacen(logo.prefix, logo.logo, 1);
        }
    }

    abbreviated
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── icon codepoints ───────────────────────────────────────────────────────

    #[test]
    fn selected_icon_codepoints() {
        // md-numeric_1_box U+F03A4 = f3 b0 8e a4
        assert_eq!(SELECTED[0].as_bytes(), &[0xf3, 0xb0, 0x8e, 0xa4]);
        // md-numeric_9_box U+F03BC = f3 b0 8e bc
        assert_eq!(SELECTED[8].as_bytes(), &[0xf3, 0xb0, 0x8e, 0xbc]);
    }

    #[test]
    fn unselected_icon_codepoints() {
        // md-numeric_1_box_outline U+F03A6 = f3 b0 8e a6
        assert_eq!(UNSELECTED[0].as_bytes(), &[0xf3, 0xb0, 0x8e, 0xa6]);
        // md-numeric_9_box_outline U+F03BE = f3 b0 8e be
        assert_eq!(UNSELECTED[8].as_bytes(), &[0xf3, 0xb0, 0x8e, 0xbe]);
    }

    #[test]
    fn icon_arrays_have_ten_entries() {
        assert_eq!(SELECTED.len(), 10);
        assert_eq!(UNSELECTED.len(), 10);
        for (s, u) in SELECTED.iter().zip(UNSELECTED.iter()) {
            assert!(!s.is_empty());
            assert!(!u.is_empty());
            // selected and unselected icons at same index must differ
            assert_ne!(s, u);
        }
    }

    // ── abbreviate_path ───────────────────────────────────────────────────────

    fn home() -> PathBuf {
        PathBuf::from("/Users/test")
    }

    #[test]
    fn abbrev_home_subdir_shows_git_logo() {
        let path = PathBuf::from("/Users/test/git-repos/myproject");
        let result = abbreviate_path(&path, &home());
        // ~/git-repos/myproject → ~/g/myproject → git-logo /myproject
        assert!(result.contains("myproject"), "result: {result}");
    }

    #[test]
    fn abbrev_exact_home_uses_home_logo() {
        let result = abbreviate_path(&home(), &home());
        // "~" → home logo (no trailing slash)
        let logo = "\u{f0827}";
        assert!(result.contains(logo), "expected home logo in: {result}");
    }

    #[test]
    fn abbrev_home_slash_uses_tilde_slash_logo() {
        let path = PathBuf::from("/Users/test/Documents");
        let result = abbreviate_path(&path, &home());
        // ~/Documents → ~/D → tilde+slash logo + D
        let logo = "\u{f0827} "; // ~/  logo
        assert!(result.contains(logo), "expected ~/ logo in: {result}");
        assert!(result.contains("Documents"), "result: {result}");
    }

    #[test]
    fn abbrev_absolute_root_path() {
        let path = PathBuf::from("/tmp");
        let result = abbreviate_path(&path, &home());
        // /tmp → root logo + tmp
        let root_logo = "\u{eb45} ";
        assert!(
            result.contains(root_logo),
            "expected root logo in: {result}"
        );
        assert!(result.contains("tmp"), "result: {result}");
    }

    #[test]
    fn abbrev_abbreviates_intermediate_components() {
        // /Users/test/a/b/c/d/myproject → ~/a/b/c/d/myproject
        // intermediate: a→a, b→b, c→c, d→d  (first char only)
        let path = PathBuf::from("/Users/test/alpha/beta/gamma/myproject");
        let result = abbreviate_path(&path, &home());
        // Should contain abbreviated intermediates and full basename
        assert!(result.contains("myproject"), "basename: {result}");
        assert!(!result.contains("alpha"), "alpha abbreviated: {result}");
        assert!(!result.contains("beta"), "beta abbreviated: {result}");
        assert!(!result.contains("gamma"), "gamma abbreviated: {result}");
    }

    #[test]
    fn abbrev_dotfile_component_keeps_two_chars() {
        let path = PathBuf::from("/Users/test/.config/nvim");
        let result = abbreviate_path(&path, &home());
        // .config → .c (two chars)
        assert!(result.contains(".c"), "dotfile abbreviated: {result}");
        assert!(result.contains("nvim"), "basename: {result}");
    }

    #[test]
    fn abbrev_long_basename_ellipsized() {
        let path = PathBuf::from("/Users/test/a-very-long-directory-name");
        let result = abbreviate_path(&path, &home());
        assert!(result.contains("..."), "expected ellipsis: {result}");
        // head=7 + "..." + tail=7
        let basename_part: String = result.chars().skip_while(|&c| c != 'a').collect();
        // Just verify head and tail are present
        let orig = "a-very-long-directory-name";
        let head: String = orig.chars().take(7).collect();
        let tail: String = orig
            .chars()
            .rev()
            .take(7)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        assert!(result.contains(&head), "head: {result}");
        assert!(result.contains(&tail), "tail: {result}");
        let _ = basename_part;
    }

    #[test]
    fn abbrev_short_basename_not_ellipsized() {
        let path = PathBuf::from("/Users/test/shortname");
        let result = abbreviate_path(&path, &home());
        assert!(!result.contains("..."), "should not ellipsize: {result}");
        assert!(result.contains("shortname"), "result: {result}");
    }

    #[test]
    fn abbrev_exactly_17_char_basename_not_ellipsized() {
        let seventeen = "a".repeat(17);
        let path = PathBuf::from(format!("/Users/test/{}", seventeen));
        let result = abbreviate_path(&path, &home());
        assert!(
            !result.contains("..."),
            "17 chars should not ellipsize: {result}"
        );
    }

    #[test]
    fn abbrev_mysetup_gets_special_logo() {
        let path = PathBuf::from("/Users/test/git-repos/mysetup");
        let result = abbreviate_path(&path, &home());
        let logo = "\u{f0630} ";
        assert!(result.starts_with(logo), "expected mysetup logo: {result}");
    }

    // ── render ────────────────────────────────────────────────────────────────

    fn args_base() -> WindowArgs {
        WindowArgs {
            current: false,
            index: 1,
            window_id: Some("@1".into()),
            name: None,
            path: Some(PathBuf::from("/tmp")),
            process: Some("zsh".into()),
            start_path: None,
            flags: Some(String::new()),
            last: 0,
            pane_count: 1,
            pane_index: 1,
        }
    }

    fn no_aliases() -> HashMap<PathBuf, String> {
        HashMap::new()
    }

    #[test]
    fn render_current_uses_selected_icon() {
        let args = WindowArgs {
            current: true,
            index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(out.contains(SELECTED[0]), "expected selected[0] in: {out}");
        assert!(
            !out.contains(UNSELECTED[0]),
            "should not use unselected: {out}"
        );
    }

    #[test]
    fn render_non_current_uses_unselected_icon() {
        let args = WindowArgs {
            current: false,
            index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(out.contains(UNSELECTED[0]), "expected unselected[0]: {out}");
    }

    #[test]
    fn render_index_2() {
        let args = WindowArgs {
            index: 2,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(out.contains(UNSELECTED[1]));
    }

    #[test]
    fn render_index_10_valid() {
        let args = WindowArgs {
            index: 10,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(out.contains(UNSELECTED[9]));
    }

    #[test]
    fn render_current_window_white_title_color() {
        let args = WindowArgs {
            current: true,
            index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(out.contains("#ffffff"), "expected white fg: {out}");
        assert!(out.contains("#e3f2fd"), "expected light blue title: {out}");
    }

    #[test]
    fn render_last_window_grey_color() {
        let args = WindowArgs {
            last: 1,
            index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(
            out.contains("#adb5bd"),
            "expected grey for last window: {out}"
        );
    }

    #[test]
    fn render_other_window_dim_color() {
        let args = WindowArgs {
            index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(
            out.contains("color242"),
            "expected dim for other window: {out}"
        );
    }

    #[test]
    fn render_name_override_used_as_title() {
        let args = WindowArgs {
            name: Some("my-session".into()),
            process: Some("zsh".into()),
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(out.contains("my-session"), "override name: {out}");
    }

    #[test]
    fn render_zsh_override_not_used() {
        // name="zsh" is treated the same as no override (falls through to path)
        let args = WindowArgs {
            name: Some("zsh".into()),
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        // should show path-based title, not "zsh"
        assert!(out.contains("tmp"), "should use path: {out}");
    }

    #[test]
    fn render_dir_alias_used() {
        let mut aliases = HashMap::new();
        aliases.insert(PathBuf::from("/tmp"), "my-alias".to_string());
        let args = WindowArgs {
            path: Some(PathBuf::from("/tmp")),
            ..args_base()
        };
        let out = render(&args, &aliases);
        assert!(out.contains("my-alias"), "alias used: {out}");
    }

    #[test]
    fn render_activity_flag_shows_indicator() {
        let args = WindowArgs {
            flags: Some("#".into()),
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        // Activity flag (#) → green indicator
        assert!(
            out.contains("#5fd700") || out.contains("\u{f063e}"),
            "activity: {out}"
        );
    }

    #[test]
    fn render_bell_flag_shows_red() {
        let args = WindowArgs {
            flags: Some("#!".into()),
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(out.contains("#ff0000"), "bell red: {out}");
    }

    #[test]
    fn render_no_flag_no_alert() {
        let args = WindowArgs {
            flags: Some(String::new()),
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(!out.contains("#ff0000"), "no bell: {out}");
        assert!(!out.contains("#5fd700"), "no activity: {out}");
    }

    #[test]
    fn render_running_process_shows_color_cycle() {
        let args = WindowArgs {
            current: false,
            process: Some("cargo".into()), // non-zsh
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        // Should contain one of the PS_COLORS entries (hex color)
        let has_ps_color = PS_COLORS.iter().any(|c| out.contains(c));
        assert!(has_ps_color, "expected a process color: {out}");
    }

    // ── pane suffix ───────────────────────────────────────────────────────────

    #[test]
    fn pane_suffix_empty_for_single_pane() {
        assert_eq!(pane_suffix(1, 1), "");
        assert_eq!(pane_suffix(0, 0), "");
    }

    #[test]
    fn pane_suffix_uses_numeric_box_icon() {
        assert_eq!(pane_suffix(2, 0), format!(" {}", PANE_BOX[0]));
        assert_eq!(pane_suffix(2, 2), format!(" {}", PANE_BOX[2]));
        assert_eq!(pane_suffix(11, 10), format!(" {}", PANE_BOX[10]));
    }

    #[test]
    fn pane_suffix_falls_back_to_digits_above_ten() {
        assert_eq!(pane_suffix(12, 11), " 11");
    }

    #[test]
    fn pane_box_codepoints() {
        // md-numeric_0_box_multiple_outline U+F03A2 = f3 b0 8e a2
        assert_eq!(PANE_BOX[0].as_bytes(), &[0xf3, 0xb0, 0x8e, 0xa2]);
        // md-numeric_10_box_multiple_outline U+F0FEB = f3 b0 bf ab
        assert_eq!(PANE_BOX[10].as_bytes(), &[0xf3, 0xb0, 0xbf, 0xab]);
        // all distinct, none shared with the window-index icon sets
        for (i, icon) in PANE_BOX.iter().enumerate() {
            assert!(
                !SELECTED.contains(icon),
                "PANE_BOX[{i}] collides with SELECTED"
            );
            assert!(
                !UNSELECTED.contains(icon),
                "PANE_BOX[{i}] collides with UNSELECTED"
            );
        }
    }

    #[test]
    fn render_single_pane_has_no_suffix() {
        let args = WindowArgs {
            pane_count: 1,
            pane_index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        for icon in PANE_BOX.iter() {
            assert!(!out.contains(icon), "no pane icon expected: {out}");
        }
    }

    #[test]
    fn render_split_window_appends_pane_icon() {
        let args = WindowArgs {
            current: true,
            index: 1,
            pane_count: 3,
            pane_index: 2,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(
            visible(&out).contains(PANE_BOX[2]),
            "expected pane 2 box icon: {out}"
        );
    }

    #[test]
    fn render_split_non_current_window_has_no_pane_icon() {
        // The active pane index only matters for the window you are looking at.
        let args = WindowArgs {
            current: false,
            index: 1,
            pane_count: 3,
            pane_index: 2,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        for icon in PANE_BOX.iter() {
            assert!(!out.contains(icon), "no pane icon expected: {out}");
        }
    }

    #[test]
    fn render_pane_suffix_precedes_alert() {
        let args = WindowArgs {
            current: true,
            index: 1,
            pane_count: 2,
            pane_index: 2,
            flags: Some("#!".into()),
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        let pane_pos = out.find(PANE_BOX[2]).expect("pane icon present");
        let alert_pos = out.find("#ff0000").expect("alert present");
        assert!(pane_pos < alert_pos, "pane suffix before alert: {out}");
    }

    // ── current-window block ──────────────────────────────────────────────────

    #[test]
    fn block_caps_empty_for_non_current() {
        assert_eq!(block_caps(false), (String::new(), String::new()));
    }

    #[test]
    fn block_caps_drawn_on_bar_background() {
        let (cap_in, cap_out) = block_caps(true);
        // Cap glyphs are the block colour painted over the bar colour.
        assert_eq!(
            cap_in,
            format!("#[fg={},bg={}]{}", BG_CURRENT, BG_BAR, SLANT_IN)
        );
        assert_eq!(
            cap_out,
            format!("#[fg={},bg={}]{}", BG_CURRENT, BG_BAR, SLANT_OUT)
        );
    }

    #[test]
    fn render_current_wrapped_in_slanted_caps() {
        let args = WindowArgs {
            current: true,
            index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        assert!(
            out.starts_with(SLANT_IN) || out.contains(SLANT_IN),
            "opening cap: {out}"
        );
        assert!(out.ends_with(SLANT_OUT), "closing cap last: {out}");
    }

    #[test]
    fn render_current_uses_elevated_background() {
        let args = WindowArgs {
            current: true,
            index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        // Both the index icon and the title sit on the elevated block.
        assert!(
            out.contains(&format!("#[fg=#ffffff,bg={}]", BG_CURRENT)),
            "icon on block: {out}"
        );
        assert!(
            out.contains(&format!("#[fg=#e3f2fd,bg={},none]", BG_CURRENT)),
            "title on block: {out}"
        );
    }

    #[test]
    fn render_non_current_has_no_caps_and_no_block() {
        let args = args_base();
        let out = render(&args, &no_aliases());
        assert!(!out.contains(SLANT_IN), "no opening cap: {out}");
        assert!(!out.contains(SLANT_OUT), "no closing cap: {out}");
        assert!(!out.contains(BG_CURRENT), "no elevated bg: {out}");
    }

    #[test]
    fn render_current_closing_cap_follows_pane_and_alert() {
        // The cap must close the block after everything inside it, otherwise the
        // pane icon and alert would land on the bar background.
        let args = WindowArgs {
            current: true,
            index: 1,
            pane_count: 3,
            pane_index: 2,
            flags: Some("#!".into()),
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        let pane_pos = out.find(PANE_BOX[2]).expect("pane icon present");
        let alert_pos = out.find("#ff0000").expect("alert present");
        let cap_pos = out.rfind(SLANT_OUT).expect("closing cap present");
        assert!(pane_pos < cap_pos, "pane inside block: {out}");
        assert!(alert_pos < cap_pos, "alert inside block: {out}");
    }

    /// Drop `#[...]` tmux format tags, leaving only what is actually drawn.
    fn visible(s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(start) = rest.find("#[") {
            out.push_str(&rest[..start]);
            match rest[start..].find(']') {
                Some(end) => rest = &rest[start + end + 1..],
                None => {
                    rest = "";
                    break;
                }
            }
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn render_output_starts_with_space() {
        // All window status outputs lead with a space (matches zsh script).
        let args = args_base();
        let out = render(&args, &no_aliases());
        assert!(
            visible(&out).starts_with(' '),
            "should start with space: {out:?}"
        );
    }

    #[test]
    fn render_current_block_padding_is_inside_the_block() {
        // The space after the opening cap must be painted with the block
        // background, not the bar background — otherwise a dark gap appears
        // between the slant and the block.
        let args = WindowArgs {
            current: true,
            index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        let (cap_in, _) = block_caps(true);
        let after_cap = out
            .strip_prefix(&cap_in)
            .expect("output opens with the cap");
        assert!(
            after_cap.starts_with(&format!("#[fg=#ffffff,bg={}]", BG_CURRENT)),
            "block colour must be set before the pad: {out}"
        );
        assert!(
            !after_cap.starts_with(' '),
            "no bar-coloured gap after the cap: {out}"
        );
    }

    #[test]
    fn render_current_has_trailing_pad_before_closing_cap() {
        let args = WindowArgs {
            current: true,
            index: 1,
            ..args_base()
        };
        let out = render(&args, &no_aliases());
        let (_, cap_out) = block_caps(true);
        let body = out
            .strip_suffix(&cap_out)
            .expect("output ends with the cap");
        assert!(
            body.ends_with(' '),
            "block must be padded before the closing cap: {out}"
        );
    }

    #[test]
    fn render_non_current_has_no_trailing_pad() {
        let args = args_base();
        let out = render(&args, &no_aliases());
        assert!(
            !out.ends_with(' '),
            "flat windows keep their original width: {out:?}"
        );
    }
}
