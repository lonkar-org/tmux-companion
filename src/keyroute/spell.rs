//! Key names in tmux's spelling, from the spellings the apps in its panes use.
//!
//! A wrong spelling fails silently: a collision nobody sees, or later a claim
//! tmux never matches. So every conversion here has a test, and the rules are
//! the ones `mysetup`'s nvim publisher uses, so the two halves agree.
//!
//! tmux prints modifiers as `C-`, `M-`, `S-`, in that order, and keeps some
//! names apart that a terminal without extended keys can't tell apart: `M-A`
//! and `M-S-a`, `C-A` and `C-a` are separate bindings in `list-keys`. A
//! comparison goes through [`canonical`], which folds each pair onto one name.
//!
//! ```
//! use tmux_companion::keyroute::spell;
//!
//! assert_eq!(spell::from_nvim_key("<C-L>").as_deref(), Some("C-l"));
//! assert_eq!(spell::from_zsh("^[f").as_deref(), Some("M-f"));
//! assert_eq!(spell::canonical("M-S-a"), "M-A");
//! ```

/// nvim's names for special keys, lowercased, to tmux's.
fn nvim_special(name: &str) -> Option<&'static str> {
    Some(match name {
        "cr" | "enter" | "return" => "Enter",
        "tab" => "Tab",
        "esc" => "Escape",
        "space" => "Space",
        "bs" => "BSpace",
        "del" => "DC",
        "insert" => "IC",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PPage",
        "pagedown" => "NPage",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "lt" => "<",
        "bslash" => "\\",
        "bar" => "|",
        _ => return None,
    })
}

/// Modifiers on one key.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Mods {
    ctrl: bool,
    meta: bool,
    shift: bool,
}

/// A function key: `F` and one or two digits.
fn is_function_key(base: &str) -> bool {
    base.strip_prefix('F')
        .is_some_and(|n| !n.is_empty() && n.len() <= 2 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Join modifiers and a base key the way tmux prints them, folded: shift on a
/// letter becomes the capital letter. With `ctrl_ignores_case`, nvim's rule,
/// `<C-L>` is ctrl-l; tmux's own `C-A` is ctrl-shift-a and stays apart from
/// `C-a`, so tmux names go through with it false.
///
/// `None` for a key outside the routing scope: no Ctrl, no Alt, not an F key.
fn join(mut mods: Mods, base: &str, ctrl_ignores_case: bool) -> Option<String> {
    let mut base = base.to_string();
    if base.chars().count() == 1 && base.chars().all(|c| c.is_ascii_alphabetic()) {
        if mods.shift {
            base = base.to_ascii_uppercase();
            mods.shift = false;
        } else if mods.ctrl && ctrl_ignores_case {
            base = base.to_ascii_lowercase();
        }
    }
    if !(mods.ctrl || mods.meta || is_function_key(&base)) {
        return None;
    }
    let mut out = String::new();
    if mods.ctrl {
        out.push_str("C-");
    }
    if mods.meta {
        out.push_str("M-");
    }
    if mods.shift {
        out.push_str("S-");
    }
    out.push_str(&base);
    Some(out)
}

/// Split leading `X-` modifiers off a key name, with `accept` saying what each
/// letter means. `None` when a modifier is one tmux never sees.
fn split_mods(mut inner: &str, accept: fn(char, &mut Mods) -> bool) -> Option<(Mods, &str)> {
    let mut mods = Mods::default();
    loop {
        let mut chars = inner.chars();
        let (Some(m), Some('-')) = (chars.next(), chars.next()) else {
            break;
        };
        let rest = chars.as_str();
        // `M--` is Alt and minus, `C-` alone is not a modifier
        if rest.is_empty() || !m.is_ascii_alphabetic() {
            break;
        }
        if !accept(m, &mut mods) {
            return None;
        }
        inner = rest;
    }
    Some((mods, inner))
}

/// The first key of an nvim lhs as written: `<M-a>` from `<M-a>x`, `g` from `gx`.
pub fn first_key(lhs: &str) -> &str {
    if !lhs.starts_with('<') {
        return lhs.chars().next().map_or("", |c| &lhs[..c.len_utf8()]);
    }
    let Some(close) = lhs[1..].find('>') else {
        return "<";
    };
    let mut end = close + 2;
    // `<M->>`: the first `>` closes `<M->` too early, take the second
    if lhs[..end].ends_with("->") && lhs[end..].starts_with('>') {
        end += 1;
    }
    &lhs[..end]
}

/// One nvim key to tmux: `<M-a>` → `M-a`, `<M-H>` → `M-H`, `<C-L>` → `C-l`.
pub fn from_nvim_key(key: &str) -> Option<String> {
    let inner = key.strip_prefix('<')?.strip_suffix('>')?;
    let (mods, base) = split_mods(inner, |m, mods| {
        match m.to_ascii_uppercase() {
            'A' | 'M' | 'T' => mods.meta = true,
            'C' => mods.ctrl = true,
            'S' => mods.shift = true,
            // D- is Cmd on macOS, which never reaches tmux as itself
            _ => return false,
        }
        true
    })?;
    let base = if base.chars().count() == 1 {
        base.to_string()
    } else if is_function_key(&base.to_ascii_uppercase()) {
        base.to_ascii_uppercase()
    } else {
        nvim_special(&base.to_ascii_lowercase())?.to_string()
    };
    join(mods, &base, true)
}

/// The tmux spelling of the first key of an nvim lhs, or `None` when it is
/// outside the routing scope.
pub fn from_nvim_lhs(lhs: &str) -> Option<String> {
    from_nvim_key(first_key(lhs))
}

/// fzf's names for special keys to tmux's.
fn fzf_special(name: &str) -> Option<&'static str> {
    Some(match name {
        "enter" | "return" => "Enter",
        "tab" => "Tab",
        "esc" => "Escape",
        "space" => "Space",
        "bs" | "bspace" => "BSpace",
        "del" => "DC",
        "insert" => "IC",
        "home" => "Home",
        "end" => "End",
        "pgup" | "page-up" => "PPage",
        "pgdn" | "page-down" => "NPage",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        _ => return None,
    })
}

/// Modifiers spelled as words joined to the key, `ctrl-alt-x` or `ctrl+shift+b`,
/// with `sep` between them. `None` for a modifier tmux never sees, like `cmd`.
fn from_words(key: &str, sep: char, special: fn(&str) -> Option<&'static str>) -> Option<String> {
    let mut mods = Mods::default();
    let mut rest = key;
    while let Some((word, after)) = rest.split_once(sep) {
        // `ctrl+-`: the key itself is the separator
        if after.is_empty() {
            break;
        }
        match word.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "meta" | "opt" | "option" => mods.meta = true,
            "shift" => mods.shift = true,
            "cmd" | "command" | "super" | "win" => return None,
            // page-up: not a modifier, the whole rest is the name
            _ => break,
        }
        rest = after;
    }
    let base = if rest.chars().count() == 1 {
        rest.to_string()
    } else if is_function_key(&rest.to_ascii_uppercase()) {
        rest.to_ascii_uppercase()
    } else {
        special(&rest.to_ascii_lowercase())?.to_string()
    };
    join(mods, &base, true)
}

/// An fzf `--bind` key: `alt-a` → `M-a`, `ctrl-q` → `C-q`, `ctrl-alt-x` → `C-M-x`.
pub fn from_fzf(key: &str) -> Option<String> {
    from_words(key, '-', fzf_special)
}

/// A claude keystroke: `meta+p` → `M-p`, `ctrl+shift+b` → `C-B`.
pub fn from_claude(key: &str) -> Option<String> {
    from_words(key, '+', |name| {
        Some(match name {
            "escape" | "esc" => "Escape",
            "enter" | "return" => "Enter",
            "tab" => "Tab",
            "space" => "Space",
            "backspace" => "BSpace",
            "delete" => "DC",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            "home" => "Home",
            "end" => "End",
            "pageup" => "PPage",
            "pagedown" => "NPage",
            _ => return None,
        })
    })
}

/// A nanorc key: `^X` → `C-x`, `M-x` → `M-x`, `Sh-M-c` → `M-C`, `F5` → `F5`.
/// nano's `M-` keys don't care about case unless `Sh-` says so.
pub fn from_nano(key: &str) -> Option<String> {
    if let Some(rest) = key.strip_prefix("Sh-M-") {
        return join(
            Mods {
                meta: true,
                shift: true,
                ..Mods::default()
            },
            &rest.to_ascii_lowercase(),
            true,
        );
    }
    if let Some(rest) = key.strip_prefix("M-") {
        let base = if rest.eq_ignore_ascii_case("space") {
            "Space".to_string()
        } else {
            rest.to_ascii_lowercase()
        };
        return join(
            Mods {
                meta: true,
                ..Mods::default()
            },
            &base,
            true,
        );
    }
    if let Some(rest) = key.strip_prefix('^') {
        let base = if rest.eq_ignore_ascii_case("space") {
            "Space".to_string()
        } else {
            rest.to_ascii_lowercase()
        };
        return join(
            Mods {
                ctrl: true,
                ..Mods::default()
            },
            &base,
            true,
        );
    }
    is_function_key(key).then(|| key.to_string())
}

/// A zsh `bindkey` sequence: `^A` → `C-a`, `^[f` → `M-f`, `^[^H` → `C-M-h`.
/// An escape sequence for an arrow or a function key is left out: which key
/// it is depends on the terminal.
pub fn from_zsh(seq: &str) -> Option<String> {
    let (meta, rest) = match seq.strip_prefix("^[") {
        Some(r) if !r.is_empty() => (true, r),
        _ => (false, seq),
    };
    let mut chars = rest.chars();
    let (ctrl, c) = match (chars.next()?, chars.next()) {
        ('^', Some(c)) => (true, c),
        (c, None) => (false, c),
        _ => return None,
    };
    if chars.next().is_some() || (ctrl && !c.is_ascii_alphabetic()) || (meta && c == '[' && !ctrl) {
        return None;
    }
    join(
        Mods {
            ctrl,
            meta,
            shift: false,
        },
        &c.to_string(),
        true,
    )
}

/// A tmux key name folded so two spellings of one key compare equal:
/// `M-S-a` → `M-A`, `C-S-a` → `C-A`. A name with nothing to fold
/// comes back as it went in.
pub fn canonical(key: &str) -> String {
    let parsed = split_mods(key, |m, mods| {
        match m {
            'C' => mods.ctrl = true,
            'M' => mods.meta = true,
            'S' => mods.shift = true,
            _ => return false,
        }
        true
    });
    match parsed {
        Some((mods, base)) => join(mods, base, false).unwrap_or_else(|| key.to_string()),
        None => key.to_string(),
    }
}

/// Whether tmux's root table could be routing on this key: Ctrl, Alt or an F
/// key, and not a mouse event or a `User` key.
pub fn in_scope(key: &str) -> bool {
    if key.contains("Mouse")
        || key.contains("Wheel")
        || key.contains("Click")
        || key.contains("Drag")
        || key.starts_with("User")
    {
        return false;
    }
    let Some((mods, base)) = split_mods(key, |m, mods| {
        match m {
            'C' => mods.ctrl = true,
            'M' => mods.meta = true,
            'S' => mods.shift = true,
            _ => return false,
        }
        true
    }) else {
        return false;
    };
    mods.ctrl || mods.meta || is_function_key(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvim_spellings_become_tmux_ones() {
        let cases = [
            ("<A-a>", Some("M-a")),
            ("<M-a>", Some("M-a")),
            ("<T-a>", Some("M-a")),
            ("<A-S-h>", Some("M-H")),
            ("<A-H>", Some("M-H")),
            ("<M-L>", Some("M-L")),
            ("<C-a>", Some("C-a")),
            ("<C-L>", Some("C-l")),
            ("<c-l>", Some("C-l")),
            ("<C-Tab>", Some("C-Tab")),
            ("<A-CR>", Some("M-Enter")),
            ("<M-cr>", Some("M-Enter")),
            ("<F5>", Some("F5")),
            ("<f12>", Some("F12")),
            ("<S-F5>", Some("S-F5")),
            ("<C-M-a>", Some("C-M-a")),
            ("<M-C-a>", Some("C-M-a")),
            ("<M-1>", Some("M-1")),
            ("<M-;>", Some("M-;")),
            ("<M-->", Some("M--")),
            ("<M->>", Some("M->")),
            ("<M-lt>", Some("M-<")),
            ("<M-Space>", Some("M-Space")),
            ("<C-S-Left>", Some("C-S-Left")),
            ("<C-S-a>", Some("C-A")),
        ];
        for (nvim, tmux) in cases {
            assert_eq!(from_nvim_key(nvim).as_deref(), tmux, "{nvim}");
        }
    }

    #[test]
    fn keys_tmux_root_cannot_route_on_are_none() {
        for nvim in [
            "<CR>", "<Tab>", "<S-Tab>", "<Esc>", "<D-s>", "<Up>", "<Nop>", "g", "",
        ] {
            assert_eq!(from_nvim_key(nvim), None, "{nvim}");
        }
    }

    #[test]
    fn only_the_first_key_of_an_lhs_counts() {
        assert_eq!(first_key("<M-a>x"), "<M-a>");
        assert_eq!(first_key("gx"), "g");
        assert_eq!(first_key("<C-W><C-D>"), "<C-W>");
        assert_eq!(first_key("<M->>x"), "<M->>");
        assert_eq!(first_key("<"), "<");
        assert_eq!(from_nvim_lhs("<M-a>x").as_deref(), Some("M-a"));
        assert_eq!(from_nvim_lhs("<C-W><C-D>").as_deref(), Some("C-w"));
        assert_eq!(
            from_nvim_lhs(" ff"),
            None,
            "a leader map starts with a space"
        );
    }

    #[test]
    fn fzf_claude_nano_and_zsh_spellings() {
        assert_eq!(from_fzf("alt-a").as_deref(), Some("M-a"));
        assert_eq!(from_fzf("ctrl-alt-x").as_deref(), Some("C-M-x"));
        assert_eq!(from_fzf("alt-bs").as_deref(), Some("M-BSpace"));
        assert_eq!(from_fzf("alt-up").as_deref(), Some("M-Up"));
        assert_eq!(from_fzf("ctrl-/").as_deref(), Some("C-/"));
        assert_eq!(from_fzf("page-up"), None);
        assert_eq!(from_fzf("start"), None);
        assert_eq!(from_claude("meta+p").as_deref(), Some("M-p"));
        assert_eq!(from_claude("ctrl+shift+b").as_deref(), Some("C-B"));
        assert_eq!(from_claude("ctrl+-").as_deref(), Some("C--"));
        assert_eq!(from_claude("ctrl+enter").as_deref(), Some("C-Enter"));
        assert_eq!(from_claude("cmd+k"), None);
        assert_eq!(from_claude("shift+tab"), None);
        assert_eq!(from_nano("^X").as_deref(), Some("C-x"));
        assert_eq!(from_nano("M-Q").as_deref(), Some("M-q"));
        assert_eq!(from_nano("Sh-M-c").as_deref(), Some("M-C"));
        assert_eq!(from_nano("F12").as_deref(), Some("F12"));
        assert_eq!(from_nano("^Space").as_deref(), Some("C-Space"));
        assert_eq!(from_zsh("^A").as_deref(), Some("C-a"));
        assert_eq!(from_zsh("^[f").as_deref(), Some("M-f"));
        assert_eq!(from_zsh("^[^H").as_deref(), Some("C-M-h"));
        assert_eq!(from_zsh("^[[A"), None);
        assert_eq!(from_zsh("^?"), None);
        assert_eq!(from_zsh("a"), None);
    }

    #[test]
    fn canonical_folds_the_pairs_tmux_keeps_apart() {
        assert_eq!(canonical("M-S-a"), "M-A");
        assert_eq!(canonical("M-A"), "M-A");
        assert_eq!(canonical("C-A"), "C-A", "ctrl-shift-a, apart from C-a");
        assert_eq!(canonical("C-S-a"), "C-A");
        assert_eq!(canonical("C-a"), "C-a");
        assert_eq!(canonical("C-M-S-Left"), "C-M-S-Left");
        assert_eq!(canonical("S-F5"), "S-F5");
        assert_eq!(canonical("M-;"), "M-;");
        assert_eq!(canonical("Enter"), "Enter");
    }

    #[test]
    fn scope_is_ctrl_alt_and_function_keys() {
        for k in ["M-a", "C-a", "F5", "S-F5", "C-Tab", "M-Enter", "C-M-S-Left"] {
            assert!(in_scope(k), "{k}");
        }
        for k in [
            "Enter",
            "a",
            "S-Left",
            "M-MouseDown3Pane",
            "C-MouseDown1Pane",
            "User0",
            "WheelUpPane",
        ] {
            assert!(!in_scope(k), "{k}");
        }
    }
}
