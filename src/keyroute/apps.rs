//! The config rung for the apps with no live interface: claude, fzf, vim,
//! nano and zsh. Each reads the app's own configuration the way the app does,
//! over a table of the app's defaults where those can't be read from outside.
//!
//! The parsers are pure and tested; [`super::discover`] does the reading.
//!
//! The default tables are copied from the app's own documentation, and only
//! the keys in scope are kept, with the first keystroke of a chord standing
//! for it, since that is the key tmux would see first:
//!
//! - claude: the action table of Claude Code's keybindings reference, in the
//!   contexts where claude is usually sitting (Global, Chat, Task).
//! - fzf: `man fzf`, AVAILABLE ACTIONS, as fzf 0.74.3 prints it.
//!
//! nano's defaults are not shipped: nothing installed lists them, and a
//! table written from memory would be a guess. nanorc `bind` lines and
//! `[keys.app.nano] claims` are what counts for nano.

use super::discover::{Row, Rung};
use super::spell;

/// The claude version the default table was last checked against.
pub const CLAUDE_CHECKED: &str = "Claude Code keybindings reference, 2026-10-01";

/// claude's defaults in scope, as (context, key in claude's spelling, action).
pub const CLAUDE_DEFAULTS: &[(&str, &str, &str)] = &[
    ("Global", "ctrl+c", "app:interrupt"),
    ("Global", "ctrl+d", "app:exit"),
    ("Global", "ctrl+t", "app:toggleTodos"),
    ("Global", "ctrl+o", "app:toggleTranscript"),
    ("Global", "ctrl+shift+b", "app:toggleBrief"),
    ("Global", "ctrl+up", "app:diffFileListUp"),
    ("Global", "meta+up", "app:diffFileListUp"),
    ("Global", "ctrl+down", "app:diffFileListDown"),
    ("Global", "meta+down", "app:diffFileListDown"),
    ("Global", "ctrl+]", "app:openArtifact"),
    ("Global", "ctrl+r", "history:search"),
    (
        "Chat",
        "ctrl+x",
        "chat: ctrl+x chords (kill agents, queue, send now, editor)",
    ),
    ("Chat", "meta+p", "chat:modelPicker"),
    ("Chat", "meta+o", "chat:fastMode"),
    ("Chat", "ctrl+y", "chat:defaultToNewerModel"),
    ("Chat", "meta+t", "chat:thinkingToggle"),
    ("Chat", "meta+w", "chat:workflowKeywordToggle"),
    ("Chat", "ctrl+enter", "chat:sendNow"),
    ("Chat", "ctrl+j", "chat:newline"),
    ("Chat", "ctrl+_", "chat:undo"),
    ("Chat", "ctrl+-", "chat:undo"),
    ("Chat", "ctrl+g", "chat:externalEditor"),
    ("Chat", "ctrl+s", "chat:stash"),
    ("Chat", "ctrl+v", "chat:imagePaste"),
    ("Chat", "ctrl+l", "chat:clearInput"),
    ("Task", "ctrl+b", "task:background"),
];

/// fzf's defaults in scope, as (key in fzf's spelling, action).
pub const FZF_DEFAULTS: &[(&str, &str)] = &[
    ("ctrl-a", "beginning-of-line"),
    ("ctrl-b", "backward-char"),
    ("ctrl-c", "abort"),
    ("ctrl-d", "delete-char/eof"),
    ("ctrl-e", "end-of-line"),
    ("ctrl-f", "forward-char"),
    ("ctrl-g", "abort"),
    ("ctrl-h", "backward-delete-char"),
    ("ctrl-i", "toggle+down"),
    ("ctrl-j", "down"),
    ("ctrl-k", "up"),
    ("ctrl-l", "clear-screen"),
    ("ctrl-n", "down-match"),
    ("ctrl-p", "up-match"),
    ("ctrl-q", "abort"),
    ("ctrl-u", "unix-line-discard"),
    ("ctrl-w", "unix-word-rubout"),
    ("ctrl-y", "yank"),
    ("ctrl-/", "toggle-wrap-word"),
    ("alt-/", "toggle-wrap-word"),
    ("alt-b", "backward-word"),
    ("alt-f", "forward-word"),
    ("alt-d", "kill-word"),
    ("alt-bs", "backward-kill-word"),
    ("alt-left", "backward-word"),
    ("alt-right", "forward-word"),
    ("alt-up", "up-match"),
    ("alt-down", "down-match"),
];

fn row(layer: &str, mode: &str, key: String, desc: &str, source: &str) -> Row {
    Row {
        layer: layer.into(),
        mode: mode.into(),
        key,
        desc: desc.into(),
        source: source.into(),
        rung: Rung::Config,
        pane: String::new(),
    }
}

// claude -------------------------------------------------------------------

/// claude's defaults with `keybindings.json` laid over them: a key the file
/// binds replaces the default in that context, and `null` removes it.
pub fn claude_rows(keybindings: Option<&str>) -> anyhow::Result<Vec<Row>> {
    let mut table: std::collections::BTreeMap<(String, String), Option<String>> = CLAUDE_DEFAULTS
        .iter()
        .map(|(ctx, key, action)| ((ctx.to_string(), key.to_string()), Some(action.to_string())))
        .collect();
    if let Some(text) = keybindings {
        let v: serde_json::Value =
            serde_json::from_str(text).map_err(|e| anyhow::anyhow!("keybindings.json: {e}"))?;
        for block in v
            .get("bindings")
            .and_then(|b| b.as_array())
            .into_iter()
            .flatten()
        {
            let Some(ctx) = block.get("context").and_then(|c| c.as_str()) else {
                continue;
            };
            for (key, action) in block
                .get("bindings")
                .and_then(|b| b.as_object())
                .into_iter()
                .flatten()
            {
                let first = key.split_whitespace().next().unwrap_or("").to_lowercase();
                table.insert(
                    (ctx.to_string(), first),
                    action.as_str().map(str::to_string),
                );
            }
        }
    }
    Ok(table
        .into_iter()
        .filter_map(|((ctx, key), action)| {
            let action = action?;
            Some(row(
                "claude",
                &ctx.to_lowercase(),
                spell::from_claude(&key)?,
                &action,
                &key,
            ))
        })
        .collect())
}

// fzf ----------------------------------------------------------------------

/// fzf's defaults with every `--bind` in `opts` laid over them.
pub fn fzf_rows(opts: &str) -> Vec<Row> {
    let mut table: std::collections::BTreeMap<String, String> = FZF_DEFAULTS
        .iter()
        .map(|(k, a)| (k.to_string(), a.to_string()))
        .collect();
    for (key, action) in fzf_binds(opts) {
        table.insert(key, action);
    }
    table
        .into_iter()
        .filter_map(|(key, action)| Some(row("fzf", "", spell::from_fzf(&key)?, &action, &key)))
        .collect()
}

/// The `key:action` pairs of every `--bind` in an options string.
pub fn fzf_binds(opts: &str) -> Vec<(String, String)> {
    let words = shell_words(opts);
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let value = if let Some(v) = words[i].strip_prefix("--bind=") {
            Some(v.to_string())
        } else if words[i] == "--bind" {
            i += 1;
            words.get(i).cloned()
        } else {
            None
        };
        for entry in value.as_deref().map(split_top_commas).unwrap_or_default() {
            if let Some((key, action)) = entry.split_once(':') {
                out.push((key.trim().to_string(), action.trim().to_string()));
            }
        }
        i += 1;
    }
    out
}

/// Split on commas outside brackets: `execute(a, b)` is one action.
fn split_top_commas(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in text.chars() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Split a string the way a shell splits words, quotes and backslashes only.
fn shell_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

// vim ----------------------------------------------------------------------

/// `:map` and `:map!` output, as vim prints it: a mode column, the lhs, flags
/// and the rhs.
pub fn vim_rows(listing: &str) -> Vec<Row> {
    let mut out = Vec::new();
    for line in listing.lines() {
        if line.len() < 4 || line.trim().is_empty() {
            continue;
        }
        let (mode_col, rest) = line.split_at(3);
        let mut words = rest.split_whitespace();
        let Some(lhs) = words.next() else {
            continue;
        };
        let rhs = words
            .skip_while(|w| matches!(*w, "*" | "&" | "@" | "*@" | "&@"))
            .collect::<Vec<_>>()
            .join(" ");
        let Some(key) = spell::from_nvim_lhs(lhs) else {
            continue;
        };
        let modes: &[&str] = match mode_col.trim() {
            "" => &["n", "x", "o"],
            "v" => &["x", "s"],
            "!" => &["i", "c"],
            "l" => &["i"],
            m => &[m][..],
        };
        for m in modes {
            out.push(row("vim", m, key.clone(), "", &rhs));
        }
    }
    out
}

// nano ---------------------------------------------------------------------

/// `bind` lines of a nanorc. `unbind` takes a key away again.
pub fn nano_rows(nanorc: &str) -> Vec<Row> {
    let mut table: std::collections::BTreeMap<(String, String), Option<String>> =
        Default::default();
    for line in nanorc.lines() {
        let mut w = line.split_whitespace();
        match (w.next(), w.next(), w.next()) {
            (Some("bind"), Some(key), Some(function)) => {
                let menu = w.next().unwrap_or("all").to_string();
                table.insert((key.to_string(), menu), Some(function.to_string()));
            }
            (Some("unbind"), Some(key), menu) => {
                table.insert((key.to_string(), menu.unwrap_or("all").to_string()), None);
            }
            _ => {}
        }
    }
    table
        .into_iter()
        .filter_map(|((key, menu), function)| {
            Some(row(
                "nano",
                &menu,
                spell::from_nano(&key)?,
                &function?,
                &key,
            ))
        })
        .collect()
}

// zsh ----------------------------------------------------------------------

/// `bindkey -L` output: `bindkey [-M keymap] "^A" widget`. Ranges (`-R`) are
/// self-insert and never in scope.
pub fn zsh_rows(listing: &str, default_keymap: &str) -> Vec<Row> {
    let mut out = Vec::new();
    for line in listing.lines() {
        let words = shell_words(line);
        if words.first().map(String::as_str) != Some("bindkey") || words.iter().any(|w| w == "-R") {
            continue;
        }
        let (keymap, rest) = match words.get(1).map(String::as_str) {
            Some("-M") => (
                words.get(2).cloned().unwrap_or_default(),
                &words[3.min(words.len())..],
            ),
            _ => (default_keymap.to_string(), &words[1..]),
        };
        let (Some(seq), Some(widget)) = (rest.first(), rest.get(1)) else {
            continue;
        };
        if let Some(key) = spell::from_zsh(seq) {
            out.push(row("zsh", &keymap, key, widget, seq));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(rows: &[Row]) -> Vec<(String, String)> {
        rows.iter()
            .map(|r| (r.mode.clone(), r.key.clone()))
            .collect()
    }

    #[test]
    fn claude_defaults_are_in_tmux_spelling() {
        let rows = claude_rows(None).unwrap();
        let has = |mode: &str, key: &str| rows.iter().any(|r| r.mode == mode && r.key == key);
        assert!(has("chat", "M-p"));
        assert!(has("chat", "C-g"));
        assert!(has("global", "C-B"), "ctrl+shift+b");
        assert!(has("global", "M-Up"));
        assert!(has("task", "C-b"));
        assert!(has("chat", "C-Enter"));
    }

    #[test]
    fn keybindings_json_rebinds_and_unbinds_over_the_defaults() {
        let json = r#"{"bindings": [{"context": "Chat", "bindings": {
            "ctrl+g": null, "ctrl+e": "chat:externalEditor", "meta+k ctrl+s": "chat:stash", "enter": "chat:submit"}}]}"#;
        let rows = claude_rows(Some(json)).unwrap();
        let chat: Vec<&str> = rows
            .iter()
            .filter(|r| r.mode == "chat")
            .map(|r| r.key.as_str())
            .collect();
        assert!(!chat.contains(&"C-g"), "null removes the default");
        assert!(chat.contains(&"C-e"));
        assert!(chat.contains(&"M-k"), "a chord counts by its first key");
        assert!(!chat.contains(&"Enter"), "out of scope");
        assert!(claude_rows(Some("{oops")).is_err());
    }

    #[test]
    fn fzf_opts_lay_binds_over_the_defaults() {
        let opts = r#"--height 40% --bind 'alt-a:select-all,ctrl-y:execute(echo {} | pbcopy, x)' --bind=ctrl-a:toggle-all,start:reload(ls)"#;
        let rows = fzf_rows(opts);
        let get = |k: &str| rows.iter().find(|r| r.key == k).map(|r| r.desc.clone());
        assert_eq!(get("M-a").as_deref(), Some("select-all"));
        assert_eq!(
            get("C-y").as_deref(),
            Some("execute(echo {} | pbcopy, x)"),
            "a comma in brackets is one action"
        );
        assert_eq!(
            get("C-a").as_deref(),
            Some("toggle-all"),
            "the bind wins over the default"
        );
        assert_eq!(get("M-b").as_deref(), Some("backward-word"));
        assert!(
            rows.iter().all(|r| r.source != "start"),
            "events are not keys"
        );
    }

    #[test]
    fn vim_map_listings_become_rows_per_mode() {
        let listing = "\nn  <M-a>       * :echo<CR>\n   <C-K>         <C-W>k\nv  <D-c>         \"*y\n!  <M-b>         x\n";
        assert_eq!(
            keys(&vim_rows(listing)),
            [
                ("n".into(), "M-a".into()),
                ("n".into(), "C-k".into()),
                ("x".into(), "C-k".into()),
                ("o".into(), "C-k".into()),
                ("i".into(), "M-b".into()),
                ("c".into(), "M-b".into()),
            ]
        );
        assert_eq!(vim_rows(listing)[0].source, ":echo<CR>");
    }

    #[test]
    fn nanorc_binds_and_unbinds() {
        let rc = "set linenumbers\nbind ^X exit main\nbind M-q exit all\nbind Sh-M-c copy main\nbind F5 help all\nunbind ^X main\nbind ^S savefile main\n";
        let got = keys(&nano_rows(rc));
        assert!(got.contains(&("all".into(), "M-q".into())));
        assert!(got.contains(&("main".into(), "M-C".into())));
        assert!(got.contains(&("all".into(), "F5".into())));
        assert!(got.contains(&("main".into(), "C-s".into())));
        assert!(!got.contains(&("main".into(), "C-x".into())), "unbound");
    }

    #[test]
    fn zsh_bindkey_listings_keep_ctrl_and_meta() {
        let listing = "bindkey \"^A\" beginning-of-line\nbindkey \"^[f\" forward-word\nbindkey -M vicmd \"^R\" redo\nbindkey \"^[[A\" up-line\nbindkey -R \"!\"-\"~\" self-insert\nbindkey \"^[^H\" backward-kill-word\n";
        let got = keys(&zsh_rows(listing, "emacs"));
        assert_eq!(
            got,
            [
                ("emacs".into(), "C-a".into()),
                ("emacs".into(), "M-f".into()),
                ("vicmd".into(), "C-r".into()),
                ("emacs".into(), "C-M-h".into()),
            ]
        );
    }
}
