//! Projects: one tmux session each, with the windows a layout asks for.
//!
//! Why sessions rather than more windows. With every project as two windows in
//! one session, "which project" and "which tool" share a single index, so at
//! five projects you are hopping through ten unrelated windows to reach the
//! other half of the one you are in. A session per project splits that into two
//! axes, and the toggle moves between tools without touching the project.
//!
//! The list is sessions first, most recently attached first, then the
//! directories zoxide knows. A directory zoxide has never seen cannot be picked
//! from the list, because the list is visits; typing a path that matches
//! nothing opens it anyway and records the first visit.

use std::collections::HashMap;

/// Where a row came from, which decides what picking it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A live tmux session: picking switches to it.
    Session,
    /// A directory zoxide knows: picking opens or creates its session.
    Directory,
}

/// One row of the project picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Session or directory.
    pub kind: Kind,
    /// The session's own name, or a directory's basename.
    pub label: String,
    /// The project directory.
    pub path: String,
    /// The `colourNNN` this project is painted with, if it has one.
    pub colour: Option<String>,
}

/// A tmux session name cannot hold a dot or a colon.
pub fn session_name(path: &str) -> String {
    let base = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path);
    base.chars()
        .map(|c| if c == '.' || c == ':' { '_' } else { c })
        .collect()
}

/// Shorten a path for display, touching only what sits under `home`.
///
/// The last component stays whole because it is the one being read, everything
/// between home and it collapses to a single letter, and a dotted directory
/// keeps its dot so `.config` and `config` do not both show as `c`.
pub fn short_path(path: &str, home: &str) -> String {
    if path == home {
        return "~home".to_string();
    }
    let Some(rest) = path.strip_prefix(&format!("{home}/")) else {
        return path.to_string();
    };
    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    let Some((last, head)) = parts.split_last() else {
        return format!("~/{rest}");
    };
    if head.is_empty() {
        return format!("~/{last}");
    }
    let abbrev: Vec<String> = head
        .iter()
        .map(|c| {
            let mut chars = c.chars();
            match chars.next() {
                Some('.') => format!(".{}", chars.next().unwrap_or('.')),
                Some(first) => first.to_string(),
                None => String::new(),
            }
        })
        .collect();
    format!("~/{}/{last}", abbrev.join("/"))
}

/// Parse `tmux list-sessions` output into rows, most recently attached first.
///
/// tmux lists alphabetically and this wants recency, because a picker whose
/// first row is the session you are in and whose second is the one you were in
/// before makes "go back" a keypress rather than a search.
pub fn sessions_from(listing: &str) -> Vec<Row> {
    let mut with_time: Vec<(i64, Row)> = listing
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut f = line.split('\t');
            let last_attached: i64 = f.next()?.trim().parse().unwrap_or(0);
            let label = f.next()?.to_string();
            let path = f.next()?.to_string();
            let colour = f.next().filter(|c| !c.is_empty()).map(str::to_string);
            Some((
                last_attached,
                Row {
                    kind: Kind::Session,
                    label,
                    path,
                    colour,
                },
            ))
        })
        .collect();
    with_time.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    with_time.into_iter().map(|(_, r)| r).collect()
}

/// Turn `zoxide query -l` output into rows, colouring each from the project
/// map.
pub fn directories_from(
    listing: &str,
    project_theme: &HashMap<String, String>,
    theme_colour: &HashMap<String, String>,
) -> Vec<Row> {
    listing
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|path| {
            let label = session_name(path);
            let colour = project_theme
                .get(&label)
                .and_then(|theme| theme_colour.get(theme))
                .cloned();
            Row {
                kind: Kind::Directory,
                label: path
                    .rsplit('/')
                    .find(|s| !s.is_empty())
                    .unwrap_or(path)
                    .to_string(),
                path: path.to_string(),
                colour,
            }
        })
        .collect()
}

/// Sessions then directories, with any path already listed dropped.
///
/// A project that is open is a session row and a zoxide row at once, and the
/// session row is the useful one: it knows the session's real name, which is
/// not always derivable from the path. Session `y` sits at the home directory,
/// and deriving its name from that path opened a brand new session called
/// `yogesh` the first time this was got wrong.
pub fn merge(sessions: Vec<Row>, directories: Vec<Row>) -> Vec<Row> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(sessions.len() + directories.len());
    for row in sessions.into_iter().chain(directories) {
        if seen.insert(row.path.clone()) {
            out.push(row);
        }
    }
    out
}

/// Parse `themes/_project-map.tsv`: a project name to a theme name.
pub fn parse_project_map(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .filter(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty())
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// A theme file's `@theme-color-main-1`, keyed by the file's stem.
pub fn theme_colours(files: &[(String, String)]) -> HashMap<String, String> {
    files
        .iter()
        .filter_map(|(name, text)| {
            let colour = text.lines().find_map(|line| {
                let mut w = line.split_whitespace();
                (w.next() == Some("set") && w.next() == Some("@theme-color-main-1"))
                    .then(|| w.next())
                    .flatten()
            })?;
            Some((name.clone(), colour.to_string()))
        })
        .collect()
}

/// Resolve a typed query to a directory.
///
/// The only way into a directory zoxide has never seen: it lists visits, and a
/// repository reached by `cd`, or cloned and never visited, does not appear at
/// all. Tried absolute, then `~`-relative, then relative to the pane, then
/// relative to home.
pub fn resolve_typed(query: &str, cwd: &str, home: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let expanded = match query.strip_prefix("~/") {
        Some(rest) => format!("{home}/{rest}"),
        None if query == "~" => home.to_string(),
        None => query.to_string(),
    };
    for candidate in [
        expanded.clone(),
        format!("{cwd}/{query}"),
        format!("{home}/{query}"),
    ] {
        let p = std::path::Path::new(&candidate);
        if p.is_dir() {
            return Some(
                p.canonicalize()
                    .map(|c| c.display().to_string())
                    .unwrap_or(candidate),
            );
        }
    }
    None
}

// ── Collecting ───────────────────────────────────────────────────────────────

/// Every row the picker should show: sessions first, then zoxide.
pub async fn collect(config: &crate::config::Config, home: &str) -> Vec<Row> {
    let sessions = sessions_from(&tmux_sessions().await);
    if !config.project.zoxide {
        return sessions;
    }
    let map = std::fs::read_to_string(project_map_path(home))
        .map(|t| parse_project_map(&t))
        .unwrap_or_default();
    let colours = theme_colours(&read_theme_files(home));
    merge(
        sessions,
        directories_from(&zoxide_list().await, &map, &colours),
    )
}

/// `tmux list-sessions`, with the fields the rows need.
async fn tmux_sessions() -> String {
    let out = tokio::process::Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_last_attached}\t#{session_name}\t#{session_path}\t#{@theme-color-main-1}",
        ])
        .output()
        .await;
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        // No server running yet is the ordinary case for the first project of
        // the day, not an error.
        Err(_) => String::new(),
    }
}

/// `zoxide query -l`, empty when zoxide is not installed.
async fn zoxide_list() -> String {
    let out = tokio::process::Command::new("zoxide")
        .args(["query", "-l"])
        .output()
        .await;
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

/// Where the project-to-theme map lives.
pub fn project_map_path(home: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(home).join(".config/tmux/themes/_project-map.tsv")
}

/// Every theme file, as (stem, contents).
fn read_theme_files(home: &str) -> Vec<(String, String)> {
    let dir = std::path::PathBuf::from(home).join(".config/tmux/themes");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "tmux"))
        .filter_map(|p| {
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&p).ok()?;
            Some((stem, text))
        })
        .collect()
}

/// Tell zoxide this directory was visited, the same as `z` would.
pub async fn record_visit(dir: &str) {
    let _ = tokio::process::Command::new("zoxide")
        .args(["add", dir])
        .status()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_name_cannot_hold_a_dot_or_a_colon() {
        assert_eq!(
            session_name("/Users/y/lonkar-org/yogesh.lonkar.org"),
            "yogesh_lonkar_org"
        );
        assert_eq!(session_name("/tmp/a:b"), "a_b");
        assert_eq!(session_name("/Users/y/git-repos/mysetup"), "mysetup");
    }

    #[test]
    fn a_trailing_slash_does_not_produce_an_empty_name() {
        assert_eq!(session_name("/Users/y/mysetup/"), "mysetup");
    }

    #[test]
    fn short_path_collapses_what_is_between_home_and_the_last_component() {
        let home = "/Users/yogesh";
        assert_eq!(short_path("/Users/yogesh", home), "~home");
        assert_eq!(short_path("/Users/yogesh/git-repos", home), "~/git-repos");
        assert_eq!(
            short_path("/Users/yogesh/lonkar-org/yogesh.lonkar.org", home),
            "~/l/yogesh.lonkar.org"
        );
        assert_eq!(short_path("/usr/local/bin", home), "/usr/local/bin");
    }

    #[test]
    fn a_dotted_directory_keeps_its_dot_so_two_do_not_collide() {
        let home = "/Users/yogesh";
        assert_eq!(short_path("/Users/yogesh/.config/nvim", home), "~/.c/nvim");
        assert_eq!(short_path("/Users/yogesh/config/nvim", home), "~/c/nvim");
    }

    #[test]
    fn sessions_come_back_most_recently_attached_first() {
        // tmux lists alphabetically; the picker wants recency, so that up-arrow
        // from the first row is the session you were in before.
        let listing = "100\talpha\t/a\tcolour60\n300\tzulu\t/z\tcolour98\n200\tmike\t/m\t\n";
        let rows = sessions_from(listing);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["zulu", "mike", "alpha"]);
    }

    #[test]
    fn a_session_with_no_theme_has_no_colour_rather_than_a_wrong_one() {
        let rows = sessions_from("200\tmike\t/m\t\n");
        assert_eq!(rows[0].colour, None);
    }

    #[test]
    fn directories_take_their_colour_through_the_project_map() {
        let project_theme = HashMap::from([("mysetup".to_string(), "indigo".to_string())]);
        let theme_colour = HashMap::from([("indigo".to_string(), "colour60".to_string())]);
        let rows = directories_from(
            "/Users/y/git-repos/mysetup\n",
            &project_theme,
            &theme_colour,
        );
        assert_eq!(rows[0].colour.as_deref(), Some("colour60"));
        assert_eq!(rows[0].label, "mysetup");
        assert_eq!(rows[0].kind, Kind::Directory);
    }

    #[test]
    fn a_directory_the_map_does_not_know_has_no_colour() {
        let rows = directories_from("/tmp/unknown\n", &HashMap::new(), &HashMap::new());
        assert_eq!(rows[0].colour, None);
    }

    #[test]
    fn an_open_project_appears_once_as_its_session() {
        // The session row knows the session's real name, which a path does not
        // always give: session `y` sits at the home directory, and deriving a
        // name from that opened a new session called `yogesh` the first time.
        let sessions = sessions_from("100\ty\t/Users/yogesh\t\n");
        let dirs = directories_from("/Users/yogesh\n", &HashMap::new(), &HashMap::new());
        let merged = merge(sessions, dirs);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, Kind::Session);
        assert_eq!(merged[0].label, "y");
    }

    #[test]
    fn directories_that_are_not_open_survive_the_merge() {
        let sessions = sessions_from("100\tone\t/one\t\n");
        let dirs = directories_from("/two\n/three\n", &HashMap::new(), &HashMap::new());
        assert_eq!(merge(sessions, dirs).len(), 3);
    }

    #[test]
    fn the_project_map_skips_comments_and_blank_values() {
        let map = parse_project_map("# a comment\nmysetup\tindigo\nbroken\t\n\n");
        assert_eq!(map.get("mysetup").map(String::as_str), Some("indigo"));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn theme_colours_come_from_the_files_own_setting() {
        let files = vec![
            (
                "indigo".to_string(),
                "set @theme-color-main-1 colour60\n".to_string(),
            ),
            ("nothing".to_string(), "set -g status on\n".to_string()),
        ];
        let colours = theme_colours(&files);
        assert_eq!(colours.get("indigo").map(String::as_str), Some("colour60"));
        assert!(!colours.contains_key("nothing"));
    }

    #[test]
    fn a_typed_path_is_tried_absolute_then_relative() {
        let tmp = std::env::temp_dir();
        let home = tmp.join(format!("tc-proj-home-{}", std::process::id()));
        let project = home.join("aproject");
        std::fs::create_dir_all(&project).expect("create dirs");

        let home_s = home.display().to_string();
        let abs = project.display().to_string();

        assert!(resolve_typed(&abs, "/tmp", &home_s).is_some());
        assert!(resolve_typed("~/aproject", "/tmp", &home_s).is_some());
        assert!(
            resolve_typed("aproject", "/tmp", &home_s).is_some(),
            "relative to home"
        );
        assert!(
            resolve_typed("aproject", &home_s, "/nowhere").is_some(),
            "relative to the pane"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_typed_path_that_is_nowhere_resolves_to_nothing() {
        assert_eq!(resolve_typed("definitely-not-here", "/tmp", "/tmp"), None);
        assert_eq!(resolve_typed("   ", "/tmp", "/tmp"), None);
        assert_eq!(resolve_typed("", "/tmp", "/tmp"), None);
    }
}
