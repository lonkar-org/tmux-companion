//! Open a URL or a file reference found in text.
//!
//! Driven from tmux copy mode: a selection, a line, or the token under the
//! cursor. URLs go to a browser, files go to an editor, and a file token may
//! carry `:line` or `:line:col` the way compiler and grep output does.
//!
//! Nearly all of this is extraction, which is why nearly all of it is pure. The
//! only impure parts are asking the filesystem whether a path exists and
//! handing the answer to something else.

/// What was found in a piece of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A URL, to open in a browser.
    Url(String),
    /// A file, with the line and column when the token carried them.
    File {
        /// The resolved path.
        path: std::path::PathBuf,
        /// One-based line, or zero when the token had none.
        line: u32,
        /// One-based column, or zero.
        column: u32,
    },
}

/// Strip the punctuation a token picks up from prose, diffs and logs.
///
/// A leading bracket or quote, then any number of trailing ones, then a
/// trailing dot or colon: `(src/main.rs:12)` and `see src/main.rs.` both name
/// a file, and neither is spelled the way the filesystem spells it.
pub fn clean_token(token: &str) -> &str {
    const LEAD: [char; 6] = ['(', '[', '{', '<', '"', '\''];
    const TRAIL: [char; 9] = [')', ']', '}', '>', '"', '\'', '`', ',', ';'];

    let mut t = token;
    if let Some(first) = t.chars().next()
        && LEAD.contains(&first)
    {
        t = &t[first.len_utf8()..];
    }
    while let Some(last) = t.chars().last() {
        if !TRAIL.contains(&last) && last != '.' && last != ':' {
            break;
        }
        t = &t[..t.len() - last.len_utf8()];
    }
    t
}

/// Split a token into a path and its `:line:col` suffix, if it has one.
pub fn split_position(token: &str) -> (&str, u32, u32) {
    let parts: Vec<&str> = token.rsplitn(3, ':').collect();
    match parts.as_slice() {
        // rsplitn gives them backwards: col, line, rest.
        [col, line, rest] => match (col.parse::<u32>(), line.parse::<u32>()) {
            (Ok(c), Ok(l)) if !rest.is_empty() => {
                (&token[..token.len() - col.len() - line.len() - 2], l, c)
            }
            _ => split_line_only(token),
        },
        _ => split_line_only(token),
    }
}

/// The `:line` case, for a token with only one number on the end.
fn split_line_only(token: &str) -> (&str, u32, u32) {
    match token.rsplit_once(':') {
        Some((rest, line)) if !rest.is_empty() => match line.parse::<u32>() {
            Ok(l) => (rest, l, 0),
            Err(_) => (token, 0, 0),
        },
        _ => (token, 0, 0),
    }
}

/// The first URL in a piece of text.
///
/// The character class excludes whitespace and the brackets and quotes prose
/// wraps a link in, which is what stops a trailing `)` becoming part of the
/// address.
pub fn find_url(text: &str) -> Option<String> {
    let start = text.find("http://").or_else(|| text.find("https://"))?;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '<' | '>' | '"' | '\'' | '(' | ')' | '{' | '}' | '[' | ']'
                )
        })
        .unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(['.', ',', ';', ':']);
    (url.len() > "https://".len()).then(|| url.to_string())
}

/// Expand `~` and resolve a path against a base directory.
pub fn absolute(path: &str, base: &std::path::Path, home: &str) -> std::path::PathBuf {
    let expanded = match path.strip_prefix("~/") {
        Some(rest) => format!("{home}/{rest}"),
        None if path == "~" => home.to_string(),
        None => path.to_string(),
    };
    let p = std::path::PathBuf::from(&expanded);
    if p.is_absolute() { p } else { base.join(p) }
}

/// The candidate paths a token could mean, in the order to try them.
///
/// The `a/` and `b/` prefixes are git's, and a path copied out of a diff has
/// one on it. Trying the bare form second means `a/src/main.rs` finds
/// `src/main.rs` without a diff-shaped path ever being preferred over a real
/// directory called `a`.
pub fn candidates(token: &str) -> Vec<&str> {
    let mut out = vec![token];
    if let Some(rest) = token
        .strip_prefix("a/")
        .or_else(|| token.strip_prefix("b/"))
    {
        out.push(rest);
    }
    out
}

/// Break a token on the characters that glue a path to something else.
///
/// `Update(some/path.zsh)` and `key=some/path:12` both contain a path that a
/// whitespace split leaves welded to its neighbour.
pub fn fragments(token: &str) -> Vec<&str> {
    token
        .split([
            '(', ')', '[', ']', '{', '}', '<', '>', '"', '\'', '=', ',', ';',
        ])
        .filter(|f| !f.is_empty())
        .collect()
}

/// Find something openable in a piece of text.
///
/// A URL first, because a line holding both is almost always about the link.
/// Then each whitespace-separated token, cleaned, tried whole and then broken
/// into fragments. `exists` is passed in so the whole search is testable
/// without touching a filesystem.
pub fn scan(
    text: &str,
    base: &std::path::Path,
    home: &str,
    exists: &dyn Fn(&std::path::Path) -> bool,
) -> Option<Target> {
    scan_at(text, None, base, home, exists)
}

/// Where each whitespace-separated word sits, in columns.
///
/// Columns and not bytes, because the number this is compared against comes
/// from tmux's `#{copy_cursor_x}`, which counts cells on the screen.
fn word_spans(text: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<(usize, usize)> = None;
    for (col, (byte, ch)) in text.char_indices().enumerate() {
        if ch.is_whitespace() {
            if let Some((c0, b0)) = start.take() {
                out.push((c0, col, &text[b0..byte]));
            }
        } else if start.is_none() {
            start = Some((col, byte));
        }
    }
    if let Some((c0, b0)) = start {
        out.push((c0, text.chars().count(), &text[b0..]));
    }
    out
}

/// How far a word is from the cursor: zero when the cursor is inside it.
fn distance_from(span: (usize, usize), cursor: usize) -> usize {
    let (start, end) = span;
    if cursor >= start && cursor < end {
        0
    } else if cursor < start {
        start - cursor
    } else {
        cursor + 1 - end
    }
}

/// Scan text, preferring whatever sits under a cursor column.
///
/// This is what the copy-mode binding uses. Selecting the path first is work
/// nobody should have to do: the cursor is already on the thing, and a line
/// like `see src/a.rs:2 and src/b.rs:9` has two right answers where only the
/// one under the cursor is the one meant. With no cursor the order is the
/// line's own, which is what every other caller wants.
pub fn scan_at(
    text: &str,
    cursor: Option<usize>,
    base: &std::path::Path,
    home: &str,
    exists: &dyn Fn(&std::path::Path) -> bool,
) -> Option<Target> {
    let mut words = word_spans(text);
    if let Some(cursor) = cursor {
        words.sort_by_key(|(start, end, _)| distance_from((*start, *end), cursor));
        // Nearest first, and a URL is only preferred over a file when it is
        // the nearer of the two, so a cursor sitting on a path does not open a
        // browser because the line also mentioned a link.
        for (_, _, raw) in &words {
            if let Some(url) = find_url(raw) {
                return Some(Target::Url(url));
            }
            if let Some(target) = file_in(raw, base, home, exists) {
                return Some(target);
            }
        }
        return None;
    }
    if let Some(url) = find_url(text) {
        return Some(Target::Url(url));
    }
    for raw in text.split_whitespace() {
        if let Some(target) = file_in(raw, base, home, exists) {
            return Some(target);
        }
    }
    None
}

/// The file one word names, if it names one that exists.
fn file_in(
    raw: &str,
    base: &std::path::Path,
    home: &str,
    exists: &dyn Fn(&std::path::Path) -> bool,
) -> Option<Target> {
    let token = clean_token(raw);
    if token.is_empty() {
        return None;
    }
    let mut tries: Vec<&str> = vec![token];
    tries.extend(fragments(token).into_iter().filter(|f| *f != token));
    for candidate in tries {
        let (path, line, column) = split_position(clean_token(candidate));
        for form in candidates(path) {
            let abs = absolute(form, base, home);
            if exists(&abs) {
                return Some(Target::File {
                    path: abs,
                    line,
                    column,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// A filesystem that knows about exactly these paths.
    fn knows(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| paths.iter().any(|k| p == Path::new(k))
    }

    #[test]
    fn a_url_is_found_in_prose() {
        assert_eq!(
            find_url("see https://example.com/a for more"),
            Some("https://example.com/a".to_string())
        );
    }

    #[test]
    fn a_wrapped_url_does_not_keep_its_bracket() {
        assert_eq!(
            find_url("(https://example.com/a)"),
            Some("https://example.com/a".to_string())
        );
        assert_eq!(
            find_url("see <https://example.com/a>."),
            Some("https://example.com/a".to_string())
        );
    }

    #[test]
    fn a_trailing_full_stop_is_not_part_of_the_address() {
        assert_eq!(
            find_url("at https://example.com/a."),
            Some("https://example.com/a".to_string())
        );
    }

    #[test]
    fn text_with_no_link_has_no_url() {
        assert_eq!(find_url("nothing here"), None);
        assert_eq!(find_url("ftp://example.com"), None);
    }

    #[test]
    fn a_token_loses_the_punctuation_prose_put_on_it() {
        assert_eq!(clean_token("(src/main.rs)"), "src/main.rs");
        assert_eq!(clean_token("'src/main.rs',"), "src/main.rs");
        assert_eq!(clean_token("src/main.rs."), "src/main.rs");
        assert_eq!(clean_token("src/main.rs:"), "src/main.rs");
    }

    #[test]
    fn a_clean_token_is_left_alone() {
        assert_eq!(clean_token("src/main.rs"), "src/main.rs");
        assert_eq!(clean_token(""), "");
    }

    #[test]
    fn a_line_and_column_are_split_off() {
        assert_eq!(split_position("src/main.rs:12:5"), ("src/main.rs", 12, 5));
        assert_eq!(split_position("src/main.rs:12"), ("src/main.rs", 12, 0));
        assert_eq!(split_position("src/main.rs"), ("src/main.rs", 0, 0));
    }

    #[test]
    fn something_that_is_not_a_number_is_part_of_the_path() {
        // A windows-style path or a URL fragment is not a line number.
        assert_eq!(split_position("src/main.rs:abc"), ("src/main.rs:abc", 0, 0));
        assert_eq!(split_position("C:/tmp/file"), ("C:/tmp/file", 0, 0));
    }

    #[test]
    fn a_tilde_is_the_home_directory() {
        assert_eq!(
            absolute("~/notes.txt", Path::new("/tmp"), "/home/me"),
            PathBuf::from("/home/me/notes.txt")
        );
    }

    #[test]
    fn a_relative_path_resolves_against_the_pane() {
        assert_eq!(
            absolute("src/main.rs", Path::new("/work/repo"), "/home/me"),
            PathBuf::from("/work/repo/src/main.rs")
        );
    }

    #[test]
    fn an_absolute_path_is_left_alone() {
        assert_eq!(
            absolute("/etc/hosts", Path::new("/work"), "/home/me"),
            PathBuf::from("/etc/hosts")
        );
    }

    #[test]
    fn a_diff_path_is_tried_with_and_without_its_prefix() {
        assert_eq!(
            candidates("a/src/main.rs"),
            vec!["a/src/main.rs", "src/main.rs"]
        );
        assert_eq!(candidates("src/main.rs"), vec!["src/main.rs"]);
    }

    #[test]
    fn a_path_welded_to_its_neighbour_is_still_found() {
        let fs = knows(&["/repo/some/path.zsh"]);
        let found = scan("Update(some/path.zsh)", Path::new("/repo"), "/home/me", &fs);
        assert_eq!(
            found,
            Some(Target::File {
                path: PathBuf::from("/repo/some/path.zsh"),
                line: 0,
                column: 0
            })
        );
    }

    #[test]
    fn a_key_equals_path_with_a_line_number_is_found() {
        let fs = knows(&["/repo/some/path"]);
        let found = scan("key=some/path:12", Path::new("/repo"), "/home/me", &fs);
        assert_eq!(
            found,
            Some(Target::File {
                path: PathBuf::from("/repo/some/path"),
                line: 12,
                column: 0
            })
        );
    }

    #[test]
    fn compiler_output_gives_a_line_and_a_column() {
        let fs = knows(&["/repo/src/main.rs"]);
        let found = scan(
            "error at src/main.rs:42:9: something",
            Path::new("/repo"),
            "/home/me",
            &fs,
        );
        assert_eq!(
            found,
            Some(Target::File {
                path: PathBuf::from("/repo/src/main.rs"),
                line: 42,
                column: 9
            })
        );
    }

    #[test]
    fn a_url_wins_over_a_file_on_the_same_line() {
        // A line holding both is almost always about the link.
        let fs = knows(&["/repo/README.md"]);
        let found = scan(
            "README.md says https://example.com/a",
            Path::new("/repo"),
            "/home/me",
            &fs,
        );
        assert_eq!(
            found,
            Some(Target::Url("https://example.com/a".to_string()))
        );
    }

    #[test]
    fn a_path_that_is_not_there_is_not_a_target() {
        let fs = knows(&[]);
        assert_eq!(
            scan("src/nope.rs", Path::new("/repo"), "/home/me", &fs),
            None
        );
    }

    #[test]
    fn a_git_diff_path_resolves_without_its_prefix() {
        let fs = knows(&["/repo/src/main.rs"]);
        let found = scan("--- a/src/main.rs", Path::new("/repo"), "/home/me", &fs);
        assert_eq!(
            found,
            Some(Target::File {
                path: PathBuf::from("/repo/src/main.rs"),
                line: 0,
                column: 0
            })
        );
    }

    #[test]
    fn empty_text_finds_nothing_rather_than_panicking() {
        let fs = knows(&[]);
        assert_eq!(scan("", Path::new("/repo"), "/home/me", &fs), None);
        assert_eq!(scan("   \t  ", Path::new("/repo"), "/home/me", &fs), None);
    }
}

/// Quote one argument for the shell tmux runs a split's command through.
///
/// Single quotes, with any single quote inside closed, escaped and reopened,
/// which is the only form that needs no other escaping.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The command that opens an editor on a file, at a line and column.
///
/// `{path}`, `{line}` and `{column}` in the template are replaced; the path is
/// quoted for the shell and the numbers are not, because they are numbers.
///
/// This exists because the command was built as
/// `nvim +call cursor(2,22) src/main.rs` and handed to `split-window`, which
/// runs it through `sh`. Unquoted parentheses are a syntax error there, so the
/// pane opened, the shell complained to a pane nobody was looking at, and it
/// closed again. Opening a file at a line had never worked.
pub fn editor_command(template: &str, path: &str, line: usize, column: usize) -> String {
    template
        .replace("{path}", &shell_quote(path))
        .replace("{line}", &line.max(1).to_string())
        .replace("{column}", &column.max(1).to_string())
}

#[cfg(test)]
mod editor_command_tests {
    use super::*;

    #[test]
    fn the_cursor_call_survives_a_shell() {
        let cmd = editor_command(
            "nvim '+call cursor({line},{column})' {path}",
            "src/main.rs",
            2,
            22,
        );
        assert_eq!(cmd, "nvim '+call cursor(2,22)' 'src/main.rs'");

        // The whole point: `sh -c` has to accept it.
        let status = std::process::Command::new("sh")
            .arg("-n")
            .arg("-c")
            .arg(&cmd)
            .status()
            .expect("sh runs");
        assert!(status.success(), "sh cannot parse: {cmd}");
    }

    #[test]
    fn a_path_with_a_space_or_a_quote_is_still_one_argument() {
        let cmd = editor_command("nvim {path}", "/tmp/my notes/it's here.md", 1, 1);
        assert_eq!(cmd, "nvim '/tmp/my notes/it'\\''s here.md'");
        let status = std::process::Command::new("sh")
            .arg("-n")
            .arg("-c")
            .arg(&cmd)
            .status()
            .expect("sh runs");
        assert!(status.success(), "sh cannot parse: {cmd}");
    }

    #[test]
    fn a_file_with_no_line_opens_at_the_top() {
        // `open` reports 0 for "no line was in the text", and no editor takes
        // line zero.
        let cmd = editor_command("nvim '+call cursor({line},{column})' {path}", "a.rs", 0, 0);
        assert_eq!(cmd, "nvim '+call cursor(1,1)' 'a.rs'");
    }
}

/// The `split-window` argument vector that opens the editor.
///
/// `target` is the pane to split, which is `$TMUX_PANE` when there is one.
/// Leaving it out is what the first version did, and tmux then splits the
/// current pane of the most recently used session rather than the pane the
/// command was run in. With one session nobody notices; with a second session
/// touched more recently the editor opens in a window the person is not
/// looking at, having reported success.
pub fn split_window_args(
    config: &crate::config::Open,
    command: String,
    target: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["split-window".into(), config.split.flag().to_string()];
    if let Some(pane) = target {
        args.push("-t".into());
        args.push(pane.to_string());
    }
    if config.size_percent > 0 {
        args.push("-l".into());
        args.push(format!("{}%", config.size_percent.min(95)));
    }
    args.push(command);
    args
}

#[cfg(test)]
mod scan_at_tests {
    use super::*;
    use std::path::Path;

    fn exists(names: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| names.iter().any(|n| p.to_string_lossy().ends_with(n))
    }

    fn file_name(t: Option<Target>) -> String {
        match t {
            Some(Target::File { path, .. }) => path.display().to_string(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn words_are_measured_in_columns() {
        assert_eq!(
            word_spans("ab  cde f"),
            vec![(0, 2, "ab"), (4, 7, "cde"), (8, 9, "f")]
        );
    }

    #[test]
    fn a_word_at_the_end_is_not_lost() {
        assert_eq!(word_spans("one two"), vec![(0, 3, "one"), (4, 7, "two")]);
    }

    #[test]
    fn a_cursor_inside_a_word_is_no_distance_from_it() {
        assert_eq!(distance_from((4, 7), 4), 0);
        assert_eq!(distance_from((4, 7), 6), 0);
        assert_eq!(distance_from((4, 7), 7), 1);
        assert_eq!(distance_from((4, 7), 2), 2);
    }

    #[test]
    fn the_cursor_picks_between_two_paths_on_one_line() {
        // The reason this exists. Both paths are real and the line's own order
        // would always answer with the first, which is wrong half the time.
        let text = "see src/a.rs:2 and src/b.rs:9";
        let e = exists(&["src/a.rs", "src/b.rs"]);
        let base = Path::new("/repo");
        assert!(file_name(scan_at(text, Some(5), base, "/home/y", &e)).ends_with("src/a.rs"));
        assert!(file_name(scan_at(text, Some(20), base, "/home/y", &e)).ends_with("src/b.rs"));
    }

    #[test]
    fn a_cursor_on_a_path_does_not_open_the_line_s_url() {
        // Without the cursor, find_url runs over the whole line first and wins.
        // With one, the nearest word decides, so sitting on a path opens an
        // editor rather than a browser.
        let text = "https://example.com src/a.rs:2";
        let e = exists(&["src/a.rs"]);
        let base = Path::new("/repo");
        assert!(file_name(scan_at(text, Some(22), base, "/home/y", &e)).ends_with("src/a.rs"));
        assert!(matches!(
            scan_at(text, Some(2), base, "/home/y", &e),
            Some(Target::Url(_))
        ));
    }

    #[test]
    fn the_nearest_word_wins_when_the_cursor_is_on_whitespace() {
        let text = "src/a.rs:2   plain";
        let e = exists(&["src/a.rs"]);
        assert!(
            file_name(scan_at(text, Some(11), Path::new("/repo"), "/home/y", &e))
                .ends_with("src/a.rs")
        );
    }

    #[test]
    fn no_cursor_keeps_the_old_order() {
        // Every other caller passes None and must see exactly what it saw
        // before this existed: the first URL on the line, whatever else is on
        // it.
        let text = "src/a.rs:2 https://example.com";
        let e = exists(&["src/a.rs"]);
        assert!(matches!(
            scan_at(text, None, Path::new("/repo"), "/home/y", &e),
            Some(Target::Url(_))
        ));
    }

    #[test]
    fn a_line_with_nothing_on_it_finds_nothing() {
        let e = exists(&[]);
        assert!(
            scan_at(
                "just some words",
                Some(3),
                Path::new("/repo"),
                "/home/y",
                &e
            )
            .is_none()
        );
        assert!(scan_at("", Some(0), Path::new("/repo"), "/home/y", &e).is_none());
    }

    #[test]
    fn the_cursor_may_sit_past_the_end_of_the_line() {
        // tmux reports the cursor where it is, and in copy mode that can be
        // one past the last character of a short line.
        let e = exists(&["src/a.rs"]);
        assert!(
            file_name(scan_at(
                "src/a.rs:2",
                Some(99),
                Path::new("/repo"),
                "/home/y",
                &e
            ))
            .ends_with("src/a.rs")
        );
    }
}

#[cfg(test)]
mod split_window_args_tests {
    use super::*;
    use crate::config::{Open, Split};

    fn config(split: Split, size_percent: u16) -> Open {
        Open {
            split,
            size_percent,
            editor: String::new(),
        }
    }

    #[test]
    fn a_pane_is_targeted_when_there_is_one() {
        let args = split_window_args(&config(Split::Right, 0), "nvim a.rs".into(), Some("%7"));
        assert_eq!(args, vec!["split-window", "-h", "-t", "%7", "nvim a.rs"]);
    }

    #[test]
    fn no_pane_leaves_the_target_off() {
        // Outside tmux there is no $TMUX_PANE, and an empty -t is worse than
        // no -t: tmux takes it as a target it cannot resolve and fails.
        let args = split_window_args(&config(Split::Right, 0), "nvim a.rs".into(), None);
        assert_eq!(args, vec!["split-window", "-h", "nvim a.rs"]);
    }

    #[test]
    fn a_size_follows_the_target() {
        let args = split_window_args(&config(Split::Bottom, 40), "nvim a.rs".into(), Some("%7"));
        assert_eq!(
            args,
            vec!["split-window", "-v", "-t", "%7", "-l", "40%", "nvim a.rs"]
        );
    }

    #[test]
    fn a_size_over_ninety_five_is_clamped() {
        let args = split_window_args(&config(Split::Bottom, 400), "nvim a.rs".into(), None);
        assert!(args.contains(&"95%".to_string()), "{args:?}");
    }
}
